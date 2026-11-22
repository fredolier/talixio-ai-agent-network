//! Trois niveaux, de vrais process : maître → chef → lecteur. Question au parent, compte rendu
//! en cascade, activité du petit-enfant au journal du maître, relance, coupure du lien puis
//! reprise sans perte, arrêt d'un petit-enfant par l'API.
//! Écrit à l'origine par la vérification indépendante (docs/VERIFICATION.md).
#![cfg(target_os = "linux")]

use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};

const TOKEN: &str = "jeton-de-test-0123456789";


struct Ctx {
    file: String,
}

async fn fake_model(State(ctx): State<Arc<Ctx>>, Json(body): Json<Value>) -> Json<Value> {
    let file = &ctx.file;
    let folder = std::path::Path::new(file).parent().unwrap().to_str().unwrap().to_string();
    let messages = body["messages"].as_array().unwrap();
    let system = messages[0]["content"].as_str().unwrap();
    let who = ["maître", "chef", "lecteur"].into_iter().find(|n| system.contains(&format!("Tu es « {n} »"))).unwrap();
    let last = messages.last().unwrap();
    let role = last["role"].as_str().unwrap();
    let text = last["content"].as_str().unwrap_or("");
    let call = |name: &str, args: Value| {
        json!({ "role": "assistant", "content": null, "tool_calls": [
            { "id": "call_1", "type": "function", "function": { "name": name, "arguments": args.to_string() } }
        ] })
    };
    let say = |text: &str| json!({ "role": "assistant", "content": text });
    let finish = |summary: &str| call("finish_mission", json!({ "outcome": "achieved", "summary": summary, "evidence": "vu" }));
    let child = text.contains("source=\"enfant\"");
    let parent = text.contains("source=\"parent\"");
    let user = text.contains("source=\"utilisateur\"");
    let message = match (who, role) {
        ("maître", "user") if child && text.contains("RELANCE-FAIT") => say("RELANCE OK"),
        ("maître", "user") if child && text.contains("RECU-APRES") => say("COUPURE OK"),
        ("maître", "user") if child && text.contains("SECRET-42") => say("FINI SECRET-42"),
        ("maître", "user") if user && text.contains("demarre") => call("spawn_agent", json!({
            "name": "chef", "role": "chef d'équipe", "instructions": "Délègue.",
            "objective": "Trouve et rapporte le contenu.", "success_criteria": "Le contenu exact est rapporté.",
            "capabilities": { "fs_read": [folder], "max_tokens": 1000000, "spawn": { "max_children": 1, "max_depth": 1 } }
        })),
        ("maître", "user") if user && text.contains("deuxieme") => call("send_to_agent", json!({ "agent": "chef", "message": "consigne-deux" })),
        ("maître", "user") if user && text.contains("troisieme") => call("send_to_agent", json!({ "agent": "chef", "message": "consigne-trois" })),
        ("maître", _) => say("C'est noté."),
        ("chef", "user") if parent && text.contains("consigne-deux") => finish("RELANCE-FAIT"),
        ("chef", "user") if parent && text.contains("consigne-trois") => finish("RECU-APRES"),
        ("chef", "user") if parent && text.contains("Mission") => call("spawn_agent", json!({
            "name": "lecteur", "role": "lecteur", "instructions": "Lis.",
            "objective": "Rapporte le contenu.", "success_criteria": "Le contenu exact est rapporté.",
            "capabilities": { "fs_read": [folder], "max_tokens": 500000 }
        })),
        ("chef", "user") if child && text.contains("type=\"question\"") => call("send_to_agent", json!({ "agent": "lecteur", "message": format!("REPONSE-CHEMIN {file}") })),
        ("chef", "user") if child && text.contains("SECRET-42") => finish("Contenu : SECRET-42"),
        // le chef veut conclure dès qu'il a délégué : refusé tant que le lecteur travaille
        ("chef", "tool") if text.contains("« lecteur » créé") => finish("TROP-TOT"),
        ("chef", "tool") if text.contains("encore au travail") => say("j'attends le lecteur"),
        ("chef", _) => say("ok"),
        ("lecteur", "user") if parent && text.contains("REPONSE-CHEMIN") => call("read_file", json!({ "path": file })),
        ("lecteur", "user") if parent && text.contains("Mission") => call("ask_parent", json!({ "question": "QUESTION-CHEMIN ?" })),
        ("lecteur", "tool") if text.contains("SECRET-42") => finish("Contenu : SECRET-42"),
        (_, _) => say("j'attends"),
    };
    Json(json!({ "choices": [{ "message": message }], "usage": { "prompt_tokens": 10, "completion_tokens": 5 } }))
}

struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    async fn get(&self, path: &str) -> Value {
        self.http.get(format!("{}{path}", self.base)).bearer_auth(TOKEN).send().await.unwrap().json().await.unwrap()
    }
    async fn say(&self, text: &str) {
        let r = self.http.post(format!("{}/api/messages", self.base)).bearer_auth(TOKEN).json(&json!({ "text": text }));
        assert_eq!(r.send().await.unwrap().status(), 202);
    }
    async fn wait_event(&self, what: &str, matches: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let body = self.get("/api/activity?after=0&limit=5000").await;
            if let Some(event) = body["events"].as_array().unwrap().iter().find(|e| matches(e)) {
                return event.clone();
            }
            assert!(start.elapsed() < Duration::from_secs(40), "jamais vu : {what}\njournal : {body:#}");
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
}

fn alive(pid: u64) -> bool {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    !stat.is_empty() && !stat.contains(") Z ")
}

fn signal(pid: u64, sig: &str) {
    assert!(std::process::Command::new("kill").args([sig, &pid.to_string()]).status().unwrap().success());
}

#[tokio::test(flavor = "multi_thread")]
async fn trois_niveaux_de_bout_en_bout() {
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path().canonicalize().unwrap();
    let data = tmp.join("data");
    std::fs::create_dir_all(&data).unwrap();
    let file = data.join("secret.txt");
    std::fs::write(&file, "SECRET-42").unwrap();

    let ctx = Arc::new(Ctx { file: file.to_str().unwrap().to_string() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_port = listener.local_addr().unwrap().port();
    let app = Router::new().route("/v1/chat/completions", post(fake_model)).with_state(ctx.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let config = tmp.join("talixio.toml");
    std::fs::write(&config, format!(r#"
model = "main"
listen = "127.0.0.1:{port}"
state_dir = {state:?}
log_max_bytes = 12345

[timing]
heartbeat_secs = 1
suspend_after_secs = 2
die_after_secs = 40

[models.main]
provider = "openai"
base_url = "http://127.0.0.1:{model_port}/v1"
model = "faux"

[capabilities]
fs_read = [{data:?}]
max_tokens = 5000000

[capabilities.spawn]
max_children = 2
max_depth = 2
"#, state = tmp.join("state"))).unwrap();
    let _root = tokio::process::Command::new(env!("CARGO_BIN_EXE_talixio-agent"))
        .args(["root", "--config", config.to_str().unwrap()])
        .env("TALIXIO_API_TOKEN", TOKEN)
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let start = Instant::now();
    let fingerprint = loop {
        if let Ok(f) = std::fs::read_to_string(tmp.join("state/tls-fingerprint")) { break f; }
        assert!(start.elapsed() < Duration::from_secs(15));
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let pinned = talixio_agent::tls::pinned_client(&fingerprint).unwrap();
    let api = Api { http: reqwest::Client::builder().use_preconfigured_tls(pinned).build().unwrap(), base: format!("https://127.0.0.1:{port}") };
    while api.http.get(format!("{}/api/health", api.base)).send().await.is_err() {
        assert!(start.elapsed() < Duration::from_secs(15));
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // 1. trois niveaux, question au parent, compte rendu en cascade
    api.say("demarre").await;
    api.wait_event("fin", |e| e["agent_name"] == "maître" && e["kind"] == "message" && e["text"] == "FINI SECRET-42").await;
    // un parent ne conclut pas pendant que son enfant travaille : refusé, et rien n'est remonté au maître
    let refused = api.wait_event("conclusion prématurée refusée", |e| e["agent_name"] == "chef" && e["kind"] == "tool_error" && e["text"].as_str().unwrap().contains("encore au travail (lecteur)")).await;
    assert!(refused["text"].as_str().unwrap().contains("stop_agent"));
    let journal = api.get("/api/activity?after=0&limit=5000").await;
    assert!(!journal["events"].as_array().unwrap().iter().any(|e| e["text"].as_str().unwrap().contains("TROP-TOT") && e["kind"] != "tool" && e["kind"] != "tool_error"), "{journal:#}");
    api.wait_event("lecture du lecteur", |e| e["agent_name"] == "lecteur" && e["kind"] == "tool").await;
    // rien de ce qu'a fait le petit-enfant ne manque au journal du maître, dès son premier geste
    let journal = api.get("/api/activity?after=0&limit=5000").await;
    let lecteur: Vec<String> = journal["events"].as_array().unwrap().iter().filter(|e| e["agent_name"] == "lecteur").map(|e| format!("{} {}", e["kind"].as_str().unwrap(), e["text"].as_str().unwrap())).collect();
    assert!(lecteur.len() >= 3, "journal du petit-enfant : {lecteur:#?}");
    assert!(lecteur[0].starts_with("input du parent : Mission"), "premier geste perdu : {lecteur:#?}");
    assert!(lecteur[1].starts_with("tool ask_parent"), "question perdue : {lecteur:#?}");
    assert!(lecteur[2].starts_with("report question au parent"), "question perdue : {lecteur:#?}");
    tokio::time::sleep(Duration::from_secs(1)).await;
    let tree = api.get("/api/tree").await;
    let chef = &tree["children"][0];
    let lecteur = &chef["children"][0];
    assert_eq!((chef["name"].as_str(), lecteur["name"].as_str()), (Some("chef"), Some("lecteur")));
    assert_eq!(lecteur["parent_id"], chef["id"]);
    assert_eq!(lecteur["depth"], 2);
    assert!(chef["tokens_used"].as_u64().unwrap() > lecteur["tokens_used"].as_u64().unwrap());
    assert!(lecteur["tokens_used"].as_u64().unwrap() > 0);
    let chef_pid = chef["pid"].as_u64().unwrap();
    let lecteur_pid = lecteur["pid"].as_u64().unwrap();
    let lecteur_id = lecteur["id"].as_str().unwrap().to_string();

    // 2. relance d'un agent en pause
    assert_eq!(chef["status"], "idle");
    api.say("deuxieme").await;
    api.wait_event("relance", |e| e["agent_name"] == "maître" && e["text"] == "RELANCE OK").await;

    // 3. coupure du lien : le chef est figé, le maître le voit injoignable, un message part pendant la coupure
    signal(chef_pid, "-STOP");
    let start = Instant::now();
    loop {
        let tree = api.get("/api/tree").await;
        if tree["children"][0]["status"] == "disconnected" {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(15), "coupure jamais vue : {}", tree["children"][0]["status"]);
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    api.say("troisieme").await;
    api.wait_event("envoi pendant la coupure", |e| e["agent_name"] == "maître" && e["kind"] == "tool" && e["text"].as_str().unwrap().contains("consigne-trois")).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    signal(chef_pid, "-CONT");
    api.wait_event("message reçu après la coupure", |e| e["agent_name"] == "maître" && e["text"] == "COUPURE OK").await;
    assert_ne!(api.get("/api/tree").await["children"][0]["status"], "disconnected");

    // 4. arrêt d'un petit-enfant par l'API
    assert!(alive(lecteur_pid));
    let r = api.http.post(format!("{}/api/agents/{lecteur_id}/stop", api.base)).bearer_auth(TOKEN).json(&json!({})).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let start = Instant::now();
    while alive(lecteur_pid) {
        assert!(start.elapsed() < Duration::from_secs(15), "le petit-enfant vit encore");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let tree = api.get("/api/tree").await;
    assert_eq!(tree["children"][0]["children"][0]["status"], "stopped");
}
