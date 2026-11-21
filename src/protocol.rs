//! Messages échangés sur le lien entre un enfant et son parent (JSON sur WebSocket).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::model::{ModelRequest, ModelResponse, ServiceRequest, ServiceResponse};
use crate::policy::Capabilities;

/// Une trame. `seq` est présent sur les messages qui ne doivent pas se perdre : le
/// destinataire les acquitte, et l'émetteur les renvoie après une reconnexion.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Frame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    pub msg: Msg,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Msg {
    // --- enfant → parent
    /// Première trame de toute connexion : l'enfant s'annonce, avec un aléa. Le secret partagé
    /// ne circule jamais : chacun prouve qu'il le détient (voir `Challenge` et `Proof`).
    Hello { agent_id: String, nonce: String },
    /// L'enfant prouve qui il est, puis décrit son état.
    Proof { proof: String, node: Box<NodeInfo> },
    /// À la place de `Proof`, au tout premier démarrage : l'enfant prouve qui il est et demande
    /// la clé de sa configuration scellée. Le parent répond `Key`, une seule fois, puis raccroche.
    Unseal { proof: String },
    /// L'état de l'enfant et de toute sa descendance.
    Tree { node: Box<NodeInfo> },
    /// Ce que l'enfant dit à son parent : fin de mission, question, erreur.
    Report { kind: ReportKind, text: String },
    /// Journal d'activité, relayé jusqu'au maître.
    Activity { event: ActivityEvent },
    /// Appel de modèle, relayé jusqu'au maître, seul à détenir les clés.
    ModelRequest { request_id: String, origin: String, request: Box<ModelRequest> },
    /// Appel d'un modèle de service (transcription, voix, image, plongements, prévision,
    /// décision), relayé de même. Chaque parent revérifie le droit, la taille et le quota.
    ServiceRequest { request_id: String, origin: String, request: Box<ServiceRequest> },

    // --- parent → enfant
    /// Le parent prouve qui il est, et met l'enfant au défi avec son propre aléa.
    Challenge { nonce: String, proof: String },
    Welcome,
    /// La clé de la configuration scellée de l'enfant.
    Key { key: String },
    Rejected { reason: String },
    Instruction { text: String },
    ModelResponse {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<Box<ModelResponse>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    ServiceResponse {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<Box<ServiceResponse>>,
        /// Sans réponse : la demande est-elle quand même partie chez le fournisseur ?
        #[serde(default)]
        spent: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Ordre d'arrêt à faire suivre jusqu'à un descendant.
    StopDescendant { target: String },
    /// Arrêt de l'enfant et de toute sa descendance.
    Shutdown { reason: String },

    // --- les deux sens
    Ping,
    Pong,
    Ack { seq: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReportKind {
    /// Mission accomplie : le critère de réussite est atteint, preuves à l'appui.
    Achieved,
    /// Mission impossible en l'état : l'agent s'arrête et attend de nouvelles consignes.
    Impossible,
    /// L'agent a besoin d'une information ou d'une décision pour continuer.
    Question,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Starting,
    Idle,
    Working,
    /// Parent injoignable : l'agent ne fait plus rien en attendant.
    Suspended,
    /// Vu du parent : l'enfant ne répond plus.
    Disconnected,
    Stopped,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NodeInfo {
    pub id: String,
    pub name: String,
    pub role: String,
    pub parent_id: Option<String>,
    pub depth: u32,
    pub status: Status,
    pub model: String,
    /// Mode de déploiement : `root` ou `local` pour l'instant.
    pub target: String,
    pub host: String,
    pub os: String,
    pub ips: Vec<String>,
    pub pid: u32,
    pub started_at_ms: u64,
    pub last_seen_ms: u64,
    pub tokens_used: u64,
    /// Appels aux modèles de service, par nature, descendance comprise.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub calls_used: std::collections::BTreeMap<String, u64>,
    pub capabilities: Capabilities,
    /// Posé par celui qui reçoit l'arbre, jamais cru de celui qui l'envoie : ce que cet agent
    /// déclare (droits, modèle) dépasse ce que son parent déclare pouvoir accorder.
    #[serde(default)]
    pub unverified: bool,
    pub children: Vec<NodeInfo>,
}

impl NodeInfo {
    pub fn contains(&self, id: &str) -> bool {
        self.find(id).is_some()
    }

    pub fn find(&self, id: &str) -> Option<&NodeInfo> {
        if self.id == id {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(id))
    }

    /// Les identifiants de toute la descendance (sans celui de l'agent lui-même).
    pub fn descendant_ids<'a>(&'a self, out: &mut Vec<&'a str>) {
        for child in &self.children {
            out.push(&child.id);
            child.descendant_ids(out);
        }
    }

    /// Nombre d'agents et profondeur de l'arbre, s'il tient dans les limites.
    pub fn within(&self, max_nodes: usize, max_depth: usize) -> bool {
        fn walk(node: &NodeInfo, depth: usize, left: &mut usize) -> bool {
            if depth == 0 || *left == 0 {
                return false;
            }
            *left -= 1;
            node.children.iter().all(|c| walk(c, depth - 1, left))
        }
        let mut left = max_nodes;
        walk(self, max_depth, &mut left)
    }

    /// Un arbre déclaré par un enfant n'est pas cru sur parole : textes nettoyés et bornés,
    /// profondeur et parenté recalculées.
    pub fn tidy(&mut self) {
        self.id = crate::clean(&self.id, 64, false);
        self.name = crate::clean(&self.name, 80, false);
        self.role = crate::clean(&self.role, 200, false);
        self.model = crate::clean(&self.model, 80, false);
        self.target = crate::clean(&self.target, 40, false);
        self.host = crate::clean(&self.host, 255, false);
        self.os = crate::clean(&self.os, 80, false);
        self.ips.truncate(8);
        for ip in &mut self.ips {
            *ip = crate::clean(ip, 64, false);
        }
        for child in &mut self.children {
            child.parent_id = Some(self.id.clone());
            child.depth = self.depth.saturating_add(1);
            child.capabilities.tidy_declared();
            child.calls_used.retain(|nature, _| crate::policy::counted_nature(nature));
            child.tidy();
        }
    }

    /// Les droits déclarés par la descendance tiennent-ils dans une taille raisonnable ?
    /// (Ceux de l'agent lui-même sont connus de son parent, qui les a accordés.)
    pub fn declared_within(&self, max_bytes: usize) -> bool {
        self.children.iter().all(|c| c.capabilities.declared_size() <= max_bytes && c.declared_within(max_bytes))
    }

    /// Compare, dans toute la descendance, ce que chaque agent déclare à ce que son parent
    /// déclare pouvoir accorder. Ce n'est pas une preuve (tout est déclaré par la branche),
    /// mais une incohérence est signalée, et suit la descendance.
    pub fn mark_unverified(&mut self) {
        for child in &mut self.children {
            let coherent = self.capabilities.check_grant(&child.capabilities).is_ok()
                && self.capabilities.models.contains(&child.model);
            child.unverified = self.unverified || !coherent;
            child.mark_unverified();
        }
    }

    /// Marque l'agent et toute sa descendance avec le même état.
    pub fn set_status_deep(&mut self, status: Status) {
        self.status = status;
        for child in &mut self.children {
            child.set_status_deep(status);
        }
    }
}

/// Les genres d'événements du journal.
pub const EVENT_KINDS: [&str; 9] = ["status", "input", "tool", "tool_error", "spawn", "report", "message", "error", "link"];

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// Numéro d'ordre, attribué par le maître.
    #[serde(default)]
    pub id: u64,
    pub agent_id: String,
    pub agent_name: String,
    pub ts_ms: u64,
    /// Un des `EVENT_KINDS` ; tout autre genre, venu d'un enfant, devient `autre`.
    pub kind: String,
    pub text: String,
}

/// File des messages à ne pas perdre, côté émetteur.
#[derive(Default)]
pub struct Outbox {
    next_seq: u64,
    pending: VecDeque<Frame>,
}

/// Au-delà, les messages les plus anciens d'un lien coupé sont abandonnés.
const OUTBOX_LIMIT: usize = 5000;

impl Outbox {
    pub fn push(&mut self, msg: Msg) -> Frame {
        self.next_seq += 1;
        let frame = Frame { seq: Some(self.next_seq), msg };
        if self.pending.len() >= OUTBOX_LIMIT {
            self.pending.pop_front();
        }
        self.pending.push_back(frame.clone());
        frame
    }

    pub fn ack(&mut self, seq: u64) {
        while self.pending.front().is_some_and(|f| f.seq.unwrap_or(0) <= seq) {
            self.pending.pop_front();
        }
    }

    pub fn pending(&self) -> Vec<Frame> {
        self.pending.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip_as_tagged_json() {
        let frame = Frame { seq: Some(7), msg: Msg::Instruction { text: "bonjour".into() } };
        let json = serde_json::to_string(&frame).unwrap();
        assert_eq!(json, r#"{"seq":7,"msg":{"type":"instruction","text":"bonjour"}}"#);
        let back: Frame = serde_json::from_str(&json).unwrap();
        assert_eq!(back.seq, Some(7));
        assert!(matches!(back.msg, Msg::Instruction { text } if text == "bonjour"));
        let request = crate::model::ModelRequest {
            model: "main".into(),
            system: "s".into(),
            messages: vec![crate::model::Message::user(vec![crate::model::Block::Text { text: "t".into() }])],
            tools: vec![crate::model::ToolSpec { name: "n".into(), description: "d".into(), parameters: serde_json::json!({}) }],
        };
        let frame = Frame { seq: None, msg: Msg::ModelRequest { request_id: "r".into(), origin: "o".into(), request: Box::new(request) } };
        let json = serde_json::to_string(&frame).unwrap();
        let back: Frame = serde_json::from_str(&json).unwrap_or_else(|e| panic!("{e} : {json}"));
        assert!(matches!(back.msg, Msg::ModelRequest { .. }));
        let response = crate::model::ModelResponse {
            message: crate::model::Message {
                role: crate::model::Role::Assistant,
                content: vec![crate::model::Block::ToolUse { id: "c".into(), name: "n".into(), input: serde_json::json!({ "a": 1 }) }],
                raw: Some(serde_json::json!({ "role": "assistant", "content": null })),
            },
            usage: Default::default(),
        };
        let frame = Frame { seq: None, msg: Msg::ModelResponse { request_id: "r".into(), response: Some(Box::new(response)), error: None } };
        let back: Frame = serde_json::from_str(&serde_json::to_string(&frame).unwrap()).unwrap();
        assert!(matches!(back.msg, Msg::ModelResponse { response: Some(_), .. }));
        let ack: Frame = serde_json::from_str(&serde_json::to_string(&Frame { seq: None, msg: Msg::Ack { seq: 3 } }).unwrap()).unwrap();
        assert!(matches!(ack.msg, Msg::Ack { seq: 3 }));
        let ping: Frame = serde_json::from_str(r#"{"msg":{"type":"ping"}}"#).unwrap();
        assert!(ping.seq.is_none() && matches!(ping.msg, Msg::Ping));
    }

    #[test]
    fn outbox_keeps_unacknowledged_frames() {
        let mut outbox = Outbox::default();
        for text in ["a", "b", "c"] {
            outbox.push(Msg::Instruction { text: text.into() });
        }
        outbox.ack(2);
        let left = outbox.pending();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].seq, Some(3));
    }
}
