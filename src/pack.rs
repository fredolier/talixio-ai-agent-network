//! Les paquets : des commandes (des scripts, avec leurs arguments permis) et des rôles (des
//! consignes toutes faites) qu'une personne installe pour son maître.
//!
//! Le maître ne s'en installe jamais lui-même : `talixio-agent pack install` est lancé par
//! l'exploitant, dans un dossier que le maître ne peut pas écrire. À son démarrage, le maître
//! vérifie chaque fichier contre l'empreinte SHA-256 notée à l'installation, et refuse de
//! démarrer si l'un d'eux a changé ou s'il peut lui-même écrire le dossier. Chaque commande
//! d'un paquet porte l'empreinte de son programme, revérifiée à chaque lancement.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::policy::ExecRule;

const MANIFEST: &str = "pack.toml";
const LOCK: &str = "pack.lock";
/// Un paquet reste petit : des scripts et des textes.
pub(crate) const MAX_FILE: u64 = 2 * 1024 * 1024;
pub(crate) const MAX_FILES: usize = 64;

/// Les paquets livrés dans l'exécutable.
type Files = &'static [(&'static str, &'static [u8])];
const EMBEDDED: &[(&str, Files)] = &[(
    "catalogue",
    &[
        (MANIFEST, include_bytes!("../packs/catalogue/pack.toml")),
        ("consigne.md", include_bytes!("../packs/catalogue/consigne.md")),
        ("lire-web", include_bytes!("../packs/catalogue/lire-web")),
        ("lire-page", include_bytes!("../packs/catalogue/lire-page")),
        ("sites.txt", include_bytes!("../packs/catalogue/sites.txt")),
    ],
), (
    "audit",
    &[
        (MANIFEST, include_bytes!("../packs/audit/pack.toml")),
        ("brief.md", include_bytes!("../packs/audit/brief.md")),
        ("report.md", include_bytes!("../packs/audit/report.md")),
        ("fetch-repo", include_bytes!("../packs/audit/fetch-repo")),
        ("repo-map", include_bytes!("../packs/audit/repo-map")),
        ("repo-git", include_bytes!("../packs/audit/repo-git")),
        ("new-report", include_bytes!("../packs/audit/new-report")),
        ("check-report", include_bytes!("../packs/audit/check-report")),
    ],
)];

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub name: String,
    pub version: String,
    pub description: String,
    /// Un fichier du paquet à lire avant d'installer (mode d'emploi, consigne à envoyer au maître).
    #[serde(default)]
    pub guide: Option<String>,
    #[serde(default, rename = "command")]
    pub commands: Vec<Command>,
    #[serde(default, rename = "role")]
    pub roles: Vec<Role>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    /// Le nom d'un fichier du paquet.
    pub program: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Arguments permis, comme dans une règle `exec`. Obligatoires : un paquet ne donne jamais
    /// une commande à arguments libres.
    pub args: Vec<String>,
    /// Environment variables the command accepts a secret in (a token, a password). The
    /// operator decides, on the master, which secret goes there and for which roles.
    #[serde(default)]
    pub secrets: Vec<String>,
    /// Plain settings that go with those secrets (the address a token is valid for, a user
    /// name): the operator fills them next to the secret, and they travel with it.
    #[serde(default)]
    pub env: Vec<String>,
}

/// A command of an installed pack that accepts secrets: what the master shows to the operator.
#[derive(Clone, Debug, Serialize)]
pub struct SecretCommand {
    /// `pack/program`.
    pub name: String,
    pub program: PathBuf,
    pub secrets: Vec<String>,
    pub env: Vec<String>,
    /// The installed roles that hold this command.
    pub roles: Vec<String>,
    /// SHA-256 of the program: a secret tied to it is for this content, not for the name.
    pub sha256: String,
}

/// The name of a variable a pack may receive: its own, in capitals, so that it cannot be one
/// that the system, the shell or a tool gives a meaning to (`PATH`, `LD_PRELOAD`, `GIT_…`).
pub fn variable_ok(pack: &str, name: &str) -> bool {
    let prefix = format!("{}_", pack.to_uppercase().replace('-', "_"));
    name.len() > prefix.len() && name.starts_with(&prefix) && variable_shape_ok(name)
}

/// The shape alone, for an agent that does not know which pack a command comes from.
pub fn variable_shape_ok(name: &str) -> bool {
    const TAKEN: [&str; 26] = [
        "LD_", "DYLD_", "GIT_", "BASH_", "PYTHON", "NODE_", "NPM_", "PERL", "RUBY", "JAVA_", "SSH_", "LC_", "XDG_", "TALIXIO_", "SSL_", "CURL_", "OPENSSL_", "GCONV_",
        "MALLOC_", "RES_", "DOCKER_", "AWS_", "HTTP_", "HTTPS_", "ALL_", "NO_",
    ];
    (3..=64).contains(&name.len())
        && name.contains('_')
        && name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !TAKEN.iter().any(|taken| name.starts_with(taken))
        && !name.ends_with("_PROXY")
        && !crate::spawn::SAFE_ENV.contains(&name)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub name: String,
    pub description: String,
    pub instructions: String,
    /// Les commandes du paquet que ce rôle emploie (noms de programmes).
    #[serde(default)]
    pub commands: Vec<String>,
    /// Modèle conseillé, si le maître le détient.
    #[serde(default)]
    pub model: Option<String>,
    /// Cible conseillée, si le maître l'a.
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct Lock {
    version: String,
    files: BTreeMap<String, String>,
    /// Paquet venu d'un dépôt signé : la clé qui a signé l'index, et la date de celui-ci.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    depot_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    depot_issued: Option<u64>,
}

/// Un paquet lu, d'où qu'il vienne : son manifeste et ses fichiers.
pub struct Pack {
    pub manifest: Manifest,
    pub files: BTreeMap<String, Vec<u8>>,
    /// Venu d'un dépôt signé : la clé de l'index et sa date (secondes depuis 1970).
    pub origin: Option<(String, u64)>,
}

/// Un rôle installé, prêt à servir : ses consignes et les règles de ses commandes.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct InstalledRole {
    pub pack: String,
    /// La version et l'identifiant (empreinte du contenu) du paquet dont il vient.
    pub version: String,
    pub pack_id: String,
    pub role: Role,
    pub exec: Vec<ExecRule>,
}

/// The installed roles a child receives: none unless it may create agents, and then only the
/// roles whose every command it holds itself (a role never brings a command its user lacks).
pub fn roles_for_child(roles: &BTreeMap<String, InstalledRole>, caps: &crate::policy::Capabilities) -> BTreeMap<String, InstalledRole> {
    if caps.spawn.is_none() {
        return BTreeMap::new();
    }
    let own = |rule: &ExecRule| caps.exec.iter().find(|own| own.program == rule.program && own.args == rule.args);
    let mut kept = BTreeMap::new();
    for (name, role) in roles.iter().filter(|(_, role)| role.exec.iter().all(|rule| own(rule).is_some())) {
        let mut role = role.clone();
        // nor a secret its user's own rule does not carry
        for rule in &mut role.exec {
            let held = own(rule).map(|own| own.secrets.clone()).unwrap_or_default();
            rule.secrets.retain(|name| held.contains(name));
            if rule.secrets.is_empty() {
                rule.env.clear();
            }
        }
        kept.insert(name.clone(), role);
    }
    kept
}

/// Ce que le maître tire des paquets installés.
#[derive(Default)]
pub struct Installed {
    pub exec: Vec<ExecRule>,
    pub secret_commands: Vec<SecretCommand>,
    pub roles: BTreeMap<String, InstalledRole>,
    pub names: Vec<String>,
    /// L'identifiant de chaque paquet installé, par nom.
    pub ids: BTreeMap<String, String>,
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Un texte d'une ligne, sans rien qui commande un terminal ni qui se cache : il sera affiché à
/// l'exploitant et inséré dans les consignes du maître.
pub(crate) fn plain_line(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && text.chars().count() <= max && crate::clean(text, max, false) == text && !text.contains(['\n', '\r'])
}

/// Un nom de paquet ou de fichier : court, sans rien qui fasse un chemin ou une option.
pub(crate) fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['.', '-'])
        && !name.ends_with('.')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        // les noms que Windows réserve à ses périphériques
        && !{
            let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
            matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.as_bytes()[3].is_ascii_digit())
        }
}

impl Pack {
    pub(crate) fn parse(files: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        let text = files.get(MANIFEST).context("pack.toml manque")?;
        // les fins de ligne d'un autre système ne font pas un autre manifeste
        let text = std::str::from_utf8(text).context("pack.toml n'est pas du texte")?.replace("\r\n", "\n");
        // l'erreur recopie la ligne fautive : nettoyée, elle ne commande pas le terminal
        let manifest: Manifest = toml::from_str(&text).map_err(|e| anyhow::anyhow!("pack.toml invalide : {}", crate::clean(&e.to_string(), 600, true)))?;
        if !plain_name(&manifest.name) {
            bail!("nom de paquet invalide : « {} »", crate::clean(&manifest.name, 80, false));
        }
        if !plain_name(&manifest.version) {
            bail!("version invalide (lettres, chiffres, points, tirets)");
        }
        if !plain_line(&manifest.description, 300) {
            bail!("description du paquet : une ligne de 300 caractères au plus, sans caractère de commande");
        }
        if files.len() > MAX_FILES {
            bail!("trop de fichiers ({}, {MAX_FILES} au plus)", files.len());
        }
        if let Some(bad) = files.keys().find(|name| !plain_name(name) || *name == LOCK) {
            bail!("nom de fichier invalide : « {} »", crate::clean(bad, 80, false));
        }
        if manifest.guide.as_ref().is_some_and(|g| !files.contains_key(g)) {
            bail!("le guide annoncé n'est pas dans le paquet");
        }
        for command in &manifest.commands {
            if !files.contains_key(&command.program) || command.program == MANIFEST {
                bail!("commande « {} » : ce fichier n'est pas dans le paquet", crate::clean(&command.program, 80, false));
            }
            // ni suite libre, ni option glissée : chaque argument est fixé, ou un emplacement connu
            if command.args.iter().any(|a| a == "**" || a.starts_with('-') && a != "-") {
                bail!("commande « {} » : arguments trop libres pour un paquet", command.program);
            }
            if command.args.len() > 16 || command.args.iter().any(|a| !plain_line(a, 200)) {
                bail!("commande « {} » : argument invalide (une ligne, sans caractère de commande)", command.program);
            }
            if command.description.as_ref().is_some_and(|d| !plain_line(d, 300)) {
                bail!("commande « {} » : description invalide (une ligne de 300 caractères au plus)", command.program);
            }
            // the variables it receives are its own: never one the system or a tool gives a meaning to
            let variables: Vec<&String> = command.secrets.iter().chain(&command.env).collect();
            if let Some(bad) = variables.iter().find(|v| !variable_ok(&manifest.name, v)) {
                bail!(
                    "command \"{}\": variable \"{}\" refused (capitals, digits and underscores, starting with \"{}_\")",
                    command.program,
                    crate::clean(bad, 80, false),
                    manifest.name.to_uppercase().replace('-', "_")
                );
            }
            if variables.len() > 8 || variables.iter().any(|v| variables.iter().filter(|w| w == &v).count() > 1) || (command.secrets.is_empty() && !command.env.is_empty()) {
                bail!("command \"{}\": at most 8 variables, each named once, and `env` only next to `secrets`", command.program);
            }
            // one program, one declaration: a rule with fewer arguments does not receive less or more
            if manifest.commands.iter().any(|other| other.program == command.program && (other.secrets != command.secrets || other.env != command.env)) {
                bail!("command \"{}\": every rule of a program declares the same `secrets` and `env`", command.program);
            }
        }
        for role in &manifest.roles {
            let valid = !role.name.is_empty() && role.name.chars().count() <= 40 && role.name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
            if !valid {
                bail!("nom de rôle invalide : « {} »", crate::clean(&role.name, 80, false));
            }
            if !plain_line(&role.description, 300) {
                bail!("rôle « {} » : description invalide (une ligne de 300 caractères au plus)", role.name);
            }
            if role.model.iter().chain(&role.target).any(|n| !plain_name(n)) {
                bail!("rôle « {} » : modèle ou cible conseillés invalides", role.name);
            }
            // les consignes sont un texte de plusieurs lignes, mais sans rien qui se cache
            let hidden = crate::clean(&role.instructions, usize::MAX, true) != role.instructions;
            if role.instructions.len() > 20_000 || role.instructions.trim().is_empty() || hidden {
                bail!("rôle « {} » : consignes vides, trop longues, ou avec des caractères de commande", role.name);
            }
            if manifest.roles.iter().filter(|r| r.name == role.name).count() > 1 {
                bail!("rôle « {} » : déclaré deux fois", role.name);
            }
            if let Some(unknown) = role.commands.iter().find(|c| !manifest.commands.iter().any(|k| k.program == **c)) {
                bail!("rôle « {} » : commande « {} » absente du paquet", role.name, crate::clean(unknown, 80, false));
            }
        }
        // sur un disque qui ne distingue pas les majuscules, deux noms n'en feraient qu'un
        let folded: std::collections::BTreeSet<String> = files.keys().map(|n| n.to_lowercase()).collect();
        if folded.len() != files.len() {
            bail!("deux fichiers ne diffèrent que par la casse");
        }
        Ok(Self { manifest, files, origin: None })
    }

    /// Lit un paquet dans un dossier : des fichiers ordinaires, à plat, sans lien.
    fn read_dir(dir: &Path) -> Result<Self> {
        let mut files = BTreeMap::new();
        for entry in std::fs::read_dir(dir).with_context(|| format!("lecture de {}", dir.display()))? {
            let entry = entry?;
            if files.len() >= MAX_FILES {
                bail!("{} : trop de fichiers ({MAX_FILES} au plus)", dir.display());
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == LOCK {
                continue;
            }
            let meta = entry.path().symlink_metadata()?;
            if !meta.is_file() {
                bail!("{} : seuls des fichiers ordinaires, à plat, font un paquet", entry.path().display());
            }
            if meta.len() > MAX_FILE {
                bail!("{} : fichier trop gros", entry.path().display());
            }
            files.insert(name, std::fs::read(entry.path())?);
        }
        Self::parse(files).with_context(|| format!("paquet {}", dir.display()))
    }

    /// Les paquets disponibles : ceux de l'exécutable, ou ceux d'un dossier (un sous-dossier par paquet).
    pub fn available(from: Option<&Path>) -> Result<Vec<Self>> {
        match from {
            None => EMBEDDED
                .iter()
                .map(|(name, files)| {
                    Self::parse(files.iter().map(|(n, b)| (n.to_string(), b.to_vec())).collect()).with_context(|| format!("paquet embarqué {name}"))
                })
                .collect(),
            Some(dir) => {
                let mut packs = Vec::new();
                let mut entries: Vec<_> = std::fs::read_dir(dir).with_context(|| format!("lecture de {}", dir.display()))?.collect::<std::io::Result<_>>()?;
                entries.sort_by_key(|e| e.file_name());
                for entry in entries {
                    if entry.path().join(MANIFEST).is_file() {
                        let pack = Self::read_dir(&entry.path())?;
                        if entry.file_name().to_string_lossy() != pack.manifest.name {
                            bail!("{} : le dossier d'un paquet porte le nom de son manifeste (« {} »)", entry.path().display(), pack.manifest.name);
                        }
                        packs.push(pack);
                    }
                }
                Ok(packs)
            }
        }
    }

    pub fn find(from: Option<&Path>, name: &str) -> Result<Self> {
        Self::available(from)?.into_iter().find(|p| p.manifest.name == name).with_context(|| format!("paquet « {} » introuvable (talixio-agent pack list)", crate::clean(name, 80, false)))
    }

    /// L'identifiant du paquet : l'empreinte SHA-256 de tout son contenu (le nom et l'empreinte
    /// de chaque fichier). Deux paquets de même nom et de contenu différent n'ont pas le même ;
    /// c'est lui, pas le nom, qu'on fixe dans la configuration du maître (`[packs]`).
    pub fn id(&self) -> String {
        let listing: String = self.files.iter().map(|(name, bytes)| format!("{name}\0{}\n", sha256_hex(bytes))).collect();
        sha256_hex(listing.as_bytes())
    }

    /// Ce que le paquet donnerait, en clair : à lire avant d'installer.
    pub fn describe(&self) -> String {
        let m = &self.manifest;
        let mut out = format!("{} (version {})\n{}\nIdentifiant : {}\n", m.name, m.version, m.description, self.id());
        out.push_str("\nCommandes données au maître, qui peut les confier à ses agents :\n");
        for command in &m.commands {
            out.push_str(&format!("  {} {}\n", command.program, command.args.join(" ")));
            if let Some(text) = &command.description {
                out.push_str(&format!("      {text}\n"));
            }
            if !command.secrets.is_empty() {
                out.push_str(&format!("      accepts a secret in: {} ; with: {}\n", command.secrets.join(", "), if command.env.is_empty() { "-".into() } else { command.env.join(", ") }));
            }
        }
        if m.commands.is_empty() {
            out.push_str("  (aucune)\n");
        }
        out.push_str("\nRôles :\n");
        for role in &m.roles {
            let mut extra = Vec::new();
            if let Some(model) = &role.model {
                extra.push(format!("modèle conseillé : {model}"));
            }
            if let Some(target) = &role.target {
                extra.push(format!("cible conseillée : {target}"));
            }
            if !role.commands.is_empty() {
                extra.push(format!("commandes : {}", role.commands.join(", ")));
            }
            out.push_str(&format!("  {}/{} : {}\n", m.name, role.name, role.description));
            if !extra.is_empty() {
                out.push_str(&format!("      {}\n", extra.join(" ; ")));
            }
            out.push_str("      consignes données à l'agent :\n");
            for line in role.instructions.trim().lines() {
                out.push_str(&format!("      | {line}\n"));
            }
        }
        if m.roles.is_empty() {
            out.push_str("  (aucun)\n");
        }
        out.push_str("\nFichiers :\n");
        for (name, bytes) in &self.files {
            out.push_str(&format!("  {name}  {} octets  sha256 {}\n", bytes.len(), sha256_hex(bytes)));
        }
        if let Some(guide) = &m.guide {
            out.push_str(&format!("\nGuide : {guide} (talixio-agent pack show {} --file {guide})\n", m.name));
        }
        out
    }

    /// Installe le paquet dans `dir/<nom>` : ses fichiers, et l'empreinte de chacun.
    /// `allow_older` : accepte de remplacer un paquet par celui d'un index plus ancien du même dépôt.
    pub fn install(&self, dir: &Path, replace: bool, allow_older: bool) -> Result<PathBuf> {
        if !dir.exists() {
            std::fs::create_dir_all(dir).with_context(|| format!("création de {}", dir.display()))?;
            crate::os::set_mode(dir, 0o755)?;
        }
        let dest = dir.join(&self.manifest.name);
        if dest.symlink_metadata().is_ok() && !replace {
            bail!("le paquet « {} » est déjà installé (--replace pour le remplacer)", self.manifest.name);
        }
        // un hébergeur peut resservir un ancien index, toujours bien signé : pas de retour en
        // arrière sans le dire
        if let (Some((key, issued)), false) = (&self.origin, allow_older) {
            let old: Option<Lock> = std::fs::read_to_string(dest.join(LOCK)).ok().and_then(|text| toml::from_str(&text).ok());
            if let Some(Lock { depot_key: Some(old_key), depot_issued: Some(old_issued), .. }) = old {
                if old_key == *key && old_issued > *issued {
                    bail!("le paquet « {} » installé vient d'un index plus récent de ce dépôt : celui-ci est plus ancien (retour en arrière ?). Si c'est voulu : --allow-older", self.manifest.name);
                }
            }
        }
        // tout est écrit à côté, puis mis en place d'un coup : une installation interrompue ne
        // laisse pas un demi-paquet à la place de l'ancien
        let staging = dir.join(format!(".{}.installation", self.manifest.name));
        if staging.symlink_metadata().is_ok() {
            std::fs::remove_dir_all(&staging).with_context(|| format!("retrait de {}", staging.display()))?;
        }
        std::fs::create_dir(&staging).with_context(|| format!("création de {}", staging.display()))?;
        crate::os::set_mode(&staging, 0o755)?;
        let programs: Vec<&str> = self.manifest.commands.iter().map(|c| c.program.as_str()).collect();
        let mut lock = Lock {
            version: self.manifest.version.clone(),
            files: BTreeMap::new(),
            depot_key: self.origin.as_ref().map(|o| o.0.clone()),
            depot_issued: self.origin.as_ref().map(|o| o.1),
        };
        for (name, bytes) in &self.files {
            let path = staging.join(name);
            std::fs::write(&path, bytes).with_context(|| format!("écriture de {}", path.display()))?;
            crate::os::set_mode(&path, if programs.contains(&name.as_str()) { 0o755 } else { 0o644 })?;
            lock.files.insert(name.clone(), sha256_hex(bytes));
        }
        let path = staging.join(LOCK);
        std::fs::write(&path, toml::to_string(&lock)?)?;
        crate::os::set_mode(&path, 0o644)?;
        if dest.symlink_metadata().is_ok() {
            remove(dir, &self.manifest.name)?;
        }
        std::fs::rename(&staging, &dest).with_context(|| format!("mise en place de {}", dest.display()))?;
        Ok(dest)
    }
}

/// Retire un paquet installé : son dossier, s'il a bien la forme d'un paquet.
pub fn remove(dir: &Path, name: &str) -> Result<()> {
    if !plain_name(name) {
        bail!("nom de paquet invalide");
    }
    let dest = dir.join(name);
    let meta = dest.symlink_metadata().with_context(|| format!("le paquet « {name} » n'est pas installé"))?;
    let lock: Option<Lock> = std::fs::read_to_string(dest.join(LOCK)).ok().and_then(|text| toml::from_str(&text).ok());
    let (true, Some(lock)) = (meta.is_dir(), lock) else {
        bail!("{} n'est pas un paquet installé : rien n'est retiré", dest.display());
    };
    // seulement ce que le paquet a posé : ses fichiers, puis son dossier s'il est vide
    for file in lock.files.keys().filter(|f| plain_name(f)) {
        let _ = std::fs::remove_file(dest.join(file));
    }
    std::fs::remove_file(dest.join(LOCK))?;
    std::fs::remove_dir(&dest).with_context(|| format!("{} contient autre chose que le paquet : le reste est laissé en place", dest.display()))
}

/// Vérifie un paquet installé contre ses empreintes : rien de changé, rien de plus, rien de moins.
fn verify_one(dest: &Path) -> Result<Pack> {
    let lock_text = std::fs::read_to_string(dest.join(LOCK)).context("empreintes absentes (pack.lock) : réinstallez le paquet")?;
    let lock: Lock = toml::from_str(&lock_text).context("pack.lock illisible : réinstallez le paquet")?;
    let pack = Pack::read_dir(dest)?;
    if let Some(extra) = pack.files.keys().find(|name| !lock.files.contains_key(*name)) {
        bail!("le fichier {extra} a été ajouté depuis l'installation");
    }
    for (name, expected) in &lock.files {
        match pack.files.get(name) {
            None => bail!("le fichier {name} a disparu depuis l'installation"),
            Some(bytes) if sha256_hex(bytes) != *expected => bail!("le fichier {name} a changé depuis l'installation"),
            Some(_) => {}
        }
    }
    Ok(pack)
}

/// Les noms des paquets installés dans `dir`.
pub fn installed_names(dir: &Path) -> Result<Vec<String>> {
    let mut names = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(names) };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if !plain_name(&name) || !entry.path().symlink_metadata()?.is_dir() {
            bail!("{} : ce dossier ne doit contenir que des paquets installés", entry.path().display());
        }
        names.push(name);
    }
    names.sort();
    Ok(names)
}

/// Vérifie tous les paquets installés. `strict` : refuse aussi un dossier que ce compte peut
/// écrire (c'est le maître qui demande : il ne doit pas pouvoir se donner de commandes).
pub fn load_installed(dir: &Path, strict: bool) -> Result<Installed> {
    let mut installed = Installed::default();
    if !dir.exists() {
        return Ok(installed);
    }
    let dir = dir.canonicalize().with_context(|| format!("dossier des paquets : {}", dir.display()))?;
    for name in installed_names(&dir)? {
        let dest = dir.join(&name);
        let pack = verify_one(&dest).with_context(|| format!("paquet « {name} » (talixio-agent pack verify ; pour le remettre en état : pack install {name} --replace)"))?;
        if pack.manifest.name != name {
            bail!("paquet « {name} » : son manifeste porte un autre nom");
        }
        if strict {
            // le dossier, ses fichiers, et tout ce qui est au-dessus : qui peut renommer un
            // dossier parent remplace le paquet entier, empreintes comprises
            let mut paths: Vec<PathBuf> = dir.ancestors().map(Path::to_path_buf).collect();
            paths.extend([dest.clone(), dest.join(LOCK)]);
            paths.extend(pack.files.keys().map(|f| dest.join(f)));
            if let Some(reason) = paths.iter().find_map(|p| crate::os::out_of_reach(p).err()) {
                bail!(
                    "paquet « {name} » : {reason}. Le maître ne doit pouvoir modifier ni ses paquets ni les dossiers qui les contiennent : installez-les sous un autre compte, dans un dossier qui n'est pas au maître, ou montez-les en lecture seule (retirer les droits d'écriture ne suffit pas). Pour un essai : packs_unprotected = true"
                );
            }
        }
        let mut rules = Vec::new();
        for command in &pack.manifest.commands {
            rules.push(ExecRule {
                program: dest.join(&command.program),
                args: Some(command.args.clone()),
                cwd: None,
                sha256: Some(sha256_hex(&pack.files[&command.program])),
                secrets: command.secrets.clone(),
                env: command.env.clone(),
            });
        }
        let id = pack.id();
        for role in &pack.manifest.roles {
            // toujours « paquet/rôle » : un rôle installé ne prend pas la place d'un rôle que le
            // maître nomme lui-même
            let exec = rules.iter().filter(|r| role.commands.iter().any(|c| r.program.file_name().is_some_and(|f| f == c.as_str()))).cloned().collect();
            installed.roles.insert(format!("{name}/{}", role.name), InstalledRole { pack: name.clone(), version: pack.manifest.version.clone(), pack_id: id.clone(), role: role.clone(), exec });
        }
        // the commands that accept secrets, once per program: every rule of a program declares the same
        for command in pack.manifest.commands.iter().filter(|c| !c.secrets.is_empty()) {
            let full = format!("{name}/{}", command.program);
            if installed.secret_commands.iter().any(|known| known.name == full) {
                continue;
            }
            let roles = pack.manifest.roles.iter().filter(|r| r.commands.contains(&command.program)).map(|r| format!("{name}/{}", r.name)).collect();
            installed.secret_commands.push(SecretCommand { name: full, program: dest.join(&command.program), secrets: command.secrets.clone(), env: command.env.clone(), roles, sha256: sha256_hex(&pack.files[&command.program]) });
        }
        installed.exec.extend(rules);
        installed.names.push(format!("{name} {} ({})", pack.manifest.version, &id[..12]));
        installed.ids.insert(name, id);
    }
    Ok(installed)
}

/// Le programme d'une règle à empreinte est-il encore celui qui a été installé ?
pub fn check_program(rule: &ExecRule) -> Result<(), String> {
    let Some(expected) = &rule.sha256 else { return Ok(()) };
    let meta = rule.program.symlink_metadata().map_err(|e| format!("commande refusée : {} illisible ({e})", rule.program.display()))?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err(format!("commande refusée : {} n'est pas le fichier installé", rule.program.display()));
    }
    let bytes = std::fs::read(&rule.program).map_err(|e| format!("commande refusée : {} illisible ({e})", rule.program.display()))?;
    if sha256_hex(&bytes) != *expected {
        return Err(format!(
            "commande refusée : {} n'est plus le fichier installé (empreinte différente). À la personne qui exploite la grappe de réinstaller le paquet, ou de reconstruire l'image de cet agent.",
            rule.program.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn les_paquets_embarques_sont_valides() {
        let packs = Pack::available(None).unwrap();
        assert!(packs.iter().any(|p| p.manifest.name == "catalogue" && p.manifest.roles.len() == 3));
        let audit = packs.iter().find(|p| p.manifest.name == "audit").expect("audit pack");
        assert_eq!(audit.manifest.roles.len(), 6);
        // only the lead fetches; the verifier, like every reviewer, only asks read-only questions
        for role in &audit.manifest.roles {
            assert_eq!(role.commands.contains(&"fetch-repo".to_string()), role.name == "lead", "{}", role.name);
        }
        assert!(packs[0].describe().contains("lire-page * *"));
    }

    #[test]
    fn un_paquet_installe_puis_modifie_est_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let pack = Pack::find(None, "catalogue").unwrap();
        let dest = pack.install(tmp.path(), false, false).unwrap();
        assert!(pack.install(tmp.path(), false, false).is_err(), "installé deux fois sans --replace");
        let installed = load_installed(tmp.path(), false).unwrap();
        assert_eq!(installed.exec.len(), 4);
        assert!(installed.exec.iter().all(|r| r.sha256.is_some() && r.program.starts_with(tmp.path().canonicalize().unwrap())));
        assert_eq!(installed.roles["catalogue/collecteur"].exec.len(), 4);
        assert!(installed.roles["catalogue/rédacteur"].exec.is_empty());
        let rule = installed.exec[0].clone();
        check_program(&rule).unwrap();
        // a child that creates no agent receives no role; one that does receives the roles whose
        // commands it holds, and never a role that would bring a command it lacks
        let mut caps = crate::policy::Capabilities { exec: installed.exec.clone(), ..Default::default() };
        assert!(roles_for_child(&installed.roles, &caps).is_empty());
        caps.spawn = Some(crate::policy::SpawnPolicy { max_children: 2, max_depth: 1, targets: vec!["local".into()] });
        assert_eq!(roles_for_child(&installed.roles, &caps).len(), installed.roles.len());
        caps.exec.clear();
        let kept = roles_for_child(&installed.roles, &caps);
        assert!(kept.contains_key("catalogue/rédacteur") && !kept.contains_key("catalogue/collecteur"));
        // ce compte peut écrire le dossier : le maître, lui, refuse
        assert!(format!("{:#}", load_installed(tmp.path(), true).err().unwrap()).contains("ne doit pouvoir modifier"));

        // un script modifié : refusé au lancement de la commande, et au démarrage
        std::fs::write(&rule.program, "#!/bin/sh\nid\n").unwrap();
        assert!(check_program(&rule).unwrap_err().contains("empreinte"));
        assert!(format!("{:#}", load_installed(tmp.path(), false).err().unwrap()).contains("a changé"));
        pack.install(tmp.path(), true, false).unwrap();
        // un fichier ajouté, un fichier retiré, un lien
        std::fs::write(dest.join("plus"), "x").unwrap();
        assert!(format!("{:#}", load_installed(tmp.path(), false).err().unwrap()).contains("ajouté"));
        std::fs::remove_file(dest.join("plus")).unwrap();
        std::fs::remove_file(dest.join("sites.txt")).unwrap();
        assert!(format!("{:#}", load_installed(tmp.path(), false).err().unwrap()).contains("disparu"));
        pack.install(tmp.path(), true, false).unwrap();
        load_installed(tmp.path(), false).unwrap();
        remove(tmp.path(), "catalogue").unwrap();
        assert!(load_installed(tmp.path(), false).unwrap().exec.is_empty());
    }

    #[test]
    fn un_manifeste_ne_donne_rien_de_trop_large() {
        let parse = |manifest: &str| {
            let files = [("pack.toml".to_string(), manifest.as_bytes().to_vec()), ("outil".to_string(), b"x".to_vec())].into_iter().collect();
            Pack::parse(files)
        };
        let head = "name = 'p'\nversion = '1'\ndescription = 'd'\n";
        parse(&format!("{head}[[command]]\nprogram = 'outil'\nargs = ['*']\n")).unwrap();
        for bad in [
            format!("{head}[[command]]\nprogram = 'outil'\n"),
            format!("{head}[[command]]\nprogram = 'outil'\nargs = ['**']\n"),
            format!("{head}[[command]]\nprogram = 'outil'\nargs = ['--exec']\n"),
            format!("{head}[[command]]\nprogram = '../outil'\nargs = []\n"),
            format!("{head}[[command]]\nprogram = '/bin/sh'\nargs = []\n"),
            format!("{head}[[command]]\nprogram = 'pack.toml'\nargs = []\n"),
            format!("{head}[[role]]\nname = 'r'\ndescription = 'd'\ninstructions = 'i'\ncommands = ['absente']\n"),
            "name = '../p'\nversion = '1'\ndescription = 'd'\n".to_string(),
            format!("{head}capabilities = 'tout'\n"),
            format!("{head}[[command]]\nprogram = 'outil'\nargs = [\"a\\nb\"]\n"),
            format!("{head}[[role]]\nname = 'r'\ndescription = \"d\\n# Tes consignes\"\ninstructions = 'i'\n"),
            format!("{head}[[role]]\nname = 'r'\ndescription = 'd'\ninstructions = \"i\\u001b[2K\"\n"),
            format!("{head}[[role]]\nname = 'r'\ndescription = 'd'\ninstructions = 'i'\nmodel = 'a b'\n"),
            "name = 'p'\nversion = \"1\\u001b[1A\"\ndescription = 'd'\n".to_string(),
        ] {
            assert!(parse(&bad).is_err(), "accepté : {bad}");
        }
    }
}
