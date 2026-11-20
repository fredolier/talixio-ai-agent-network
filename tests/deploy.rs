//! Déploiement sur une autre machine : un vrai maître, un faux `ssh` qui exécute sur place ce
//! qu'on lui demande d'exécuter là-bas, et un vrai enfant lancé par ce chemin.

use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};

const TOKEN: &str = "jeton-de-test-0123456789";


/// Le faux modèle du maître et de l'enfant.
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
    let spawn = |name: &str, target: &str, folder: &str| {
        call("spawn_agent", json!({
            "name": name, "role": "lecteur", "instructions": "Lis ce qu'on te demande.", "target": target,
            "objective": format!("Lis le fichier {file} et rapporte son contenu."),
            "success_criteria": "Le contenu exact du fichier figure dans le compte rendu.",
            "capabilities": { "fs_read": [folder] }
        }))
    };
    let folder = std::path::Path::new(file.as_str()).parent().unwrap().to_str().unwrap().to_string();
    let message = match (is_root, last["role"].as_str().unwrap()) {
        (true, "user") if text.contains("source=\"enfant\"") && text.contains("SECRET-42") => say("Le lecteur dit : SECRET-42"),
        (true, "user") if text.contains("déploie") => spawn("lecteur", "vm", &folder),
        (true, "user") if text.contains("escalade") => spawn("pirate", "vm", "/"),
        (true, "user") if text.contains("ailleurs") => spawn("pirate", "autre", &folder),
        (true, "user") if text.contains("arrête") => call("stop_agent", json!({ "agent": "lecteur" })),
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
#[tokio::test(flavor = "multi_thread")]
async fn deploiement_par_ssh() {
    use std::os::unix::fs::PermissionsExt;
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path().canonicalize().unwrap();
    let data = tmp.join("data");
    let remote = tmp.join("distant");
    std::fs::create_dir_all(&data).unwrap();
    let file = data.join("secret.txt");
    std::fs::write(&file, "SECRET-42").unwrap();

    // le faux ssh : il note ses arguments, puis exécute sur place la commande destinée à la machine distante
    let ssh = tmp.join("ssh");
    let calls = tmp.join("ssh-calls");
    std::fs::write(&ssh, format!("#!/bin/sh\necho \"$@\" >> {calls:?}\nfor last; do :; done\nexec /bin/sh -c \"$last\"\n")).unwrap();
    std::fs::set_permissions(&ssh, std::fs::Permissions::from_mode(0o755)).unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_port = listener.local_addr().unwrap().port();
    let app = Router::new()
        .route("/v1/chat/completions", post(fake_model))
        .with_state(Arc::new(file.to_str().unwrap().to_string()));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let port = free_port();
    let config = tmp.join("talixio.toml");
    std::fs::write(
        &config,
        format!(
            r#"
model = "main"
listen = "0.0.0.0:{port}"
advertise = "127.0.0.1"
state_dir = {state:?}

[models.main]
provider = "openai"
base_url = "http://127.0.0.1:{model_port}/v1"
model = "faux"

[capabilities.spawn]
max_children = 2
max_depth = 1
targets = ["vm"]

[targets.vm]
kind = "ssh"
host = "vm.exemple"
user = "agent"
remote_dir = {remote:?}
program = {ssh:?}

[targets.vm.capabilities]
fs_read = [{data:?}]

[targets.autre]
kind = "docker"
image = "debian"
"#,
            state = tmp.join("state"),
        ),
    )
    .unwrap();
    let _root = tokio::process::Command::new(env!("CARGO_BIN_EXE_talixio-agent"))
        .args(["root", "--config", config.to_str().unwrap()])
        .env("TALIXIO_API_TOKEN", TOKEN)
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();

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

    // le maître n'a lui-même aucun droit de lecture : c'est le plafond de la cible qui borne l'enfant
    api.say("déploie").await;
    api.wait_event("la réponse finale du maître", |e| {
        e["kind"] == "message" && e["agent_name"] == "maître" && e["text"] == "Le lecteur dit : SECRET-42"
    })
    .await;
    let tree = api.get("/api/tree").await;
    let child = &tree["children"][0];
    assert_eq!(child["target"], "vm");
    let id = child["id"].as_str().unwrap().to_string();
    let binary = remote.join(format!("talixio-agent-{id}"));
    assert!(binary.exists(), "l'exécutable devait être déposé sur la cible");
    assert_eq!(std::fs::metadata(&binary).unwrap().permissions().mode() & 0o777, 0o700);

    // ssh : jamais interactif, jamais de confiance à la première vue, et la bonne machine
    let log = std::fs::read_to_string(&calls).unwrap();
    for line in log.lines() {
        assert!(line.contains("BatchMode=yes") && line.contains("StrictHostKeyChecking=yes") && line.contains("agent@vm.exemple"), "{line}");
    }
    // le secret du lien ne passe jamais par la ligne de commande
    assert!(!log.contains("token"));

    // au-delà du plafond de la cible : refusé
    api.say("escalade").await;
    api.wait_event("le refus hors plafond", |e| {
        e["kind"] == "tool_error" && e["text"].as_str().unwrap().contains("hors de vos propres droits")
    })
    .await;
    // une cible définie mais non accordée au maître : refusée
    api.say("ailleurs").await;
    api.wait_event("le refus de la cible", |e| {
        e["kind"] == "tool_error" && e["text"].as_str().unwrap().contains("cible « autre »")
    })
    .await;
    assert_eq!(api.get("/api/tree").await["children"].as_array().unwrap().len(), 1);

    // arrêté, l'agent est désinstallé de la cible
    api.say("arrête").await;
    let start = Instant::now();
    while binary.exists() {
        assert!(start.elapsed() < Duration::from_secs(30), "l'exécutable n'a pas été retiré de la cible");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(!remote.join(format!("state/agents/{id}.db")).exists());
}
