//! Les outils d'un agent. Chacun n'existe que si l'agent en a le droit, et chaque appel est revérifié.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::process::Command;

use crate::config::Bootstrap;
use crate::model::ToolSpec;
use crate::node::{ChildRec, Node};
use crate::policy::Capabilities;
use crate::protocol::{Msg, NodeInfo, Outbox, ReportKind, Status};
use crate::{now_ms, random_hex, spawn};

const MAX_READ: u64 = 256 * 1024;
const MAX_OUTPUT: usize = 64 * 1024;
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

fn spec(name: &str, description: String, properties: Value, required: &[&str]) -> ToolSpec {
    ToolSpec {
        name: name.to_string(),
        description,
        parameters: json!({ "type": "object", "properties": properties, "required": required }),
    }
}

fn paths(list: &[PathBuf]) -> String {
    list.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
}

/// Les outils que cet agent peut utiliser, d'après ses droits.
pub fn specs(node: &Node) -> Vec<ToolSpec> {
    let caps = &node.caps;
    let mut tools = Vec::new();
    let path = json!({ "path": { "type": "string", "description": "Chemin absolu." } });
    if !caps.fs_read.is_empty() {
        tools.push(spec(
            "read_file",
            format!("Lit un fichier texte. Dossiers autorisés : {}.", paths(&caps.fs_read)),
            path.clone(),
            &["path"],
        ));
        tools.push(spec(
            "list_dir",
            format!("Liste le contenu d'un dossier. Dossiers autorisés : {}.", paths(&caps.fs_read)),
            path.clone(),
            &["path"],
        ));
    }
    if !caps.fs_write.is_empty() {
        tools.push(spec(
            "write_file",
            format!("Écrit un fichier texte (remplace son contenu). Dossiers autorisés : {}.", paths(&caps.fs_write)),
            json!({ "path": { "type": "string", "description": "Chemin absolu." }, "content": { "type": "string" } }),
            &["path", "content"],
        ));
    }
    if !caps.exec.is_empty() {
        let allowed = caps.exec.iter().map(|r| r.describe()).collect::<Vec<_>>().join(" ; ");
        tools.push(spec(
            "run_command",
            format!(
                "Lance un programme, sans shell (pas de redirection, de tube ni de variable). \
                 Seules ces commandes sont autorisées, « * » valant un argument libre et « ** » la suite libre : {allowed}."
            ),
            json!({
                "program": { "type": "string" },
                "args": { "type": "array", "items": { "type": "string" } },
            }),
            &["program"],
        ));
    }
    if let Some(policy) = &caps.spawn {
        tools.push(spec(
            "spawn_agent",
            format!(
                "Crée un agent enfant, sur cette machine, pour lui confier une mission. Il travaille seul jusqu'à \
                 atteindre le critère de réussite ou constater que c'est impossible, puis te rend compte ; son compte \
                 rendu arrive plus tard, dans un message. Il ne voit pas ta conversation : mets dans la mission tout \
                 ce qu'il doit savoir. Ses droits ne peuvent pas dépasser les tiens ; donne-lui le strict nécessaire. \
                 Limites : {} enfants en vie au plus, {} niveau(x) sous toi.",
                policy.max_children, policy.max_depth
            ),
            json!({
                "name": { "type": "string", "description": "Nom court et unique : lettres, chiffres, tirets." },
                "role": { "type": "string", "description": "Sa spécialité, en quelques mots." },
                "instructions": { "type": "string", "description": "Comment il doit travailler : méthode, limites, format des comptes rendus." },
                "objective": { "type": "string", "description": "La mission à accomplir." },
                "success_criteria": { "type": "string", "description": "Critère de réussite mesurable : ce qui, vérifié, prouve que la mission est accomplie." },
                "model": { "type": "string", "description": format!("Alias du modèle, parmi : {}. Par défaut : le tien.", caps.models.join(", ")) },
                "capabilities": {
                    "type": "object",
                    "description": "Ses droits. Tout ce qui est omis est refusé.",
                    "properties": {
                        "fs_read": { "type": "array", "items": { "type": "string" }, "description": "Dossiers lisibles (chemins absolus)." },
                        "fs_write": { "type": "array", "items": { "type": "string" }, "description": "Dossiers inscriptibles (chemins absolus)." },
                        "exec": {
                            "type": "array",
                            "description": "Commandes autorisées, reprises de tes propres règles.",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "program": { "type": "string" },
                                    "args": { "type": "array", "items": { "type": "string" }, "description": "Arguments exacts ; « * » : un argument libre ; « ** » en dernier : suite libre. Omis : arguments libres." }
                                },
                                "required": ["program"]
                            }
                        },
                        "spawn": {
                            "type": "object",
                            "description": "Droit de créer ses propres enfants. Omis : aucun.",
                            "properties": { "max_children": { "type": "integer" }, "max_depth": { "type": "integer" } },
                            "required": ["max_children", "max_depth"]
                        },
                        "max_tokens": { "type": "integer", "description": "Budget de tokens, descendance comprise." }
                    }
                }
            }),
            &["name", "role", "instructions", "objective", "success_criteria"],
        ));
        tools.push(spec(
            "send_to_agent",
            "Envoie un message à l'un de tes enfants : réponse à sa question, précision, ou nouvelle mission (avec son critère de réussite mesurable).".into(),
            json!({ "agent": { "type": "string", "description": "Nom ou identifiant de l'enfant." }, "message": { "type": "string" } }),
            &["agent", "message"],
        ));
        tools.push(spec(
            "stop_agent",
            "Arrête définitivement l'un de tes enfants, et toute sa descendance.".into(),
            json!({ "agent": { "type": "string", "description": "Nom ou identifiant de l'enfant." } }),
            &["agent"],
        ));
        tools.push(spec("list_agents", "Liste tes enfants et leur état.".into(), json!({}), &[]));
    }
    if node.uplink.is_some() {
        tools.push(spec(
            "ask_parent",
            "Pose une question à ton parent quand il te manque une information, un droit ou une décision pour avancer. La réponse arrivera dans un message ; en attendant, avance sur ce qui n'en dépend pas, ou termine ton tour.".into(),
            json!({ "question": { "type": "string" } }),
            &["question"],
        ));
        tools.push(spec(
            "finish_mission",
            "Conclut ta mission et te met en pause jusqu'aux prochaines consignes de ton parent. À appeler seulement quand le critère de réussite est atteint et vérifié, ou quand tu as établi que la mission est impossible en l'état.".into(),
            json!({
                "outcome": { "type": "string", "enum": ["achieved", "impossible"] },
                "summary": { "type": "string", "description": "Ce qui a été fait, ou ce qui bloque et ce qu'il faudrait pour débloquer." },
                "evidence": { "type": "string", "description": "La mesure du critère de réussite : valeurs, sorties de commandes, chemins de fichiers." }
            }),
            &["outcome", "summary", "evidence"],
        ));
    }
    tools
}

fn text<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input[key].as_str().ok_or_else(|| format!("argument « {key} » manquant ou invalide"))
}

/// Exécute un outil. Le droit est revérifié ici, quoi que le modèle ait demandé.
pub async fn execute(node: &Arc<Node>, name: &str, input: &Value) -> Result<String, String> {
    if !input.is_object() {
        return Err("arguments illisibles : un objet JSON est attendu".into());
    }
    let caps = &node.caps;
    match name {
        "read_file" => {
            let path = caps.resolve_read(Path::new(text(input, "path")?))?;
            let meta = tokio::fs::metadata(&path).await.map_err(|e| e.to_string())?;
            if !meta.is_file() {
                return Err("ce n'est pas un fichier".into());
            }
            if meta.len() > MAX_READ {
                return Err(format!("fichier trop gros ({} octets, maximum {MAX_READ})", meta.len()));
            }
            let bytes = tokio::fs::read(&path).await.map_err(|e| e.to_string())?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
        "list_dir" => {
            let path = caps.resolve_read(Path::new(text(input, "path")?))?;
            let mut dir = tokio::fs::read_dir(&path).await.map_err(|e| e.to_string())?;
            let mut entries = Vec::new();
            while let Some(entry) = dir.next_entry().await.map_err(|e| e.to_string())? {
                let kind = entry.file_type().await.map(|t| if t.is_dir() { "/" } else { "" }).unwrap_or("");
                entries.push(format!("{}{kind}", entry.file_name().to_string_lossy()));
                if entries.len() >= 2000 {
                    entries.push("… [liste tronquée]".into());
                    break;
                }
            }
            entries.sort();
            Ok(if entries.is_empty() { "(dossier vide)".into() } else { entries.join("\n") })
        }
        "write_file" => {
            let path = caps.resolve_write(Path::new(text(input, "path")?))?;
            let content = text(input, "content")?;
            tokio::fs::write(&path, content).await.map_err(|e| e.to_string())?;
            Ok(format!("{} octets écrits dans {}", content.len(), path.display()))
        }
        "run_command" => run_command(caps, input).await,
        "spawn_agent" => spawn_agent(node, input).await,
        "send_to_agent" => {
            let id = node.child_id(text(input, "agent")?).ok_or("aucun enfant de ce nom")?;
            node.send_child(&id, Msg::Instruction { text: text(input, "message")?.to_string() }, true)?;
            Ok("message transmis ; sa réponse arrivera dans un prochain message".into())
        }
        "stop_agent" => {
            let id = node.child_id(text(input, "agent")?).ok_or("aucun enfant de ce nom")?;
            node.stop_child(&id, "arrêté par son parent");
            Ok("agent arrêté".into())
        }
        "list_agents" => {
            let children = node.snapshot().children;
            if children.is_empty() {
                return Ok("aucun enfant".into());
            }
            let lines: Vec<String> = children
                .iter()
                .map(|c| format!("{} ({}) — rôle : {} — état : {:?} — {} descendant(s)", c.name, c.id, c.role, c.status, c.children.len()))
                .collect();
            Ok(lines.join("\n"))
        }
        "ask_parent" => {
            let uplink = node.uplink.as_ref().ok_or("cet agent n'a pas de parent")?;
            let question = text(input, "question")?;
            uplink.send_reliable(Msg::Report { kind: ReportKind::Question, text: question.to_string() });
            node.mission.lock().unwrap().awaiting_parent = true;
            node.emit("report", format!("question au parent : {question}"));
            Ok("question transmise ; la réponse arrivera dans un prochain message".into())
        }
        "finish_mission" => {
            let uplink = node.uplink.as_ref().ok_or("cet agent n'a pas de parent")?;
            let kind = match text(input, "outcome")? {
                "achieved" => ReportKind::Achieved,
                "impossible" => ReportKind::Impossible,
                other => return Err(format!("outcome inconnu : {other} (achieved ou impossible)")),
            };
            let report = format!("{}\n\nPreuves : {}", text(input, "summary")?, text(input, "evidence")?);
            uplink.send_reliable(Msg::Report { kind, text: report.clone() });
            *node.mission.lock().unwrap() = Default::default();
            node.emit("report", format!("mission {} : {report}", if kind == ReportKind::Achieved { "accomplie" } else { "impossible" }));
            Ok("compte rendu transmis ; tu es en pause jusqu'aux prochaines consignes".into())
        }
        other => Err(format!("outil inconnu ou non autorisé : {other}")),
    }
}

async fn run_command(caps: &Capabilities, input: &Value) -> Result<String, String> {
    let program = text(input, "program")?;
    let args: Vec<String> = match &input["args"] {
        Value::Null => Vec::new(),
        Value::Array(items) => items
            .iter()
            .map(|a| a.as_str().map(str::to_string).ok_or("les arguments doivent être des chaînes"))
            .collect::<Result<_, _>>()?,
        _ => return Err("« args » doit être une liste de chaînes".into()),
    };
    caps.check_exec(program, &args)?;

    let mut command = Command::new(program);
    command.args(&args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    spawn::apply_safe_env(&mut command);
    let child = command.spawn().map_err(|e| format!("lancement impossible : {e}"))?;
    let output = tokio::time::timeout(COMMAND_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| format!("commande interrompue après {} s", COMMAND_TIMEOUT.as_secs()))?
        .map_err(|e| e.to_string())?;

    let clip = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes);
        if text.len() > MAX_OUTPUT {
            let mut end = MAX_OUTPUT;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}\n… [sortie tronquée]", &text[..end])
        } else {
            text.into_owned()
        }
    };
    let code = output.status.code().map(|c| c.to_string()).unwrap_or_else(|| "interrompu".into());
    let mut result = format!("code de sortie : {code}\n--- sortie ---\n{}", clip(&output.stdout));
    if !output.stderr.is_empty() {
        result.push_str(&format!("\n--- erreurs ---\n{}", clip(&output.stderr)));
    }
    Ok(result)
}

async fn spawn_agent(node: &Arc<Node>, input: &Value) -> Result<String, String> {
    let policy = node.caps.spawn.as_ref().ok_or("cet agent n'a pas le droit de créer des enfants")?;
    let name = text(input, "name")?.trim().to_string();
    let valid = !name.is_empty() && name.len() <= 40 && name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_');
    if !valid {
        return Err("nom invalide : 1 à 40 lettres, chiffres, tirets ou tirets bas".into());
    }
    let role = text(input, "role")?.to_string();
    let objective = text(input, "objective")?.trim();
    let criteria = text(input, "success_criteria")?.trim();
    if objective.is_empty() || criteria.is_empty() {
        return Err("une mission a besoin d'un objectif et d'un critère de réussite mesurable".into());
    }

    let mut caps: Capabilities = match &input["capabilities"] {
        Value::Null => Capabilities::default(),
        value => serde_json::from_value(value.clone()).map_err(|e| format!("droits invalides : {e}"))?,
    };
    let model = input["model"].as_str().unwrap_or(&node.model).to_string();
    caps.models = vec![model.clone()];
    if let Some(spawn) = &mut caps.spawn {
        spawn.targets = vec!["local".into()];
    }
    node.caps.check_grant(&caps)?;
    let listen = node.listen.get().ok_or("cet agent n'accepte pas de connexion d'enfants")?;

    let boot = Bootstrap {
        agent_id: format!("a-{}", &random_hex(1)[..10]),
        name: name.clone(),
        role: role.clone(),
        instructions: text(input, "instructions")?.to_string(),
        model: model.clone(),
        capabilities: caps.clone(),
        parent_id: node.id.clone(),
        parent_name: node.name.clone(),
        parent_url: format!("ws://{listen}/link"),
        token: random_hex(2),
        depth: node.depth + 1,
        target: "local".into(),
        timing: node.timing,
    };
    let placeholder = NodeInfo {
        id: boot.agent_id.clone(),
        name: name.clone(),
        role,
        parent_id: Some(node.id.clone()),
        depth: boot.depth,
        status: Status::Starting,
        model,
        target: boot.target.clone(),
        host: node.host.host.clone(),
        os: node.host.os.clone(),
        ips: node.host.ips.clone(),
        pid: 0,
        started_at_ms: now_ms(),
        last_seen_ms: now_ms(),
        tokens_used: 0,
        capabilities: caps.clone(),
        children: Vec::new(),
    };
    // inscrit avant le lancement : la place est prise, et l'enfant sera reconnu dès sa connexion
    {
        let mut children = node.children.lock().unwrap();
        let alive = children.values().filter(|c| !c.stopped);
        if alive.clone().count() as u32 >= policy.max_children {
            return Err(format!("limite de {} enfants en vie atteinte : arrêtes-en un d'abord", policy.max_children));
        }
        if alive.clone().any(|c| c.name == name) {
            return Err(format!("un enfant s'appelle déjà « {name} »"));
        }
        children.insert(
            boot.agent_id.clone(),
            ChildRec {
                id: boot.agent_id.clone(),
                name: name.clone(),
                token: boot.token.clone(),
                caps,
                subtree: placeholder,
                conn: None,
                ever_connected: false,
                last_seen: Instant::now(),
                last_seen_ms: now_ms(),
                stopped: false,
                outbox: Outbox::default(),
                last_recv_seq: 0,
                tokens_used: 0,
                mission_open: false,
                process: None,
            },
        );
    }
    match spawn::spawn_local(node, &boot).await {
        Ok(process) => {
            if let Some(child) = node.children.lock().unwrap().get_mut(&boot.agent_id) {
                child.process = Some(process);
            }
        }
        Err(e) => {
            node.children.lock().unwrap().remove(&boot.agent_id);
            return Err(e);
        }
    }
    let mission = format!("Mission : {objective}\n\nCritère de réussite mesurable : {criteria}");
    node.send_child(&boot.agent_id, Msg::Instruction { text: mission }, true)?;
    node.emit("spawn", format!("agent {name} ({}) créé — rôle : {}", boot.agent_id, boot.role));
    node.tree_dirty.notify_one();
    Ok(format!(
        "agent « {name} » créé (identifiant {}). Sa mission lui est transmise ; son compte rendu arrivera dans un prochain message.",
        boot.agent_id
    ))
}
