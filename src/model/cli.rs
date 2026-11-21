//! Un modèle servi par la commande officielle `claude` (Claude Code), avec l'abonnement du
//! compte qui fait tourner le maître.
//!
//! Le maître n'appelle pas l'API d'Anthropic et ne lit ni ne garde aucun jeton d'abonnement :
//! il lance `claude -p`, non modifié, qui se connecte par ses propres moyens (sa connexion sur
//! la machine, ou `CLAUDE_CODE_OAUTH_TOKEN` que la personne a tiré de `claude setup-token`).
//!
//! `claude` tient lui-même la boucle d'outils, alors que partout ailleurs c'est l'agent. On
//! garde pourtant la même interface (`complete` : une demande, une réponse) et donc les mêmes
//! contrôles : ses outils intégrés sont coupés, il n'a que ceux de l'agent, servis par un pont
//! (`talixio-agent mcp-bridge`, qu'il lance lui-même et qui rappelle le maître). Quand il
//! appelle un outil, l'appel est retenu et rendu à l'agent comme n'importe quel `tool_use` ;
//! l'agent l'exécute avec ses droits et sa réponse, à la demande suivante, libère l'appel.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, oneshot};

use super::{Block, Message, ModelRequest, ModelResponse, Role, ToolSpec, Usage};
use crate::config::ModelConfig;

/// Nom du serveur d'outils tel que `claude` le voit.
const SERVER: &str = "talixio";
/// Un tour en attente de la réponse de l'agent à un outil est abandonné après ce délai.
const PARKED: Duration = Duration::from_secs(300);
/// `claude` doit répondre (texte ou appel d'outil) dans ce délai.
const STEP: Duration = Duration::from_secs(900);
/// Après un premier appel d'outil, on attend brièvement ceux que le modèle a demandés avec lui.
const GRACE: Duration = Duration::from_millis(250);
/// Tours vivants au plus : chacun est un process `claude`.
const MAX_TURNS: usize = 16;

/// Ce qu'il faut pour lancer `claude` et pour qu'il rappelle le maître. Posé au démarrage.
#[derive(Clone, Debug)]
pub struct Setup {
    /// La commande `claude` ; un nom seul est cherché dans `PATH`.
    pub program: PathBuf,
    /// Variables d'environnement transmises à `claude` en plus du strict nécessaire.
    pub env: Vec<String>,
    /// Dossier de travail de `claude` : vide, à nous, sans rien qu'il puisse charger.
    pub workdir: PathBuf,
    /// Cet exécutable : `claude` le relance en pont.
    pub exe: PathBuf,
}

static SETUP: OnceLock<Setup> = OnceLock::new();
/// Où le pont rappelle le maître : adresse et empreinte du certificat. Connu une fois l'écoute ouverte.
static CALLBACK: OnceLock<(String, String)> = OnceLock::new();

pub fn install(setup: Setup) {
    let _ = SETUP.set(setup);
}

pub fn set_callback(addr: std::net::SocketAddr, fingerprint: &str) {
    let _ = CALLBACK.set((format!("https://127.0.0.1:{}", addr.port()), fingerprint.to_string()));
}

/// La commande `claude` répond-elle ? Rend sa version. Ne dit rien de sa connexion : une
/// commande non connectée échouera au premier appel, et le journal du maître dira pourquoi.
pub async fn probe() -> Result<String, String> {
    let setup = SETUP.get().ok_or("seul le maître lance `claude`")?;
    let mut command = tokio::process::Command::new(&setup.program);
    apply_env(&mut command, setup);
    command.arg("--version").stdin(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(20), command.output())
        .await
        .map_err(|_| "la commande `claude` ne répond pas".to_string())?
        .map_err(|_| "la commande `claude` est introuvable sur la machine du maître : installez Claude Code, ou indiquez son chemin (`claude_program`)".to_string())?;
    if !output.status.success() {
        return Err("la commande `claude` a échoué".into());
    }
    Ok(crate::clean(String::from_utf8_lossy(&output.stdout).trim(), 80, false))
}

/// `claude` ne reçoit que le nécessaire : jamais les clés du maître ni son jeton d'API.
fn apply_env(command: &mut tokio::process::Command, setup: &Setup) {
    const BASE: [&str; 26] = [
        "PATH", "HOME", "USER", "LOGNAME", "SHELL", "LANG", "LC_ALL", "TZ", "TMPDIR", "TEMP", "TMP", "SystemRoot", "USERPROFILE", "APPDATA",
        "LOCALAPPDATA", "PATHEXT", "ComSpec", "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDE_CONFIG_DIR", "HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY",
        "https_proxy", "http_proxy", "no_proxy", "NODE_EXTRA_CA_CERTS",
    ];
    command.env_clear();
    for name in BASE.iter().copied().chain(setup.env.iter().map(String::as_str)) {
        // une clé d'API passerait avant l'abonnement, et serait facturée à l'usage
        if matches!(name, "ANTHROPIC_API_KEY" | "ANTHROPIC_AUTH_TOKEN" | "TALIXIO_API_TOKEN") {
            continue;
        }
        if let Ok(value) = std::env::var(name) {
            command.env(name, value);
        }
    }
}

/// Ce que `claude` nous fait savoir pendant un tour.
enum Event {
    /// Il appelle un outil ; la réponse de l'agent passera par `reply`.
    Call { name: String, input: Value, reply: oneshot::Sender<(String, bool)> },
    /// Un message du modèle : son identifiant, son texte, ce qu'il a consommé.
    Said { id: String, text: String, usage: Usage },
    /// Fin du tour.
    Done { text: String, failed: bool },
    /// Le process s'est arrêté.
    Gone,
}

/// Un tour en cours : un process `claude` vivant, pour un agent.
struct Turn {
    /// Le jeton du pont de ce tour : lui seul ouvre la route de rappel, pour ce tour seul.
    token: String,
    tools: Vec<ToolSpec>,
    events_tx: mpsc::UnboundedSender<Event>,
    events: tokio::sync::Mutex<mpsc::UnboundedReceiver<Event>>,
    /// Appels d'outils rendus à l'agent, en attente de sa réponse, par identifiant.
    waiting: Mutex<HashMap<String, oneshot::Sender<(String, bool)>>>,
    /// Fichiers de ce tour (consignes, réglage du pont), effacés avec lui.
    files: Vec<PathBuf>,
    session: String,
    model: String,
    _child: tokio::process::Child,
}

/// Par agent : son tour en cours.
static TURNS: Mutex<Option<HashMap<String, Arc<Turn>>>> = Mutex::new(None);
/// Par agent et modèle : la session `claude` qui tient sa conversation, et combien de messages elle en connaît.
static SESSIONS: Mutex<Option<Sessions>> = Mutex::new(None);
/// Clé : agent et alias du modèle. Valeur : identifiant de session, messages connus.
type Sessions = HashMap<(String, String), (String, usize)>;
/// Par jeton de pont : le tour.
static BRIDGES: Mutex<Option<HashMap<String, Weak<Turn>>>> = Mutex::new(None);
static NEXT_CALL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn with<T, R>(slot: &Mutex<Option<HashMap<T, R>>>) -> std::sync::MutexGuard<'_, Option<HashMap<T, R>>> {
    let mut guard = slot.lock().unwrap();
    guard.get_or_insert_with(HashMap::new);
    guard
}

impl Drop for Turn {
    fn drop(&mut self) {
        with(&BRIDGES).as_mut().unwrap().remove(&self.token);
        for file in &self.files {
            let _ = std::fs::remove_file(file);
        }
    }
}

/// Le connecteur d'un alias : quel modèle demander à `claude`.
pub struct ClaudeCode {
    model: String,
    effort: Option<String>,
}

impl ClaudeCode {
    pub fn new(cfg: &ModelConfig) -> Result<Self> {
        if SETUP.get().is_none() {
            bail!("la commande `claude` n'est pas préparée");
        }
        let plain = !cfg.model.is_empty() && cfg.model.len() <= 100 && cfg.model.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'[' | b']'));
        if !plain || cfg.model.starts_with('-') {
            bail!("nom de modèle invalide pour `claude`");
        }
        let effort = cfg.effort.clone().filter(|e| matches!(e.as_str(), "low" | "medium" | "high" | "xhigh" | "max"));
        Ok(Self { model: cfg.model.clone(), effort })
    }

    pub async fn complete(&self, origin: &str, request: &ModelRequest) -> Result<ModelResponse> {
        if request.messages.iter().any(|m| m.content.iter().any(|b| matches!(b, Block::Image { .. }))) {
            bail!("les images ne passent pas encore par `claude`");
        }
        let key = (origin.to_string(), request.model.clone());
        // un tour en cours pour cet agent : sa demande doit porter les réponses aux outils attendus
        let parked = with(&TURNS).as_mut().unwrap().remove(origin);
        let turn = match parked {
            Some(turn) if turn.model == request.model && answer(&turn, request) => turn,
            Some(turn) => {
                // la conversation de l'agent a pris un autre chemin : ce tour et sa session sont abandonnés
                with(&SESSIONS).as_mut().unwrap().remove(&(origin.to_string(), turn.model.clone()));
                drop(turn);
                self.start(&key, request).await?
            }
            None => self.start(&key, request).await?,
        };
        let outcome = listen(&turn).await;
        match outcome {
            Ok((message, usage, finished)) => {
                if finished {
                    with(&SESSIONS).as_mut().unwrap().insert(key, (turn.session.clone(), request.messages.len() + 1));
                } else {
                    park(origin, turn);
                }
                Ok(ModelResponse { message, usage })
            }
            Err(e) => {
                with(&SESSIONS).as_mut().unwrap().remove(&key);
                Err(e)
            }
        }
    }

    /// Lance `claude` pour un nouveau tour de cet agent.
    async fn start(&self, key: &(String, String), request: &ModelRequest) -> Result<Arc<Turn>> {
        let setup = SETUP.get().context("la commande `claude` n'est pas préparée")?;
        let (callback, fingerprint) = CALLBACK.get().context("le maître n'écoute pas encore")?;
        if with(&TURNS).as_ref().unwrap().len() >= MAX_TURNS {
            bail!("trop de tours `claude` en cours ({MAX_TURNS})");
        }
        // la session connaît déjà le début de la conversation : on ne lui envoie que la suite
        let known = with(&SESSIONS).as_ref().unwrap().get(key).cloned().filter(|(_, seen)| *seen < request.messages.len() && *seen > 0);
        let (session, resume, prompt) = match known {
            Some((session, seen)) => (session, true, render(&request.messages[seen..], false)),
            None => (uuid(), false, render(&request.messages, request.messages.len() > 1)),
        };
        if prompt.trim().is_empty() {
            bail!("rien à dire à `claude`");
        }
        let token = crate::random_hex(2);
        let bridge = json!({ "mcpServers": { SERVER: {
            "command": setup.exe,
            "args": ["mcp-bridge"],
            "env": { "TALIXIO_BRIDGE_URL": callback, "TALIXIO_BRIDGE_CERT": fingerprint, "TALIXIO_BRIDGE_TOKEN": token },
        } } });
        // consignes et jeton du pont passent par des fichiers fermés, pas par la ligne de commande,
        // que les autres comptes de la machine peuvent lire
        let files = vec![setup.workdir.join(format!("{}.mcp.json", &token[..16])), setup.workdir.join(format!("{}.txt", &token[..16]))];
        for (file, content) in files.iter().zip([bridge.to_string(), request.system.clone()]) {
            write_private(file, &content).map_err(|e| anyhow!("fichier de tour : {e}"))?;
        }
        let mut command = tokio::process::Command::new(&setup.program);
        apply_env(&mut command, setup);
        // un outil peut durer (attendre un enfant) : `claude` ne doit pas l'abandonner avant nous
        command.env("MCP_TOOL_TIMEOUT", (PARKED.as_millis() + 60_000).to_string()).env("MCP_TIMEOUT", "30000");
        command
            .current_dir(&setup.workdir)
            .args(["-p", "--input-format", "stream-json", "--output-format", "stream-json", "--verbose"])
            // rien d'autre que nos outils : ni outils intégrés, ni réglages, ni extensions du compte
            .args(["--tools", "", "--strict-mcp-config", "--setting-sources", "", "--disable-slash-commands"])
            .args(["--permission-mode", "dontAsk", "--allowedTools", &format!("mcp__{SERVER}")])
            .arg("--mcp-config")
            .arg(&files[0])
            .arg("--system-prompt-file")
            .arg(&files[1])
            .args(["--model", &self.model]);
        if let Some(effort) = &self.effort {
            command.args(["--effort", effort]);
        }
        command.args(if resume { ["--resume", session.as_str()] } else { ["--session-id", session.as_str()] });
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(e) => {
                files.iter().for_each(|f| drop(std::fs::remove_file(f)));
                return Err(e).context("la commande `claude` n'a pas pu être lancée");
            }
        };
        let mut stdin = child.stdin.take().context("entrée de `claude`")?;
        let stdout = child.stdout.take().context("sortie de `claude`")?;
        let stderr = child.stderr.take().context("erreurs de `claude`")?;
        let (events_tx, events) = mpsc::unbounded_channel();
        tokio::spawn(read_events(stdout, stderr, events_tx.clone()));
        let turn = Arc::new(Turn {
            token: token.clone(),
            tools: request.tools.clone(),
            events_tx,
            events: tokio::sync::Mutex::new(events),
            waiting: Mutex::new(HashMap::new()),
            files,
            session,
            model: request.model.clone(),
            _child: child,
        });
        with(&BRIDGES).as_mut().unwrap().insert(token, Arc::downgrade(&turn));
        let line = json!({ "type": "user", "message": { "role": "user", "content": [{ "type": "text", "text": prompt }] } });
        // un message, puis fin de l'entrée : `claude` fait son tour et s'arrête
        tokio::spawn(async move {
            let _ = stdin.write_all(format!("{line}\n").as_bytes()).await;
            let _ = stdin.shutdown().await;
        });
        Ok(turn)
    }
}

/// La demande de l'agent répond-elle aux outils que ce tour attend ? Si oui, les appels sont libérés.
fn answer(turn: &Turn, request: &ModelRequest) -> bool {
    let Some(last) = request.messages.last().filter(|m| m.role == Role::User) else { return false };
    let results: Vec<(&String, &String, bool)> = last
        .content
        .iter()
        .filter_map(|b| match b {
            Block::ToolResult { tool_use_id, content, is_error } => Some((tool_use_id, content, *is_error)),
            _ => None,
        })
        .collect();
    let mut waiting = turn.waiting.lock().unwrap();
    if results.is_empty() || results.len() != waiting.len() || results.iter().any(|(id, ..)| !waiting.contains_key(*id)) {
        return false;
    }
    for (id, content, is_error) in results {
        if let Some(reply) = waiting.remove(id) {
            let _ = reply.send((content.clone(), is_error));
        }
    }
    true
}

/// Attend que `claude` rende la main : des appels d'outils (tour à poursuivre) ou sa réponse (tour fini).
async fn listen(turn: &Arc<Turn>) -> Result<(Message, Usage, bool)> {
    let mut content: Vec<Block> = Vec::new();
    let mut said: Vec<(String, String, Usage)> = Vec::new();
    let mut events = turn.events.lock().await;
    let mut calls = 0;
    let finished = loop {
        // après un premier appel d'outil, on ne prend plus que ceux qui l'accompagnent
        let wait = if calls > 0 { GRACE } else { STEP };
        let event = match tokio::time::timeout(wait, events.recv()).await {
            Ok(Some(event)) => event,
            Ok(None) | Err(_) if calls > 0 => break false,
            Ok(None) => bail!("`claude` s'est arrêté sans répondre"),
            Err(_) => bail!("`claude` n'a pas répondu à temps"),
        };
        match event {
            Event::Call { name, input, reply } => {
                let id = format!("cc_{}", NEXT_CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
                turn.waiting.lock().unwrap().insert(id.clone(), reply);
                content.push(Block::ToolUse { id, name, input });
                calls += 1;
            }
            // un même message arrive bloc par bloc, avec le même compte : on garde le dernier
            Event::Said { id, text, usage } => match said.iter_mut().find(|(known, ..)| *known == id) {
                Some(entry) => {
                    entry.1.push_str(&text);
                    entry.2 = usage;
                }
                None => said.push((id, text, usage)),
            },
            Event::Done { failed: true, text } => bail!("`claude` a rendu une erreur : {}", crate::clean(&text, 300, false)),
            Event::Done { text, .. } if calls == 0 => {
                if said.iter().all(|(_, t, _)| t.trim().is_empty()) && !text.trim().is_empty() {
                    said.push((String::new(), text, Usage::default()));
                }
                break true;
            }
            Event::Done { .. } => break false,
            Event::Gone if calls > 0 => break false,
            Event::Gone => bail!("`claude` s'est arrêté sans répondre (est-il connecté ? détail dans la sortie d'erreur du maître)"),
        }
    };
    let mut usage = Usage::default();
    let mut text = String::new();
    for (_, part, used) in &said {
        text.push_str(part);
        usage.input += used.input;
        usage.output += used.output;
        usage.cache_read += used.cache_read;
        usage.cache_write += used.cache_write;
    }
    if !text.trim().is_empty() {
        content.insert(0, Block::Text { text });
    }
    Ok((Message { role: Role::Assistant, content, raw: None }, usage, finished))
}

/// Garde le tour en attendant la réponse de l'agent ; passé le délai, il est abandonné.
fn park(origin: &str, turn: Arc<Turn>) {
    let weak = Arc::downgrade(&turn);
    with(&TURNS).as_mut().unwrap().insert(origin.to_string(), turn);
    let origin = origin.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(PARKED).await;
        let mut turns = with(&TURNS);
        let turns = turns.as_mut().unwrap();
        if turns.get(&origin).is_some_and(|current| weak.upgrade().is_some_and(|ours| Arc::ptr_eq(current, &ours))) {
            turns.remove(&origin);
        }
    });
}

/// Un enfant du maître n'est plus : les tours en cours et les sessions de sa branche sont oubliés.
pub fn forget_branch(child_id: &str) {
    let prefix = format!("{child_id}~");
    with(&TURNS).as_mut().unwrap().retain(|origin, _| !origin.starts_with(&prefix));
    with(&SESSIONS).as_mut().unwrap().retain(|(origin, _), _| !origin.starts_with(&prefix));
}

/// Lit ce que `claude` écrit : une ligne JSON par événement.
async fn read_events(stdout: tokio::process::ChildStdout, stderr: tokio::process::ChildStderr, events: mpsc::UnboundedSender<Event>) {
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            eprintln!("claude : {}", crate::clean(&line, 500, false));
        }
    });
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
        let sent = match event["type"].as_str() {
            Some("assistant") if event["parent_tool_use_id"].is_null() => {
                let message = &event["message"];
                let text: String = message["content"].as_array().into_iter().flatten().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect();
                let count = |name: &str| message["usage"][name].as_u64().unwrap_or(0);
                let usage = Usage {
                    input: count("input_tokens"),
                    output: count("output_tokens"),
                    cache_read: count("cache_read_input_tokens"),
                    cache_write: count("cache_creation_input_tokens"),
                };
                events.send(Event::Said { id: message["id"].as_str().unwrap_or("").to_string(), text, usage })
            }
            Some("result") => {
                let failed = event["is_error"].as_bool().unwrap_or(false) || event["subtype"].as_str().is_some_and(|s| s != "success");
                let text = event["result"].as_str().or(event["subtype"].as_str()).unwrap_or("").to_string();
                events.send(Event::Done { text, failed })
            }
            _ => Ok(()),
        };
        if sent.is_err() {
            return;
        }
    }
    let _ = events.send(Event::Gone);
}

/// La conversation, en clair, pour une session qui ne la connaît pas (ou sa suite, pour une qui la connaît).
fn render(messages: &[Message], with_roles: bool) -> String {
    let mut out = String::new();
    if with_roles {
        out.push_str("Voici la conversation jusqu'ici, reprise depuis une autre session. Poursuis-la à partir de son dernier message.\n\n");
    }
    for message in messages {
        for block in &message.content {
            let (label, text) = match block {
                Block::Text { text } => (if message.role == Role::User { "Reçu" } else { "Toi" }, text.clone()),
                Block::ToolUse { name, input, .. } => ("Toi, appel d'outil", format!("{name} {input}")),
                Block::ToolResult { content, is_error, .. } => (if *is_error { "Outil, erreur" } else { "Outil, résultat" }, content.clone()),
                Block::Image { .. } => continue,
            };
            if with_roles || !matches!(block, Block::Text { .. }) {
                out.push_str(&format!("[{label}]\n{text}\n\n"));
            } else {
                out.push_str(&text);
                out.push_str("\n\n");
            }
        }
    }
    out.trim_end().to_string()
}

fn uuid() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// Un fichier fermé aux autres comptes : créé vide et fermé, puis seulement rempli.
fn write_private(path: &std::path::Path, content: &str) -> Result<(), String> {
    use std::io::Write;
    let _ = std::fs::remove_file(path);
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| e.to_string())?;
    crate::os::make_private(path)?;
    file.write_all(content.as_bytes()).map_err(|e| e.to_string())
}

/// Le pont d'un tour transmet un message du protocole d'outils (JSON-RPC) : la réponse à lui
/// rendre, s'il en attend une. `None` : jeton inconnu. Le pont n'a aucun autre pouvoir que de
/// poser ici les appels d'outils de son tour ; c'est l'agent qui les exécute, avec ses droits.
pub async fn bridge(token: &str, message: Value) -> Option<Option<Value>> {
    let turn = with(&BRIDGES).as_ref().unwrap().get(token).and_then(Weak::upgrade)?;
    let id = message.get("id").cloned().filter(|id| !id.is_null());
    let method = message["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => Ok(json!({
            "protocolVersion": message["params"]["protocolVersion"].as_str().unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER, "version": env!("CARGO_PKG_VERSION") },
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": turn.tools.iter().map(|t| json!({ "name": t.name, "description": t.description, "inputSchema": t.parameters })).collect::<Vec<_>>() })),
        "tools/call" => {
            let name = message["params"]["name"].as_str().unwrap_or("").to_string();
            let input = match &message["params"]["arguments"] {
                Value::Null => json!({}),
                other => other.clone(),
            };
            let (reply, answer) = oneshot::channel();
            let sent = turn.tools.iter().any(|t| t.name == name) && turn.events_tx.send(Event::Call { name, input, reply }).is_ok();
            // on ne retient pas le tour pendant l'attente : s'il est abandonné, l'appel finit en erreur
            drop(turn);
            let (text, is_error) = match sent {
                true => answer.await.unwrap_or_else(|_| ("tour abandonné".to_string(), true)),
                false => ("outil inconnu".to_string(), true),
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        _ => Err(anyhow!("méthode inconnue")),
    };
    // une notification n'attend pas de réponse
    let Some(id) = id else { return Some(None) };
    Some(Some(match result {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err(_) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32601, "message": "method not found" } }),
    }))
}

/// Le pont : lancé par `claude`, il relaie son protocole d'outils (une ligne JSON par message)
/// vers le maître, et les réponses en retour. Il ne sait rien faire d'autre.
pub async fn run_bridge() -> Result<()> {
    let var = |name: &str| std::env::var(name).with_context(|| format!("{name} manque : le pont est lancé par `claude`, pas à la main"));
    let (url, fingerprint, token) = (var("TALIXIO_BRIDGE_URL")?, var("TALIXIO_BRIDGE_CERT")?, var("TALIXIO_BRIDGE_TOKEN")?);
    let pinned = crate::tls::pinned_client(&fingerprint).map_err(|e| anyhow!("{e}"))?;
    let http = reqwest::Client::builder().use_preconfigured_tls(pinned).no_proxy().build()?;
    let out = Arc::new(tokio::sync::Mutex::new(tokio::io::stdout()));
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut tasks = tokio::task::JoinSet::new();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let (http, out, url, token) = (http.clone(), out.clone(), format!("{url}/bridge"), token.clone());
        // un appel d'outil peut durer : chaque message a sa propre demande
        tasks.spawn(async move {
            let response = http.post(url).bearer_auth(token).header("content-type", "application/json").body(line).send().await;
            if let Ok(response) = response {
                if response.status() == reqwest::StatusCode::OK {
                    if let Ok(body) = response.text().await {
                        let mut out = out.lock().await;
                        let _ = out.write_all(format!("{}\n", body.trim()).as_bytes()).await;
                        let _ = out.flush().await;
                    }
                }
            }
        });
    }
    // l'entrée est fermée : `claude` a fini, les appels encore en vol n'ont plus de lecteur
    tasks.shutdown().await;
    Ok(())
}
