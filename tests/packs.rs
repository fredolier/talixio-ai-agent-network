//! Les paquets : installés par l'exploitant, pris par le maître à son démarrage, et vérifiés.
//! Un agent est créé par son rôle, lance la commande du paquet ; le script modifié est refusé,
//! et le maître ne redémarre pas sur un paquet altéré.
#![cfg(unix)]

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};

const TOKEN: &str = "jeton-de-test-0123456789";

/// Le faux modèle : le maître crée un agent par son rôle, sans consignes ni commandes ;
/// l'agent, s'il a bien reçu les consignes du rôle, lance la commande du paquet.
async fn fake_model(State(program): State<String>, Json(body): Json<Value>) -> Json<Value> {
    let messages = body["messages"].as_array().unwrap();
    let system = messages[0]["content"].as_str().unwrap();
    let is_root = system.contains("agent maître");
    let last = messages.last().unwrap();
    let text = last["content"].as_str().unwrap_or("");
    let call = |name: &str, args: Value| {
        json!({ "role": "assistant", "content": null, "tool_calls": [
            { "id": "call_1", "type": "function", "function": { "name": name, "arguments": args.to_string() } }
        ] })
    };
    let say = |text: &str| json!({ "role": "assistant", "content": text });
    let finish = |summary: String| call("finish_mission", json!({ "outcome": "achieved", "summary": summary, "evidence": "sortie lue" }));
    let message = match (is_root, last["role"].as_str().unwrap()) {
        (true, "user") if text.contains("salue") => {
            assert!(system.contains("# Rôles installés") && system.contains("- bonjour/salueur : Dit bonjour."), "{system}");
            let name = if text.contains("encore") { "second" } else { "premier" };
            call("spawn_agent", json!({ "name": name, "role": "bonjour/salueur", "objective": "Dis bonjour.", "success_criteria": "Le bonjour est rendu." }))
        }
        (true, _) => say("C'est noté."),
        (false, "user") => {
            assert!(system.contains("CONSIGNE-DU-ROLE"), "les consignes du rôle manquent : {system}");
            call("run_command", json!({ "program": program, "args": ["monde"] }))
        }
        (false, _) => finish(format!("Rendu : {}", text.chars().take(300).collect::<String>())),
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
}

fn agent(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_talixio-agent")).args(args).output().unwrap()
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
async fn un_paquet_installe_sert_et_reste_verifie() {
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path().canonicalize().unwrap();
    let state = tmp.join("state");
    let packs = tmp.join("packs");

    // un paquet source : un script et un rôle
    let source = tmp.join("source/bonjour");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("dire"), "#!/bin/sh\necho \"bonjour $1\"\n").unwrap();
    std::fs::write(
        source.join("pack.toml"),
        "name = 'bonjour'\nversion = '1'\ndescription = 'Dire bonjour.'\n[[command]]\nprogram = 'dire'\nargs = ['*']\n\
         [[role]]\nname = 'salueur'\ndescription = 'Dit bonjour.'\ninstructions = 'CONSIGNE-DU-ROLE : lance ta commande.'\ncommands = ['dire']\n",
    )
    .unwrap();
    let from = tmp.join("source");
    let listed = agent(&["pack", "list", "--from", from.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&listed.stdout).contains("bonjour  (version 1)"));
    let installed = agent(&["pack", "install", "bonjour", "--from", from.to_str().unwrap(), "--dir", packs.to_str().unwrap()]);
    assert!(installed.status.success(), "{}", String::from_utf8_lossy(&installed.stderr));
    let program = packs.join("bonjour/dire");

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_port = listener.local_addr().unwrap().port();
    let app = Router::new().route("/v1/chat/completions", post(fake_model)).with_state(program.to_str().unwrap().to_string());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let config = tmp.join("talixio.toml");
    let text = |unprotected: bool| {
        format!(
            "model = \"main\"\nlisten = \"127.0.0.1:{port}\"\nstate_dir = {state:?}\npacks_dir = {packs:?}\npacks_unprotected = {unprotected}\n\
             [models.main]\nprovider = \"openai\"\nbase_url = \"http://127.0.0.1:{model_port}/v1\"\nmodel = \"faux\"\n\
             [capabilities.spawn]\nmax_children = 4\nmax_depth = 1\n"
        )
    };

    // un dossier de paquets que le maître peut écrire : il refuse de démarrer
    std::fs::write(&config, text(false)).unwrap();
    let refused = agent(&["root", "--config", config.to_str().unwrap()]);
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("ne doit pouvoir modifier"), "{}", String::from_utf8_lossy(&refused.stderr));

    // le paquet fixé par son identifiant : un autre identifiant, ou un paquet non listé, et le maître ne démarre pas
    let verify = String::from_utf8_lossy(&agent(&["pack", "verify", "--dir", packs.to_str().unwrap()]).stdout).to_string();
    let pin = verify.lines().find(|l| l.starts_with("bonjour = \"")).expect("identifiant affiché").to_string();
    for (wrong, why) in [("bonjour = \"00\"".to_string(), "n'est pas celui"), ("autre = \"00\"".to_string(), "pas installé")] {
        std::fs::write(&config, format!("{}[packs]\n{wrong}\n", text(true))).unwrap();
        let refused = agent(&["root", "--config", config.to_str().unwrap()]);
        assert!(String::from_utf8_lossy(&refused.stderr).contains(why), "{}", String::from_utf8_lossy(&refused.stderr));
    }

    // accepté pour l'essai : l'agent créé par son rôle reçoit consignes et commande, et la lance
    std::fs::write(&config, format!("{}[packs]\n{pin}\n", text(true))).unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    api.say("salue").await;
    let report = api.wait_event("le bonjour", |e| e["kind"] == "report" && e["text"].as_str().is_some_and(|t| t.contains("Rendu :"))).await;
    assert!(report["text"].as_str().unwrap().contains("bonjour monde"), "{report}");
    // la règle de l'enfant porte l'empreinte du programme, sans que le maître l'ait recopiée
    let tree = api.get("/api/tree").await;
    let rule = &tree["children"][0]["capabilities"]["exec"][0];
    assert_eq!(rule["program"], program.to_str().unwrap());
    assert_eq!(rule["sha256"].as_str().unwrap().len(), 64, "{rule}");

    // le script est modifié sous le maître en marche : la commande est refusée
    std::fs::write(&program, "#!/bin/sh\necho pirate\n").unwrap();
    api.say("salue encore").await;
    let report = api.wait_event("le refus", |e| e["kind"] == "report" && e["text"].as_str().is_some_and(|t| t.contains("n'est plus le fichier installé"))).await;
    assert!(!report["text"].as_str().unwrap().contains("pirate"), "{report}");
    root.kill().await.unwrap();

    // et le maître ne redémarre pas sur un paquet altéré ; `pack verify` dit pourquoi
    let refused = agent(&["root", "--config", config.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("a changé depuis l'installation"), "{}", String::from_utf8_lossy(&refused.stderr));
    let verify = agent(&["pack", "verify", "--dir", packs.to_str().unwrap()]);
    assert!(!verify.status.success());

    // une empreinte ne s'écrit pas à la main dans la configuration
    std::fs::write(&config, format!("{}[[capabilities.exec]]\nprogram = \"/bin/ls\"\nargs = []\nsha256 = \"00\"\n", text(true))).unwrap();
    let refused = agent(&["root", "--config", config.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("`sha256` ne s'écrit pas"), "{}", String::from_utf8_lossy(&refused.stderr));
}
