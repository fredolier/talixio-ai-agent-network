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
    // le rôle installé est montré à l'exploitant avec son paquet et l'identifiant de celui-ci,
    // et le journal dit de quel paquet l'agent tient son rôle
    let roles = api.get("/api/roles").await;
    let shown = &roles["installed"][0];
    assert_eq!(shown["name"], "bonjour/salueur", "{roles}");
    assert_eq!(shown["pack"], "bonjour");
    let id = shown["pack_id"].as_str().unwrap();
    assert!(id.len() == 64 && pin.contains(id), "{shown}");
    assert_eq!(shown["exec"][0]["sha256"], rule["sha256"]);
    api.wait_event("le paquet au journal", |e| e["kind"] == "spawn" && e["text"].as_str().is_some_and(|t| t.contains("rôle installé « bonjour/salueur », paquet bonjour") && t.contains(id))).await;

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

/// Le dépôt signé, de bout en bout par la ligne de commande : clé, publication, lecture,
/// installation ; rien sans la clé, rien d'un fichier changé chez l'hébergeur.
#[test]
fn un_depot_signe_par_la_ligne_de_commande() {
    let tmp = tempfile::tempdir().unwrap();
    let path = |name: &str| tmp.path().join(name).to_str().unwrap().to_string();
    let text = |out: &std::process::Output| format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    std::fs::create_dir_all(tmp.path().join("src/bonjour")).unwrap();
    std::fs::write(tmp.path().join("src/bonjour/pack.toml"), "name = \"bonjour\"\nversion = \"2\"\ndescription = \"Dit bonjour.\"\n\n[[command]]\nprogram = \"salut\"\nargs = [\"*\"]\n").unwrap();
    std::fs::write(tmp.path().join("src/bonjour/salut"), "#!/bin/sh\necho bonjour \"$1\"\n").unwrap();

    let made = agent(&["pack", "keygen", "--out", &path("cle")]);
    assert!(made.status.success(), "{}", text(&made));
    let public = String::from_utf8_lossy(&made.stdout).lines().last().unwrap().to_string();
    assert!(!agent(&["pack", "keygen", "--out", &path("cle")]).status.success(), "une clé n'en écrase pas une autre");
    let published = agent(&["pack", "publish", "--from", &path("src"), "--out", &path("depot"), "--key-file", &path("cle")]);
    assert!(published.status.success() && text(&published).contains(&public), "{}", text(&published));

    // sans clé : rien ; avec : la liste, puis l'installation
    let refused = agent(&["pack", "list", "--depot", &path("depot")]);
    assert!(!refused.status.success() && text(&refused).contains("aucune clé de confiance"), "{}", text(&refused));
    let listed = agent(&["pack", "list", "--depot", &path("depot"), "--key", &public]);
    assert!(listed.status.success() && text(&listed).contains("bonjour  (version 2)") && text(&listed).contains("signé par la clé"), "{}", text(&listed));
    let installed = agent(&["pack", "install", "bonjour", "--depot", &path("depot"), "--key", &public, "--dir", &path("packs")]);
    assert!(installed.status.success(), "{}", text(&installed));
    assert!(agent(&["pack", "verify", "--dir", &path("packs")]).status.success());
    assert!(!agent(&["pack", "list", "--depot", "http://exemple.fr/p", "--key", &public]).status.success());
    // une clé avec un dossier ordinaire : refus, jamais un repli vers le non signé ; et un dépôt
    // dont l'index a disparu n'est pas lu comme un dossier ordinaire
    let mixed = agent(&["pack", "install", "bonjour", "--from", &path("src"), "--key", &public, "--dir", &path("packs2")]);
    assert!(!mixed.status.success() && !tmp.path().join("packs2").exists(), "{}", text(&mixed));
    let plain = agent(&["pack", "list", "--from", &path("src")]);
    assert!(plain.status.success() && text(&plain).contains("NON signée"), "{}", text(&plain));
    std::fs::create_dir_all(tmp.path().join("sans-index/bonjour")).unwrap();
    std::fs::copy(tmp.path().join("src/bonjour/pack.toml"), tmp.path().join("sans-index/bonjour/pack.toml")).unwrap();
    std::fs::copy(tmp.path().join("src/bonjour/salut"), tmp.path().join("sans-index/bonjour/salut")).unwrap();
    assert!(!agent(&["pack", "install", "bonjour", "--depot", &path("sans-index"), "--key", &public, "--dir", &path("packs2")]).status.success());

    // un ancien index, toujours bien signé, ne remplace pas un paquet plus récent du même dépôt
    std::fs::write(tmp.path().join("src/bonjour/pack.toml"), "name = \"bonjour\"\nversion = \"3\"\ndescription = \"Dit bonjour.\"\n\n[[command]]\nprogram = \"salut\"\nargs = [\"*\"]\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(1100));
    assert!(agent(&["pack", "publish", "--from", &path("src"), "--out", &path("depot3"), "--key-file", &path("cle")]).status.success());
    assert!(agent(&["pack", "install", "bonjour", "--replace", "--depot", &path("depot3"), "--key", &public, "--dir", &path("packs")]).status.success());
    let back = agent(&["pack", "install", "bonjour", "--replace", "--depot", &path("depot"), "--key", &public, "--dir", &path("packs")]);
    assert!(!back.status.success() && text(&back).contains("plus ancien"), "{}", text(&back));
    assert!(agent(&["pack", "install", "bonjour", "--replace", "--allow-older", "--depot", &path("depot"), "--key", &public, "--dir", &path("packs")]).status.success());
    // une clé de signature rangée dans un paquet n'est pas publiée
    std::fs::copy(tmp.path().join("cle"), tmp.path().join("src/bonjour/cle")).unwrap();
    let leak = agent(&["pack", "publish", "--from", &path("src"), "--out", &path("depot4"), "--key-file", &path("cle")]);
    assert!(!leak.status.success() && text(&leak).contains("clé de signature"), "{}", text(&leak));
    std::fs::remove_file(tmp.path().join("src/bonjour/cle")).unwrap();

    // l'hébergeur change un fichier : refus, et rien n'est installé à la place
    std::fs::write(tmp.path().join("depot/bonjour/2/salut"), "#!/bin/sh\necho pirate! \"$1\"\n").unwrap();
    let tampered = agent(&["pack", "install", "bonjour", "--replace", "--depot", &path("depot"), "--key", &public, "--dir", &path("packs")]);
    assert!(!tampered.status.success() && text(&tampered).contains("n'est pas ce que l'index signé annonce"), "{}", text(&tampered));
    assert!(std::fs::read_to_string(tmp.path().join("packs/bonjour/salut")).unwrap().contains("bonjour"));
    // un manifeste qui tente de commander le terminal : l'erreur est nettoyée
    std::fs::create_dir_all(tmp.path().join("piege/x")).unwrap();
    std::fs::write(tmp.path().join("piege/x/pack.toml"), "name = \"x\"\n\u{1b}]0;TITRE\u{7}\u{1b}[31m = \n").unwrap();
    let trap = agent(&["pack", "list", "--from", &path("piege")]);
    assert!(!trap.status.success() && !trap.stderr.contains(&0x1b) && !trap.stderr.contains(&7), "{}", text(&trap));
}
