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
    /// Alias du modèle du maître, parmi `models`. Absent : le maître démarre sans modèle et
    /// reçoit le sien depuis l'interface.
    #[serde(default)]
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
    /// Taille maximale du journal d'un agent, en octets. Au-delà, le journal tourne : l'ancien
    /// est gardé une fois, puis jeté. La limite descend de parent en enfant.
    #[serde(default = "default_log_max")]
    pub log_max_bytes: u64,
    /// Un catalogue de fournisseurs et de modèles à la place de celui de l'exécutable. Il fixe
    /// les adresses où vont les clés saisies dans l'interface : à protéger comme ce fichier-ci.
    pub catalog_file: Option<PathBuf>,
    /// La commande `claude` (Claude Code), pour les modèles servis par l'abonnement du compte
    /// du maître. Par défaut : `claude`, cherché dans `PATH`.
    pub claude_program: Option<PathBuf>,
    /// Variables d'environnement du maître transmises à `claude` en plus du strict nécessaire.
    #[serde(default)]
    pub claude_env: Vec<String>,
    /// Peut être vide : les modèles s'activent alors depuis l'interface.
    #[serde(default)]
    pub models: BTreeMap<String, ModelConfig>,
    #[serde(default)]
    pub capabilities: Capabilities,
    /// Cibles de déploiement : d'autres machines (SSH) ou des conteneurs (Docker).
    #[serde(default)]
    pub targets: BTreeMap<String, Target>,
    /// Dossier des paquets installés par l'exploitant (`talixio-agent pack install`) : commandes
    /// et rôles. Le maître le lit, le vérifie, et ne doit pas pouvoir l'écrire.
    #[serde(default)]
    pub packs_dir: Option<PathBuf>,
    /// Pour un essai seulement : accepter un dossier de paquets que le maître peut écrire.
    #[serde(default)]
    pub packs_unprotected: bool,
    /// Les rôles des paquets installés (jamais lus dans le fichier).
    #[serde(skip)]
    pub pack_roles: BTreeMap<String, crate::pack::InstalledRole>,
    /// Les paquets installés, nom et version (jamais lus dans le fichier).
    #[serde(skip)]
    pub pack_names: Vec<String>,
}

/// Une cible de déploiement : où et comment lancer un enfant ailleurs que sur la machine du
/// parent, et le plafond de ses droits là-bas.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// `ssh` ou `docker`.
    pub kind: String,
    /// SSH : la machine, et le compte sous lequel l'agent tournera.
    pub host: Option<String>,
    /// SSH : compte distant. Docker : numéro du compte dans le conteneur (`--user`), jamais 0.
    pub user: Option<String>,
    pub port: Option<u16>,
    /// SSH : clé privée à utiliser. Sans elle, celles du compte du maître.
    pub identity_file: Option<PathBuf>,
    /// SSH : dossier distant, réservé aux agents, où déposer l'exécutable et l'état.
    pub remote_dir: Option<String>,
    /// Docker : l'image du conteneur.
    pub image: Option<String>,
    /// Docker : dossiers de la machine montrés au conteneur.
    #[serde(default)]
    pub mounts: Vec<Mount>,
    /// Docker : réseau Docker où placer le conteneur. À donner quand le maître tourne lui-même
    /// dans un conteneur de ce réseau : l'agent le joint alors directement, sans passer par la machine.
    pub network: Option<String>,
    /// Docker : l'exécutable de l'agent est déjà dans l'image (`/talixio-agent`), au lieu d'être
    /// monté depuis la machine. Pour un maître en conteneur, dont l'exécutable n'est pas un
    /// fichier de la machine.
    #[serde(default)]
    pub agent_in_image: bool,
    /// Docker : mémoire du conteneur, « 512m » ou « 2g ». Par défaut 1 Go.
    pub memory: Option<String>,
    /// Docker : filtre d'appels système à la place de celui de Docker (un fichier de la machine).
    /// Sert à un navigateur, dont le bac à sable demande des appels que Docker refuse d'ordinaire.
    pub seccomp: Option<PathBuf>,
    /// Le programme `ssh` ou `docker`. Par défaut `/usr/bin/ssh`, `/usr/bin/docker`.
    pub program: Option<PathBuf>,
    /// Plafond des droits d'un agent sur cette cible : dossiers et commandes, tels qu'ils
    /// existent là-bas. Un enfant déployé ici n'en reçoit jamais plus.
    #[serde(default)]
    pub capabilities: Capabilities,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mount {
    /// Un dossier de la machine qui lance le conteneur…
    pub host: Option<PathBuf>,
    /// … ou un volume Docker (celui du maître, quand il tourne lui-même dans un conteneur et ne
    /// connaît pas les chemins de la machine), et le sous-dossier du volume à montrer.
    pub volume: Option<String>,
    pub subpath: Option<String>,
    pub container: PathBuf,
    #[serde(default)]
    pub write: bool,
}

/// Rien, dans une cible, ne doit pouvoir passer pour une option ou un morceau de commande :
/// ces valeurs finissent sur une ligne de commande, parfois interprétée par un shell distant.
fn plain(value: &str, extra: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with('-')
        && value.chars().all(|c| c.is_ascii_alphanumeric() || extra.contains(c))
}

/// Chemin de la machine pour un montage : rien qui puisse passer pour une option, un autre
/// montage (« : ») ou une remontée (« .. »). Sous Windows, la lettre de lecteur est admise.
fn plain_host_path(text: &str) -> bool {
    if text.contains("..") {
        return false;
    }
    if cfg!(windows) {
        let bytes = text.as_bytes();
        bytes.len() > 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/') && plain(&text[3..], "\\/._-")
    } else {
        text.len() > 1 && text.starts_with('/') && plain(&text[1..], "/._-")
    }
}

impl Target {
    pub fn validate(&self) -> Result<()> {
        self.capabilities.validate().map_err(anyhow::Error::msg)?;
        if self.capabilities.spawn.is_some() || !self.capabilities.models.is_empty() || self.capabilities.max_tokens.is_some() {
            bail!("le plafond d'une cible ne porte que sur les dossiers et les commandes");
        }
        if let Some(program) = &self.program {
            if !program.is_absolute() {
                bail!("`program` doit être un chemin absolu");
            }
        }
        match self.kind.as_str() {
            "ssh" => {
                check_host(self.host.as_deref().context("cible ssh : `host` manquant")?)?;
                let user = self.user.as_deref().context("cible ssh : `user` manquant")?;
                if !plain(user, "_-.") {
                    bail!("cible ssh : `user` invalide");
                }
                let dir = self.remote_dir.as_deref().context("cible ssh : `remote_dir` manquant")?;
                if !dir.starts_with('/') || !plain(&dir[1..], "/._-") || dir.contains("..") || dir.contains("//") || dir.ends_with('/') || dir == "/tmp" || dir == "/var/tmp" {
                    bail!("cible ssh : `remote_dir` doit être un chemin absolu simple (lettres, chiffres, / . _ -)");
                }
                if self.image.is_some() || !self.mounts.is_empty() {
                    bail!("cible ssh : `image` et `mounts` ne valent que pour docker");
                }
                if self.memory.is_some() || self.seccomp.is_some() || self.network.is_some() || self.agent_in_image {
                    bail!("cible ssh : `memory`, `seccomp`, `network` et `agent_in_image` ne valent que pour docker");
                }
            }
            "docker" => {
                if !plain(self.image.as_deref().context("cible docker : `image` manquante")?, "/._:@-") {
                    bail!("cible docker : `image` invalide");
                }
                let user = self.user.as_deref().context("cible docker : `user` manquant (numéro de compte non privilégié, « 1000:1000 »)")?;
                if !user.split(':').all(|n| n.parse::<u32>().is_ok_and(|n| n > 0)) || user.split(':').count() > 2 {
                    bail!("cible docker : `user` doit être un numéro de compte non privilégié, « 1000 » ou « 1000:1000 »");
                }
                for mount in &self.mounts {
                    // Le chemin dans le conteneur est toujours celui d'un Linux ; celui de la
                    // machine suit le système qui lance le conteneur.
                    let inside = mount.container.to_str().unwrap_or("");
                    if !inside.starts_with('/') || !plain(&inside[1..], "/._-") || inside.contains("..") {
                        bail!("cible docker : montage invalide ({})", mount.container.display());
                    }
                    // ni par-dessus l'agent ou son état, ni par-dessus ce que le conteneur tient du noyau
                    let inside = inside.trim_end_matches('/');
                    if ["/talixio-agent", "/state", "/tmp", "/proc", "/sys", "/dev"].iter().any(|r| inside == *r || inside.starts_with(&format!("{r}/"))) || inside.is_empty() {
                        bail!("cible docker : montage sur un emplacement réservé ({})", mount.container.display());
                    }
                    match (&mount.host, &mount.volume) {
                        (Some(host), None) if mount.subpath.is_none() => {
                            if !host.is_absolute() || !plain_host_path(host.to_str().unwrap_or("")) {
                                bail!("cible docker : montage invalide ({})", host.display());
                            }
                            // le socket Docker n'est jamais montré à un agent, ni le dossier qui le porte
                            let text = host.to_str().unwrap_or("").trim_end_matches('/');
                            if text.ends_with("docker.sock") || matches!(text, "/run" | "/var/run" | "/var" | "") {
                                bail!("cible docker : le socket Docker n'est jamais monté dans le conteneur d'un agent ({})", host.display());
                            }
                        }
                        (None, Some(volume)) => {
                            // un nom de volume, jamais un chemin : « / » ferait un montage de dossier
                            if !plain(volume, "._-") || volume.starts_with('.') {
                                bail!("cible docker : volume invalide ({volume})");
                            }
                            // toujours un sous-dossier : le volume entier peut porter les clés du maître
                            let subpath = mount.subpath.as_deref().context("cible docker : un montage `volume` demande `subpath` (un sous-dossier, jamais le volume entier)")?;
                            if !plain(subpath, "/._-") || subpath.split('/').any(|part| part.is_empty() || part.starts_with('.')) {
                                bail!("cible docker : sous-dossier de volume invalide ({subpath})");
                            }
                        }
                        _ => bail!("cible docker : un montage donne `host` (un dossier de la machine) ou `volume` (avec `subpath` au besoin), pas les deux"),
                    }
                }
                if let Some(network) = &self.network {
                    // un réseau nommé, créé par l'exploitant : ni « host », ni celui d'un autre conteneur
                    if !plain(network, "._-") || matches!(network.as_str(), "host" | "none" | "bridge" | "default") {
                        bail!("cible docker : `network` doit être le nom d'un réseau Docker dédié");
                    }
                }
                if let Some(memory) = &self.memory {
                    let (number, unit) = memory.split_at(memory.len().saturating_sub(1));
                    if !matches!(unit, "m" | "g") || number.len() > 5 || !number.parse::<u32>().is_ok_and(|n| n > 0) {
                        bail!("cible docker : `memory` s'écrit « 512m » ou « 2g »");
                    }
                }
                if let Some(seccomp) = &self.seccomp {
                    // un fichier précis de la machine, jamais « unconfined » ni une option de plus
                    if !seccomp.is_absolute() || !plain_host_path(seccomp.to_str().unwrap_or("")) || seccomp.extension().is_none_or(|e| e != "json") {
                        bail!("cible docker : `seccomp` doit être le chemin absolu d'un fichier .json");
                    }
                }
                if self.host.is_some() || self.identity_file.is_some() || self.remote_dir.is_some() || self.port.is_some() {
                    bail!("cible docker : `host`, `port`, `identity_file` et `remote_dir` ne valent que pour ssh");
                }
            }
            other => bail!("type de cible inconnu : « {other} » (ssh ou docker)"),
        }
        Ok(())
    }

    /// Une ligne pour décrire la cible à un agent.
    pub fn describe(&self) -> String {
        let place = match self.kind.as_str() {
            "ssh" => format!("machine {} (ssh)", self.host.as_deref().unwrap_or("?")),
            _ => format!("conteneur {} (docker)", self.image.as_deref().unwrap_or("?")),
        };
        let list = |paths: &[PathBuf]| {
            if paths.is_empty() { "aucun".to_string() } else { paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ") }
        };
        let caps = &self.capabilities;
        let commands = if caps.exec.is_empty() { "aucune".to_string() } else { caps.exec.iter().map(|r| r.describe()).collect::<Vec<_>>().join(" ; ") };
        format!("{place} — droits au plus : lecture {} ; écriture {} ; commandes {commands}", list(&caps.fs_read), list(&caps.fs_write))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TlsFiles {
    /// Certificat (et sa chaîne), au format PEM.
    pub cert_file: PathBuf,
    /// Clé privée, au format PEM, lisible par son seul propriétaire.
    pub key_file: PathBuf,
}

#[derive(Clone, Debug, Default, Deserialize)]
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
    /// Ce que le modèle sait faire : `llm` (par défaut : il tient une conversation),
    /// `transcription`, `speech`, `image`, `embedding`, `forecast` ou `decision`.
    /// `provider` dit alors par quelle API : `openai` (transcription, speech, image, embedding),
    /// `elevenlabs` (speech), `timesfm` (forecast), `systemone` (decision).
    pub nature: Option<String>,
    /// `llm` : le modèle sait lire une image (outil `describe_image`).
    #[serde(default)]
    pub vision: bool,
    /// `speech` : la voix employée quand l'agent n'en demande pas.
    pub voice: Option<String>,
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
        // une empreinte ne s'écrit pas à la main : elle vient des paquets installés
        let rules = cfg.capabilities.exec.iter().chain(cfg.targets.values().flat_map(|t| &t.capabilities.exec));
        if rules.clone().any(|rule| rule.sha256.is_some()) {
            bail!("`sha256` ne s'écrit pas dans la configuration : il est posé par les paquets installés (packs_dir)");
        }
        // les paquets installés par l'exploitant : vérifiés contre leurs empreintes, et hors de
        // portée d'écriture du maître, sans quoi il ne démarre pas
        if let Some(dir) = &cfg.packs_dir {
            if !dir.is_absolute() {
                bail!("packs_dir doit être un chemin absolu");
            }
            let installed = crate::pack::load_installed(dir, !cfg.packs_unprotected).context("paquets installés")?;
            let real = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            let inside = |program: &Path| program.starts_with(dir) || program.starts_with(&real);
            // une commande de paquet ne se réécrit pas à la main : sa règle vient du paquet, avec son empreinte
            if let Some(rule) = cfg.capabilities.exec.iter().find(|rule| inside(&rule.program)) {
                bail!("la règle sur {} est dans le dossier des paquets : elle vient du paquet installé, retirez-la de [capabilities]", rule.program.display());
            }
            // le plafond d'une cible ne cite, sous ce dossier, que des commandes installées ; il en
            // reçoit l'empreinte, que l'agent vérifiera là-bas avant chaque lancement
            for (name, target) in &mut cfg.targets {
                for rule in target.capabilities.exec.iter_mut().filter(|rule| inside(&rule.program)) {
                    let Some(known) = installed.exec.iter().find(|k| k.program == rule.program && k.args == rule.args && rule.cwd.is_none()) else {
                        bail!("[targets.{name}] : {} n'est pas une commande d'un paquet installé (talixio-agent pack verify)", rule.describe());
                    };
                    rule.sha256 = known.sha256.clone();
                }
            }
            cfg.capabilities.exec.extend(installed.exec);
            cfg.pack_roles = installed.roles;
            cfg.pack_names = installed.names;
        }
        // sans `model`, le maître reçoit le sien depuis l'interface
        if !cfg.model.is_empty() {
            let Some(model) = cfg.models.get(&cfg.model) else {
                bail!("le modèle « {} » n'est pas défini dans [models]", cfg.model);
            };
            if model.nature.as_deref().is_some_and(|n| n != "llm") {
                bail!("le modèle du maître, « {} », doit tenir une conversation (nature llm)", cfg.model);
            }
        }
        if cfg.capabilities.models.is_empty() {
            cfg.capabilities.models = cfg.models.keys().cloned().collect();
        }
        if cfg.models.contains_key("") {
            bail!("[models] : un modèle a besoin d'un alias");
        }
        if let Some(unknown) = cfg.capabilities.models.iter().find(|m| !cfg.models.contains_key(*m)) {
            bail!("capabilities.models cite un modèle non défini : « {unknown} »");
        }
        // les dossiers accordés sont figés sur leur chemin réel, une fois pour toutes
        cfg.capabilities.canonicalize().map_err(|e| anyhow::anyhow!("[capabilities] : {e}"))?;
        if let Some(host) = &cfg.advertise {
            check_host(host)?;
        }
        for (name, target) in &cfg.targets {
            if !crate::policy::valid_target_name(name) || name == "local" || name == "root" {
                bail!("nom de cible invalide : « {name} » (minuscules, chiffres, tirets)");
            }
            target.validate().with_context(|| format!("[targets.{name}]"))?;
        }
        // Un sous-dossier de volume monté pour un agent n'a jamais un parent qu'un autre conteneur
        // peut écrire : celui-ci le remplacerait par un lien vers le reste du volume, que Docker suit.
        let volumes: Vec<(&String, &str, bool)> = cfg
            .targets
            .values()
            .flat_map(|t| &t.mounts)
            .filter_map(|m| Some((m.volume.as_ref()?, m.subpath.as_deref()?, m.write)))
            .collect();
        for (volume, parent, write) in &volumes {
            if let Some((_, child, _)) = volumes.iter().find(|(v, child, _)| v == volume && *write && child.starts_with(&format!("{parent}/"))) {
                bail!("cibles docker : le sous-dossier « {child} » du volume {volume} est monté alors que son parent « {parent} » l'est en écriture");
            }
        }
        if let Some(spawn) = &cfg.capabilities.spawn {
            if let Some(unknown) = spawn.targets.iter().find(|t| *t != "local" && !cfg.targets.contains_key(*t)) {
                bail!("capabilities.spawn.targets cite une cible non définie : « {unknown} »");
            }
        }
        Ok(cfg)
    }

    /// Ce que les outils des agents ne doivent jamais atteindre : les fichiers de clés.
    /// Un fichier de clé déclaré mais introuvable est une erreur : on ne protège pas ce qu'on
    /// ne sait pas situer. Le dossier `.ssh` du compte (clés, machines connues, réglages de
    /// `ssh`) est interdit d'office, comme ce que la commande `claude` garde (`.claude`,
    /// `.claude.json`, `CLAUDE_CONFIG_DIR`).
    pub fn secret_paths(&self) -> Result<Vec<PathBuf>> {
        let keys = self.models.values().filter_map(|m| m.api_key_file.as_ref());
        let ssh = self.targets.values().filter_map(|t| t.identity_file.as_ref());
        let mut paths = Vec::new();
        for path in keys.chain(ssh).chain(self.tls.as_ref().map(|t| &t.key_file)) {
            paths.push(path.canonicalize().with_context(|| format!("fichier de clé introuvable : {}", path.display()))?);
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
        // le dossier personnel : les clés SSH, et ce que la commande `claude` y garde (sa connexion,
        // et les conversations de tous les agents qu'elle sert), même s'ils n'existent pas encore
        if let Some(home) = home.map(PathBuf::from).and_then(|h| h.canonicalize().ok()) {
            paths.extend([".ssh", ".claude", ".claude.json"].map(|name| home.join(name)).map(|path| path.canonicalize().unwrap_or(path)));
        }
        if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from).filter(|d| d.is_absolute()) {
            paths.push(dir.canonicalize().unwrap_or(dir));
        }
        // les paquets installés : ni lus ni écrits par un outil de fichiers, seulement lancés
        if let Some(dir) = self.packs_dir.as_ref().and_then(|d| d.canonicalize().ok()) {
            paths.push(dir);
        }
        // le socket Docker : qui l'ouvre commande la machine. Jamais par un outil d'agent.
        if cfg!(unix) {
            for socket in ["/var/run/docker.sock", "/run/docker.sock"] {
                paths.push(PathBuf::from(socket));
                if let Ok(real) = Path::new(socket).canonicalize() {
                    paths.push(real);
                }
            }
        }
        Ok(paths)
    }
}

/// Un fichier secret doit être un vrai fichier, à nous, fermé aux autres comptes.
pub fn ensure_private(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path).with_context(|| format!("{}", path.display()))?;
    if !meta.is_file() {
        bail!("{} n'est pas un fichier ordinaire", path.display());
    }
    crate::os::check_private(path).map_err(anyhow::Error::msg)?;
    Ok(())
}

/// Crée le dossier d'état, fermé aux autres comptes, et renvoie son chemin réel.
pub fn prepare_state_dir(path: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(path).with_context(|| format!("création de {}", path.display()))?;
    crate::os::make_private(path).map_err(anyhow::Error::msg)?;
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
    /// Ce que les outils ne doivent jamais atteindre sur cette machine (fichiers de clés du
    /// maître) : transmis de parent en enfant tant qu'on reste sur la même machine.
    #[serde(default)]
    pub deny: Vec<PathBuf>,
    /// Déployé sur une cible : le plafond de droits de celle-ci, que l'enfant revérifie à l'arrivée.
    #[serde(default)]
    pub ceiling: Option<Capabilities>,
    /// Taille maximale de son journal, fixée par le parent.
    #[serde(default = "default_log_max")]
    pub log_max_bytes: u64,
    /// La nature de chaque modèle accordé (`llm`, `vision`, `transcription`…), pour que l'agent
    /// sache lequel sert à quoi. Indicatif : le maître revérifie la nature à chaque appel.
    #[serde(default)]
    pub natures: BTreeMap<String, String>,
}

/// Ce que l'enfant reçoit au lancement : de quoi joindre son parent, et sa configuration
/// scellée. Il ne peut ni la lire ni la changer : seul le parent en détient la clé, et il ne
/// la remet, une seule fois, qu'à l'enfant qui a prouvé qui il est sur le lien chiffré.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub agent_id: String,
    pub parent_addr: String,
    pub parent_cert: String,
    pub token: String,
    /// Aléa puis configuration chiffrée et signée (ChaCha20-Poly1305), en hexadécimal.
    pub sealed: String,
}

impl Envelope {
    /// Ce qui est lié au chiffrement sans être chiffré : une enveloppe ne s'ouvre que pour
    /// l'agent et le parent pour lesquels elle a été faite.
    fn bound(agent_id: &str, parent_cert: &str) -> Vec<u8> {
        format!("talixio-config-v1\n{agent_id}\n{parent_cert}").into_bytes()
    }

    fn key(key: &str) -> Result<ring::aead::LessSafeKey> {
        let bytes = crate::unhex(key).filter(|k| k.len() == 32).context("clé de configuration invalide")?;
        let key = ring::aead::UnboundKey::new(&ring::aead::CHACHA20_POLY1305, &bytes).map_err(|_| anyhow::anyhow!("clé refusée"))?;
        Ok(ring::aead::LessSafeKey::new(key))
    }

    /// Une clé neuve, pour un seul enfant et une seule enveloppe.
    pub fn new_key() -> Result<String> {
        use ring::rand::SecureRandom;
        let mut key = [0u8; 32];
        ring::rand::SystemRandom::new().fill(&mut key).map_err(|_| anyhow::anyhow!("pas de source d'aléa"))?;
        Ok(crate::hex(&key))
    }

    pub fn seal(boot: &Bootstrap, key: &str) -> Result<Self> {
        use ring::rand::SecureRandom;
        let mut nonce = [0u8; 12];
        ring::rand::SystemRandom::new().fill(&mut nonce).map_err(|_| anyhow::anyhow!("pas de source d'aléa"))?;
        let mut data = serde_json::to_vec(boot)?;
        let aad = ring::aead::Aad::from(Self::bound(&boot.agent_id, &boot.parent_cert));
        Self::key(key)?
            .seal_in_place_append_tag(ring::aead::Nonce::assume_unique_for_key(nonce), aad, &mut data)
            .map_err(|_| anyhow::anyhow!("chiffrement impossible"))?;
        Ok(Self {
            agent_id: boot.agent_id.clone(),
            parent_addr: boot.parent_addr.clone(),
            parent_cert: boot.parent_cert.clone(),
            token: boot.token.clone(),
            sealed: format!("{}{}", crate::hex(&nonce), crate::hex(&data)),
        })
    }

    /// Ouvre la configuration. Un seul octet changé, dans la configuration ou dans ce qui
    /// l'accompagne, et elle est refusée en bloc.
    pub fn open(&self, key: &str) -> Result<Bootstrap> {
        let mut data = crate::unhex(&self.sealed).filter(|d| d.len() > 12 + 16).context("configuration scellée illisible")?;
        let nonce: [u8; 12] = data[..12].try_into().expect("longueur vérifiée");
        let aad = ring::aead::Aad::from(Self::bound(&self.agent_id, &self.parent_cert));
        let clear = Self::key(key)?
            .open_in_place(ring::aead::Nonce::assume_unique_for_key(nonce), aad, &mut data[12..])
            .map_err(|_| anyhow::anyhow!("configuration scellée altérée, ou clé qui n'est pas la sienne"))?;
        let boot: Bootstrap = serde_json::from_slice(clear).context("configuration scellée invalide")?;
        if boot.agent_id != self.agent_id || boot.parent_addr != self.parent_addr || boot.parent_cert != self.parent_cert || boot.token != self.token {
            bail!("l'enveloppe ne correspond pas à la configuration qu'elle contient");
        }
        Ok(boot)
    }
}

fn default_log_max() -> u64 {
    5 * 1024 * 1024
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

#[cfg(test)]
mod sealed_tests {
    use super::*;

    fn boot() -> Bootstrap {
        serde_json::from_value(serde_json::json!({
            "agent_id": "a-1", "name": "n", "role": "r", "instructions": "i", "model": "m",
            "capabilities": { "fs_read": ["/srv/data"] }, "parent_id": "p", "parent_name": "p",
            "parent_addr": "127.0.0.1:1", "parent_cert": "c".repeat(64), "token": "t", "depth": 1, "target": "local",
            "timing": { "heartbeat_secs": 1, "suspend_after_secs": 2, "die_after_secs": 4 }
        }))
        .unwrap()
    }

    #[test]
    fn une_configuration_scellee_ne_se_lit_ni_ne_se_change_sans_la_cle() {
        let key = Envelope::new_key().unwrap();
        let envelope = Envelope::seal(&boot(), &key).unwrap();
        // rien de la configuration n'est lisible dans l'enveloppe
        let wire = serde_json::to_string(&envelope).unwrap();
        assert!(!wire.contains("/srv/data") && !wire.contains("fs_read") && !wire.contains("instructions"));
        // la bonne clé l'ouvre
        assert_eq!(envelope.open(&key).unwrap().capabilities.fs_read, vec![PathBuf::from("/srv/data")]);
        // une autre clé, non
        assert!(envelope.open(&Envelope::new_key().unwrap()).is_err());
        // un seul caractère changé dans la configuration : refusée en bloc
        let mut forged = envelope.clone();
        let last = forged.sealed.pop().unwrap();
        forged.sealed.push(if last == '0' { '1' } else { '0' });
        assert!(forged.open(&key).is_err());
        // l'enveloppe d'un agent ne vaut pas pour un autre, ni pour un autre parent
        for change in [|e: &mut Envelope| e.agent_id = "a-2".into(), |e: &mut Envelope| e.parent_cert = "d".repeat(64), |e: &mut Envelope| e.parent_addr = "10.0.0.1:1".into(), |e: &mut Envelope| e.token = "x".into()] {
            let mut moved = envelope.clone();
            change(&mut moved);
            assert!(moved.open(&key).is_err());
        }
        // deux scellés de la même configuration ne se ressemblent pas
        assert_ne!(Envelope::seal(&boot(), &key).unwrap().sealed, envelope.sealed);
    }
}
