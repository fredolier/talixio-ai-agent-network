//! Le lien entre un enfant et son parent.
//!
//! L'enfant se connecte au parent (WebSocket). Chacun prouve à l'autre qu'il détient leur
//! secret partagé, sans jamais l'envoyer (défi-réponse HMAC, dans les deux sens). Chaque
//! côté surveille l'autre par des battements. Sans parent, l'enfant se met en veille ;
//! si le parent ne revient pas, il s'éteint avec toute sa descendance.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{Sink, SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message as TMessage;

use crate::model::{ModelRequest, ModelResponse, ServiceFail, ServiceRequest, ServiceResponse};
use crate::node::{Input, Node, MAX_REPORTS};
use crate::protocol::{Frame, Msg, NodeInfo, Outbox, ReportKind, Status};
use crate::{now_ms, random_hex, secret_eq};

/// Durée maximale de toute la présentation, de la première trame à l'accueil.
const HANDSHAKE: Duration = Duration::from_secs(10);
/// Un envoi qui n'aboutit pas dans ce délai : le lien est considéré comme perdu.
const SEND_TIMEOUT: Duration = Duration::from_secs(30);
/// Taille maximale d'une trame : une conversation entière y passe à chaque appel de modèle.
pub const MAX_FRAME: usize = 16 * 1024 * 1024;
/// Taille maximale de toute autre trame venue d'un enfant (arbre, compte rendu, journal).
const MAX_SMALL_FRAME: usize = 1024 * 1024;
/// Un arbre déclaré par un enfant : nombre d'agents et profondeur au plus.
const MAX_TREE_NODES: usize = 2000;
const MAX_TREE_DEPTH: usize = 32;
/// Taille maximale des droits qu'un descendant peut déclarer ; au-delà, l'arbre est refusé.
const MAX_DECLARED_CAPS: usize = 16 * 1024;
/// Longueur maximale d'un compte rendu d'enfant, en caractères.
const MAX_REPORT_CHARS: usize = 20_000;

/// Envoie une trame, sans attendre indéfiniment un correspondant qui ne lit plus.
async fn put<S, M>(sink: &mut S, frame: &Frame) -> bool
where
    S: Sink<M> + Unpin,
    M: From<String>,
{
    matches!(tokio::time::timeout(SEND_TIMEOUT, sink.send(M::from(encode(frame)))).await, Ok(Ok(())))
}

fn plain(msg: Msg) -> Frame {
    Frame { seq: None, msg }
}

// ---------------------------------------------------------------------------------------
// Côté enfant
// ---------------------------------------------------------------------------------------

type ModelReply = Result<ModelResponse, String>;
type ServiceReply = Result<ServiceResponse, ServiceFail>;

/// Attente maximale de la réponse à un appel de service : un peu plus que le délai du maître chez le fournisseur.
const SERVICE_TIMEOUT: Duration = Duration::from_secs(660);
/// A secret is a short answer from the master's own disk: no reason to wait long.
const SECRET_TIMEOUT: Duration = Duration::from_secs(20);
const LINK_CUT: &str = "le lien avec le parent a été coupé pendant l'appel : il a pu aboutir, il est compté";

/// Le lien d'un enfant vers son parent.
pub struct Uplink {
    out: Mutex<UpOut>,
    connected: watch::Sender<bool>,
    pending: Mutex<HashMap<String, oneshot::Sender<ModelReply>>>,
    pending_services: Mutex<HashMap<String, oneshot::Sender<ServiceReply>>>,
    pending_secrets: Mutex<HashMap<String, oneshot::Sender<Result<String, String>>>>,
    last_recv_seq: AtomicU64,
}

#[derive(Default)]
struct UpOut {
    outbox: Outbox,
    conn: Option<mpsc::UnboundedSender<Frame>>,
}

const LINK_LOST: &str = "lien avec le parent perdu";

impl Uplink {
    pub fn new() -> Self {
        Self {
            out: Mutex::new(UpOut::default()),
            connected: watch::channel(false).0,
            pending: Mutex::new(HashMap::new()),
            pending_services: Mutex::new(HashMap::new()),
            pending_secrets: Mutex::new(HashMap::new()),
            last_recv_seq: AtomicU64::new(0),
        }
    }

    /// Envoi sans garantie : perdu si le lien est coupé.
    pub fn send(&self, msg: Msg) {
        if let Some(conn) = &self.out.lock().unwrap().conn {
            let _ = conn.send(Frame { seq: None, msg });
        }
    }

    /// Envoi garanti : renvoyé à la reconnexion tant qu'il n'est pas acquitté.
    pub fn send_reliable(&self, msg: Msg) {
        let mut out = self.out.lock().unwrap();
        let frame = out.outbox.push(msg);
        if let Some(conn) = &out.conn {
            let _ = conn.send(frame);
        }
    }

    pub fn is_connected(&self) -> bool {
        *self.connected.borrow()
    }

    /// Attend que le parent soit joignable. C'est la mise en veille de l'agent.
    pub async fn wait_connected(&self) {
        let mut rx = self.connected.subscribe();
        let _ = rx.wait_for(|connected| *connected).await;
    }

    /// Appel de modèle par le parent. Si le lien tombe, attend son retour et recommence.
    pub async fn model_request(&self, origin: &str, request: ModelRequest) -> ModelReply {
        loop {
            self.wait_connected().await;
            let request_id = random_hex(1);
            let (tx, rx) = oneshot::channel();
            self.pending.lock().unwrap().insert(request_id.clone(), tx);
            self.send(Msg::ModelRequest {
                request_id: request_id.clone(),
                origin: origin.to_string(),
                request: Box::new(request.clone()),
            });
            match rx.await {
                Ok(Err(e)) if e == LINK_LOST => continue,
                Ok(reply) => return reply,
                Err(_) => continue,
            }
        }
    }

    /// Appel d'un modèle de service par le parent. Si le lien tombe, l'appel n'est pas
    /// refait : il a peut-être abouti, et un appel compté ne se fait qu'une fois.
    pub async fn service_request(&self, origin: &str, request: ServiceRequest) -> ServiceReply {
        let request_id = random_hex(1);
        let (tx, rx) = oneshot::channel();
        self.pending_services.lock().unwrap().insert(request_id.clone(), tx);
        // inscrit d'abord, vérifié ensuite : une coupure entre les deux ne laisse pas l'appel sans fin
        if !self.is_connected() {
            self.pending_services.lock().unwrap().remove(&request_id);
            return Err("parent injoignable : réessaie plus tard".into());
        }
        self.send(Msg::ServiceRequest { request_id: request_id.clone(), origin: origin.to_string(), request: Box::new(request) });
        match tokio::time::timeout(SERVICE_TIMEOUT, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err(ServiceFail { reason: LINK_CUT.into(), spent: true }),
            Err(_) => {
                self.pending_services.lock().unwrap().remove(&request_id);
                Err(ServiceFail { reason: "le parent n'a pas répondu à temps : l'appel a pu aboutir, il est compté".into(), spent: true })
            }
        }
    }

    /// Asks the parent for the secrets of a command. Not asked twice: the command runs without.
    pub async fn secret_request(&self, ask: crate::secrets::SecretAsk) -> Result<String, String> {
        let request_id = random_hex(1);
        let (tx, rx) = oneshot::channel();
        self.pending_secrets.lock().unwrap().insert(request_id.clone(), tx);
        if !self.is_connected() {
            self.pending_secrets.lock().unwrap().remove(&request_id);
            return Err("parent out of reach".into());
        }
        self.send(Msg::SecretRequest { request_id: request_id.clone(), ask: Box::new(ask) });
        match tokio::time::timeout(SECRET_TIMEOUT, rx).await {
            Ok(Ok(reply)) => reply,
            Ok(Err(_)) => Err("the link to the parent was cut".into()),
            Err(_) => {
                self.pending_secrets.lock().unwrap().remove(&request_id);
                Err("the parent did not answer in time".into())
            }
        }
    }

    fn fail_pending(&self) {
        self.pending_secrets.lock().unwrap().clear();
        for (_, tx) in self.pending_services.lock().unwrap().drain() {
            let _ = tx.send(Err(ServiceFail { reason: LINK_CUT.into(), spent: true }));
        }
        for (_, tx) in self.pending.lock().unwrap().drain() {
            let _ = tx.send(Err(LINK_LOST.to_string()));
        }
    }
}

impl Default for Uplink {
    fn default() -> Self {
        Self::new()
    }
}

enum Ended {
    /// Le lien n'a pas pu s'établir, ou est tombé.
    Lost { was_connected: bool },
    /// Le parent ne veut pas, ou plus, de cet enfant.
    Dismissed(String),
}

/// Maintient le lien vers le parent, pour toute la vie de l'enfant.
pub async fn run_uplink(node: Arc<Node>, addr: String, cert: String, token: String) {
    let Ok(tls) = crate::tls::pinned_client(&cert) else {
        node.terminate("empreinte du certificat du parent invalide", 2).await
    };
    let tls = tokio_rustls::TlsConnector::from(Arc::new(tls));
    let uplink = node.uplink.clone().expect("un enfant a un lien vers son parent");
    let mut lost_since = Instant::now();
    let mut backoff = Duration::from_millis(250);
    loop {
        match session(&node, &uplink, &addr, &cert, &tls, &token).await {
            Ended::Dismissed(reason) => node.terminate(&reason, 0).await,
            Ended::Lost { was_connected } => {
                if was_connected {
                    lost_since = Instant::now();
                    backoff = Duration::from_millis(250);
                    node.set_status(Status::Suspended);
                    eprintln!("[{}] parent injoignable : mise en veille", node.name);
                }
            }
        }
        if lost_since.elapsed() >= node.timing.die_after() {
            node.terminate("parent injoignable depuis trop longtemps", 3).await;
        }
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
}

/// Premier geste d'un enfant : aller chercher chez son parent la clé de sa configuration.
/// Le parent doit d'abord prouver qui il est (certificat épinglé, puis secret du lien).
pub async fn fetch_key(agent_id: &str, addr: &str, cert: &str, token: &str) -> Option<String> {
    let tls = tokio_rustls::TlsConnector::from(Arc::new(crate::tls::pinned_client(cert).ok()?));
    let exchange = async {
        let tcp = tokio::net::TcpStream::connect(addr).await.ok()?;
        let name = rustls::pki_types::ServerName::try_from(crate::tls::NAME).ok()?;
        let stream = tls.connect(name, tcp).await.ok()?;
        let (ws, _) = tokio_tungstenite::client_async(format!("wss://{addr}/link"), stream).await.ok()?;
        let (mut sink, mut stream) = ws.split();
        let mine = random_hex(1);
        if !put::<_, TMessage>(&mut sink, &plain(Msg::Hello { agent_id: agent_id.to_string(), nonce: mine.clone() })).await {
            return None;
        }
        let Some(Ok(TMessage::Text(text))) = stream.next().await else { return None };
        let Some(Msg::Challenge { nonce: theirs, proof }) = decode(text.as_str()).map(|f| f.msg) else { return None };
        if theirs.len() > 128 || !secret_eq(&proof, &crate::proof(token, "parent", [&mine, &theirs, agent_id, cert])) {
            return None;
        }
        let proof = crate::proof(token, "unseal", [&mine, &theirs, agent_id, cert]);
        if !put::<_, TMessage>(&mut sink, &plain(Msg::Unseal { proof })).await {
            return None;
        }
        let Some(Ok(TMessage::Text(text))) = stream.next().await else { return None };
        match decode(text.as_str()).map(|f| f.msg) {
            Some(Msg::Key { key }) if key.len() == 64 => Some(key),
            _ => None,
        }
    };
    tokio::time::timeout(HANDSHAKE, exchange).await.ok().flatten()
}

async fn session(
    node: &Arc<Node>,
    uplink: &Arc<Uplink>,
    addr: &str,
    cert: &str,
    tls: &tokio_rustls::TlsConnector,
    token: &str,
) -> Ended {
    let lost = Ended::Lost { was_connected: false };
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    // connexion chiffrée : seul le certificat attendu est accepté, quel que soit le nom présenté
    let connect = async {
        let tcp = tokio::net::TcpStream::connect(addr).await.ok()?;
        let name = rustls::pki_types::ServerName::try_from(crate::tls::NAME).ok()?;
        let stream = tls.connect(name, tcp).await.ok()?;
        tokio_tungstenite::client_async_with_config(format!("wss://{addr}/link"), stream, Some(config)).await.ok()
    };
    let Ok(Some((ws, _))) = tokio::time::timeout(HANDSHAKE, connect).await else { return lost };
    let (mut sink, mut stream) = ws.split();

    // présentation : le parent doit prouver qui il est avant que l'enfant ne lui dise quoi que ce soit
    let handshake = async {
        let mine = random_hex(1);
        if !put::<_, TMessage>(&mut sink, &plain(Msg::Hello { agent_id: node.id.clone(), nonce: mine.clone() })).await {
            return None;
        }
        let Some(Ok(TMessage::Text(text))) = stream.next().await else { return None };
        // un refus à ce stade ne prouve rien sur son auteur : il ne vaut pas ordre d'arrêt
        let Some(Msg::Challenge { nonce: theirs, proof }) = decode(text.as_str()).map(|f| f.msg) else { return None };
        if theirs.len() > 128 || !secret_eq(&proof, &crate::proof(token, "parent", [&mine, &theirs, &node.id, cert])) {
            return None;
        }
        let proof = crate::proof(token, "child", [&mine, &theirs, &node.id, cert]);
        if !put::<_, TMessage>(&mut sink, &plain(Msg::Proof { proof, node: Box::new(node.snapshot()) })).await {
            return None;
        }
        let Some(Ok(TMessage::Text(text))) = stream.next().await else { return None };
        match decode(text.as_str()).map(|f| f.msg) {
            Some(Msg::Welcome) => Some(Ok(())),
            Some(Msg::Rejected { reason }) => Some(Err(crate::clean(&reason, 200, false))),
            _ => None,
        }
    };
    match tokio::time::timeout(HANDSHAKE, handshake).await {
        Ok(Some(Ok(()))) => {}
        Ok(Some(Err(reason))) => return Ended::Dismissed(format!("refusé par le parent : {reason}")),
        _ => return lost,
    }

    // connecté : d'abord ce qui attendait, puis le fil normal
    let (tx, mut rx) = mpsc::unbounded_channel::<Frame>();
    let backlog = {
        let mut out = uplink.out.lock().unwrap();
        out.conn = Some(tx);
        out.outbox.pending()
    };
    uplink.connected.send_replace(true);
    if *node.status.lock().unwrap() == Status::Suspended {
        node.set_status(Status::Idle);
    }
    node.tree_dirty.notify_one();

    let mut ended = Ended::Lost { was_connected: true };
    let mut last_rx = Instant::now();
    let mut tick = tokio::time::interval(node.timing.heartbeat());
    let mut backlog = backlog.into_iter();
    'link: loop {
        if let Some(frame) = backlog.next() {
            if !put::<_, TMessage>(&mut sink, &frame).await {
                break;
            }
            continue;
        }
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(TMessage::Text(text))) => {
                    last_rx = Instant::now();
                    let Some(frame) = decode(text.as_str()) else { continue };
                    for reply in child_receive(node, uplink, frame, &mut ended) {
                        if !put::<_, TMessage>(&mut sink, &reply).await {
                            break 'link;
                        }
                    }
                    if matches!(ended, Ended::Dismissed(_)) {
                        break;
                    }
                }
                Some(Ok(TMessage::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => last_rx = Instant::now(),
            },
            Some(frame) = rx.recv() => {
                if !put::<_, TMessage>(&mut sink, &frame).await {
                    break;
                }
            }
            _ = tick.tick() => {
                if last_rx.elapsed() >= node.timing.suspend_after() {
                    break;
                }
                if !put::<_, TMessage>(&mut sink, &plain(Msg::Ping)).await {
                    break;
                }
            }
        }
    }
    uplink.out.lock().unwrap().conn = None;
    uplink.connected.send_replace(false);
    uplink.fail_pending();
    ended
}

/// Traite une trame venue du parent. Renvoie les réponses immédiates à lui faire.
fn child_receive(node: &Arc<Node>, uplink: &Arc<Uplink>, frame: Frame, ended: &mut Ended) -> Vec<Frame> {
    let mut replies = Vec::new();
    if let Some(seq) = frame.seq {
        replies.push(Frame { seq: None, msg: Msg::Ack { seq } });
        if seq <= uplink.last_recv_seq.load(Ordering::Relaxed) {
            return replies; // déjà reçu avant une coupure
        }
        uplink.last_recv_seq.store(seq, Ordering::Relaxed);
    }
    match frame.msg {
        Msg::Ping => replies.push(Frame { seq: None, msg: Msg::Pong }),
        Msg::Ack { seq } => uplink.out.lock().unwrap().outbox.ack(seq),
        Msg::Instruction { text } => {
            let _ = node.inbox.send(Input::Parent(text));
        }
        Msg::ModelResponse { request_id, response, error } => {
            if let Some(tx) = uplink.pending.lock().unwrap().remove(&request_id) {
                let reply = match (response, error) {
                    (Some(response), _) => Ok(*response),
                    (None, error) => Err(error.unwrap_or_else(|| "réponse vide".into())),
                };
                let _ = tx.send(reply);
            }
        }
        Msg::ServiceResponse { request_id, response, error, spent } => {
            if let Some(tx) = uplink.pending_services.lock().unwrap().remove(&request_id) {
                let reply = match (response, error) {
                    (Some(response), _) => Ok(*response),
                    (None, error) => Err(ServiceFail { reason: crate::clean(&error.unwrap_or_else(|| "réponse vide".into()), 500, false), spent }),
                };
                let _ = tx.send(reply);
            }
        }
        Msg::SecretResponse { request_id, sealed, error } => {
            if let Some(tx) = uplink.pending_secrets.lock().unwrap().remove(&request_id) {
                let _ = tx.send(sealed.filter(|s| s.len() <= 64 * 1024).ok_or_else(|| crate::clean(&error.unwrap_or_else(|| "empty answer".into()), 300, false)));
            }
        }
        Msg::StopDescendant { target } => {
            if let Err(e) = node.stop_descendant(&target) {
                node.emit("error", format!("arrêt de {target} : {e}"));
            }
        }
        Msg::Shutdown { reason } => *ended = Ended::Dismissed(format!("arrêt demandé par le parent : {reason}")),
        _ => {}
    }
    replies
}

/// Événements gardés par enfant en attendant l'arbre qui annonce leur auteur.
const MAX_EARLY_EVENTS: usize = 64;

/// Inscrit au journal l'événement d'un descendant, sous le nom que ce nœud lui connaît.
fn record_child_event(node: &Arc<Node>, name: String, mut event: crate::protocol::ActivityEvent) {
    event.agent_name = name;
    if !crate::protocol::EVENT_KINDS.contains(&event.kind.as_str()) {
        event.kind = "autre".into();
    }
    event.text = crate::clean(&event.text, 4000, true);
    node.record(event);
}

/// Renvoie l'arbre au parent quand il change, et régulièrement pour donner signe de vie.
pub async fn run_tree_reporter(node: Arc<Node>) {
    let Some(uplink) = node.uplink.clone() else { return };
    loop {
        let _ = tokio::time::timeout(Duration::from_secs(10), node.tree_dirty.notified()).await;
        tokio::time::sleep(Duration::from_millis(100)).await; // regroupe les changements rapprochés
        if uplink.is_connected() {
            uplink.send(Msg::Tree { node: Box::new(node.snapshot()) });
        }
    }
}

// ---------------------------------------------------------------------------------------
// Côté parent
// ---------------------------------------------------------------------------------------

static CONNECTION: AtomicU64 = AtomicU64::new(1);

/// Sert la connexion d'un enfant, du premier message à la coupure. `permit` : la place
/// occupée parmi les présentations en cours, rendue dès que l'enfant est reconnu ou refusé.
pub async fn serve_child(node: Arc<Node>, socket: WebSocket, permit: tokio::sync::OwnedSemaphorePermit) {
    let (mut sink, mut stream) = socket.split();
    const REFUSED: &str = "présentation refusée";

    // présentation : rien n'est cru, ni journalisé, tant que l'enfant n'a pas prouvé qui il est
    let handshake = async {
        let Some(Ok(WsMessage::Text(text))) = stream.next().await else { return Err(None) };
        let Some(Msg::Hello { agent_id, nonce: theirs }) = decode_quiet(text.as_str()).map(|f| f.msg) else {
            return Err(None);
        };
        let token = node.children.lock().unwrap().get(&agent_id).map(|c| c.token.clone());
        let Some(token) = token.filter(|_| theirs.len() <= 128) else { return Err(Some(REFUSED)) };
        let mine = random_hex(1);
        let cert = node.cert.get().map(String::as_str).unwrap_or_default();
        let proof = crate::proof(&token, "parent", [&theirs, &mine, &agent_id, cert]);
        if !put::<_, WsMessage>(&mut sink, &plain(Msg::Challenge { nonce: mine.clone(), proof })).await {
            return Err(None);
        }
        let Some(Ok(WsMessage::Text(text))) = stream.next().await else { return Err(None) };
        match decode_quiet(text.as_str()).map(|f| f.msg) {
            Some(Msg::Proof { proof, node: info }) if secret_eq(&proof, &crate::proof(&token, "child", [&theirs, &mine, &agent_id, cert])) => {
                Ok((agent_id, info))
            }
            // premier démarrage : l'enfant, reconnu, vient chercher la clé de sa configuration.
            // Elle n'est remise qu'une fois : qui volerait l'enveloppe ensuite n'en tirerait rien.
            Some(Msg::Unseal { proof }) if secret_eq(&proof, &crate::proof(&token, "unseal", [&theirs, &mine, &agent_id, cert])) => {
                let key = node.children.lock().unwrap().get_mut(&agent_id).filter(|c| !c.stopped).and_then(|c| c.seal_key.take());
                let Some(key) = key else { return Err(Some(REFUSED)) };
                let _ = put::<_, WsMessage>(&mut sink, &plain(Msg::Key { key })).await;
                Err(None)
            }
            _ => Err(Some(REFUSED)),
        }
    };
    let (agent_id, info) = match tokio::time::timeout(HANDSHAKE, handshake).await {
        Ok(Ok(proven)) => proven,
        Ok(Err(Some(reason))) => {
            let _ = put::<_, WsMessage>(&mut sink, &plain(Msg::Rejected { reason: reason.into() })).await;
            return;
        }
        _ => return,
    };
    drop(permit);

    let (tx, mut rx) = mpsc::unbounded_channel::<Frame>();
    let conn_id = CONNECTION.fetch_add(1, Ordering::Relaxed);
    let accepted = {
        let mut children = node.children.lock().unwrap();
        match children.get_mut(&agent_id) {
            Some(child) if !child.stopped => {
                child.conn = Some((conn_id, tx));
                child.ever_connected = true;
                child.last_seen = Instant::now();
                child.last_seen_ms = now_ms();
                Some((child.outbox.pending(), child.name.clone()))
            }
            _ => None,
        }
    };
    let Some((backlog, child_name)) = accepted else {
        let _ = put::<_, WsMessage>(&mut sink, &plain(Msg::Rejected { reason: "agent arrêté".into() })).await;
        return;
    };
    accept_tree(&node, &agent_id, *info);
    node.tree_dirty.notify_one();
    node.emit("link", format!("agent {child_name} connecté"));
    if !put::<_, WsMessage>(&mut sink, &plain(Msg::Welcome)).await {
        return;
    }

    let mut last_rx = Instant::now();
    let mut tick = tokio::time::interval(node.timing.heartbeat());
    let mut backlog = backlog.into_iter();
    'link: loop {
        if let Some(frame) = backlog.next() {
            if !put::<_, WsMessage>(&mut sink, &frame).await {
                break;
            }
            continue;
        }
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(WsMessage::Text(text))) => {
                    last_rx = Instant::now();
                    let Some(frame) = decode(text.as_str()) else { continue };
                    // seuls un appel de modèle (une conversation entière) ou de service (un son) peuvent être volumineux
                    if text.len() > MAX_SMALL_FRAME && !matches!(frame.msg, Msg::ModelRequest { .. } | Msg::ServiceRequest { .. }) {
                        continue;
                    }
                    for reply in parent_receive(&node, &agent_id, frame) {
                        if !put::<_, WsMessage>(&mut sink, &reply).await {
                            break 'link;
                        }
                    }
                }
                Some(Ok(WsMessage::Close(_))) | Some(Err(_)) | None => break,
                Some(Ok(_)) => last_rx = Instant::now(),
            },
            outgoing = rx.recv() => match outgoing {
                Some(frame) => {
                    let closing = matches!(frame.msg, Msg::Shutdown { .. });
                    if !put::<_, WsMessage>(&mut sink, &frame).await || closing {
                        break;
                    }
                }
                None => break, // l'enfant a été arrêté, ou une connexion plus récente l'a remplacée
            },
            _ = tick.tick() => {
                if last_rx.elapsed() >= node.timing.suspend_after() {
                    break;
                }
                if !put::<_, WsMessage>(&mut sink, &plain(Msg::Ping)).await {
                    break;
                }
            }
        }
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), sink.close()).await;
    let dropped = {
        let mut children = node.children.lock().unwrap();
        match children.get_mut(&agent_id) {
            Some(child) if child.conn.as_ref().is_some_and(|(id, _)| *id == conn_id) => {
                child.conn = None;
                !child.stopped
            }
            _ => false,
        }
    };
    if dropped {
        node.emit("link", format!("agent {child_name} déconnecté"));
        node.tree_dirty.notify_one();
    }
}

/// Enregistre l'arbre qu'un enfant déclare, s'il est recevable. Rien n'y est cru sur parole :
/// l'identité et les droits de l'enfant sont ceux que le parent lui a donnés, l'arbre est
/// borné, et un enfant ne peut pas y revendiquer un agent qui existe ailleurs dans la grappe
/// (il détournerait les ordres d'arrêt et signerait le journal à sa place).
fn accept_tree(node: &Node, child_id: &str, mut info: NodeInfo) -> bool {
    if !info.within(MAX_TREE_NODES, MAX_TREE_DEPTH) || !info.declared_within(MAX_DECLARED_CAPS) {
        return false;
    }
    let mut children = node.children.lock().unwrap();
    {
        let mut ids = Vec::new();
        info.descendant_ids(&mut ids);
        let mut seen = std::collections::HashSet::new();
        let foreign = |id: &str| {
            id == node.id
                || id == child_id
                || node.parent_id.as_deref() == Some(id)
                || children.values().any(|c| c.id != child_id && c.subtree.contains(id))
        };
        if ids.iter().any(|id| !seen.insert(*id) || foreign(id)) {
            return false;
        }
    }
    let Some(child) = children.get_mut(child_id) else { return false };
    info.id = child.id.clone();
    info.name = child.name.clone();
    info.role = child.subtree.role.clone();
    info.parent_id = child.subtree.parent_id.clone();
    info.depth = child.subtree.depth;
    info.model = child.subtree.model.clone();
    info.target = child.subtree.target.clone();
    info.capabilities = child.caps.clone();
    info.unverified = false;
    info.tidy();
    info.mark_unverified();
    // un descendant ne porte pas le nom de ce nœud ni celui d'un de ses enfants directs :
    // au journal, on le prendrait pour eux
    let child_id = child_id.to_string();
    let mut taken: std::collections::HashSet<String> = children.values().map(|c| c.name.clone()).collect();
    taken.insert(node.name.clone());
    fn rename(info: &mut NodeInfo, taken: &std::collections::HashSet<String>) {
        for child in &mut info.children {
            if taken.contains(&child.name) {
                let short: String = child.id.chars().take(6).collect();
                child.name = format!("{} ({short})", child.name);
            }
            rename(child, taken);
        }
    }
    rename(&mut info, &taken);
    if let Some(child) = children.get_mut(&child_id) {
        child.subtree = info;
    }
    true
}

/// Traite une trame venue d'un enfant. Renvoie les réponses immédiates à lui faire.
fn parent_receive(node: &Arc<Node>, child_id: &str, frame: Frame) -> Vec<Frame> {
    let mut replies = Vec::new();
    let (child_name, duplicate) = {
        let mut children = node.children.lock().unwrap();
        let Some(child) = children.get_mut(child_id) else { return replies };
        child.last_seen = Instant::now();
        child.last_seen_ms = now_ms();
        let duplicate = match frame.seq {
            Some(seq) if seq <= child.last_recv_seq => true,
            Some(seq) => {
                child.last_recv_seq = seq;
                false
            }
            None => false,
        };
        (child.name.clone(), duplicate)
    };
    if let Some(seq) = frame.seq {
        replies.push(Frame { seq: None, msg: Msg::Ack { seq } });
        if duplicate {
            return replies; // déjà reçu avant une coupure
        }
    }
    match frame.msg {
        Msg::Ping => replies.push(Frame { seq: None, msg: Msg::Pong }),
        Msg::Ack { seq } => {
            if let Some(child) = node.children.lock().unwrap().get_mut(child_id) {
                child.outbox.ack(seq);
            }
        }
        Msg::Tree { node: info } => {
            if accept_tree(node, child_id, *info) {
                node.tree_dirty.notify_one();
                // le maître tient le répertoire des rôles de toute la grappe
                // (écrit à part de la tâche du réseau, et pas plus d'une fois toutes les deux
                // secondes par branche : un enfant bavard ne ralentit pas les autres)
                if node.providers.is_some() && node.store.roles_due(child_id) {
                    let subtree = node.children.lock().unwrap().get(child_id).map(|c| c.subtree.clone());
                    if let Some(subtree) = subtree {
                        let store = node.store.clone();
                        tokio::task::spawn_blocking(move || store.roles_seen(&subtree));
                    }
                }
            }
            // les événements arrivés avant l'arbre qui annonce leur auteur ; les autres sont jetés
            let known: Vec<_> = {
                let mut children = node.children.lock().unwrap();
                let Some(child) = children.get_mut(child_id) else { return replies };
                let early = std::mem::take(&mut child.early_events);
                early.into_iter().filter_map(|e| child.subtree.find(&e.agent_id).map(|n| (n.name.clone(), e))).collect()
            };
            for (name, event) in known {
                record_child_event(node, name, event);
            }
        }
        Msg::Report { kind, text } => {
            // un compte rendu n'est attendu que d'un enfant en mission, et en nombre limité
            let expected = {
                let mut children = node.children.lock().unwrap();
                let Some(child) = children.get_mut(child_id) else { return replies };
                let expected = child.mission_open && child.reports < MAX_REPORTS;
                if expected {
                    child.reports += 1;
                    if kind != ReportKind::Question {
                        child.mission_open = false;
                    }
                }
                expected
            };
            if expected {
                let text = crate::clean(&text, MAX_REPORT_CHARS, true);
                let _ = node.inbox.send(Input::Child { id: child_id.to_string(), name: child_name, kind, text });
            } else {
                node.emit("link", format!("compte rendu inattendu de {child_name}, ignoré"));
            }
        }
        Msg::Activity { event } => {
            // seul un agent de cette branche peut en être l'auteur, sous le nom qu'on lui connaît
            let author = {
                let mut children = node.children.lock().unwrap();
                let Some(child) = children.get_mut(child_id) else { return replies };
                let author = child.subtree.find(&event.agent_id).map(|n| n.name.clone());
                if author.is_none() && child.early_events.len() < MAX_EARLY_EVENTS {
                    // un descendant tout juste créé : l'arbre qui l'annonce n'est pas encore arrivé
                    child.early_events.push(event.clone());
                }
                author
            };
            if let Some(name) = author {
                record_child_event(node, name, event);
            }
        }
        Msg::ModelRequest { request_id, origin, request } => {
            if request_id.len() > 64 {
                return replies;
            }
            let origin = crate::clean(&origin, 64, false);
            let node = node.clone();
            let child_id = child_id.to_string();
            tokio::spawn(async move {
                let msg = match node.complete_for_child(&child_id, &origin, *request).await {
                    Ok(response) => Msg::ModelResponse { request_id, response: Some(Box::new(response)), error: None },
                    Err(error) => Msg::ModelResponse { request_id, response: None, error: Some(error) },
                };
                let _ = node.send_child(&child_id, msg, false);
            });
        }
        Msg::ServiceRequest { request_id, origin, request } => {
            if request_id.len() > 64 {
                return replies;
            }
            let origin = crate::clean(&origin, 64, false);
            let node = node.clone();
            let child_id = child_id.to_string();
            tokio::spawn(async move {
                let msg = match node.service_for_child(&child_id, &origin, *request).await {
                    Ok(response) => Msg::ServiceResponse { request_id, response: Some(Box::new(response)), error: None, spent: true },
                    Err(fail) => Msg::ServiceResponse { request_id, response: None, error: Some(fail.reason), spent: fail.spent },
                };
                let _ = node.send_child(&child_id, msg, false);
            });
        }
        Msg::SecretRequest { request_id, ask } => {
            if request_id.len() > 64 {
                return replies;
            }
            let node = node.clone();
            let child_id = child_id.to_string();
            tokio::spawn(async move {
                let msg = match node.secret_for_child(&child_id, *ask).await {
                    Ok(sealed) => Msg::SecretResponse { request_id, sealed: Some(sealed), error: None },
                    Err(error) => Msg::SecretResponse { request_id, sealed: None, error: Some(error) },
                };
                let _ = node.send_child(&child_id, msg, false);
            });
        }
        _ => {}
    }
    replies
}

/// Surveille les enfants : process terminé, ou silence trop long.
pub async fn run_child_monitor(node: Arc<Node>) {
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    loop {
        tick.tick().await;
        let mut gone: Vec<(String, String, &str)> = Vec::new();
        {
            let mut children = node.children.lock().unwrap();
            for child in children.values_mut().filter(|c| !c.stopped) {
                let mut exited = child.process.as_mut().is_some_and(|p| matches!(p.try_wait(), Ok(Some(_))));
                // par ssh, ce process n'est que la session qui a lancé l'agent : sa fin ne dit rien de lui
                if exited && child.target.as_ref().is_some_and(|t| t.kind == "ssh") && child.ever_connected {
                    child.process = None;
                    exited = false;
                }
                let silent = child.conn.is_none() && child.last_seen.elapsed() >= node.timing.die_after();
                if exited {
                    gone.push((child.id.clone(), child.name.clone(), "son process s'est terminé"));
                } else if silent {
                    gone.push((child.id.clone(), child.name.clone(), "injoignable depuis trop longtemps"));
                }
            }
        }
        for (id, name, why) in gone {
            node.stop_child(&id, why);
            let _ = node.inbox.send(Input::System(format!("L'agent enfant « {name} » ({id}) est perdu : {why}.")));
        }
    }
}

fn encode(frame: &Frame) -> String {
    serde_json::to_string(frame).expect("une trame se sérialise toujours")
}

/// Décode une trame d'un correspondant reconnu. Le journal ne reprend rien de son contenu.
fn decode(text: &str) -> Option<Frame> {
    let frame = decode_quiet(text);
    if frame.is_none() {
        eprintln!("trame illisible, ignorée ({} octets)", text.len());
    }
    frame
}

/// Décode une trame d'un inconnu : sans rien écrire nulle part.
fn decode_quiet(text: &str) -> Option<Frame> {
    serde_json::from_str(text).ok()
}
