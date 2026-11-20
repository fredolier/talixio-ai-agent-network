//! Un nœud de la grappe : l'état d'un agent, ses enfants, son lien vers son parent.

use std::collections::{BTreeMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc, Notify};

use crate::config::{Bootstrap, RootConfig, Timing};
use crate::link::Uplink;
use crate::model::{ModelRequest, ModelResponse, Providers};
use crate::policy::Capabilities;
use crate::protocol::{ActivityEvent, Frame, Msg, NodeInfo, Outbox, ReportKind, Status};
use crate::now_ms;

/// Ce qui réveille un agent.
#[derive(Debug)]
pub enum Input {
    /// La personne, par l'API du maître.
    User(String),
    /// Le parent.
    Parent(String),
    /// Un enfant. Son texte est une donnée, jamais un ordre.
    Child { id: String, name: String, kind: ReportKind, text: String },
    /// Le système lui-même (un enfant perdu, par exemple).
    System(String),
}

pub struct Node {
    pub id: String,
    pub name: String,
    pub role: String,
    pub depth: u32,
    pub parent_id: Option<String>,
    pub parent_name: Option<String>,
    pub instructions: String,
    pub model: String,
    pub caps: Capabilities,
    pub timing: Timing,
    pub target: String,
    pub state_dir: PathBuf,
    pub started_at_ms: u64,
    pub host: HostInfo,
    pub status: Mutex<Status>,
    pub tokens_used: AtomicU64,
    pub children: Mutex<BTreeMap<String, ChildRec>>,
    pub inbox: mpsc::UnboundedSender<Input>,
    /// Lien vers le parent. Absent chez le maître.
    pub uplink: Option<Arc<Uplink>>,
    /// Les modèles et leurs clés. Présents chez le maître seulement.
    pub providers: Option<Providers>,
    /// Le journal de toute la grappe. Présent chez le maître seulement.
    pub activity: Option<ActivityLog>,
    /// Adresse où les enfants se connectent, une fois l'écoute ouverte.
    pub listen: OnceLock<SocketAddr>,
    /// Signale que l'arbre a changé et doit être renvoyé au parent.
    pub tree_dirty: Notify,
    /// Où en est la mission confiée par le parent.
    pub mission: Mutex<Mission>,
}

/// L'état de la mission en cours d'un agent.
#[derive(Clone, Copy, Debug, Default)]
pub struct Mission {
    /// Une mission a été confiée et n'est pas encore conclue.
    pub open: bool,
    /// L'agent a posé une question à son parent et attend la réponse.
    pub awaiting_parent: bool,
    /// Relances successives sans progrès.
    pub nudges: u32,
}

/// Ce qu'un parent sait d'un enfant.
pub struct ChildRec {
    pub id: String,
    pub name: String,
    pub token: String,
    pub caps: Capabilities,
    /// Dernier état connu de l'enfant et de sa descendance.
    pub subtree: NodeInfo,
    /// Connexion en cours, s'il y en a une.
    pub conn: Option<(u64, mpsc::UnboundedSender<Frame>)>,
    pub ever_connected: bool,
    pub last_seen: Instant,
    pub last_seen_ms: u64,
    pub stopped: bool,
    pub outbox: Outbox,
    pub last_recv_seq: u64,
    /// Tokens consommés par l'enfant et sa descendance, comptés par le parent.
    pub tokens_used: u64,
    /// L'enfant a une mission en cours : on attend encore son compte rendu.
    pub mission_open: bool,
    pub process: Option<tokio::process::Child>,
}

impl ChildRec {
    fn view(&self) -> NodeInfo {
        let mut node = self.subtree.clone();
        node.last_seen_ms = self.last_seen_ms;
        if self.stopped {
            node.set_status_deep(Status::Stopped);
        } else if self.conn.is_none() && self.ever_connected {
            node.set_status_deep(Status::Disconnected);
        }
        node
    }
}

#[derive(Clone, Debug)]
pub struct HostInfo {
    pub host: String,
    pub os: String,
    pub ips: Vec<String>,
}

impl HostInfo {
    pub fn detect() -> Self {
        // l'adresse qu'utiliserait cette machine pour sortir : aucun paquet n'est envoyé
        let ip = std::net::UdpSocket::bind("0.0.0.0:0")
            .and_then(|s| s.connect("192.0.2.1:9").and_then(|_| s.local_addr()))
            .map(|a| a.ip().to_string())
            .ok()
            .filter(|ip| ip != "0.0.0.0");
        Self {
            host: gethostname::gethostname().to_string_lossy().into_owned(),
            os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            ips: ip.into_iter().collect(),
        }
    }
}

/// Le journal d'activité de la grappe, tenu par le maître.
pub struct ActivityLog {
    events: Mutex<VecDeque<ActivityEvent>>,
    next_id: AtomicU64,
    pub live: broadcast::Sender<ActivityEvent>,
}

const ACTIVITY_KEPT: usize = 10_000;

impl ActivityLog {
    fn new() -> Self {
        Self { events: Mutex::new(VecDeque::new()), next_id: AtomicU64::new(1), live: broadcast::channel(1024).0 }
    }

    pub fn push(&self, mut event: ActivityEvent) {
        let mut events = self.events.lock().unwrap();
        event.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if events.len() >= ACTIVITY_KEPT {
            events.pop_front();
        }
        events.push_back(event.clone());
        drop(events);
        let _ = self.live.send(event);
    }

    pub fn after(&self, id: u64, limit: usize) -> Vec<ActivityEvent> {
        let events = self.events.lock().unwrap();
        events.iter().filter(|e| e.id > id).take(limit).cloned().collect()
    }
}

impl Node {
    pub fn new_root(cfg: &RootConfig, providers: Providers) -> (Arc<Self>, mpsc::UnboundedReceiver<Input>) {
        let (inbox, rx) = mpsc::unbounded_channel();
        let node = Self {
            id: format!("a-{}", &crate::random_hex(1)[..10]),
            name: cfg.name.clone(),
            role: cfg.role.clone(),
            depth: 0,
            parent_id: None,
            parent_name: None,
            instructions: cfg.instructions.clone(),
            model: cfg.model.clone(),
            caps: cfg.capabilities.clone(),
            timing: cfg.timing,
            target: "root".into(),
            state_dir: cfg.state_dir.clone(),
            started_at_ms: now_ms(),
            host: HostInfo::detect(),
            status: Mutex::new(Status::Starting),
            tokens_used: AtomicU64::new(0),
            children: Mutex::new(BTreeMap::new()),
            inbox,
            uplink: None,
            providers: Some(providers),
            activity: Some(ActivityLog::new()),
            listen: OnceLock::new(),
            tree_dirty: Notify::new(),
            mission: Mutex::new(Mission::default()),
        };
        (Arc::new(node), rx)
    }

    pub fn new_child(boot: &Bootstrap, state_dir: PathBuf) -> (Arc<Self>, mpsc::UnboundedReceiver<Input>) {
        let (inbox, rx) = mpsc::unbounded_channel();
        let node = Self {
            id: boot.agent_id.clone(),
            name: boot.name.clone(),
            role: boot.role.clone(),
            depth: boot.depth,
            parent_id: Some(boot.parent_id.clone()),
            parent_name: Some(boot.parent_name.clone()),
            instructions: boot.instructions.clone(),
            model: boot.model.clone(),
            caps: boot.capabilities.clone(),
            timing: boot.timing,
            target: boot.target.clone(),
            state_dir,
            started_at_ms: now_ms(),
            host: HostInfo::detect(),
            status: Mutex::new(Status::Starting),
            tokens_used: AtomicU64::new(0),
            children: Mutex::new(BTreeMap::new()),
            inbox,
            uplink: Some(Arc::new(Uplink::new())),
            providers: None,
            activity: None,
            listen: OnceLock::new(),
            tree_dirty: Notify::new(),
            mission: Mutex::new(Mission::default()),
        };
        (Arc::new(node), rx)
    }

    /// L'agent et toute sa descendance, tels que connus ici.
    pub fn snapshot(&self) -> NodeInfo {
        let children = self.children.lock().unwrap().values().map(ChildRec::view).collect();
        NodeInfo {
            id: self.id.clone(),
            name: self.name.clone(),
            role: self.role.clone(),
            parent_id: self.parent_id.clone(),
            depth: self.depth,
            status: *self.status.lock().unwrap(),
            model: self.model.clone(),
            target: self.target.clone(),
            host: self.host.host.clone(),
            os: self.host.os.clone(),
            ips: self.host.ips.clone(),
            pid: std::process::id(),
            started_at_ms: self.started_at_ms,
            last_seen_ms: now_ms(),
            tokens_used: self.tokens_used.load(Ordering::Relaxed),
            capabilities: self.caps.clone(),
            children,
        }
    }

    pub fn set_status(&self, status: Status) {
        let changed = {
            let mut current = self.status.lock().unwrap();
            let changed = *current != status;
            *current = status;
            changed
        };
        if changed {
            self.tree_dirty.notify_one();
        }
    }

    /// Ajoute une ligne au journal de la grappe.
    pub fn emit(&self, kind: &str, text: impl Into<String>) {
        let text: String = text.into();
        let text = if text.chars().count() > 4000 {
            format!("{}… [tronqué]", text.chars().take(4000).collect::<String>())
        } else {
            text
        };
        self.record(ActivityEvent {
            id: 0,
            agent_id: self.id.clone(),
            agent_name: self.name.clone(),
            ts_ms: now_ms(),
            kind: kind.to_string(),
            text,
        });
    }

    /// Inscrit un événement (le sien ou celui d'un descendant) : au journal chez le maître, vers le parent sinon.
    pub fn record(&self, event: ActivityEvent) {
        if let Some(log) = &self.activity {
            log.push(event);
        } else if let Some(uplink) = &self.uplink {
            uplink.send_reliable(Msg::Activity { event });
        }
    }

    /// Appelle un modèle : directement chez le maître, en remontant par le parent sinon.
    pub async fn complete(&self, origin: &str, request: ModelRequest) -> Result<ModelResponse, String> {
        match (&self.providers, &self.uplink) {
            (Some(providers), _) => providers.complete(&request).await.map_err(|e| format!("{e:#}")),
            (None, Some(uplink)) => uplink.model_request(origin, request).await,
            (None, None) => Err("aucun accès aux modèles".into()),
        }
    }

    /// Traite l'appel de modèle d'un enfant, après avoir vérifié qu'il y a droit.
    pub async fn complete_for_child(
        &self,
        child_id: &str,
        origin: &str,
        request: ModelRequest,
    ) -> Result<ModelResponse, String> {
        {
            let children = self.children.lock().unwrap();
            let child = children.get(child_id).ok_or("enfant inconnu")?;
            if child.stopped {
                return Err("agent arrêté".into());
            }
            if !child.caps.models.contains(&request.model) {
                return Err(format!("modèle « {} » non autorisé pour cet agent", request.model));
            }
            if let Some(budget) = child.caps.max_tokens {
                if child.tokens_used >= budget {
                    return Err(format!("budget de tokens épuisé ({budget})"));
                }
            }
        }
        let response = self.complete(origin, request).await?;
        if let Some(child) = self.children.lock().unwrap().get_mut(child_id) {
            child.tokens_used += response.usage.total();
        }
        Ok(response)
    }

    /// Envoie un message à un enfant. `reliable` : il sera renvoyé après une coupure.
    pub fn send_child(&self, child_id: &str, msg: Msg, reliable: bool) -> Result<(), String> {
        let mut children = self.children.lock().unwrap();
        let child = children.get_mut(child_id).ok_or("enfant inconnu")?;
        if child.stopped {
            return Err(format!("l'agent {} est arrêté", child.name));
        }
        if matches!(msg, Msg::Instruction { .. }) {
            child.mission_open = true;
        }
        let frame = if reliable { child.outbox.push(msg) } else { Frame { seq: None, msg } };
        if let Some((_, conn)) = &child.conn {
            let _ = conn.send(frame);
        }
        Ok(())
    }

    /// Retrouve un enfant direct par son identifiant ou son nom.
    pub fn child_id(&self, id_or_name: &str) -> Option<String> {
        let children = self.children.lock().unwrap();
        children
            .values()
            .find(|c| c.id == id_or_name || c.name == id_or_name)
            .map(|c| c.id.clone())
    }

    /// Arrête un agent de la descendance. L'ordre descend de parent en enfant.
    pub fn stop_descendant(&self, target: &str) -> Result<(), String> {
        let via = {
            let children = self.children.lock().unwrap();
            children
                .values()
                .find(|c| !c.stopped && c.subtree.contains(target))
                .map(|c| c.id.clone())
                .ok_or("agent introuvable ou déjà arrêté")?
        };
        if via == target {
            self.stop_child(&via, "arrêt demandé");
            Ok(())
        } else {
            self.send_child(&via, Msg::StopDescendant { target: target.to_string() }, true)
        }
    }

    /// Arrête un enfant direct : ordre d'extinction, puis fin du process après un court délai.
    pub fn stop_child(&self, child_id: &str, reason: &str) {
        let (name, mut process) = {
            let mut children = self.children.lock().unwrap();
            let Some(child) = children.get_mut(child_id) else { return };
            if child.stopped {
                return;
            }
            if let Some((_, conn)) = &child.conn {
                let _ = conn.send(Frame { seq: None, msg: Msg::Shutdown { reason: reason.to_string() } });
            }
            child.stopped = true;
            child.conn = None;
            (child.name.clone(), child.process.take())
        };
        self.emit("status", format!("agent {name} arrêté : {reason}"));
        self.tree_dirty.notify_one();
        if let Some(mut child) = process.take() {
            tokio::spawn(async move {
                // le temps pour lui d'éteindre sa propre descendance
                if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                    let _ = child.kill().await;
                }
            });
        }
    }

    pub fn live_children(&self) -> usize {
        self.children.lock().unwrap().values().filter(|c| !c.stopped).count()
    }

    /// Des enfants en vie dont on attend encore le compte rendu ?
    pub fn awaiting_children(&self) -> bool {
        self.children.lock().unwrap().values().any(|c| !c.stopped && c.mission_open)
    }

    /// Éteint toute la descendance, puis ce process.
    pub async fn terminate(&self, reason: &str, code: i32) -> ! {
        eprintln!("[{}] extinction : {reason}", self.name);
        let ids: Vec<String> = self.children.lock().unwrap().keys().cloned().collect();
        for id in &ids {
            let _ = self.send_child(id, Msg::Shutdown { reason: reason.to_string() }, false);
        }
        if !ids.is_empty() {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let processes: Vec<tokio::process::Child> = {
            let mut children = self.children.lock().unwrap();
            children.values_mut().filter_map(|c| c.process.take()).collect()
        };
        for mut process in processes {
            let _ = process.start_kill();
        }
        std::process::exit(code);
    }
}
