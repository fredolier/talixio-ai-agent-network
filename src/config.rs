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
    /// Adresse d'écoute : l'API et le lien des enfants, en TLS. Par défaut, la machine seule.
    #[serde(default = "default_listen")]
    pub listen: SocketAddr,
    /// Nom ou adresse par lequel les enfants d'autres machines joignent le maître. Par défaut :
    /// l'adresse d'écoute, ou l'adresse de sortie de la machine si l'écoute est sur toutes les interfaces.
    pub advertise: Option<String>,
    /// Certificat TLS fourni par l'exploitant. Absent : le maître fabrique le sien.
    pub tls: Option<TlsFiles>,
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
pub struct TlsFiles {
    /// Certificat (et sa chaîne), au format PEM.
    pub cert_file: PathBuf,
    /// Clé privée, au format PEM, lisible par son seul propriétaire.
    pub key_file: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    /// `anthropic`, ou `openai` pour OpenAI et tout serveur compatible.
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    /// Fichier qui contient la clé, lisible par son seul propriétaire (0600). À préférer à la
    /// variable d'environnement. La clé elle-même ne va jamais dans ce fichier-ci.
    pub api_key_file: Option<PathBuf>,
    /// Nom de la variable d'environnement qui contient la clé.
    pub api_key_env: Option<String>,
    /// `low`, `medium`, `high`, `xhigh` ou `max`.
    pub effort: Option<String>,
    pub max_tokens: Option<u64>,
    /// Anthropic : laisser un autre modèle répondre quand celui-ci décline (activé par défaut).
    pub refusal_fallback: Option<bool>,
}

impl ModelConfig {
    pub fn api_key(&self) -> Result<String> {
        if let Some(path) = &self.api_key_file {
            ensure_private(path)?;
            let key = std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?;
            return Ok(key.trim().to_string());
        }
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
        // les dossiers accordés sont figés sur leur chemin réel, une fois pour toutes
        cfg.capabilities.canonicalize().map_err(|e| anyhow::anyhow!("[capabilities] : {e}"))?;
        if let Some(host) = &cfg.advertise {
            check_host(host)?;
        }
        Ok(cfg)
    }

    /// Ce que les outils des agents ne doivent jamais atteindre : les fichiers de clés.
    pub fn secret_paths(&self) -> Vec<PathBuf> {
        let keys = self.models.values().filter_map(|m| m.api_key_file.as_ref());
        keys.chain(self.tls.as_ref().map(|t| &t.key_file)).filter_map(|p| p.canonicalize().ok()).collect()
    }
}

/// Un fichier secret doit être un vrai fichier, à nous, fermé aux autres comptes.
pub fn ensure_private(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path).with_context(|| format!("{}", path.display()))?;
    if !meta.is_file() {
        bail!("{} n'est pas un fichier ordinaire", path.display());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        // SAFETY: geteuid ne fait que lire l'identité du process.
        let me = unsafe { libc::geteuid() };
        if meta.uid() != me {
            bail!("{} appartient à un autre compte", path.display());
        }
        if meta.mode() & 0o077 != 0 {
            bail!("{} est lisible par d'autres comptes : chmod 600", path.display());
        }
    }
    Ok(())
}

/// Crée le dossier d'état, fermé aux autres comptes, et renvoie son chemin réel.
pub fn prepare_state_dir(path: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(path).with_context(|| format!("création de {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("droits de {}", path.display()))?;
    }
    path.canonicalize().with_context(|| format!("{}", path.display()))
}

/// Un nom de machine ou une adresse, sans rien qui puisse passer pour une option ou un morceau de commande.
pub fn check_host(host: &str) -> Result<()> {
    let ok = !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | ':' | '_'));
    if !ok {
        bail!("nom de machine invalide : « {host} »");
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
    /// Où joindre le parent : `machine:port`.
    pub parent_addr: String,
    /// Empreinte du certificat TLS du parent : l'enfant refuse tout autre correspondant.
    pub parent_cert: String,
    /// Secret partagé avec le parent. Il ne circule jamais sur le lien : chacun prouve qu'il le détient.
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
