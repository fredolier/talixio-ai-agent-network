//! Claude, par l'API Messages d'Anthropic.

use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{json, Value};

use super::{post_json, Block, Message, ModelRequest, ModelResponse, Role, Usage};
use crate::config::ModelConfig;

const API_VERSION: &str = "2026-10-21";
const FALLBACK_BETA: &str = "server-side-fallback-2026-07-01";
/// Modèles dont les filtres de sécurité peuvent décliner une requête, et qui acceptent
/// qu'un autre modèle prenne alors le relais côté serveur.
const FALLBACK_MODELS: [&str; 5] =
    ["claude-fable-5-2", "claude-fable-5-1", "claude-opus-5-5", "claude-opus-5", "claude-sonnet-5-5"];

pub struct Anthropic {
    http: reqwest::Client,
    url: String,
    headers: HeaderMap,
    model: String,
    effort: Option<String>,
    max_tokens: u64,
    fallback: bool,
}

impl Anthropic {
    pub fn new(http: reqwest::Client, cfg: &ModelConfig, key: String) -> Result<Self> {
        if key.is_empty() {
            bail!("modèle Anthropic « {} » : clé d'API manquante", cfg.model);
        }
        let fallback = cfg.refusal_fallback.unwrap_or(true) && FALLBACK_MODELS.contains(&cfg.model.as_str());
        let mut headers = HeaderMap::new();
        let mut key = HeaderValue::from_str(key.trim()).context("clé d'API invalide")?;
        key.set_sensitive(true);
        headers.insert("x-api-key", key);
        headers.insert("anthropic-version", HeaderValue::from_static(API_VERSION));
        if fallback {
            headers.insert("anthropic-beta", HeaderValue::from_static(FALLBACK_BETA));
        }
        let base = cfg.base_url.as_deref().unwrap_or("https://api.anthropic.com").trim_end_matches('/');
        Ok(Self {
            http,
            url: format!("{base}/v1/messages"),
            headers,
            model: cfg.model.clone(),
            effort: cfg.effort.clone(),
            max_tokens: cfg.max_tokens.unwrap_or(16000),
            fallback,
        })
    }

    pub async fn complete(&self, request: &ModelRequest) -> Result<ModelResponse> {
        let body = self.body(request);
        let response = post_json(&self.http, &self.url, &self.headers, &body).await?;
        parse(response)
    }

    fn body(&self, request: &ModelRequest) -> Value {
        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "system": [{ "type": "text", "text": request.system }],
            "messages": request.messages.iter().map(wire_message).collect::<Vec<_>>(),
            // la conversation relue à chaque étape est facturée au tarif du cache
            "cache_control": { "type": "ephemeral" },
        });
        if !request.tools.is_empty() {
            body["tools"] = request
                .tools
                .iter()
                .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters }))
                .collect();
        }
        if let Some(effort) = &self.effort {
            body["output_config"] = json!({ "effort": effort });
        }
        if self.fallback {
            body["fallbacks"] = json!("default");
        }
        body
    }
}

fn wire_message(message: &Message) -> Value {
    if message.role == Role::Assistant {
        // tel que reçu : les blocs de raisonnement doivent revenir inchangés
        if let Some(raw) = &message.raw {
            return json!({ "role": "assistant", "content": raw });
        }
    }
    let content: Vec<Value> = message
        .content
        .iter()
        .map(|block| match block {
            Block::Text { text } => json!({ "type": "text", "text": text }),
            Block::Image { media_type, data } => {
                json!({ "type": "image", "source": { "type": "base64", "media_type": media_type, "data": data } })
            }
            Block::ToolUse { id, name, input } => {
                json!({ "type": "tool_use", "id": id, "name": name, "input": input })
            }
            Block::ToolResult { tool_use_id, content, is_error } => json!({
                "type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": is_error,
            }),
        })
        .collect();
    let role = if message.role == Role::User { "user" } else { "assistant" };
    json!({ "role": role, "content": content })
}

fn parse(response: Value) -> Result<ModelResponse> {
    let stop = response["stop_reason"].as_str().unwrap_or("");
    if stop == "refusal" {
        let category = response["stop_details"]["category"].as_str().unwrap_or("non précisée");
        bail!("le modèle a décliné la requête (catégorie : {category})");
    }
    let raw = response["content"].clone();
    let mut content = Vec::new();
    for block in raw.as_array().map(Vec::as_slice).unwrap_or_default() {
        match block["type"].as_str() {
            Some("text") => content.push(Block::Text { text: block["text"].as_str().unwrap_or("").to_string() }),
            Some("tool_use") => content.push(Block::ToolUse {
                id: block["id"].as_str().unwrap_or("").to_string(),
                name: block["name"].as_str().unwrap_or("").to_string(),
                input: block["input"].clone(),
            }),
            _ => {} // raisonnement, relais de modèle : gardés dans `raw`
        }
    }
    if stop == "max_tokens" && content.iter().any(|b| matches!(b, Block::ToolUse { .. })) {
        bail!("réponse du modèle tronquée au milieu d'un appel d'outil (max_tokens atteint)");
    }
    let n = |key: &str| response["usage"][key].as_u64().unwrap_or(0);
    Ok(ModelResponse {
        message: Message { role: Role::Assistant, content, raw: Some(raw) },
        usage: Usage {
            input: n("input_tokens"),
            output: n("output_tokens"),
            cache_read: n("cache_read_input_tokens"),
            cache_write: n("cache_creation_input_tokens"),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tool_use_and_keeps_raw_content() {
        let response = json!({
            "stop_reason": "tool_use",
            "content": [
                { "type": "thinking", "thinking": "", "signature": "sig" },
                { "type": "text", "text": "Je regarde." },
                { "type": "tool_use", "id": "toolu_1", "name": "read_file", "input": { "path": "/a" } }
            ],
            "usage": { "input_tokens": 10, "output_tokens": 5, "cache_read_input_tokens": 3 }
        });
        let parsed = parse(response).unwrap();
        assert_eq!(parsed.message.text(), "Je regarde.");
        assert_eq!(parsed.message.tool_uses().len(), 1);
        assert_eq!(parsed.usage.total(), 18);
        // au tour suivant, le message repart tel que reçu, raisonnement compris
        let wire = wire_message(&parsed.message);
        assert_eq!(wire["content"][0]["signature"], "sig");
    }

    #[test]
    fn refusal_is_an_error() {
        let response = json!({ "stop_reason": "refusal", "stop_details": { "category": "cyber" }, "content": [] });
        assert!(parse(response).unwrap_err().to_string().contains("cyber"));
    }
}
