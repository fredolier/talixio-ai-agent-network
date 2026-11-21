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
    /// Affiche la grappe.
    Tree(Remote),
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
enum ServiceAction {
    /// Inscrit le maître auprès du système (systemd, launchd, ou tâche planifiée) et le démarre.
    Install {
        /// Fichier de configuration (TOML).
        #[arg(short, long, default_value = "talixio.toml")]
        config: PathBuf,
        /// Linux : fichier de variables d'environnement (les clés lues par `api_key_env`), fermé aux autres comptes.
        #[arg(long)]
        env_file: Option<PathBuf>,
        /// Linux, installation en root : le compte ordinaire sous lequel le maître tournera.
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
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Root { config, workdir } => {
            if let Some(dir) = workdir {
                std::env::set_current_dir(&dir).with_context(|| format!("dossier de travail {}", dir.display()))?;
            }
            run_root(&config).await
        }
        Command::Service { action } => service(action),
        Command::Child { state_dir, log_file } => run_child(state_dir, log_file).await,
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
    check(&spec.exe, "exécutable", &[0, uid])?;
    check(&spec.config, "configuration", &[0, uid])?;
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
    let mut spec = Spec { kind, exe: std::env::current_exe()?, config: PathBuf::new(), workdir: std::env::current_dir()?, home, env_file: None, user: None, uid };
    match action {
        ServiceAction::Install { config, env_file, user, print } => {
            // la configuration doit être bonne avant d'en faire un service qui redémarre en boucle
            let cfg = RootConfig::load(&config)?;
            spec.config = config.canonicalize().with_context(|| format!("configuration {}", config.display()))?;
            if cfg!(windows) {
                // sous Windows, le chemin canonique porte un préfixe que les autres outils ne lisent pas
                spec.config = std::path::absolute(&config)?;
            }
            if let Some(env) = &env_file {
                ensure_private(env)?;
                spec.env_file = Some(env.canonicalize()?);
            }
            spec.user = user;
            #[cfg(unix)]
            if kind == service::Kind::SystemdSystem {
                system_paths_are_safe(&spec)?;
            }
            let plan = service::install_plan(&spec).map_err(anyhow::Error::msg)?;
            let from_env: Vec<&str> = cfg.models.values().filter_map(|m| m.api_key_env.as_deref()).filter(|name| !name.is_empty()).collect::<std::collections::BTreeSet<_>>().into_iter().collect();
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
    eprintln!("[{}] agent maître {} — API sur https://{addr}", node.name, node.id);
    eprintln!("[{}] empreinte du certificat : {fingerprint}", node.name);
    eprintln!("[{}] jeton de l'API : {}", node.name, cfg.state_dir.join("api-token").display());

    tokio::spawn(link::run_child_monitor(node.clone()));
    tokio::spawn(spawn::keep_logs_small(node.state_dir.join("logs"), node.log_max_bytes));
    tokio::spawn(print_activity(node.clone()));
    tokio::spawn(agent::run_agent(node.clone(), inbox));
    // les agents permanents reviennent, avec leur mémoire et des droits revérifiés
    tokio::spawn({
        let node = node.clone();
        async move { talixio_agent::tools::restore_permanent(&node).await }
    });
    shutdown_signal().await;
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
