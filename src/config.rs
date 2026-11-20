//! Configuration du maître (fichier TOML) et consignes de démarrage d'un enfant.

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::policy::Capabilities;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RootConfig {
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default = "default_role")]
    pub role: String,
    #[serde(default)]
    pub instructions: String,
    /// Alias du modèle du maître, parmi `models`.
    pub model: String,
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    #[serde(default = "default_state_dir")]
    pub state_dir: PathBuf,
    #[serde(default)]
    pub timing: Timing,
    pub models: BTreeMap<String, ModelConfig>,
    #[serde(default)]
    pub capabilities: Capabilities,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// `anthropic`, ou `openai` pour OpenAI et tout serveur compatible.
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    /// Nom de la variable d'environnement qui contient la clé. La clé elle-même ne va jamais dans ce fichier.
    pub api_key_env: Option<String>,
    /// `low`, `medium`, `high`, `xhigh` ou `max`.
    pub effort: Option<String>,
    pub max_tokens: Option<u64>,
    /// Anthropic : laisser un autre modèle répondre quand celui-ci décline (activé par défaut).
    pub refusal_fallback: Option<bool>,
}

impl ModelConfig {
    pub fn api_key(&self) -> Result<String> {
        match self.api_key_env.as_deref().filter(|name| !name.is_empty()) {
            Some(name) => std::env::var(name)
                .with_context(|| format!("la variable d'environnement {name} n'est pas définie")),
            None => Ok(String::new()),
        }
    }
}

/// Délais du lien parent-enfant, en secondes.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Timing {
    /// Intervalle entre deux battements.
    pub heartbeat_secs: u64,
    /// Sans nouvelles du parent pendant ce délai, l'enfant se met en veille.
    pub suspend_after_secs: u64,
    /// Sans nouvelles pendant ce délai, l'enfant s'éteint, avec toute sa descendance.
    pub die_after_secs: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Self { heartbeat_secs: 5, suspend_after_secs: 20, die_after_secs: 300 }
    }
}

impl Timing {
    pub fn heartbeat(&self) -> Duration {
        Duration::from_secs(self.heartbeat_secs.max(1))
    }
    pub fn suspend_after(&self) -> Duration {
        Duration::from_secs(self.suspend_after_secs.max(2))
    }
    pub fn die_after(&self) -> Duration {
        Duration::from_secs(self.die_after_secs.max(self.suspend_after_secs))
    }
}

impl RootConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?;
        let mut cfg: RootConfig =
            toml::from_str(&text).with_context(|| format!("{} n'est pas une configuration valide", path.display()))?;
        if !cfg.models.contains_key(&cfg.model) {
            bail!("le modèle « {} » n'est pas défini dans [models]", cfg.model);
        }
        if cfg.capabilities.models.is_empty() {
            cfg.capabilities.models = cfg.models.keys().cloned().collect();
        }
        if let Some(unknown) = cfg.capabilities.models.iter().find(|m| !cfg.models.contains_key(*m)) {
            bail!("capabilities.models cite un modèle non défini : « {unknown} »");
        }
        cfg.capabilities.validate().map_err(anyhow::Error::msg)?;
        ensure_loopback(&cfg.listen)?;
        Ok(cfg)
    }
}

/// Tant que le lien n'est pas chiffré, on n'écoute que sur la machine elle-même.
pub fn ensure_loopback(addr: &SocketAddr) -> Result<()> {
    if !addr.ip().is_loopback() {
        bail!("écoute sur {addr} refusée : sans chiffrement, seule l'interface locale (127.0.0.1) est permise");
    }
    Ok(())
}

/// Ce qu'un parent transmet à l'enfant qu'il lance, sur son entrée standard.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bootstrap {
    pub agent_id: String,
    pub name: String,
    pub role: String,
    pub instructions: String,
    pub model: String,
    pub capabilities: Capabilities,
    pub parent_id: String,
    pub parent_name: String,
    pub parent_url: String,
    /// Secret partagé avec le parent, présenté à chaque connexion.
    pub token: String,
    pub depth: u32,
    pub target: String,
    pub timing: Timing,
}

fn default_name() -> String {
    "maître".into()
}
fn default_role() -> String {
    "orchestrateur".into()
}
fn default_listen() -> SocketAddr {
    ([127, 0, 0, 1], 7700).into()
}
fn default_state_dir() -> PathBuf {
    ".talixio".into()
}
