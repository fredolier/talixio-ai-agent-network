//! Les modèles : un format neutre, et un connecteur par famille d'API.

mod anthropic;
mod openai;
pub mod service;

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::ModelConfig;

pub use anthropic::Anthropic;
pub use openai::OpenAi;
pub use service::{Nature, Service, ServiceError, ServiceFail, ServiceRequest, ServiceResponse};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelRequest {
    /// Alias du modèle dans la configuration du maître.
    pub model: String,
    pub system: String,
    pub messages: Vec<Message>,
    pub tools: Vec<ToolSpec>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelResponse {
    pub message: Message,
    pub usage: Usage,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<Block>,
    /// Le message tel que le fournisseur l'a renvoyé. Renvoyé à l'identique au même
    /// fournisseur : certains y gardent un état (raisonnement) qu'il ne faut pas perdre.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Block {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String, is_error: bool },
    /// Une image jointe à une question (`data` : en base64).
    Image { media_type: String, data: String },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// Schéma JSON des arguments.
    pub parameters: Value,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Usage {
    pub fn total(&self) -> u64 {
        self.input.saturating_add(self.output).saturating_add(self.cache_read).saturating_add(self.cache_write)
    }
}

impl Message {
    pub fn user(content: Vec<Block>) -> Self {
        Self { role: Role::User, content, raw: None }
    }

    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                Block::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }

    pub fn tool_uses(&self) -> Vec<(String, String, Value)> {
        self.content
            .iter()
            .filter_map(|b| match b {
                Block::ToolUse { id, name, input } => Some((id.clone(), name.clone(), input.clone())),
                _ => None,
            })
            .collect()
    }
}

/// Un connecteur vers un modèle précis.
pub enum Provider {
    Anthropic(Anthropic),
    OpenAi(OpenAi),
}

impl Provider {
    pub async fn complete(&self, request: &ModelRequest) -> Result<ModelResponse> {
        match self {
            Provider::Anthropic(p) => p.complete(request).await,
            Provider::OpenAi(p) => p.complete(request).await,
        }
    }
}

/// Les modèles du maître, par alias. C'est ici, et seulement ici, que vivent les clés.
pub struct Providers {
    by_alias: BTreeMap<String, Provider>,
    /// Les modèles qui ne tiennent pas une conversation.
    services: BTreeMap<String, Service>,
    /// Ce qu'on peut montrer de chaque modèle : jamais la clé, ni où elle est rangée.
    catalog: Vec<ModelInfo>,
}

/// Un modèle du maître tel que l'API le montre.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ModelInfo {
    pub alias: String,
    pub provider: String,
    pub model: String,
    /// Machine qui sert le modèle (sans chemin ni identifiants éventuels de l'adresse).
    pub host: String,
    pub effort: Option<String>,
    pub max_tokens: Option<u64>,
    /// Une clé est configurée (fichier ou variable d'environnement).
    pub has_key: bool,
    /// `llm`, `transcription`, `speech`, `image`, `embedding`, `forecast` ou `decision`.
    pub nature: String,
    /// `llm` : le modèle sait lire une image.
    pub vision: bool,
}

impl Providers {
    pub fn from_config(models: &BTreeMap<String, ModelConfig>) -> Result<Self> {
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(600))
            // jamais d'autre adresse que celle de la configuration : ni requête rejouée ailleurs, ni clé qui suit
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let mut by_alias = BTreeMap::new();
        let mut services = BTreeMap::new();
        let mut catalog = Vec::new();
        for (alias, cfg) in models {
            let key = cfg.api_key()?;
            let nature = match cfg.nature.as_deref() {
                None => Nature::Llm,
                Some(name) => Nature::parse(name).ok_or_else(|| anyhow!("modèle « {alias} » : nature inconnue « {name} »"))?,
            };
            if cfg.vision && nature != Nature::Llm {
                bail!("modèle « {alias} » : `vision` ne vaut que pour un modèle de nature llm");
            }
            let default_url = match cfg.provider.as_str() {
                "anthropic" => "https://api.anthropic.com",
                "elevenlabs" => "https://api.elevenlabs.io",
                _ => "https://api.openai.com",
            };
            let host = reqwest::Url::parse(cfg.base_url.as_deref().unwrap_or(default_url))
                .ok()
                .and_then(|u| u.host_str().map(|h| match u.port() { Some(p) => format!("{h}:{p}"), None => h.to_string() }))
                .unwrap_or_default();
            catalog.push(ModelInfo {
                alias: alias.clone(),
                provider: cfg.provider.clone(),
                model: cfg.model.clone(),
                host,
                effort: cfg.effort.clone(),
                max_tokens: cfg.max_tokens,
                has_key: !key.is_empty(),
                nature: nature.as_str().to_string(),
                vision: cfg.vision,
            });
            if nature != Nature::Llm {
                let service = Service::new(http.clone(), cfg, key, nature).with_context(|| format!("modèle « {alias} »"))?;
                services.insert(alias.clone(), service);
                continue;
            }
            let provider = match cfg.provider.as_str() {
                "anthropic" => Provider::Anthropic(Anthropic::new(http.clone(), cfg, key)?),
                // OpenAI, Gemini, Mistral, Ollama, vLLM, OpenRouter… : la même API, une autre adresse
                "openai" => Provider::OpenAi(OpenAi::new(http.clone(), cfg, key)?),
                other => bail!("modèle « {alias} » : fournisseur inconnu « {other} » (anthropic ou openai)"),
            };
            by_alias.insert(alias.clone(), provider);
        }
        Ok(Self { by_alias, services, catalog })
    }

    pub fn catalog(&self) -> &[ModelInfo] {
        &self.catalog
    }

    pub fn aliases(&self) -> Vec<String> {
        self.by_alias.keys().chain(self.services.keys()).cloned().collect()
    }

    /// La nature de chaque modèle, telle qu'on la dit aux agents (`vision` : un llm qui lit les images).
    pub fn natures(&self) -> BTreeMap<String, String> {
        self.catalog.iter().map(|m| (m.alias.clone(), if m.vision { "vision".to_string() } else { m.nature.clone() })).collect()
    }

    /// Appelle un modèle de service. La nature demandée doit être celle du modèle.
    pub async fn service(&self, request: &ServiceRequest) -> Result<ServiceResponse, ServiceError> {
        let service = self.services.get(&request.model).ok_or_else(|| {
            ServiceError::Refused(match self.by_alias.contains_key(&request.model) {
                true => "ce modèle tient une conversation : il ne rend pas ce service".to_string(),
                false => "modèle inconnu".to_string(),
            })
        })?;
        service.call(&request.input).await
    }

    pub async fn complete(&self, request: &ModelRequest) -> Result<ModelResponse> {
        let provider = self
            .by_alias
            .get(&request.model)
            .ok_or_else(|| anyhow!("modèle inconnu : « {} »", request.model))?;
        provider.complete(request).await
    }
}

/// Envoi JSON avec nouvelles tentatives sur 429, 5xx et erreurs de connexion.
pub(crate) async fn post_json(
    http: &reqwest::Client,
    url: &str,
    headers: &reqwest::header::HeaderMap,
    body: &Value,
) -> Result<Value> {
    const ATTEMPTS: u32 = 3;
    let mut attempt = 1;
    loop {
        let sent = http.post(url).headers(headers.clone()).json(body).send().await;
        let (error, retry_after) = match sent {
            Ok(resp) => {
                let status = resp.status();
                let retry_after = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok());
                let text = resp.text().await.unwrap_or_default();
                if status.is_success() {
                    return serde_json::from_str(&text).map_err(|e| anyhow!("réponse illisible du modèle : {e}"));
                }
                let detail = serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
                    .unwrap_or_else(|| text.chars().take(500).collect());
                let error = anyhow!("erreur du modèle (HTTP {}) : {detail}", status.as_u16());
                let retryable = status.as_u16() == 429 || status.as_u16() == 408 || status.is_server_error();
                if !retryable {
                    return Err(error);
                }
                (error, retry_after)
            }
            // sans l'adresse appelée : elle peut porter un secret en paramètre
            Err(e) if e.is_connect() || e.is_timeout() => (anyhow!("modèle injoignable : {}", e.without_url()), None),
            Err(e) => return Err(anyhow!("appel du modèle : {}", e.without_url())),
        };
        if attempt >= ATTEMPTS {
            return Err(error);
        }
        let delay = retry_after.unwrap_or(1 << attempt).min(30);
        tokio::time::sleep(std::time::Duration::from_secs(delay)).await;
        attempt += 1;
    }
}
