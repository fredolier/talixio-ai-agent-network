//! Le lien entre un enfant et son parent.
//!
//! L'enfant se connecte au parent (WebSocket) et prouve qui il est par son jeton. Chaque
//! côté surveille l'autre par des battements. Sans parent, l'enfant se met en veille ;
//! si le parent ne revient pas, il s'éteint avec toute sa descendance.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::ws::{Message as WsMessage, WebSocket};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_tungstenite::tungstenite::Message as TMessage;

use crate::model::{ModelRequest, ModelResponse};
use crate::node::{Input, Node};
use crate::protocol::{Frame, Msg, Outbox, ReportKind, Status};
use crate::{now_ms, random_hex, secret_eq};

const HANDSHAKE: Duration = Duration::from_secs(10);
/// Taille maximale d'une trame : une conversation entière y passe à chaque appel de modèle.
pub const MAX_FRAME: usize = 64 * 1024 * 1024;

// ---------------------------------------------------------------------------------------
// Côté enfant
// ---------------------------------------------------------------------------------------

type ModelReply = Result<ModelResponse, String>;

/// Le lien d'un enfant vers son parent.
pub struct Uplink {
    out: Mutex<UpOut>,
    connected: watch::Sender<bool>,
    pending: Mutex<HashMap<String, oneshot::Sender<ModelReply>>>,
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

    fn fail_pending(&self) {
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
pub async fn run_uplink(node: Arc<Node>, url: String, token: String) {
    let uplink = node.uplink.clone().expect("un enfant a un lien vers son parent");
    let mut lost_since = Instant::now();
    let mut backoff = Duration::from_millis(250);
    loop {
        match session(&node, &uplink, &url, &token).await {
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

async fn session(node: &Arc<Node>, uplink: &Arc<Uplink>, url: &str, token: &str) -> Ended {
    let lost = Ended::Lost { was_connected: false };
    let config = tokio_tungstenite::tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME))
        .max_frame_size(Some(MAX_FRAME));
    let connect = tokio_tungstenite::connect_async_with_config(url, Some(config), false);
    let Ok(Ok((ws, _))) = tokio::time::timeout(HANDSHAKE, connect).await else { return lost };
    let (mut sink, mut stream) = ws.split();

    let hello = Msg::Hello { agent_id: node.id.clone(), token: token.to_string(), node: Box::new(node.snapshot()) };
    if sink.send(TMessage::Text(encode(&Frame { seq: None, msg: hello }).into())).await.is_err() {
        return lost;
    }
    match tokio::time::timeout(HANDSHAKE, stream.next()).await {
        Ok(Some(Ok(TMessage::Text(text)))) => match decode(text.as_str()).map(|f| f.msg) {
            Some(Msg::Welcome) => {}
            Some(Msg::Rejected { reason }) => return Ended::Dismissed(format!("refusé par le parent : {reason}")),
            _ => return lost,
        },
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
            if sink.send(TMessage::Text(encode(&frame).into())).await.is_err() {
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
                        if sink.send(TMessage::Text(encode(&reply).into())).await.is_err() {
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
                if sink.send(TMessage::Text(encode(&frame).into())).await.is_err() {
                    break;
                }
            }
            _ = tick.tick() => {
                if last_rx.elapsed() >= node.timing.suspend_after() {
                    break;
                }
                if sink.send(TMessage::Text(encode(&Frame { seq: None, msg: Msg::Ping }).into())).await.is_err() {
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

/// Sert la connexion d'un enfant, du premier message à la coupure.
pub async fn serve_child(node: Arc<Node>, socket: WebSocket) {
    let (mut sink, mut stream) = socket.split();
    let reject = |reason: &str| {
        WsMessage::Text(encode(&Frame { seq: None, msg: Msg::Rejected { reason: reason.to_string() } }).into())
    };

    // la première trame doit prouver l'identité de l'enfant
    let first = match tokio::time::timeout(HANDSHAKE, stream.next()).await {
        Ok(Some(Ok(WsMessage::Text(text)))) => decode(text.as_str()),
        _ => None,
    };
    let Some(Frame { msg: Msg::Hello { agent_id, token, node: info }, .. }) = first else {
        let _ = sink.send(reject("présentation attendue")).await;
        return;
    };
    let (tx, mut rx) = mpsc::unbounded_channel::<Frame>();
    let conn_id = CONNECTION.fetch_add(1, Ordering::Relaxed);
    let accepted = {
        let mut children = node.children.lock().unwrap();
        match children.get_mut(&agent_id) {
            Some(child) if !child.stopped && secret_eq(&child.token, &token) && info.id == agent_id => {
                child.conn = Some((conn_id, tx));
                child.ever_connected = true;
                child.last_seen = Instant::now();
                child.last_seen_ms = now_ms();
                child.subtree = sanitize(*info, child);
                Some((child.outbox.pending(), child.name.clone()))
            }
            _ => None,
        }
    };
    let Some((backlog, child_name)) = accepted else {
        let _ = sink.send(reject("agent inconnu, arrêté, ou jeton invalide")).await;
        return;
    };
    node.tree_dirty.notify_one();
    node.emit("link", format!("agent {child_name} connecté"));
    if sink.send(WsMessage::Text(encode(&Frame { seq: None, msg: Msg::Welcome }).into())).await.is_err() {
        return;
    }

    let mut last_rx = Instant::now();
    let mut tick = tokio::time::interval(node.timing.heartbeat());
    let mut backlog = backlog.into_iter();
    'link: loop {
        if let Some(frame) = backlog.next() {
            if sink.send(WsMessage::Text(encode(&frame).into())).await.is_err() {
                break;
            }
            continue;
        }
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(WsMessage::Text(text))) => {
                    last_rx = Instant::now();
                    let Some(frame) = decode(text.as_str()) else { continue };
                    for reply in parent_receive(&node, &agent_id, frame) {
                        if sink.send(WsMessage::Text(encode(&reply).into())).await.is_err() {
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
                    if sink.send(WsMessage::Text(encode(&frame).into())).await.is_err() || closing {
                        break;
                    }
                }
                None => break, // l'enfant a été arrêté, ou une connexion plus récente l'a remplacée
            },
            _ = tick.tick() => {
                if last_rx.elapsed() >= node.timing.suspend_after() {
                    break;
                }
                if sink.send(WsMessage::Text(encode(&Frame { seq: None, msg: Msg::Ping }).into())).await.is_err() {
                    break;
                }
            }
        }
    }
    let _ = sink.close().await;
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

/// Ce qu'un enfant déclare sur lui-même n'est pas cru sur parole : son identité et ses
/// droits sont ceux que le parent lui a donnés.
fn sanitize(mut info: crate::protocol::NodeInfo, child: &crate::node::ChildRec) -> crate::protocol::NodeInfo {
    info.id = child.id.clone();
    info.name = child.name.clone();
    info.role = child.subtree.role.clone();
    info.parent_id = child.subtree.parent_id.clone();
    info.depth = child.subtree.depth;
    info.model = child.subtree.model.clone();
    info.target = child.subtree.target.clone();
    info.capabilities = child.caps.clone();
    info
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
            if let Some(child) = node.children.lock().unwrap().get_mut(child_id) {
                child.subtree = sanitize(*info, child);
            }
            node.tree_dirty.notify_one();
        }
        Msg::Report { kind, text } => {
            if matches!(kind, ReportKind::Achieved | ReportKind::Impossible) {
                if let Some(child) = node.children.lock().unwrap().get_mut(child_id) {
                    child.mission_open = false;
                }
            }
            let _ = node.inbox.send(Input::Child { id: child_id.to_string(), name: child_name, kind, text });
        }
        Msg::Activity { event } => {
            // seul un agent de cette branche peut en être l'auteur
            let known = {
                let children = node.children.lock().unwrap();
                children.get(child_id).is_some_and(|c| c.subtree.contains(&event.agent_id))
            };
            if known {
                node.record(event);
            }
        }
        Msg::ModelRequest { request_id, origin, request } => {
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
                let exited = child.process.as_mut().is_some_and(|p| matches!(p.try_wait(), Ok(Some(_))));
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

fn decode(text: &str) -> Option<Frame> {
    match serde_json::from_str(text) {
        Ok(frame) => Some(frame),
        Err(e) => {
            eprintln!("trame illisible, ignorée : {e}");
            None
        }
    }
}
