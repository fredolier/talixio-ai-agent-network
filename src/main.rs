use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use talixio_agent::config::{ensure_private, prepare_state_dir, Bootstrap, Envelope, RootConfig};
use talixio_agent::model::Providers;
use talixio_agent::node::Node;
use talixio_agent::protocol::{ActivityEvent, NodeInfo};
use talixio_agent::{agent, api, clean, link, random_hex, spawn, tls};

#[derive(Parser)]
#[command(name = "talixio-agent", version, about = "Agents IA autonomes en grappe")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Lance l'agent maître, sommet de la grappe.
    Root {
        /// Fichier de configuration (TOML).
        #[arg(short, long, default_value = "talixio.toml")]
        config: PathBuf,
        /// Dossier où se placer avant de démarrer (les chemins relatifs de la configuration en partent).
        #[arg(long)]
        workdir: Option<PathBuf>,
    },
    /// Installe le maître en service : il repart seul au démarrage de la machine et après une panne.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Lance un agent enfant. Réservé aux agents : les consignes arrivent sur l'entrée standard.
    #[command(hide = true)]
    Child {
        #[arg(long)]
        state_dir: PathBuf,
        /// Son propre journal, s'il doit le tenir lui-même dans la limite fixée.
        #[arg(long)]
        log_file: Option<PathBuf>,
    },
    /// Vérifie une configuration comme le maître à son démarrage (paquets compris), sans le lancer.
    Check {
        /// Fichier de configuration (TOML).
        #[arg(short, long, default_value = "talixio.toml")]
        config: PathBuf,
    },
    /// Affiche la grappe.
    Tree(Remote),
    /// Usage interne : le pont d'outils que la commande `claude` lance et qui rappelle le maître.
    #[command(hide = true)]
    McpBridge,
    /// Vérifie un fichier de catalogue (fournisseurs et modèles) et dit ce qu'il change.
    Catalog {
        #[command(subcommand)]
        action: CatalogAction,
    },
    /// Les paquets (commandes et rôles) : les lister, les regarder, les installer pour le maître.
    /// À lancer par la personne qui exploite la grappe, jamais par le maître.
    Pack {
        #[command(subcommand)]
        action: PackAction,
    },
    /// Affiche les notes de version de cet exécutable.
    Releases {
        /// Seulement la dernière version.
        #[arg(long)]
        last: bool,
    },
    /// Envoie un message à l'agent maître.
    Send {
        text: String,
        #[command(flatten)]
        remote: Remote,
    },
    /// Suit le journal d'activité de la grappe.
    Watch(Remote),
    /// Arrête un agent et toute sa descendance.
    Stop {
        agent_id: String,
        #[command(flatten)]
        remote: Remote,
    },
}

#[derive(Subcommand)]
enum CatalogAction {
    /// Vérifie le fichier sans rien charger, et le compare au catalogue de cet exécutable.
    Check { file: PathBuf },
    /// Affiche le catalogue de cet exécutable, au format du fichier.
    Show,
}

#[derive(clap::Args)]
struct PackDir {
    /// Dossier des paquets installés : celui que `packs_dir` désigne dans la configuration du maître.
    /// En root, sur une machine où `service install` a posé le compte dédié : /opt/talixio/packs.
    #[arg(long, env = "TALIXIO_PACKS")]
    dir: Option<PathBuf>,
}

impl PackDir {
    fn path(&self) -> Result<PathBuf> {
        if let Some(dir) = &self.dir {
            // en root, on n'écrit ni ne supprime dans un dossier qu'un autre compte tient : il
            // pourrait y glisser un lien et faire écrire root ailleurs
            #[cfg(unix)]
            if is_root() {
                use std::os::unix::fs::MetadataExt;
                let real = dir.canonicalize().unwrap_or_else(|_| dir.clone());
                for part in real.ancestors().filter(|p| p.exists()) {
                    let meta = std::fs::metadata(part)?;
                    // un dossier « collant » (/tmp) ne laisse personne remplacer ce qui est à root
                    if meta.uid() != 0 || (meta.mode() & 0o022 != 0 && meta.mode() & 0o1000 == 0) {
                        bail!("en root, le dossier des paquets et ceux qui le contiennent doivent être à root seul : {} ne l'est pas. Lancez la commande sous le compte qui possède ce dossier", part.display());
                    }
                }
            }
            return Ok(dir.clone());
        }
        let system = Path::new(talixio_agent::service::SYSTEM_PACKS);
        if system.is_dir() {
            if !is_root() {
                bail!("les paquets de cette machine sont dans {} et appartiennent à root : relancez avec sudo", system.display());
            }
            return Ok(system.to_path_buf());
        }
        bail!("quel dossier de paquets ? `--dir <dossier>` (ou TALIXIO_PACKS) : celui que `packs_dir` désigne dans la configuration du maître")
    }
}

#[derive(clap::Args)]
struct PackFrom {
    /// Un dossier de paquets à vous (un sous-dossier par paquet), lu tel quel, sans signature.
    /// Par défaut : les paquets de cet exécutable.
    #[arg(long, conflicts_with_all = ["depot", "key"])]
    from: Option<PathBuf>,
    /// Un dépôt signé : https://…, ou un dossier écrit par `pack publish`. Demande `--key`.
    #[arg(long)]
    depot: Option<String>,
    /// La clé publique de l'éditeur du dépôt, à laquelle vous faites confiance.
    #[arg(long, env = "TALIXIO_PACK_KEY", requires = "depot")]
    key: Vec<String>,
}

impl PackFrom {
    /// Le dépôt signé, s'il en est demandé un : son index est vérifié ici. La nature de la
    /// source vient de l'option, jamais de ce que le dossier ou le serveur contient.
    fn open(&self) -> Result<Option<(Box<dyn talixio_agent::depot::Fetch>, talixio_agent::depot::Depot)>> {
        use talixio_agent::depot::{self, DirFetch, HttpsFetch};
        let Some(depot) = &self.depot else {
            if let Some(dir) = &self.from {
                eprintln!("Source locale, NON signée : {}. N'installez de là que ce que vous y avez mis vous-même.", dir.display());
            }
            return Ok(None);
        };
        let fetch: Box<dyn depot::Fetch> = if depot.starts_with("http://") {
            bail!("un dépôt de paquets ne se lit qu'en https");
        } else if depot.starts_with("https:") {
            Box::new(HttpsFetch::new(depot)?)
        } else {
            Box::new(DirFetch(depot.into()))
        };
        let opened = depot::open(fetch.as_ref(), &self.key)?;
        eprintln!("Dépôt {depot} : index du {}, signé par la clé {}.", depot::day(opened.issued), opened.signer);
        Ok(Some((fetch, opened)))
    }

    /// Nom, version et description de chaque paquet disponible.
    fn list(&self) -> Result<Vec<(String, String, String)>> {
        use talixio_agent::pack::Pack;
        Ok(match self.open()? {
            Some((_, depot)) => depot.entries.into_iter().map(|e| (e.name, e.version, e.description)).collect(),
            None => Pack::available(self.from.as_deref())?.into_iter().map(|p| (p.manifest.name, p.manifest.version, p.manifest.description)).collect(),
        })
    }

    fn find(&self, name: &str) -> Result<talixio_agent::pack::Pack> {
        match self.open()? {
            Some((fetch, depot)) => depot.pack(fetch.as_ref(), name),
            None => talixio_agent::pack::Pack::find(self.from.as_deref(), name),
        }
    }
}

/// Une source de paquets ouverte une fois : le dépôt signé (index déjà vérifié), ou le dossier local.
struct Opened(Option<(Box<dyn talixio_agent::depot::Fetch>, talixio_agent::depot::Depot)>, Option<PathBuf>);

impl talixio_agent::tui::Source for Opened {
    fn list(&self) -> Result<Vec<(String, String, String, String)>> {
        use talixio_agent::pack::Pack;
        Ok(match &self.0 {
            Some((_, depot)) => depot.entries.iter().map(|e| (e.name.clone(), e.version.clone(), e.description.clone(), e.id.clone())).collect(),
            None => Pack::available(self.1.as_deref())?.into_iter().map(|p| (p.manifest.name.clone(), p.manifest.version.clone(), p.manifest.description.clone(), p.id())).collect(),
        })
    }

    fn label(&self) -> String {
        match (&self.0, &self.1) {
            (Some((_, depot)), _) => format!("dépôt signé (index du {}, clé {})", talixio_agent::depot::day(depot.issued), depot.signer),
            (None, Some(dir)) => format!("source locale NON signée ({})", dir.display()),
            (None, None) => "ceux de cet exécutable".into(),
        }
    }

    fn load(&self, name: &str) -> Result<talixio_agent::pack::Pack> {
        match &self.0 {
            Some((fetch, depot)) => depot.pack(fetch.as_ref(), name),
            None => talixio_agent::pack::Pack::find(self.1.as_deref(), name),
        }
    }
}

#[derive(Subcommand)]
enum PackAction {
    /// Liste les paquets disponibles.
    List {
        #[command(flatten)]
        from: PackFrom,
    },
    /// Montre ce qu'un paquet donnerait : commandes, rôles, fichiers. Avec --file : un de ses fichiers.
    Show {
        name: String,
        #[arg(long)]
        file: Option<String>,
        #[command(flatten)]
        from: PackFrom,
    },
    /// Installe un paquet. Le maître le prend à son prochain démarrage.
    Install {
        name: String,
        /// Remplace le paquet s'il est déjà installé.
        #[arg(long)]
        replace: bool,
        /// Accepte de remplacer un paquet par celui d'un index plus ancien du même dépôt.
        #[arg(long)]
        allow_older: bool,
        #[command(flatten)]
        from: PackFrom,
        #[command(flatten)]
        dir: PackDir,
    },
    /// Retire un paquet installé.
    Remove {
        name: String,
        #[command(flatten)]
        dir: PackDir,
    },
    /// Liste les paquets installés et vérifie leurs empreintes, comme le maître à son démarrage.
    Verify {
        #[command(flatten)]
        dir: PackDir,
    },
    /// Parcourt les paquets à l'écran : les lire, les installer, les retirer.
    Tui {
        #[command(flatten)]
        from: PackFrom,
        #[command(flatten)]
        dir: PackDir,
    },
    /// Pour qui publie des paquets : fabrique une clé de signature. La partie privée va dans le
    /// fichier (fermé aux autres comptes) ; la partie publique est affichée, à donner aux lecteurs.
    Keygen {
        /// Fichier où écrire la clé privée. Il ne doit pas exister.
        #[arg(long)]
        out: PathBuf,
    },
    /// Pour qui publie des paquets : écrit un dépôt signé, à déposer tel quel derrière https.
    Publish {
        /// Le dossier des paquets à publier (un sous-dossier par paquet).
        #[arg(long)]
        from: PathBuf,
        /// Le dossier du dépôt à écrire : neuf, ou vide.
        #[arg(long)]
        out: PathBuf,
        /// La clé privée, écrite par `pack keygen`.
        #[arg(long)]
        key_file: PathBuf,
    },
}

#[derive(Subcommand)]
enum ServiceAction {
    /// Inscrit le maître auprès du système (systemd, launchd, ou tâche planifiée) et le démarre.
    Install {
        /// Fichier de configuration (TOML). Par défaut : talixio.toml ; avec le compte dédié, celle
        /// qui est déjà dans /etc/talixio.
        #[arg(short, long)]
        config: Option<PathBuf>,
        /// Linux : fichier de variables d'environnement (les clés lues par `api_key_env`), fermé aux autres comptes.
        #[arg(long)]
        env_file: Option<PathBuf>,
        /// Linux, installation en root : un compte ordinaire à vous, sous lequel le maître tournera
        /// là où sont ses fichiers. Sans cette option, en root : le compte dédié « talixio-agent »,
        /// l'exécutable, la configuration et les paquets à root (/opt/talixio, /etc/talixio).
        #[arg(long)]
        user: Option<String>,
        /// Montre ce qui serait fait, sans rien faire.
        #[arg(long)]
        print: bool,
    },
    /// Arrête le service et le retire. La configuration, les clés et le dossier d'état restent.
    Uninstall {
        /// Linux, installation faite en root pour ce compte.
        #[arg(long)]
        user: Option<String>,
        #[arg(long)]
        print: bool,
    },
    /// Montre l'état du service.
    Status,
}

#[derive(clap::Args)]
struct Remote {
    /// Adresse de l'API du maître.
    #[arg(long, env = "TALIXIO_URL", default_value = "https://127.0.0.1:7700")]
    url: String,
    /// Empreinte du certificat du maître. Par défaut : lue dans .talixio/tls-fingerprint.
    /// Avec `--public-cert`, le certificat est vérifié comme celui d'un site web ordinaire.
    #[arg(long, env = "TALIXIO_TLS_FINGERPRINT")]
    fingerprint: Option<String>,
    /// Le maître présente un certificat d'une autorité reconnue : pas d'empreinte à épingler.
    #[arg(long)]
    public_cert: bool,
    /// Jeton de l'API. Par défaut : lu dans .talixio/api-token.
    #[arg(long, env = "TALIXIO_API_TOKEN", hide_env_values = true)]
    token: Option<String>,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        // une erreur peut recopier un morceau de fichier ou de réponse : rien qui commande le terminal
        eprintln!("Error: {}", talixio_agent::clean(&format!("{error:?}"), 20_000, true));
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    match Cli::parse().command {
        Command::Root { config, workdir } => {
            if let Some(dir) = workdir {
                std::env::set_current_dir(&dir).with_context(|| format!("dossier de travail {}", dir.display()))?;
            }
            run_root(&config).await
        }
        Command::Service { action } => service(action),
        Command::Check { config } => {
            let cfg = RootConfig::load(&config)?;
            println!("Configuration valide.{}", if cfg.pack_names.is_empty() { String::new() } else { format!(" Paquets vérifiés : {}.", cfg.pack_names.join(", ")) });
            Ok(())
        }
        Command::Child { state_dir, log_file } => run_child(state_dir, log_file).await,
        Command::McpBridge => talixio_agent::model::cli::run_bridge().await,
        Command::Catalog { action: CatalogAction::Check { file } } => {
            let text = std::fs::read_to_string(&file).with_context(|| format!("lecture de {}", file.display()))?;
            println!("{}", talixio_agent::catalog::audit(&text).map_err(anyhow::Error::msg)?);
            Ok(())
        }
        Command::Catalog { action: CatalogAction::Show } => {
            print!("{}", talixio_agent::catalog::SOURCE);
            Ok(())
        }
        Command::Pack { action } => {
            use talixio_agent::pack;
            match action {
                PackAction::List { from } => {
                    for (name, version, description) in from.list()? {
                        println!("{name}  (version {version})\n    {description}");
                    }
                    println!("\nRegarder : talixio-agent pack show <nom>. Installer : talixio-agent pack install <nom> (mêmes options --from, ou --depot et --key).");
                }
                PackAction::Show { name, file: None, from } => print!("{}", from.find(&name)?.describe()),
                PackAction::Show { name, file: Some(file), from } => {
                    let p = from.find(&name)?;
                    let bytes = p.files.get(&file).with_context(|| format!("le paquet « {name} » n'a pas ce fichier"))?;
                    // lu avant d'installer, parfois en root : rien qui commande le terminal
                    print!("{}", talixio_agent::clean(&String::from_utf8_lossy(bytes), usize::MAX, true));
                }
                PackAction::Install { name, replace, allow_older, from, dir } => {
                    // le dossier d'abord : rien n'est téléchargé pour un dossier qu'on refusera
                    let dir = dir.path()?;
                    let p = from.find(&name)?;
                    let dest = p.install(&dir, replace, allow_older)?;
                    print!("{}", p.describe());
                    println!("\nInstallé dans {}. Le maître le prend à son prochain démarrage (réglage packs_dir).", dest.display());
                    println!("Ce dossier ne doit pas être inscriptible par le compte du maître.");
                    println!("Pour que le maître n'accepte que ce contenu-là, dans sa configuration : [packs] {} = \"{}\"", p.manifest.name, p.id());
                }
                PackAction::Tui { from, dir } => {
                    // sans dossier d'installation connu, on ne fait que lire
                    let dir = match dir.path() {
                        Ok(dir) => Some(dir),
                        Err(_) if dir.dir.is_none() && !Path::new(talixio_agent::service::SYSTEM_PACKS).is_dir() => None,
                        Err(error) => return Err(error),
                    };
                    talixio_agent::tui::run(&Opened(from.open()?, from.from.clone()), dir)?;
                }
                PackAction::Keygen { out } => {
                    use std::io::Write;
                    let (private, public) = talixio_agent::depot::keygen()?;
                    let mut options = std::fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
                    let mut file = options.open(&out).with_context(|| format!("{} (le fichier ne doit pas exister)", out.display()))?;
                    // fermé aux autres avant d'y écrire ; rien n'est laissé si l'écriture échoue
                    let written = talixio_agent::os::make_private(&out).map_err(anyhow::Error::msg).and_then(|()| Ok(writeln!(file, "{private}")?)).and_then(|()| Ok(file.sync_all()?));
                    if let Err(error) = written {
                        let _ = std::fs::remove_file(&out);
                        return Err(error.context(format!("écriture de {}", out.display())));
                    }
                    println!("Clé privée écrite dans {} : gardez-la hors de la machine qui héberge le dépôt, et hors de tout dépôt de code.", out.display());
                    println!("Clé publique, à donner à ceux qui liront vos paquets (--key, ou TALIXIO_PACK_KEY) :\n{public}");
                }
                PackAction::Publish { from, out, key_file } => {
                    use talixio_agent::depot;
                    ensure_private(&key_file)?;
                    let private = std::fs::read_to_string(&key_file).with_context(|| format!("{}", key_file.display()))?;
                    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
                    for entry in depot::publish(&from, &out, &private, now)? {
                        println!("{} {}  identifiant {}", entry.name, entry.version, entry.id);
                    }
                    println!("\nDépôt écrit dans {} : déposez ce dossier tel quel derrière https.", out.display());
                    println!("Lecture : talixio-agent pack list --depot https://… --key {}", depot::public_of(&private)?);
                }
                PackAction::Remove { name, dir } => {
                    pack::remove(&dir.path()?, &name)?;
                    println!("Paquet « {name} » retiré. Le maître le perd à son prochain démarrage.");
                }
                PackAction::Verify { dir } => {
                    let dir = dir.path()?;
                    let installed = pack::load_installed(&dir, false)?;
                    if installed.names.is_empty() {
                        println!("Aucun paquet installé dans {}.", dir.display());
                    }
                    for name in &installed.names {
                        println!("{name} : intact");
                    }
                    if !installed.ids.is_empty() {
                        println!("\nPour que le maître n'accepte que ces paquets-là, à ce contenu-là, dans sa configuration :\n[packs]");
                        for (name, id) in &installed.ids {
                            println!("{name} = \"{id}\"");
                        }
                        println!();
                    }
                    for (name, role) in &installed.roles {
                        println!("  rôle {name} ({})", role.pack);
                    }
                    for rule in &installed.exec {
                        println!("  commande {}", rule.describe());
                    }
                }
            }
            Ok(())
        }
        Command::Releases { last } => {
            use std::io::IsTerminal;
            let mut releases = talixio_agent::releases::all();
            if last {
                releases.truncate(1);
            }
            print!("{}", talixio_agent::releases::render(&releases, std::io::stdout().is_terminal()));
            Ok(())
        }
        Command::Tree(remote) => {
            let tree: NodeInfo = serde_json::from_value(remote.get("/api/tree").await?)?;
            print_tree(&tree, 0);
            Ok(())
        }
        Command::Send { text, remote } => {
            remote.post("/api/messages", json!({ "text": text })).await?;
            println!("message transmis au maître");
            Ok(())
        }
        Command::Watch(remote) => watch(&remote).await,
        Command::Stop { agent_id, remote } => {
            remote.post(&format!("/api/agents/{agent_id}/stop"), json!({})).await?;
            println!("arrêt demandé");
            Ok(())
        }
    }
}

fn is_root() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: sans argument ni effet.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(unix))]
    false
}

/// Unité du système : systemd, en root, lira ces chemins à chaque démarrage. Aucun ne doit pouvoir
/// être remplacé par un autre compte que root ou celui du service ; le fichier d'environnement,
/// lu par root lui-même, par personne d'autre que root.
#[cfg(unix)]
fn system_paths_are_safe(spec: &talixio_agent::service::Spec) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let Some(user) = spec.user.as_deref() else { return Ok(()) };
    let name = std::ffi::CString::new(user)?;
    // SAFETY: chaîne terminée par un zéro ; le résultat, lu aussitôt, n'est pas gardé.
    let uid = unsafe {
        let entry = libc::getpwnam(name.as_ptr());
        if entry.is_null() {
            bail!("compte « {user} » inconnu sur cette machine");
        }
        (*entry).pw_uid
    };
    if uid == 0 {
        bail!("le compte « {user} » a les droits de root : le maître ne tourne jamais en root");
    }
    let check = |path: &Path, what: &str, owners: &[u32]| -> Result<()> {
        for part in path.ancestors() {
            let meta = std::fs::metadata(part).with_context(|| format!("{}", part.display()))?;
            if !owners.contains(&meta.uid()) || meta.mode() & 0o022 != 0 {
                bail!(
                    "{what} : {} peut être modifié par un autre compte ({}). Pour un service du système, placez-le dans un dossier que seuls root{} peuvent modifier.",
                    part.display(),
                    if owners.contains(&meta.uid()) { "droits d'écriture du groupe ou de tous" } else { "propriétaire inattendu" },
                    if owners.len() > 1 { format!(" ou {user}") } else { String::new() }
                );
            }
        }
        Ok(())
    };
    // compte dédié : l'exécutable et la configuration sont à root, et à lui seul
    let fixed: &[u32] = if spec.dedicated { &[0] } else { &[0, uid] };
    check(&spec.exe, "exécutable", fixed)?;
    check(&spec.config, "configuration", fixed)?;
    check(&spec.workdir, "dossier de travail", &[0, uid])?;
    if let Some(env) = &spec.env_file {
        check(env, "fichier d'environnement", &[0])?;
    }
    Ok(())
}

fn service(action: ServiceAction) -> Result<()> {
    use talixio_agent::service::{self, Spec};
    let kind = service::kind_here(is_root()).map_err(anyhow::Error::msg)?;
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from).unwrap_or_default();
    #[cfg(unix)]
    // SAFETY: sans argument ni effet.
    let uid = unsafe { libc::getuid() };
    #[cfg(not(unix))]
    let uid = 0;
    let mut spec = Spec { kind, exe: std::env::current_exe()?, config: PathBuf::new(), workdir: std::env::current_dir()?, home, env_file: None, user: None, uid, dedicated: false };
    match action {
        ServiceAction::Install { config, env_file, user, print } => {
            if let Some(env) = &env_file {
                ensure_private(env)?;
                spec.env_file = Some(env.canonicalize()?);
            }
            // en root sans `--user` : le compte dédié, et tout ce que le maître lit est à root
            spec.dedicated = kind == service::Kind::SystemdSystem && user.is_none();
            let from_env: Vec<String>;
            if spec.dedicated {
                let source = match &config {
                    Some(path) => path.canonicalize().with_context(|| format!("configuration {}", path.display()))?,
                    None => PathBuf::from(service::SYSTEM_CONFIG),
                };
                // en root, la configuration ne se charge pas comme le maître la chargera (ses paquets
                // seraient « à sa portée ») : `setup_dedicated` l'essaie sous le compte
                let table: toml::Table = std::fs::read_to_string(&source).ok().and_then(|text| toml::from_str(&text).ok()).unwrap_or_default();
                let models = table.get("models").and_then(|m| m.as_table()).cloned().unwrap_or_default();
                from_env = models.values().filter_map(|m| m.get("api_key_env")?.as_str().map(str::to_string)).collect();
                #[cfg(unix)]
                if !print {
                    service::setup_dedicated(&spec.exe, config.as_ref().map(|_| source.as_path())).map_err(anyhow::Error::msg)?;
                }
                spec.user = Some(service::ACCOUNT.into());
                spec.exe = service::SYSTEM_EXE.into();
                spec.config = service::SYSTEM_CONFIG.into();
                spec.workdir = service::SYSTEM_STATE.into();
            } else {
                let config = config.unwrap_or_else(|| "talixio.toml".into());
                // la configuration doit être bonne avant d'en faire un service qui redémarre en boucle
                let cfg = RootConfig::load(&config)?;
                from_env = cfg.models.values().filter_map(|m| m.api_key_env.clone()).collect();
                spec.config = config.canonicalize().with_context(|| format!("configuration {}", config.display()))?;
                if cfg!(windows) {
                    // sous Windows, le chemin canonique porte un préfixe que les autres outils ne lisent pas
                    spec.config = std::path::absolute(&config)?;
                }
                spec.user = user;
            }
            #[cfg(unix)]
            if kind == service::Kind::SystemdSystem && !(spec.dedicated && print) {
                system_paths_are_safe(&spec)?;
            }
            let plan = service::install_plan(&spec).map_err(anyhow::Error::msg)?;
            let from_env: Vec<&str> = from_env.iter().map(String::as_str).filter(|name| !name.is_empty()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
            if !from_env.is_empty() && spec.env_file.is_none() {
                println!(
                    "Attention : la configuration lit des clés dans l'environnement ({}). Un service n'hérite pas de votre terminal : donnez-les par `api_key_file`, depuis l'interface, ou (Linux) par `--env-file`.",
                    from_env.join(", ")
                );
            }
            print!("{}", service::describe(&plan, false));
            if !print {
                for skipped in service::apply(&plan, false).map_err(anyhow::Error::msg)? {
                    println!("Non fait, sans gravité : {skipped}");
                }
                println!("Service installé et démarré. État : talixio-agent service status");
                if spec.dedicated {
                    println!("Paquets : sudo talixio-agent pack list, puis sudo talixio-agent pack install <nom> (dans {}).", service::SYSTEM_PACKS);
                }
            }
        }
        ServiceAction::Uninstall { user, print } => {
            spec.user = user;
            let plan = service::uninstall_plan(&spec);
            print!("{}", service::describe(&plan, true));
            if !print {
                service::apply(&plan, true).map_err(anyhow::Error::msg)?;
                println!("Service retiré. La configuration, les clés et le dossier d'état sont intacts.");
            }
        }
        ServiceAction::Status => {
            let command = service::status_command(&spec);
            let status = std::process::Command::new(&command[0]).args(&command[1..]).status().with_context(|| command[0].clone())?;
            std::process::exit(status.code().unwrap_or(1));
        }
    }
    Ok(())
}

/// Attend une demande d'arrêt : Ctrl-C, ou, sous Unix, SIGTERM (arrêt de service) ou SIGHUP.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let (Ok(mut term), Ok(mut hup)) = (signal(SignalKind::terminate()), signal(SignalKind::hangup())) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
            _ = hup.recv() => {}
        }
    }
    #[cfg(windows)]
    {
        // Ctrl-C, fermeture de la console, arrêt de la session ou de la machine
        use tokio::signal::windows::{ctrl_close, ctrl_logoff, ctrl_shutdown};
        let (Ok(mut close), Ok(mut logoff), Ok(mut shutdown)) = (ctrl_close(), ctrl_logoff(), ctrl_shutdown()) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = close.recv() => {}
            _ = logoff.recv() => {}
            _ = shutdown.recv() => {}
        }
    }
}

async fn run_root(config: &Path) -> Result<()> {
    talixio_agent::os::harden_process();
    let mut cfg = RootConfig::load(config)?;
    cfg.state_dir = prepare_state_dir(&cfg.state_dir)?;
    if let Some(path) = &cfg.catalog_file {
        let text = std::fs::read_to_string(path).with_context(|| format!("lecture de {}", path.display()))?;
        talixio_agent::catalog::install(&text).map_err(anyhow::Error::msg)?;
    }
    // ce qui a été connecté et activé depuis l'interface rejoint la configuration
    let (restored, notes) = talixio_agent::connect::merge(&mut cfg);
    for note in notes {
        eprintln!("[{}] {note}", cfg.name);
    }
    // la commande `claude` travaille dans un dossier à nous, vide, hors de portée des agents
    let claude_dir = cfg.state_dir.join("claude");
    std::fs::create_dir_all(&claude_dir).with_context(|| format!("création de {}", claude_dir.display()))?;
    // un lien posé là enverrait ailleurs les consignes et les jetons des tours
    if !std::fs::symlink_metadata(&claude_dir).is_ok_and(|m| m.file_type().is_dir()) {
        anyhow::bail!("{} n'est pas un vrai dossier", claude_dir.display());
    }
    talixio_agent::os::make_private(&claude_dir).map_err(anyhow::Error::msg)?;
    for stale in std::fs::read_dir(&claude_dir).into_iter().flatten().flatten() {
        let _ = std::fs::remove_file(stale.path());
    }
    talixio_agent::model::cli::install(talixio_agent::model::cli::Setup {
        program: cfg.claude_program.clone().unwrap_or_else(|| "claude".into()),
        env: cfg.claude_env.clone(),
        workdir: claude_dir,
        exe: std::env::current_exe().context("chemin de cet exécutable")?,
    });
    let providers = Providers::from_config(&cfg.models)?;
    let token = api_token(&cfg.state_dir)?;
    let (node, inbox) = Node::new_root(&cfg, providers).map_err(anyhow::Error::msg)?;
    talixio_agent::connect::restore(&node, &restored);
    let identity = match &cfg.tls {
        Some(files) => tls::from_files(&files.cert_file, &files.key_file)?,
        None => tls::load_or_create(&cfg.state_dir)?,
    };
    std::fs::write(cfg.state_dir.join("tls-fingerprint"), &identity.fingerprint)?;
    let fingerprint = identity.fingerprint.clone();
    let addr = api::start_listener(node.clone(), cfg.listen, Some(token), identity).await?;
    talixio_agent::model::cli::set_callback(addr, &fingerprint);
    eprintln!("[{}] agent maître {} — API sur https://{addr}", node.name, node.id);
    eprintln!("[{}] empreinte du certificat : {fingerprint}", node.name);
    if !cfg.pack_names.is_empty() {
        eprintln!("[{}] paquets installés, empreintes vérifiées : {}", node.name, cfg.pack_names.join(", "));
    }
    eprintln!("[{}] jeton de l'API : {}", node.name, cfg.state_dir.join("api-token").display());

    tokio::spawn(link::run_child_monitor(node.clone()));
    tokio::spawn(spawn::keep_logs_small(node.state_dir.join("logs"), node.log_max_bytes));
    tokio::spawn(print_activity(node.clone()));
    tokio::spawn(agent::run_agent(node.clone(), inbox));
    tokio::spawn(talixio_agent::schedule::run(node.clone()));
    // les agents permanents reviennent, avec leur mémoire et des droits revérifiés
    tokio::spawn({
        let node = node.clone();
        async move { talixio_agent::tools::restore_permanent(&node).await }
    });
    shutdown_signal().await;
    talixio_agent::model::cli::stop_all();
    node.terminate("arrêt du maître", 0).await
}

async fn run_child(state_dir: PathBuf, log_file: Option<PathBuf>) -> Result<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).context("lecture des consignes")?;
    talixio_agent::os::harden_process();
    let envelope: Envelope = serde_json::from_str(&raw).context("consignes de démarrage invalides")?;
    drop(raw);
    // la configuration est scellée : seul le parent, une fois reconnu, en remet la clé
    let mut key = None;
    for _ in 0..40 {
        key = link::fetch_key(&envelope.agent_id, &envelope.parent_addr, &envelope.parent_cert, &envelope.token).await;
        if key.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    let key = key.context("le parent n'a pas remis la clé de la configuration")?;
    let mut boot: Bootstrap = envelope.open(&key)?;
    drop(key);
    if let Some(path) = log_file {
        tokio::spawn(spawn::keep_own_log_small(path, boot.log_max_bytes));
    }
    boot.capabilities.canonicalize().map_err(anyhow::Error::msg)?;
    if let Some(ceiling) = &boot.ceiling {
        boot.capabilities.check_ceiling(ceiling).map_err(anyhow::Error::msg)?;
    }
    let state_dir = state_dir.canonicalize().context("dossier d'état")?;
    let (node, inbox) = Node::new_child(&boot, state_dir).map_err(anyhow::Error::msg)?;
    if node.caps.spawn.is_some() {
        api::start_listener(node.clone(), ([127, 0, 0, 1], 0).into(), None, tls::ephemeral()?).await?;
        tokio::spawn(link::run_child_monitor(node.clone()));
        tokio::spawn(spawn::keep_logs_small(node.state_dir.join("logs"), node.log_max_bytes));
    }
    tokio::spawn(link::run_uplink(node.clone(), boot.parent_addr.clone(), boot.parent_cert.clone(), boot.token.clone()));
    tokio::spawn(link::run_tree_reporter(node.clone()));
    tokio::select! {
        _ = agent::run_agent(node.clone(), inbox) => Ok(()),
        _ = shutdown_signal() => node.terminate("signal d'arrêt", 0).await,
    }
}

/// Le jeton de l'API : celui de l'environnement, sinon un jeton créé une fois et gardé dans le dossier d'état.
fn api_token(state_dir: &Path) -> Result<String> {
    if let Ok(token) = std::env::var("TALIXIO_API_TOKEN") {
        // un jeton donné mais faible est refusé, pas remplacé en silence : un exemple recopié ne doit pas ouvrir le maître
        let distinct = token.chars().collect::<std::collections::BTreeSet<_>>().len();
        if token.len() < 16 || distinct < 10 || token.contains("votre-choix") {
            bail!("TALIXIO_API_TOKEN est trop faible ou recopié d'un exemple : donnez un jeton tiré au hasard (par exemple `openssl rand -hex 32`), ou retirez la variable pour que le maître en crée un");
        }
        return Ok(token);
    }
    let path = state_dir.join("api-token");
    if path.symlink_metadata().is_ok() {
        // un jeton déjà là n'est repris que s'il est bien à nous, et à nous seuls
        ensure_private(&path)?;
        let existing = std::fs::read_to_string(&path)?;
        if existing.trim().len() < 32 {
            bail!("{} ne contient pas un jeton valide : supprimez-le", path.display());
        }
        return Ok(existing.trim().to_string());
    }
    let token = random_hex(2);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    use std::io::Write;
    // créé vide et fermé aux autres comptes, puis seulement rempli
    let mut file = options.open(&path)?;
    talixio_agent::os::make_private(&path).map_err(anyhow::Error::msg)?;
    file.write_all(token.as_bytes())?;
    Ok(token)
}

/// Recopie le journal de la grappe sur la sortie d'erreur du maître.
async fn print_activity(node: std::sync::Arc<Node>) {
    let Some(log) = &node.activity else { return };
    let mut live = log.live.subscribe();
    loop {
        match live.recv().await {
            Ok(event) => eprintln!("{}", format_event(&event)),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(_) => return,
        }
    }
}

fn format_event(event: &ActivityEvent) -> String {
    // rien de ce qu'un agent écrit ne doit pouvoir piloter le terminal qui l'affiche
    let text = clean(&event.text.replace('\n', " ⏎ "), 600, false);
    format!("[{}] {} : {}", clean(&event.agent_name, 80, false), clean(&event.kind, 32, false), text)
}

fn print_tree(node: &NodeInfo, indent: usize) {
    let ip = node.ips.first().map(String::as_str).unwrap_or("-");
    println!(
        "{}{} ({}) — {} — {:?} — {}@{} [{}] — {} tokens",
        "  ".repeat(indent),
        node.name,
        node.id,
        node.role,
        node.status,
        node.target,
        node.host,
        ip,
        node.tokens_used
    );
    for child in &node.children {
        print_tree(child, indent + 1);
    }
}

async fn watch(remote: &Remote) -> Result<()> {
    let mut after = 0u64;
    loop {
        let body = remote.get(&format!("/api/activity?after={after}")).await?;
        let events: Vec<ActivityEvent> = serde_json::from_value(body["events"].clone())?;
        for event in &events {
            println!("{}", format_event(event));
            after = event.id;
        }
        tokio::time::sleep(Duration::from_millis(700)).await;
    }
}

impl Remote {
    fn token(&self) -> Result<String> {
        if let Some(token) = &self.token {
            return Ok(token.clone());
        }
        std::fs::read_to_string(".talixio/api-token")
            .map(|t| t.trim().to_string())
            .context("jeton introuvable : passez --token, ou lancez la commande depuis le dossier du maître")
    }

    /// Un client qui n'accepte que le certificat du maître.
    fn http(&self) -> Result<reqwest::Client> {
        if self.public_cert {
            return Ok(reqwest::Client::new());
        }
        let fingerprint = match &self.fingerprint {
            Some(fingerprint) => fingerprint.clone(),
            None => std::fs::read_to_string(".talixio/tls-fingerprint")
                .context("empreinte du certificat introuvable : passez --fingerprint, ou lancez la commande depuis le dossier du maître")?,
        };
        Ok(reqwest::Client::builder().use_preconfigured_tls(tls::pinned_client(&fingerprint)?).build()?)
    }

    async fn call(&self, request: reqwest::RequestBuilder) -> Result<Value> {
        let response = request.bearer_auth(self.token()?).timeout(Duration::from_secs(30)).send().await?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            bail!("{} : {}", status, body["error"].as_str().unwrap_or("erreur"));
        }
        Ok(body)
    }

    async fn get(&self, path: &str) -> Result<Value> {
        self.call(self.http()?.get(format!("{}{path}", self.url.trim_end_matches('/')))).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.call(self.http()?.post(format!("{}{path}", self.url.trim_end_matches('/'))).json(&body)).await
    }
}
