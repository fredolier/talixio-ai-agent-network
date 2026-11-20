//! OpenAI et tout serveur compatible (Gemini, Mistral, Ollama, vLLM, OpenRouter…) : seule l'adresse change.

use anyhow::{bail, Context, Result};
use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{json, Value};

use super::{post_json, Block, Message, ModelRequest, ModelResponse, Role, Usage};
use crate::config::ModelConfig;

pub struct OpenAi {
    http: reqwest::Client,
    url: String,
    headers: HeaderMap,
    model: String,
    effort: Option<String>,
}

impl OpenAi {
    pub fn new(http: reqwest::Client, cfg: &ModelConfig, key: String) -> Result<Self> {
        let mut headers = HeaderMap::new();
        // les serveurs locaux n'attendent pas de clé
        let bearer = if key.trim().is_empty() { "none" } else { key.trim() };
        let mut auth = HeaderValue::from_str(&format!("Bearer {bearer}")).context("clé d'API invalide")?;
        auth.set_sensitive(true);
        headers.insert("authorization", auth);
        let base = cfg.base_url.as_deref().unwrap_or("https://api.openai.com/v1").trim_end_matches('/');
        // au-delà de « high », ces API n'ont pas d'équivalent
        let effort = cfg.effort.as_deref().map(|e| match e {
            "xhigh" | "max" => "high".to_string(),
            other => other.to_string(),
        });
        Ok(Self { http, url: format!("{base}/chat/completions"), headers, model: cfg.model.clone(), effort })
    }

    pub async fn complete(&self, request: &ModelRequest) -> Result<ModelResponse> {
        let mut body = json!({ "model": self.model, "messages": wire_messages(request) });
        if !request.tools.is_empty() {
            body["tools"] = request
                .tools
                .iter()
                .map(|t| json!({
                    "type": "function",
                    "function": { "name": t.name, "description": t.description, "parameters": t.parameters },
                }))
                .collect();
        }
        if let Some(effort) = &self.effort {
            body["reasoning_effort"] = json!(effort);
        }
        let response = match post_json(&self.http, &self.url, &self.headers, &body).await {
            // un modèle qui refuse ce niveau d'effort (ou tout niveau) : son réglage par défaut
            Err(e) if self.effort.is_some() && e.to_string().contains("reasoning_effort") => {
                body.as_object_mut().map(|o| o.remove("reasoning_effort"));
                post_json(&self.http, &self.url, &self.headers, &body).await?
            }
            other => other?,
        };
        parse(response, &body)
    }
}

fn wire_messages(request: &ModelRequest) -> Vec<Value> {
    let mut out = vec![json!({ "role": "system", "content": request.system })];
    for message in &request.messages {
        if message.role == Role::Assistant {
            if let Some(raw) = &message.raw {
                out.push(raw.clone()); // tel que reçu : certains serveurs y joignent des champs à renvoyer
                continue;
            }
            let calls: Vec<Value> = message
                .tool_uses()
                .into_iter()
                .map(|(id, name, input)| json!({
                    "id": id, "type": "function",
                    "function": { "name": name, "arguments": input.to_string() },
                }))
                .collect();
            let mut wire = json!({ "role": "assistant", "content": message.text() });
            if !calls.is_empty() {
                wire["tool_calls"] = json!(calls);
            }
            out.push(wire);
            continue;
        }
        // les résultats d'outils d'abord, chacun dans son message ; le texte ensuite
        for block in &message.content {
            if let Block::ToolResult { tool_use_id, content, .. } = block {
                out.push(json!({ "role": "tool", "tool_call_id": tool_use_id, "content": content }));
            }
        }
        let text = message.text();
        if !text.is_empty() {
            out.push(json!({ "role": "user", "content": text }));
        }
    }
    out
}

fn parse(response: Value, sent: &Value) -> Result<ModelResponse> {
    let raw = response["choices"][0]["message"].clone();
    if !raw.is_object() {
        bail!("réponse sans message : {}", response.to_string().chars().take(300).collect::<String>());
    }
    let mut content = Vec::new();
    let text = text_of(&raw["content"]);
    if !text.is_empty() {
        content.push(Block::Text { text });
    }
    for call in raw["tool_calls"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let arguments = call["function"]["arguments"].as_str().unwrap_or("{}");
        let input = if arguments.trim().is_empty() {
            json!({})
        } else {
            // des arguments illisibles deviennent `null` : l'outil les refusera et le dira au modèle
            serde_json::from_str(arguments).unwrap_or(Value::Null)
        };
        content.push(Block::ToolUse {
            id: call["id"].as_str().unwrap_or("").to_string(),
            name: call["function"]["name"].as_str().unwrap_or("").to_string(),
            input,
        });
    }
    let usage = &response["usage"];
    let usage = if usage.is_object() {
        let cached = usage["prompt_tokens_details"]["cached_tokens"].as_u64().unwrap_or(0);
        Usage {
            input: usage["prompt_tokens"].as_u64().unwrap_or(0).saturating_sub(cached),
            output: usage["completion_tokens"].as_u64().unwrap_or(0),
            cache_read: cached,
            cache_write: 0,
        }
    } else {
        // certains serveurs ne comptent pas : estimation (~4 caractères par token)
        Usage { input: sent.to_string().len() as u64 / 4, output: raw.to_string().len() as u64 / 4, ..Default::default() }
    };
    Ok(ModelResponse { message: Message { role: Role::Assistant, content, raw: Some(raw) }, usage })
}

/// Le contenu en texte : certains serveurs envoient une liste de morceaux, raisonnement compris.
fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| match p {
                Value::String(s) => Some(s.as_str()),
                p if p["type"] == "text" => p["text"].as_str(),
                _ => None,
            })
            .collect(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::ToolSpec;

    #[test]
    fn tool_results_become_tool_messages() {
        let request = ModelRequest {
            model: "m".into(),
            system: "sys".into(),
            tools: Vec::<ToolSpec>::new(),
            messages: vec![
                Message::user(vec![Block::Text { text: "lis".into() }]),
                Message {
                    role: Role::Assistant,
                    raw: None,
                    content: vec![Block::ToolUse { id: "c1".into(), name: "read_file".into(), input: json!({ "path": "/a" }) }],
                },
                Message::user(vec![Block::ToolResult { tool_use_id: "c1".into(), content: "contenu".into(), is_error: false }]),
            ],
        };
        let wire = wire_messages(&request);
        assert_eq!(wire[0]["role"], "system");
        assert_eq!(wire[2]["tool_calls"][0]["function"]["arguments"], r#"{"path":"/a"}"#);
        assert_eq!(wire[3], json!({ "role": "tool", "tool_call_id": "c1", "content": "contenu" }));
    }

    #[test]
    fn parses_tool_calls_and_chunked_text() {
        let response = json!({
            "choices": [{ "message": {
                "role": "assistant",
                "content": [{ "type": "thinking", "thinking": "…" }, { "type": "text", "text": "ok" }],
                "tool_calls": [
                    { "id": "c1", "type": "function", "function": { "name": "list_dir", "arguments": "{\"path\":\"/\"}" } },
                    { "id": "c2", "type": "function", "function": { "name": "list_dir", "arguments": "{oops" } }
                ]
            } }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 4, "prompt_tokens_details": { "cached_tokens": 2 } }
        });
        let parsed = parse(response, &json!({})).unwrap();
        assert_eq!(parsed.message.text(), "ok");
        let uses = parsed.message.tool_uses();
        assert_eq!(uses[0].2, json!({ "path": "/" }));
        assert!(uses[1].2.is_null());
        assert_eq!(parsed.usage.total(), 16);
    }
}
