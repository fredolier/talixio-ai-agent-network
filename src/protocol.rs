//! Messages échangés sur le lien entre un enfant et son parent (JSON sur WebSocket).

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::model::{ModelRequest, ModelResponse};
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
    /// Première trame de toute connexion : l'enfant prouve qui il est.
    Hello { agent_id: String, token: String, node: Box<NodeInfo> },
    /// L'état de l'enfant et de toute sa descendance.
    Tree { node: Box<NodeInfo> },
    /// Ce que l'enfant dit à son parent : fin de mission, question, erreur.
    Report { kind: ReportKind, text: String },
    /// Journal d'activité, relayé jusqu'au maître.
    Activity { event: ActivityEvent },
    /// Appel de modèle, relayé jusqu'au maître, seul à détenir les clés.
    ModelRequest { request_id: String, origin: String, request: Box<ModelRequest> },

    // --- parent → enfant
    Welcome,
    Rejected { reason: String },
    Instruction { text: String },
    ModelResponse {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        response: Option<Box<ModelResponse>>,
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
    pub capabilities: Capabilities,
    pub children: Vec<NodeInfo>,
}

impl NodeInfo {
    pub fn contains(&self, id: &str) -> bool {
        self.id == id || self.children.iter().any(|c| c.contains(id))
    }

    /// Marque l'agent et toute sa descendance avec le même état.
    pub fn set_status_deep(&mut self, status: Status) {
        self.status = status;
        for child in &mut self.children {
            child.set_status_deep(status);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivityEvent {
    /// Numéro d'ordre, attribué par le maître.
    #[serde(default)]
    pub id: u64,
    pub agent_id: String,
    pub agent_name: String,
    pub ts_ms: u64,
    /// `status`, `input`, `tool`, `tool_error`, `spawn`, `report`, `message`, `error`, `link`.
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
