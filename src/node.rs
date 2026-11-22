//! Un nœud de la grappe : l'état d'un agent, ses enfants, son lien vers son parent.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use tokio::sync::{broadcast, mpsc, Notify};

use crate::config::{Bootstrap, RootConfig, Target, Timing};
use crate::link::Uplink;
use crate::model::{Block, ModelRequest, ModelResponse, Providers, ServiceError, ServiceFail, ServiceRequest, ServiceResponse};
use crate::policy::Capabilities;
use crate::protocol::{ActivityEvent, Frame, Msg, NodeInfo, Outbox, ReportKind, Status};
use crate::store::Store;
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
    /// Une tâche planifiée : une consigne de la personne, écrite d'avance, arrivée à son heure.
    Task { name: String, text: String },
}

pub struct Node {
    pub id: String,
    pub name: String,
    pub role: String,
    pub depth: u32,
    pub parent_id: Option<String>,
    pub parent_name: Option<String>,
    pub instructions: String,
    /// Le modèle avec lequel l'agent réfléchit : lire `model()`. Vide chez un maître qui n'en a
    /// pas encore reçu.
    model: RwLock<String>,
    /// Le modèle ne se change pas pendant que l'agent tourne : il vient de la configuration du
    /// maître, ou du parent.
    pub model_fixed: bool,
    /// Les droits reçus au démarrage. Pour les modèles et les quotas d'appels, lire `rights()` :
    /// chez le maître, ils peuvent s'étendre pendant qu'il tourne.
    pub caps: Capabilities,
    /// La nature de chaque modèle accordé (`llm`, `vision`, `transcription`…). Indicatif
    /// hors du maître, qui seul la connaît vraiment et la revérifie à chaque appel.
    nature_table: RwLock<BTreeMap<String, String>>,
    /// Chez le maître : modèles activés et appels accordés depuis l'interface, en plus de `caps`.
    live: RwLock<Live>,
    pub timing: Timing,
    pub target: String,
    pub state_dir: PathBuf,
    /// Chemins interdits aux outils quoi qu'accordent les droits : l'état de l'agent, les clés.
    pub deny: Vec<PathBuf>,
    /// Marqueur secret des messages authentiques dans la conversation de l'agent.
    pub nonce: String,
    /// Groupes de process des commandes en cours, à tuer si l'agent s'éteint.
    pub running: Mutex<BTreeSet<crate::os::Group>>,
    /// La base locale de l'agent : sa mémoire et, chez le maître, le journal.
    pub store: Arc<Store>,
    /// Consommation de toute la grappe. Tenue chez le maître seulement.
    pub meter: Mutex<Meter>,
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
    /// Empreinte du certificat TLS de cette écoute.
    pub cert: OnceLock<String>,
    /// Taille maximale d'un journal d'agent, reçue du parent (du fichier de configuration chez le maître).
    pub log_max_bytes: u64,
    /// Nom ou adresse annoncé aux enfants d'autres machines.
    pub advertise: Option<String>,
    /// Cibles de déploiement. Seul le maître en a : lui seul écoute hors de sa machine.
    pub targets: BTreeMap<String, Target>,
    /// Rôles des paquets installés. Seul le maître en a.
    pub pack_roles: BTreeMap<String, crate::pack::InstalledRole>,
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

/// Compteur de tokens. Un appel réserve sa part avant de partir : des appels simultanés ne
/// peuvent pas dépasser ensemble le budget.
#[derive(Debug, Default)]
pub struct Meter {
    pub used: u64,
    reserved: u64,
    in_flight: u32,
    /// Appels aboutis aux modèles de service, par nature.
    pub calls: BTreeMap<String, u64>,
    /// Appels de service en cours, par nature : ils comptent déjà dans le quota.
    calls_in_flight: BTreeMap<String, u64>,
}

impl Meter {
    fn reserve(&mut self, limit: Option<u64>, estimate: u64, max_parallel: u32) -> Result<(), String> {
        if self.in_flight >= max_parallel {
            return Err("trop d'appels de modèle simultanés pour cet agent".into());
        }
        if let Some(limit) = limit {
            if self.used.saturating_add(self.reserved).saturating_add(estimate) > limit {
                return Err(format!("budget de tokens épuisé ({limit})"));
            }
        }
        self.reserved += estimate;
        self.in_flight += 1;
        Ok(())
    }

    /// Réserve un appel de service. Une nature absente des droits : aucun appel.
    fn reserve_call(&mut self, limits: &BTreeMap<String, u64>, nature: &str, max_parallel: u64) -> Result<(), String> {
        let limit = limits.get(nature).copied().unwrap_or(0);
        let used = self.calls.get(nature).copied().unwrap_or(0);
        let flying = self.calls_in_flight.get(nature).copied().unwrap_or(0);
        if used.saturating_add(flying) >= limit {
            return Err(match limit {
                0 => format!("aucun appel de nature « {nature} » n'est accordé à cet agent (max_calls)"),
                _ => format!("quota d'appels « {nature} » épuisé ({limit})"),
            });
        }
        if self.calls_in_flight.values().sum::<u64>() >= max_parallel {
            return Err("trop d'appels de service simultanés pour cet agent".into());
        }
        *self.calls_in_flight.entry(nature.to_string()).or_default() += 1;
        Ok(())
    }

    /// Un appel compté est un appel parti chez le fournisseur, abouti ou non.
    fn settle_call(&mut self, nature: &str, done: bool) {
        if let Some(flying) = self.calls_in_flight.get_mut(nature) {
            *flying = flying.saturating_sub(1);
        }
        if done {
            *self.calls.entry(nature.to_string()).or_default() += 1;
        }
    }

    fn settle(&mut self, estimate: u64, actual: Option<u64>) {
        self.reserved = self.reserved.saturating_sub(estimate);
        self.in_flight = self.in_flight.saturating_sub(1);
        self.used = self.used.saturating_add(actual.unwrap_or(0));
    }
}

/// Appels de modèle simultanés au plus pour un enfant et sa descendance.
const MAX_PARALLEL_CALLS: u32 = 16;
/// Comptes rendus au plus entre deux consignes du parent.
pub const MAX_REPORTS: u32 = 20;

/// Images jointes à une demande, au plus ; taille de chacune, une fois en base64 (4 Mio).
const MAX_IMAGES: usize = 8;
const MAX_IMAGE_B64: usize = (4 * 1024 * 1024usize).div_ceil(3) * 4;

/// Ce que le maître a reçu en plus depuis son démarrage : des modèles, et des appels par nature.
#[derive(Default)]
struct Live {
    models: Vec<String>,
    max_calls: BTreeMap<String, u64>,
}

/// Appels de service simultanés au plus pour un enfant et sa descendance.
const MAX_PARALLEL_SERVICES: u64 = 4;

/// Coût probable d'un appel, avant de le lancer : la taille de la demande, plus une réponse.
/// Une image jointe ne coûte pas sa taille en octets : un forfait.
fn estimate(request: &ModelRequest) -> u64 {
    let mut images = 0u64;
    let mut image_bytes = 0u64;
    for block in request.messages.iter().flat_map(|m| &m.content) {
        if let Block::Image { data, .. } = block {
            images += 1;
            image_bytes += data.len() as u64;
        }
    }
    let bytes = serde_json::to_vec(request).map(|bytes| bytes.len() as u64).unwrap_or(0);
    bytes.saturating_sub(image_bytes) / 4 + images * 3000 + 4096
}

/// Ce qu'un parent sait d'un enfant.
pub struct ChildRec {
    pub id: String,
    pub name: String,
    pub token: String,
    /// Clé de sa configuration scellée. Remise une seule fois, puis oubliée.
    pub seal_key: Option<String>,
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
    pub meter: Meter,
    /// L'enfant a une mission en cours : on attend encore son compte rendu.
    pub mission_open: bool,
    /// Comptes rendus reçus depuis la dernière consigne.
    pub reports: u32,
    /// Événements d'un descendant que l'arbre ne montre pas encore : gardés, en nombre borné,
    /// jusqu'au prochain arbre reçu de cet enfant.
    pub early_events: Vec<crate::protocol::ActivityEvent>,
    /// La cible où il a été déployé, s'il n'est pas sur la machine du parent.
    pub target: Option<Target>,
    pub process: Option<tokio::process::Child>,
}

impl ChildRec {
    fn view(&self) -> NodeInfo {
        let mut node = self.subtree.clone();
        node.last_seen_ms = self.last_seen_ms;
        // le compte du parent fait foi, pas celui que l'enfant déclare
        node.tokens_used = self.meter.used;
        node.calls_used = self.meter.calls.clone();
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
    store: Arc<Store>,
}

const ACTIVITY_KEPT: usize = 10_000;

impl ActivityLog {
    /// Reprend le journal là où le dernier démarrage l'a laissé.
    fn new(store: Arc<Store>) -> Self {
        let events: VecDeque<ActivityEvent> = store.recent(ACTIVITY_KEPT).into();
        let next_id = events.back().map(|e| e.id + 1).unwrap_or(1);
        Self { events: Mutex::new(events), next_id: AtomicU64::new(next_id), live: broadcast::channel(1024).0, store }
    }

    pub fn push(&self, mut event: ActivityEvent) {
        let mut events = self.events.lock().unwrap();
        event.id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if events.len() >= ACTIVITY_KEPT {
            events.pop_front();
        }
        events.push_back(event.clone());
        self.store.log(&event);
        drop(events);
        let _ = self.live.send(event);
    }

    pub fn after(&self, id: u64, limit: usize) -> Vec<ActivityEvent> {
        let events = self.events.lock().unwrap();
        events.iter().filter(|e| e.id > id).take(limit).cloned().collect()
    }
}

/// L'origine d'une demande venue d'un enfant : l'enfant reconnu, puis l'empreinte de ce qu'il déclare.
fn chained(child_id: &str, claimed: &str) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(claimed.as_bytes());
    format!("{}~{}", crate::clean(child_id, 40, false), &crate::hex(&digest)[..16])
}

impl Node {
    /// `cfg.state_dir` doit être le chemin réel du dossier d'état (voir `prepare_state_dir`).
    pub fn new_root(cfg: &RootConfig, providers: Providers) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Input>), String> {
        let (inbox, rx) = mpsc::unbounded_channel();
        let store = Arc::new(Store::open(&cfg.state_dir.join("talixio.db"))?);
        let mut deny = vec![cfg.state_dir.clone()];
        deny.extend(cfg.secret_paths().map_err(|e| format!("{e:#}"))?);
        let node = Self {
            id: format!("a-{}", &crate::random_hex(1)[..20]),
            name: cfg.name.clone(),
            role: cfg.role.clone(),
            depth: 0,
            parent_id: None,
            parent_name: None,
            instructions: cfg.instructions.clone(),
            model: RwLock::new(cfg.model.clone()),
            model_fixed: !cfg.model.is_empty(),
            nature_table: RwLock::new(providers.natures().into_iter().filter(|(alias, _)| cfg.capabilities.models.contains(alias)).collect()),
            live: RwLock::default(),
            caps: cfg.capabilities.clone(),
            timing: cfg.timing,
            target: "root".into(),
            state_dir: cfg.state_dir.clone(),
            deny,
            nonce: crate::random_hex(1)[..16].to_string(),
            running: Mutex::new(BTreeSet::new()),
            meter: Mutex::new(Meter::default()),
            started_at_ms: now_ms(),
            host: HostInfo::detect(),
            status: Mutex::new(Status::Starting),
            tokens_used: AtomicU64::new(0),
            children: Mutex::new(BTreeMap::new()),
            inbox,
            uplink: None,
            providers: Some(providers),
            activity: Some(ActivityLog::new(store.clone())),
            store,
            listen: OnceLock::new(),
            cert: OnceLock::new(),
            log_max_bytes: cfg.log_max_bytes,
            advertise: cfg.advertise.clone(),
            targets: cfg.targets.clone(),
            pack_roles: cfg.pack_roles.clone(),
            tree_dirty: Notify::new(),
            mission: Mutex::new(Mission::default()),
        };
        Ok((Arc::new(node), rx))
    }

    pub fn new_child(boot: &Bootstrap, state_dir: PathBuf) -> Result<(Arc<Self>, mpsc::UnboundedReceiver<Input>), String> {
        let (inbox, rx) = mpsc::unbounded_channel();
        let agents = state_dir.join("agents");
        std::fs::create_dir_all(&agents).map_err(|e| format!("{} : {e}", agents.display()))?;
        let store = Arc::new(Store::open(&agents.join(format!("{}.db", boot.agent_id)))?);
        let node = Self {
            id: boot.agent_id.clone(),
            name: boot.name.clone(),
            role: boot.role.clone(),
            depth: boot.depth,
            parent_id: Some(boot.parent_id.clone()),
            parent_name: Some(boot.parent_name.clone()),
            instructions: boot.instructions.clone(),
            model: RwLock::new(boot.model.clone()),
            model_fixed: true,
            nature_table: RwLock::new(boot.natures.iter().filter(|(alias, _)| boot.capabilities.models.contains(alias)).map(|(a, n)| (crate::clean(a, 80, false), crate::clean(n, 20, false))).collect()),
            live: RwLock::default(),
            caps: boot.capabilities.clone(),
            timing: boot.timing,
            target: boot.target.clone(),
            deny: std::iter::once(state_dir.clone()).chain(boot.deny.iter().cloned()).collect(),
            state_dir,
            nonce: crate::random_hex(1)[..16].to_string(),
            running: Mutex::new(BTreeSet::new()),
            meter: Mutex::new(Meter::default()),
            started_at_ms: now_ms(),
            host: HostInfo::detect(),
            status: Mutex::new(Status::Starting),
            tokens_used: AtomicU64::new(0),
            children: Mutex::new(BTreeMap::new()),
            inbox,
            uplink: Some(Arc::new(Uplink::new())),
            providers: None,
            activity: None,
            store,
            listen: OnceLock::new(),
            cert: OnceLock::new(),
            log_max_bytes: boot.log_max_bytes,
            advertise: None,
            targets: BTreeMap::new(),
            pack_roles: BTreeMap::new(),
            tree_dirty: Notify::new(),
            mission: Mutex::new(Mission::default()),
        };
        Ok((Arc::new(node), rx))
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
            model: self.model(),
            target: self.target.clone(),
            host: self.host.host.clone(),
            os: self.host.os.clone(),
            ips: self.host.ips.clone(),
            pid: std::process::id(),
            started_at_ms: self.started_at_ms,
            last_seen_ms: now_ms(),
            tokens_used: self.tokens_used.load(Ordering::Relaxed),
            calls_used: self.meter.lock().unwrap().calls.clone(),
            capabilities: self.rights(),
            unverified: false,
            children,
        }
    }

    /// L'adresse que doit joindre un enfant : sur cette machine (`remote` faux), ou depuis une autre.
    pub fn dial_addr(&self, remote: bool) -> Result<String, String> {
        let listen = self.listen.get().ok_or("cet agent n'accepte pas de connexion d'enfants")?;
        let port = listen.port();
        if !remote {
            let ip = if listen.ip().is_unspecified() { std::net::Ipv4Addr::LOCALHOST.into() } else { listen.ip() };
            return Ok(SocketAddr::new(ip, port).to_string());
        }
        if listen.ip().is_loopback() {
            return Err("cet agent n'écoute que sur sa propre machine : un enfant distant ne pourrait pas le joindre (réglage `listen`)".into());
        }
        let host = match (&self.advertise, listen.ip().is_unspecified()) {
            (Some(host), _) => host.clone(),
            (None, false) => listen.ip().to_string(),
            (None, true) => self.host.ips.first().cloned().ok_or("adresse de cette machine inconnue : réglage `advertise`")?,
        };
        Ok(if host.contains(':') && !host.starts_with('[') { format!("[{host}]:{port}") } else { format!("{host}:{port}") })
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
        self.check_images(&request)?;
        match (&self.providers, &self.uplink) {
            (Some(providers), _) => {
                let estimate = estimate(&request);
                self.meter.lock().unwrap().reserve(self.caps.max_tokens, estimate, u32::MAX)?;
                let result = providers.complete(origin, &request).await;
                self.meter.lock().unwrap().settle(estimate, result.as_ref().ok().map(|r| r.usage.total()));
                // le détail d'une erreur de fournisseur reste chez le maître
                result.map_err(|e| {
                    self.emit("error", format!("appel du modèle pour {} : {e:#}", crate::clean(origin, 64, false)));
                    "le modèle n'a pas pu répondre (détail dans le journal du maître)".to_string()
                })
            }
            (None, Some(uplink)) => uplink.model_request(origin, request).await,
            (None, None) => Err("aucun accès aux modèles".into()),
        }
    }

    /// Les droits de l'agent tels qu'ils valent maintenant.
    pub fn rights(&self) -> Capabilities {
        let live = self.live.read().unwrap();
        let mut rights = self.caps.clone();
        rights.models.extend(live.models.iter().filter(|alias| !self.caps.models.contains(alias)).cloned());
        // un quota fixé par la configuration ne se change pas à chaud
        for (nature, calls) in &live.max_calls {
            rights.max_calls.entry(nature.clone()).or_insert(*calls);
        }
        rights
    }

    /// Le modèle avec lequel l'agent réfléchit ; vide : aucun encore.
    pub fn model(&self) -> String {
        self.model.read().unwrap().clone()
    }

    /// Chez le maître dont la configuration ne fixe pas le modèle : celui-ci le devient
    /// (vide : plus aucun). À l'appelant de vérifier qu'il est dans les droits et tient une conversation.
    pub fn set_model(&self, alias: &str) {
        if !self.model_fixed {
            *self.model.write().unwrap() = alias.to_string();
            self.tree_dirty.notify_one();
        }
    }

    pub fn natures(&self) -> BTreeMap<String, String> {
        self.nature_table.read().unwrap().clone()
    }

    /// Chez le maître : un modèle activé pendant qu'il tourne entre dans ses droits.
    pub fn grant_model(&self, alias: &str, nature: &str) {
        self.nature_table.write().unwrap().insert(alias.to_string(), nature.to_string());
        let mut live = self.live.write().unwrap();
        if !live.models.iter().any(|a| a == alias) {
            live.models.push(alias.to_string());
        }
        drop(live);
        self.tree_dirty.notify_one();
    }

    /// Chez le maître : un modèle désactivé sort de ses droits.
    pub fn revoke_model(&self, alias: &str) {
        self.live.write().unwrap().models.retain(|a| a != alias);
        self.nature_table.write().unwrap().remove(alias);
        self.tree_dirty.notify_one();
    }

    /// Chez le maître : le nombre d'appels permis pour une nature de service.
    pub fn set_calls(&self, nature: &str, calls: u64) {
        self.live.write().unwrap().max_calls.insert(nature.to_string(), calls);
        self.tree_dirty.notify_one();
    }

    /// La configuration fixe-t-elle déjà le nombre d'appels de cette nature ?
    pub fn calls_fixed(&self, nature: &str) -> bool {
        self.caps.max_calls.contains_key(nature)
    }

    /// Chez le maître : plus aucun appel accordé à chaud pour cette nature.
    pub fn clear_calls(&self, nature: &str) {
        self.live.write().unwrap().max_calls.remove(nature);
        self.tree_dirty.notify_one();
    }

    /// Appelle un modèle de service : directement chez le maître, en remontant par le parent
    /// sinon. Le droit, la taille et le quota de cet agent (descendance comprise) sont vérifiés
    /// ici, à chaque niveau. Un appel parti chez le fournisseur est compté, abouti ou non.
    pub async fn service(&self, origin: &str, request: ServiceRequest) -> Result<ServiceResponse, ServiceFail> {
        let rights = self.rights();
        if !rights.models.contains(&request.model) {
            return Err("modèle non autorisé pour cet agent".into());
        }
        request.input.validate()?;
        let nature = request.input.nature().as_str();
        self.meter.lock().unwrap().reserve_call(&rights.max_calls, nature, u64::MAX)?;
        let result = match (&self.providers, &self.uplink) {
            (Some(providers), _) => providers.service(&request).await.map_err(|e| match e {
                ServiceError::Refused(reason) => ServiceFail::refused(reason),
                // le détail d'une erreur de fournisseur reste chez le maître
                ServiceError::Provider(e) => {
                    self.emit("error", format!("appel du modèle « {} » pour {} : {e:#}", crate::clean(&request.model, 80, false), crate::clean(origin, 64, false)));
                    ServiceFail { reason: "le modèle n'a pas pu répondre (détail dans le journal du maître) ; l'appel est compté".to_string(), spent: true }
                }
            }),
            (None, Some(uplink)) => uplink.service_request(origin, request).await,
            (None, None) => Err("aucun accès aux modèles".into()),
        };
        let spent = result.as_ref().map_or_else(|fail| fail.spent, |_| true);
        self.meter.lock().unwrap().settle_call(nature, spent);
        self.tree_dirty.notify_one();
        result
    }

    /// Traite l'appel de service d'un enfant, après avoir vérifié qu'il y a droit.
    pub async fn service_for_child(&self, child_id: &str, origin: &str, request: ServiceRequest) -> Result<ServiceResponse, ServiceFail> {
        let nature = request.input.nature().as_str();
        // un modèle retiré du parent depuis la création de l'enfant ne lui sert plus
        if !self.rights().models.contains(&request.model) {
            return Err("modèle non autorisé pour cet agent".into());
        }
        {
            let mut children = self.children.lock().unwrap();
            let child = children.get_mut(child_id).ok_or("enfant inconnu")?;
            if child.stopped {
                return Err("agent arrêté".into());
            }
            if !child.caps.models.contains(&request.model) {
                return Err("modèle non autorisé pour cet agent".into());
            }
            request.input.validate()?;
            child.meter.reserve_call(&child.caps.max_calls, nature, MAX_PARALLEL_SERVICES)?;
        }
        let result = self.service(origin, request).await;
        let spent = result.as_ref().map_or_else(|fail| fail.spent, |_| true);
        if let Some(child) = self.children.lock().unwrap().get_mut(child_id) {
            child.meter.settle_call(nature, spent);
        }
        self.tree_dirty.notify_one();
        result
    }

    /// Les images jointes à une demande : peu nombreuses, de taille bornée, d'un format connu,
    /// et pour un modèle qui lit les images. Revérifié à chaque niveau : un enfant peut forger
    /// sa demande.
    fn check_images(&self, request: &ModelRequest) -> Result<(), String> {
        let mut images = 0;
        for block in request.messages.iter().flat_map(|m| &m.content) {
            let Block::Image { media_type, data } = block else { continue };
            images += 1;
            if images > MAX_IMAGES {
                return Err(format!("trop d'images jointes (maximum {MAX_IMAGES})"));
            }
            if !matches!(media_type.as_str(), "image/png" | "image/jpeg" | "image/webp" | "image/gif") {
                return Err("image jointe d'un format non reconnu".into());
            }
            let plain = data.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'='));
            if data.is_empty() || data.len() > MAX_IMAGE_B64 || !plain {
                return Err("image jointe illisible ou trop grosse (maximum 4 Mo)".into());
            }
        }
        if images > 0 && self.natures().get(&request.model).is_none_or(|nature| nature != "vision") {
            return Err("ce modèle ne lit pas les images".into());
        }
        Ok(())
    }

    /// Traite l'appel de modèle d'un enfant, après avoir vérifié qu'il y a droit.
    pub async fn complete_for_child(
        &self,
        child_id: &str,
        origin: &str,
        request: ModelRequest,
    ) -> Result<ModelResponse, String> {
        let estimate = estimate(&request);
        if !self.rights().models.contains(&request.model) {
            return Err("modèle non autorisé pour cet agent".into());
        }
        {
            let mut children = self.children.lock().unwrap();
            let child = children.get_mut(child_id).ok_or("enfant inconnu")?;
            if child.stopped {
                return Err("agent arrêté".into());
            }
            if !child.caps.models.contains(&request.model) {
                return Err("modèle non autorisé pour cet agent".into());
            }
            child.meter.reserve(child.caps.max_tokens, estimate, MAX_PARALLEL_CALLS)?;
        }
        // ce qu'un enfant dit de l'origine n'est pas cru : à chaque niveau, elle est liée à l'enfant
        // reconnu par le lien. Une session de modèle tenue par agent ne peut donc pas être prise
        // par un autre agent, même de la même branche.
        let origin = chained(child_id, origin);
        let result = self.complete(&origin, request).await;
        if let Some(child) = self.children.lock().unwrap().get_mut(child_id) {
            child.meter.settle(estimate, result.as_ref().ok().map(|r| r.usage.total()));
        }
        self.tree_dirty.notify_one();
        result
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
            child.reports = 0;
        }
        let frame = if reliable { child.outbox.push(msg) } else { Frame { seq: None, msg } };
        if let Some((_, conn)) = &child.conn {
            let _ = conn.send(frame);
        }
        Ok(())
    }

    /// Retrouve un enfant direct par son identifiant ou son nom. Un enfant en vie passe
    /// avant un ancien enfant arrêté du même nom.
    pub fn child_id(&self, id_or_name: &str) -> Option<String> {
        let children = self.children.lock().unwrap();
        let named = |c: &&ChildRec| c.id == id_or_name || c.name == id_or_name;
        children
            .values()
            .filter(|c| !c.stopped)
            .find(named)
            .or_else(|| children.values().find(named))
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
            // un arrêt demandé vaut pour de bon : l'agent ne sera pas recréé au redémarrage
            self.store.permanent_delete(&via);
            self.stop_child(&via, "arrêt demandé");
            Ok(())
        } else {
            self.send_child(&via, Msg::StopDescendant { target: target.to_string() }, true)
        }
    }

    /// Arrête un enfant direct : ordre d'extinction, puis fin du process après un court délai.
    pub fn stop_child(&self, child_id: &str, reason: &str) {
        if self.providers.is_some() {
            crate::model::cli::forget_branch(child_id);
        }
        let (name, mut process, target) = {
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
            (child.name.clone(), child.process.take(), child.target.clone())
        };
        self.emit("status", format!("agent {name} arrêté : {reason}"));
        self.tree_dirty.notify_one();
        let id = child_id.to_string();
        tokio::spawn(async move {
            // le temps pour lui d'éteindre sa propre descendance
            if let Some(mut child) = process.take() {
                if tokio::time::timeout(Duration::from_secs(5), child.wait()).await.is_err() {
                    let _ = child.kill().await;
                }
            } else if target.is_some() {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            // déployé ailleurs : il est aussi désinstallé
            if let Some(target) = target {
                if let Err(e) = crate::spawn::uninstall(&target, &id).await {
                    eprintln!("désinstallation de {id} : {e}");
                }
            }
        });
    }

    pub fn live_children(&self) -> usize {
        self.children.lock().unwrap().values().filter(|c| !c.stopped).count()
    }

    /// Des enfants en vie dont on attend encore le compte rendu ?
    pub fn awaiting_children(&self) -> bool {
        self.children.lock().unwrap().values().any(|c| !c.stopped && c.mission_open)
    }

    /// Les enfants en vie dont on attend encore le compte rendu, par leur nom.
    pub fn busy_children(&self) -> Vec<String> {
        let mut names: Vec<String> = self.children.lock().unwrap().values().filter(|c| !c.stopped && c.mission_open).map(|c| c.name.clone()).collect();
        names.sort();
        names
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
        let (processes, deployed): (Vec<tokio::process::Child>, Vec<(String, Target)>) = {
            let mut children = self.children.lock().unwrap();
            let deployed = children.iter().filter_map(|(id, c)| Some((id.clone(), c.target.clone()?))).collect();
            (children.values_mut().filter_map(|c| c.process.take()).collect(), deployed)
        };
        // les agents déployés ailleurs sont désinstallés, sans que cela puisse retenir l'extinction
        futures_util::future::join_all(deployed.iter().map(|(id, target)| async move {
            match tokio::time::timeout(Duration::from_secs(15), crate::spawn::uninstall(target, id)).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => eprintln!("désinstallation de {id} : {e}"),
                Err(_) => eprintln!("désinstallation de {id} : cible injoignable"),
            }
        }))
        .await;
        for mut process in processes {
            let _ = process.start_kill();
        }
        // rien de ce que l'agent a lancé ne lui survit
        for group in std::mem::take(&mut *self.running.lock().unwrap()) {
            crate::os::kill_group(group);
        }
        std::process::exit(code);
    }
}
