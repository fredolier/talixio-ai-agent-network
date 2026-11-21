//! Un modèle servi par la commande `claude` (abonnement) : un vrai maître, et une fausse
//! commande `claude` (examples/faux_claude.rs) qui parle comme la vraie et lance le vrai pont.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TOKEN: &str = "jeton-de-test-0123456789";

fn faux_claude() -> PathBuf {
    let dir = Path::new(env!("CARGO_BIN_EXE_talixio-agent")).parent().unwrap().join("examples");
    dir.join(format!("faux_claude{}", std::env::consts::EXE_SUFFIX))
}

struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    async fn get(&self, path: &str) -> Value {
        self.http.get(format!("{}{path}", self.base)).bearer_auth(TOKEN).send().await.unwrap().json().await.unwrap()
    }

    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self.http.post(format!("{}{path}", self.base)).bearer_auth(TOKEN).json(&body).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }

    async fn say(&self, text: &str) {
        assert_eq!(self.post("/api/messages", json!({ "text": text })).await.0, 202);
    }

    async fn wait_message(&self, what: &str) -> String {
        let start = Instant::now();
        loop {
            let body = self.get("/api/activity?after=0&limit=5000").await;
            let found = body["events"].as_array().unwrap().iter().find(|e| e["kind"] == "message" && e["text"].as_str().is_some_and(|t| t.contains(what)));
            if let Some(event) = found {
                return event["text"].as_str().unwrap().to_string();
            }
            assert!(start.elapsed() < Duration::from_secs(30), "jamais vu : {what}\njournal : {body:#}");
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_claude_command_serves_a_model_through_our_own_tools() {
    assert!(faux_claude().is_file(), "la fausse commande claude n'est pas compilée : {}", faux_claude().display());
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path().canonicalize().unwrap();
    let (state, work, journal) = (tmp.join("state"), tmp.join("work"), tmp.join("journal"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::write(work.join("note.txt"), "hirondelle").unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let config = tmp.join("talixio.toml");
    std::fs::write(
        &config,
        format!(
            "instructions = \"consigne-reconnaissable\"\nlisten = \"127.0.0.1:{port}\"\nstate_dir = {state:?}\nclaude_program = {:?}\nclaude_env = [\"FAUX_CLAUDE_JOURNAL\", \"ANTHROPIC_API_KEY\", \"TALIXIO_API_TOKEN\"]\n[capabilities]\nfs_read = [{work:?}]\n",
            faux_claude()
        ),
    )
    .unwrap();
    let _root = tokio::process::Command::new(env!("CARGO_BIN_EXE_talixio-agent"))
        .args(["root", "--config", config.to_str().unwrap()])
        .env("TALIXIO_API_TOKEN", TOKEN)
        .env("ANTHROPIC_API_KEY", "sk-ant-cle-qui-ne-doit-pas-passer")
        .env("FAUX_CLAUDE_JOURNAL", &journal)
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let start = Instant::now();
    let fingerprint = loop {
        if let Ok(fingerprint) = std::fs::read_to_string(state.join("tls-fingerprint")) {
            break fingerprint;
        }
        assert!(start.elapsed() < Duration::from_secs(15), "le maître ne démarre pas");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let pinned = talixio_agent::tls::pinned_client(&fingerprint).unwrap();
    let api = Api { http: reqwest::Client::builder().use_preconfigured_tls(pinned).build().unwrap(), base: format!("https://127.0.0.1:{port}") };
    while api.http.get(format!("{}/api/health", api.base)).send().await.is_err() {
        assert!(start.elapsed() < Duration::from_secs(15), "le maître ne répond pas");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // le pont ne s'ouvre qu'avec le jeton d'un tour en cours : ni sans jeton, ni avec celui de l'API
    for token in ["", TOKEN] {
        let denied = api.http.post(format!("{}/bridge", api.base)).bearer_auth(token).json(&json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" })).send().await.unwrap();
        assert_eq!(denied.status(), 401);
    }

    // connecter : la commande est là ; aucune clé, rien de gardé
    let models = api.get("/api/models").await;
    let provider = models["providers"].as_array().unwrap().iter().find(|p| p["id"] == "claude-abonnement").unwrap().clone();
    assert_eq!(provider["connect"], "cli");
    assert_eq!(api.post("/api/providers/claude-abonnement/connect", json!({ "key": "" })).await, (200, json!({ "connected": "claude-abonnement", "verified": true })));
    assert!(!state.join("keys").join("claude-abonnement.key").exists());
    let (status, activated) = api.post("/api/models/activate", json!({ "provider": "claude-abonnement", "model": "sonnet" })).await;
    assert_eq!((status, &activated["default"]), (200, &json!(true)), "{activated}");

    // un tour simple, puis sa suite dans la même session
    api.say("bonjour").await;
    assert!(api.wait_message("bonjour").await.contains("écho (début, sonnet)"));
    api.say("encore").await;
    assert!(api.wait_message("encore").await.contains("écho (suite, sonnet)"));

    // un outil : servi par le pont, exécuté par le maître avec ses droits
    api.say(&format!("OUTIL read_file {}", json!({ "path": work.join("note.txt") }))).await;
    let read = api.wait_message("réponse=hirondelle").await;
    assert!(read.contains("erreur=false") && !read.contains("outils=0"), "{read}");
    // hors de ses droits : refusé comme pour tout autre modèle
    std::fs::write(tmp.join("ailleurs.txt"), "secret-ailleurs").unwrap();
    api.say(&format!("OUTIL read_file {}", json!({ "path": tmp.join("ailleurs.txt") }))).await;
    let refused = api.wait_message("erreur=true").await;
    assert!(refused.contains("hors des dossiers autorisés") && !refused.contains("secret-ailleurs"), "{refused}");
    // un outil que l'agent n'a pas : jamais exécuté
    api.say("OUTIL outil_invente {\"x\":1}").await;
    assert!(api.wait_message("outil inconnu").await.contains("erreur=true"));

    // ce que la commande a reçu : nos consignes par un fichier fermé, ses outils intégrés coupés,
    // et rien des secrets du maître, même si la configuration demandait de les transmettre
    let calls: Vec<Value> = std::fs::read_to_string(&journal).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert!(calls.len() >= 5);
    for call in &calls {
        let args: Vec<&str> = call["args"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        let env: Vec<&str> = call["env"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
        assert!(env.contains(&"FAUX_CLAUDE_JOURNAL") && !env.contains(&"ANTHROPIC_API_KEY") && !env.contains(&"TALIXIO_API_TOKEN"), "{env:?}");
        let pair = |flag: &str, value: &str| args.windows(2).any(|w| w[0] == flag && w[1] == value);
        assert!(pair("--tools", "") && pair("--setting-sources", "") && pair("--permission-mode", "dontAsk") && pair("--allowedTools", "mcp__talixio"), "{args:?}");
        assert!(args.contains(&"--strict-mcp-config") && args.contains(&"-p"));
        assert!(call["system"].as_str().unwrap().contains("consigne-reconnaissable"));
        assert!(!args.iter().any(|a| a.contains("consigne-reconnaissable") || a.contains("TALIXIO_BRIDGE_TOKEN")), "rien de sensible sur la ligne de commande");
    }
    let session = |call: &Value, flag: &str| {
        let args = call["args"].as_array().unwrap();
        args.iter().position(|a| a == flag).map(|i| args[i + 1].as_str().unwrap().to_string())
    };
    assert_eq!(session(&calls[0], "--session-id"), session(&calls[1], "--resume"));
    // les fichiers d'un tour partent avec lui
    let start = Instant::now();
    while std::fs::read_dir(state.join("claude")).unwrap().count() > 0 {
        assert!(start.elapsed() < Duration::from_secs(10), "fichiers de tour restés");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // une commande absente : le fournisseur ne se connecte pas
    api.post("/api/providers/claude-abonnement/disconnect", json!({})).await;
}
