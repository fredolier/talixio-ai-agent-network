//! Les agents permanents : le maître est tué, relancé, et retrouve ses agents avec leur mémoire.

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use axum::{routing::post, Json, Router};
use serde_json::{json, Value};

const TOKEN: &str = "jeton-de-test-0123456789";

/// Le faux modèle : le maître crée un agent permanent ; l'agent note où il en est, et, relancé, le relit.
async fn fake_model(Json(body): Json<Value>) -> Json<Value> {
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
    let spawn = |name: &str, extra: Value| {
        let mut args = json!({
            "name": name, "role": "veille", "instructions": "Veille.", "objective": "Surveille le dossier.",
            "success_criteria": "Un point d'étape est rendu.", "capabilities": {}
        });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        call("spawn_agent", args)
    };
    let message = match (is_root, last["role"].as_str().unwrap()) {
        (true, "user") if text.contains("garde un veilleur") => spawn("veilleur", json!({ "permanent": true })),
        (true, "user") if text.contains("crée un passant") => spawn("passant", json!({})),
        (true, _) => say("C'est noté."),
        // relancé : il relit sa mémoire avant tout
        (false, "user") if text.contains("viens d'être relancé") => call("memory_read", json!({})),
        (false, "tool") if text.contains("## etat") => {
            call("finish_mission", json!({ "outcome": "achieved", "summary": "Reprise : étape 1 déjà faite, je continue.", "evidence": "mémoire relue" }))
        }
        (false, "user") if text.contains("agent permanent") => call("memory_write", json!({ "key": "etat", "content": "étape 1 faite" })),
        (false, "user") => call("finish_mission", json!({ "outcome": "achieved", "summary": "Passant : fini.", "evidence": "rien" })),
        (false, _) => call("finish_mission", json!({ "outcome": "achieved", "summary": "Première vie : étape 1 notée.", "evidence": "note écrite" })),
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
            assert!(start.elapsed() < Duration::from_secs(30), "jamais vu : {what}\njournal : {body:#}");
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    /// Les enfants en vie du maître, par nom.
    async fn children(&self) -> Vec<(String, String)> {
        let tree = self.get("/api/tree").await;
        let alive = tree["children"].as_array().unwrap().iter().filter(|c| c["status"] != "stopped");
        alive.map(|c| (c["name"].as_str().unwrap().to_string(), c["id"].as_str().unwrap().to_string())).collect()
    }
}

async fn start_root(config: &Path, state: &Path, port: u16) -> (tokio::process::Child, Api) {
    let _ = std::fs::remove_file(state.join("tls-fingerprint"));
    let root = tokio::process::Command::new(env!("CARGO_BIN_EXE_talixio-agent"))
        .args(["root", "--config", config.to_str().unwrap()])
        .env("TALIXIO_API_TOKEN", TOKEN)
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
    (root, api)
}

#[tokio::test(flavor = "multi_thread")]
async fn permanent_agents_come_back_with_their_memory() {
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path().canonicalize().unwrap();
    let state = tmp.join("state");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_port = listener.local_addr().unwrap().port();
    tokio::spawn(async move { axum::serve(listener, Router::new().route("/v1/chat/completions", post(fake_model))).await.unwrap() });

    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let config = tmp.join("talixio.toml");
    let text = |children: u32| {
        format!(
            "model = \"main\"\nlisten = \"127.0.0.1:{port}\"\nstate_dir = {state:?}\n\
             [timing]\nheartbeat_secs = 1\nsuspend_after_secs = 2\ndie_after_secs = 4\n\
             [models.main]\nprovider = \"openai\"\nbase_url = \"http://127.0.0.1:{model_port}/v1\"\nmodel = \"faux\"\n\
             [capabilities.spawn]\nmax_children = {children}\nmax_depth = 1\n"
        )
    };
    std::fs::write(&config, text(4)).unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;

    // un agent permanent et un agent ordinaire
    api.say("garde un veilleur").await;
    api.wait_event("la première vie du veilleur", |e| e["text"].as_str().is_some_and(|t| t.contains("Première vie : étape 1 notée."))).await;
    api.wait_event("la création dite permanente", |e| e["kind"] == "spawn" && e["text"].as_str().unwrap().ends_with("— permanent")).await;
    api.say("crée un passant").await;
    api.wait_event("le passant", |e| e["text"].as_str().is_some_and(|t| t.contains("Passant : fini."))).await;
    let before = api.children().await;
    let mut names: Vec<&str> = before.iter().map(|(name, _)| name.as_str()).collect();
    names.sort();
    assert_eq!(names, ["passant", "veilleur"]);
    let id = before.iter().find(|(name, _)| name == "veilleur").unwrap().1.clone();

    // le maître est tué net, comme par une coupure de courant, puis relancé
    root.kill().await.unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    api.wait_event("la relance du veilleur", |e| e["kind"] == "spawn" && e["text"].as_str().unwrap().contains("agent permanent veilleur relancé")).await;
    // il revient seul, sous le même identifiant, et retrouve ce qu'il avait noté
    api.wait_event("la reprise", |e| e["text"].as_str().is_some_and(|t| t.contains("Reprise : étape 1 déjà faite"))).await;
    assert_eq!(api.children().await, vec![("veilleur".to_string(), id.clone())], "le passant ne revient pas");

    // ses droits sont revérifiés : le maître n'a plus le droit de créer, l'agent n'est pas relancé, mais reste inscrit
    root.kill().await.unwrap();
    std::fs::write(&config, text(0)).unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    let refused = api.wait_event("le refus", |e| e["kind"] == "error" && e["text"].as_str().unwrap().contains("agent permanent veilleur non relancé")).await;
    assert!(refused["text"].as_str().unwrap().contains("limite de 0 enfants"), "{refused}");
    assert!(api.children().await.is_empty());
    root.kill().await.unwrap();
    std::fs::write(&config, text(4)).unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    let start = Instant::now();
    while api.children().await != vec![("veilleur".to_string(), id.clone())] {
        assert!(start.elapsed() < Duration::from_secs(20), "le veilleur ne revient pas une fois le droit rendu");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // arrêté pour de bon, il ne revient plus
    let stopped = api.http.post(format!("{}/api/agents/{id}/stop", api.base)).bearer_auth(TOKEN).json(&json!({})).send().await.unwrap();
    assert_eq!(stopped.status(), 200);
    root.kill().await.unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert!(api.children().await.is_empty());
    let journal = api.get("/api/activity?after=0&limit=5000").await.to_string();
    assert_eq!(journal.matches("agent permanent veilleur relancé").count(), 2, "relancé deux fois, pas trois");
    root.kill().await.unwrap();
}
