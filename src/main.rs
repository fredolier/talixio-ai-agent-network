use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use serde_json::{json, Value};

use talixio_agent::config::{Bootstrap, RootConfig};
use talixio_agent::model::Providers;
use talixio_agent::node::Node;
use talixio_agent::protocol::{ActivityEvent, NodeInfo};
use talixio_agent::{agent, api, link, random_hex};

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
    },
    /// Lance un agent enfant. Réservé aux agents : les consignes arrivent sur l'entrée standard.
    #[command(hide = true)]
    Child {
        #[arg(long)]
        state_dir: PathBuf,
    },
    /// Affiche la grappe.
    Tree(Remote),
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

#[derive(clap::Args)]
struct Remote {
    /// Adresse de l'API du maître.
    #[arg(long, env = "TALIXIO_URL", default_value = "http://127.0.0.1:7700")]
    url: String,
    /// Jeton de l'API. Par défaut : lu dans .talixio/api-token.
    #[arg(long, env = "TALIXIO_API_TOKEN", hide_env_values = true)]
    token: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Root { config } => run_root(&config).await,
        Command::Child { state_dir } => run_child(state_dir).await,
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

async fn run_root(config: &Path) -> Result<()> {
    let cfg = RootConfig::load(config)?;
    let providers = Providers::from_config(&cfg.models)?;
    let token = api_token(&cfg.state_dir)?;
    let (node, inbox) = Node::new_root(&cfg, providers);
    let addr = api::start_listener(node.clone(), cfg.listen, Some(token)).await?;
    eprintln!("[{}] agent maître {} — API sur http://{addr}", node.name, node.id);
    eprintln!("[{}] jeton de l'API : {}", node.name, cfg.state_dir.join("api-token").display());

    tokio::spawn(link::run_child_monitor(node.clone()));
    tokio::spawn(print_activity(node.clone()));
    tokio::spawn(agent::run_agent(node.clone(), inbox));
    tokio::signal::ctrl_c().await?;
    node.terminate("arrêt du maître", 0).await
}

async fn run_child(state_dir: PathBuf) -> Result<()> {
    let mut raw = String::new();
    std::io::stdin().read_to_string(&mut raw).context("lecture des consignes")?;
    let boot: Bootstrap = serde_json::from_str(&raw).context("consignes de démarrage invalides")?;
    boot.capabilities.validate().map_err(anyhow::Error::msg)?;
    let (node, inbox) = Node::new_child(&boot, state_dir);
    if node.caps.spawn.is_some() {
        api::start_listener(node.clone(), ([127, 0, 0, 1], 0).into(), None).await?;
        tokio::spawn(link::run_child_monitor(node.clone()));
    }
    tokio::spawn(link::run_uplink(node.clone(), boot.parent_url.clone(), boot.token.clone()));
    tokio::spawn(link::run_tree_reporter(node.clone()));
    agent::run_agent(node, inbox).await;
    Ok(())
}

/// Le jeton de l'API : celui de l'environnement, sinon un jeton créé une fois et gardé dans le dossier d'état.
fn api_token(state_dir: &Path) -> Result<String> {
    if let Some(token) = std::env::var("TALIXIO_API_TOKEN").ok().filter(|t| t.len() >= 16) {
        return Ok(token);
    }
    std::fs::create_dir_all(state_dir)?;
    let path = state_dir.join("api-token");
    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing.trim().len() >= 16 {
            return Ok(existing.trim().to_string());
        }
    }
    let token = random_hex(2);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    use std::io::Write;
    options.open(&path)?.write_all(token.as_bytes())?;
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
    let text: String = event.text.chars().take(600).collect();
    format!("[{}] {} : {}", event.agent_name, event.kind, text.replace('\n', " ⏎ "))
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
        self.call(reqwest::Client::new().get(format!("{}{path}", self.url.trim_end_matches('/')))).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        self.call(reqwest::Client::new().post(format!("{}{path}", self.url.trim_end_matches('/'))).json(&body)).await
    }
}
