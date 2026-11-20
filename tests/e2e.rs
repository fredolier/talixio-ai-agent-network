//! La grappe de bout en bout : un vrai maître, un vrai enfant, et un faux serveur de modèles.

use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};

const TOKEN: &str = "jeton-de-test-0123456789";

/// Le faux modèle : il décide d'après qui parle (maître ou enfant) et le dernier message reçu.
async fn fake_model(State(file): State<Arc<String>>, Json(body): Json<Value>) -> Json<Value> {
    let messages = body["messages"].as_array().unwrap();
    let is_root = messages[0]["content"].as_str().unwrap().contains("agent maître");
    let last = messages.last().unwrap();
    let text = last["content"].as_str().unwrap_or("");
    let call = |name: &str, args: Value| {
        json!({ "role": "assistant", "content": null, "tool_calls": [
            { "id": "call_1", "type": "function", "function": { "name": name, "arguments": args.to_string() } }
        ] })
    };
    let say = |text: &str| json!({ "role": "assistant", "content": text });
    let spawn = |name: &str, folder: &str| {
        call("spawn_agent", json!({
            "name": name, "role": "lecteur", "instructions": "Lis ce qu'on te demande.",
            "objective": format!("Lis le fichier {file} et rapporte son contenu."),
            "success_criteria": "Le contenu exact du fichier figure dans le compte rendu.",
            "capabilities": { "fs_read": [folder] }
        }))
    };
    let folder = std::path::Path::new(file.as_str()).parent().unwrap().to_str().unwrap().to_string();
    let message = match (is_root, last["role"].as_str().unwrap()) {
        (true, "user") if text.contains("source=\"enfant\"") && text.contains("SECRET-42") => say("Le lecteur dit : SECRET-42"),
        (true, "user") if text.contains("lance le test") => spawn("lecteur", &folder),
        (true, "user") if text.contains("escalade") => spawn("pirate", "/"),
        (true, "tool") if text.contains("Erreur") => say("Création refusée."),
        (true, _) => say("C'est noté."),
        (false, "user") if text.contains("Mission") => call("read_file", json!({ "path": file.as_str() })),
        (false, "tool") if text.contains("SECRET-42") => call("finish_mission", json!({
            "outcome": "achieved", "summary": "Contenu : SECRET-42", "evidence": "fichier lu"
        })),
        (false, _) => say("Rien à faire."),
    };
    Json(json!({ "choices": [{ "message": message }], "usage": { "prompt_tokens": 10, "completion_tokens": 5 } }))
}

struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    async fn get(&self, path: &str) -> Value {
        let r = self.http.get(format!("{}{path}", self.base)).bearer_auth(TOKEN).send().await.unwrap();
        r.json().await.unwrap()
    }

    async fn say(&self, text: &str) {
        let r = self.http.post(format!("{}/api/messages", self.base)).bearer_auth(TOKEN).json(&json!({ "text": text }));
        assert_eq!(r.send().await.unwrap().status(), 202);
    }

    /// Attend qu'un événement du journal satisfasse la condition.
    async fn wait_event(&self, what: &str, matches: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let body = self.get("/api/activity?after=0&limit=5000").await;
            if let Some(event) = body["events"].as_array().unwrap().iter().find(|e| matches(e)) {
                return event.clone();
            }
            assert!(start.elapsed() < Duration::from_secs(30), "jamais vu : {what}\njournal : {body:#}");
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
}

#[cfg(unix)]
fn alive(pid: u64) -> bool {
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().unwrap().success()
}

#[tokio::test(flavor = "multi_thread")]
async fn cluster_end_to_end() {
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path();
    let data = tmp.canonicalize().unwrap().join("data");
    std::fs::create_dir_all(&data).unwrap();
    let file = data.join("secret.txt");
    std::fs::write(&file, "SECRET-42").unwrap();

    // le faux serveur de modèles
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/chat/completions", post(fake_model))
        .with_state(Arc::new(file.to_str().unwrap().to_string()));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // le maître
    let port = free_port();
    let config = tmp.join("talixio.toml");
    std::fs::write(
        &config,
        format!(
            r#"
model = "main"
listen = "127.0.0.1:{port}"
state_dir = {state:?}

[timing]
heartbeat_secs = 1
suspend_after_secs = 2
die_after_secs = 4

[models.main]
provider = "openai"
base_url = "http://127.0.0.1:{model_port}/v1"
model = "faux"

[capabilities]
fs_read = [{data:?}]

[capabilities.spawn]
max_children = 2
max_depth = 1
"#,
            state = tmp.join("state"),
        ),
    )
    .unwrap();
    let mut root = tokio::process::Command::new(env!("CARGO_BIN_EXE_talixio-agent"))
        .args(["root", "--config", config.to_str().unwrap()])
        .env("TALIXIO_API_TOKEN", TOKEN)
        .env("SECRET_DU_MAITRE", "ne-doit-pas-fuir")
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

    // le maître écrit l'empreinte de son certificat dans son dossier d'état : on n'accepte que lui
    let start = Instant::now();
    let fingerprint = loop {
        if let Ok(fingerprint) = std::fs::read_to_string(tmp.join("state/tls-fingerprint")) {
            break fingerprint;
        }
        assert!(start.elapsed() < Duration::from_secs(15), "le maître ne démarre pas");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let pinned = talixio_agent::tls::pinned_client(&fingerprint).unwrap();
    let api = Api {
        http: reqwest::Client::builder().use_preconfigured_tls(pinned).build().unwrap(),
        base: format!("https://127.0.0.1:{port}"),
    };
    while api.http.get(format!("{}/api/health", api.base)).send().await.is_err() {
        assert!(start.elapsed() < Duration::from_secs(15), "le maître ne répond pas");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    // en clair, ou sans épingler le bon certificat : rien
    assert!(reqwest::get(format!("http://127.0.0.1:{port}/api/health")).await.is_err());
    assert!(reqwest::get(format!("https://127.0.0.1:{port}/api/health")).await.is_err());

    // les notes de version, pour l'interface
    let notes = api.get("/api/releases").await;
    assert_eq!(notes["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(notes["releases"][0]["version"], notes["version"]);
    assert!(notes["releases"][0]["changes"][0]["kind"].is_string());

    // sans jeton : rien
    let denied = api.http.get(format!("{}/api/tree", api.base)).send().await.unwrap();
    assert_eq!(denied.status(), 401);

    // le maître crée un enfant, qui lit le fichier et conclut sa mission ; le maître reçoit le compte rendu
    api.say("lance le test").await;
    api.wait_event("la réponse finale du maître", |e| {
        e["kind"] == "message" && e["agent_name"] == "maître" && e["text"] == "Le lecteur dit : SECRET-42"
    })
    .await;
    // l'activité de l'enfant est remontée jusqu'au journal du maître
    api.wait_event("la lecture faite par l'enfant", |e| {
        e["agent_name"] == "lecteur" && e["kind"] == "tool" && e["text"].as_str().unwrap().starts_with("read_file")
    })
    .await;

    let tree = api.get("/api/tree").await;
    let child = &tree["children"][0];
    assert_eq!(child["name"], "lecteur");
    assert_eq!(child["role"], "lecteur");
    assert_eq!(child["parent_id"], tree["id"]);
    assert_eq!(child["depth"], 1);
    assert_eq!(child["status"], "idle");
    assert_eq!(child["capabilities"]["fs_read"][0], data.to_str().unwrap());
    assert_eq!(child["capabilities"]["spawn"], Value::Null);
    assert!(child["tokens_used"].as_u64().unwrap() > 0);
    let child_pid = child["pid"].as_u64().unwrap();
    assert!(child_pid > 0);

    // l'environnement du maître ne passe pas à l'enfant
    #[cfg(target_os = "linux")]
    {
        // un agent ferme son environnement aux autres process : seul root peut encore le lire
        if let Ok(environ) = std::fs::read(format!("/proc/{child_pid}/environ")) {
            let environ = String::from_utf8_lossy(&environ);
            assert!(!environ.contains("SECRET_DU_MAITRE") && !environ.contains(TOKEN), "fuite : {environ}");
        }
    }

    // un enfant ne peut pas recevoir plus de droits que son parent
    api.say("escalade").await;
    api.wait_event("le refus de l'escalade", |e| {
        e["kind"] == "tool_error" && e["text"].as_str().unwrap().contains("hors de vos propres droits")
    })
    .await;
    assert_eq!(api.get("/api/tree").await["children"].as_array().unwrap().len(), 1);

    // le maître disparaît brutalement : l'enfant, sans parent, finit par s'éteindre
    root.kill().await.unwrap();
    #[cfg(unix)]
    {
        assert!(alive(child_pid), "l'enfant devrait d'abord rester en veille");
        let start = Instant::now();
        while alive(child_pid) {
            assert!(start.elapsed() < Duration::from_secs(20), "l'enfant orphelin ne s'éteint pas");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
}
