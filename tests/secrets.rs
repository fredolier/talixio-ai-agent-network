//! Secrets for commands, end to end with real processes: master → chief → holder.
//! The operator adds a secret on the master and ties it to one command and one role. The
//! holder's command receives it, sealed across the chief that relays it; the model reads the
//! output without it. Another role, a disabled secret, a hand-written rule: nothing.
#![cfg(unix)]

use std::path::Path;
use std::process::Stdio;
use std::time::{Duration, Instant};

use axum::{extract::State, routing::post, Json, Router};
use serde_json::{json, Value};

const TOKEN: &str = "jeton-de-test-0123456789";
const VALUE: &str = "s3cr3t-value-0123456789";

/// The fake model: the master creates a chief (or, for "direct", an agent of the other role),
/// the chief creates a holder, and whoever holds the command runs it and reports its output.
async fn fake_model(State(program): State<String>, Json(body): Json<Value>) -> Json<Value> {
    let messages = body["messages"].as_array().unwrap();
    let system = messages[0]["content"].as_str().unwrap();
    // no instruction and no tool description ever carries the value
    assert!(!body.to_string().contains(VALUE), "the model was sent the secret");
    let who = if system.contains("agent maître") {
        "master"
    } else if system.contains("CHIEF-ROLE") {
        "chief"
    } else {
        "runner"
    };
    let last = messages.last().unwrap();
    let role = last["role"].as_str().unwrap();
    let text = last["content"].as_str().unwrap_or("");
    let call = |name: &str, args: Value| {
        json!({ "role": "assistant", "content": null, "tool_calls": [
            { "id": "call_1", "type": "function", "function": { "name": name, "arguments": args.to_string() } }
        ] })
    };
    let say = |text: &str| json!({ "role": "assistant", "content": text });
    let finish = |summary: String| call("finish_mission", json!({ "outcome": "achieved", "summary": summary, "evidence": "output read" }));
    let spawn = |name: &str, role: &str, extra: Value| {
        let mut args = json!({ "name": name, "role": role, "objective": "Run your command.", "success_criteria": "Its output is reported." });
        args.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        call("spawn_agent", args)
    };
    let user = text.contains("source=\"utilisateur\"");
    let message = match (who, role) {
        ("master", "user") if user && text.contains("direct") => spawn("direct", "demo/other", json!({})),
        // a role name of a pack that is not installed, and a secret asked for by hand: both refused or ignored
        ("master", "user") if user && text.contains("forged") => spawn("forged", "demo/ghost", json!({ "instructions": "x" })),
        ("master", "user") if user && text.contains("byhand") => spawn(
            "byhand",
            "helper",
            json!({ "instructions": "RUN-ROLE", "capabilities": { "exec": [{ "program": program, "args": [], "secrets": ["DEMO_TOKEN"] }] } }),
        ),
        ("master", "user") if user && text.contains("chief") => {
            let name = text.split_whitespace().find(|w| w.starts_with("chief")).unwrap().to_string();
            spawn(&name, "demo/chief", json!({ "capabilities": { "spawn": { "max_children": 1, "max_depth": 1 } } }))
        }
        ("master", _) => say("Noted."),
        ("chief", "user") if text.contains("Mission") => spawn("holder", "demo/holder", json!({})),
        ("chief", "user") if text.contains("Output:") => finish(format!("Chief: {}", text.chars().filter(|c| !c.is_control()).take(600).collect::<String>())),
        ("chief", _) => say("waiting"),
        ("runner", "user") => call("run_command", json!({ "program": program, "args": [] })),
        ("runner", _) => finish(format!("Output: {}", text.chars().take(300).collect::<String>())),
        _ => say("waiting"),
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

    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let response = self.http.post(format!("{}{path}", self.base)).bearer_auth(TOKEN).json(&body).send().await.unwrap();
        (response.status().as_u16(), response.json().await.unwrap_or(Value::Null))
    }

    async fn say(&self, text: &str) {
        assert_eq!(self.post("/api/messages", json!({ "text": text })).await.0, 202);
    }

    async fn journal(&self) -> String {
        self.get("/api/activity?after=0&limit=5000").await.to_string()
    }

    async fn wait_event(&self, what: &str, matches: impl Fn(&Value) -> bool) -> Value {
        let start = Instant::now();
        loop {
            let body = self.get("/api/activity?after=0&limit=5000").await;
            if let Some(event) = body["events"].as_array().unwrap().iter().find(|e| matches(e)) {
                return event.clone();
            }
            assert!(start.elapsed() < Duration::from_secs(40), "never seen: {what}\njournal: {body:#}");
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }
}

fn agent(args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_talixio-agent")).args(args).output().unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_secret_reaches_one_command_of_one_role_and_no_model() {
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path().canonicalize().unwrap();
    let state = tmp.join("state");
    let packs = tmp.join("packs");

    // a pack: one command that accepts a secret and prints what it received, three roles
    let source = tmp.join("source/demo");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("show"), "#!/bin/sh\necho \"token=$DEMO_TOKEN url=$DEMO_URL len=${#DEMO_TOKEN} path=${PATH:+set}\"\necho \"again $DEMO_TOKEN\" >&2\n").unwrap();
    std::fs::write(
        source.join("pack.toml"),
        "name = 'demo'\nversion = '1'\ndescription = 'A command with a secret.'\n\
         [[command]]\nprogram = 'show'\nargs = []\nsecrets = ['DEMO_TOKEN']\nenv = ['DEMO_URL']\n\
         [[role]]\nname = 'chief'\ndescription = 'Creates the holder.'\ninstructions = 'CHIEF-ROLE'\ncommands = ['show']\n\
         [[role]]\nname = 'holder'\ndescription = 'Runs the command.'\ninstructions = 'RUN-ROLE'\ncommands = ['show']\n\
         [[role]]\nname = 'other'\ndescription = 'Runs it too.'\ninstructions = 'RUN-ROLE'\ncommands = ['show']\n",
    )
    .unwrap();
    let from = tmp.join("source");
    let installed = agent(&["pack", "install", "demo", "--from", from.to_str().unwrap(), "--dir", packs.to_str().unwrap()]);
    assert!(installed.status.success(), "{}", String::from_utf8_lossy(&installed.stderr));
    let shown = agent(&["pack", "show", "demo", "--from", from.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&shown.stdout).contains("accepts a secret in: DEMO_TOKEN ; with: DEMO_URL"));
    let program = packs.join("demo/show");

    // a pack cannot ask for a variable that means something to the system
    for bad in ["LD_PRELOAD", "PATH", "GIT_SSH_COMMAND", "demo_token", "OTHER_TOKEN"] {
        let dir = tmp.join(format!("bad/{bad}/demo"));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("show"), "#!/bin/sh\n").unwrap();
        std::fs::write(dir.join("pack.toml"), format!("name = 'demo'\nversion = '1'\ndescription = 'x'\n[[command]]\nprogram = 'show'\nargs = []\nsecrets = ['{bad}']\n")).unwrap();
        let refused = agent(&["pack", "show", "demo", "--from", dir.parent().unwrap().to_str().unwrap()]);
        assert!(!refused.status.success() && String::from_utf8_lossy(&refused.stderr).contains("refused"), "{bad}: {}", String::from_utf8_lossy(&refused.stderr));
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_port = listener.local_addr().unwrap().port();
    let app = Router::new().route("/v1/chat/completions", post(fake_model)).with_state(program.to_str().unwrap().to_string());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let config = tmp.join("talixio.toml");
    let text = format!(
        "model = \"main\"\nlisten = \"127.0.0.1:{port}\"\nstate_dir = {state:?}\npacks_dir = {packs:?}\npacks_unprotected = true\n\
         [models.main]\nprovider = \"openai\"\nbase_url = \"http://127.0.0.1:{model_port}/v1\"\nmodel = \"fake\"\n\
         [capabilities.spawn]\nmax_children = 8\nmax_depth = 2\n"
    );

    // the right to a secret is not written by hand in the configuration
    std::fs::write(&config, format!("{text}[[capabilities.exec]]\nprogram = \"/bin/ls\"\nargs = []\nsecrets = [\"DEMO_TOKEN\"]\n")).unwrap();
    let refused = agent(&["root", "--config", config.to_str().unwrap()]);
    assert!(String::from_utf8_lossy(&refused.stderr).contains("`secrets` is not written"), "{}", String::from_utf8_lossy(&refused.stderr));

    std::fs::write(&config, &text).unwrap();
    let mut root = tokio::process::Command::new(env!("CARGO_BIN_EXE_talixio-agent"))
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
        assert!(start.elapsed() < Duration::from_secs(15), "the master does not start");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let pinned = talixio_agent::tls::pinned_client(&fingerprint).unwrap();
    let api = Api { http: reqwest::Client::builder().use_preconfigured_tls(pinned).build().unwrap(), base: format!("https://127.0.0.1:{port}") };
    while api.http.get(format!("{}/api/health", api.base)).send().await.is_err() {
        assert!(start.elapsed() < Duration::from_secs(15), "the master does not answer");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // the vault needs the token of the interface, and shows the commands that accept a secret
    let bare = api.http.get(format!("{}/api/secrets", api.base)).send().await.unwrap();
    assert_eq!(bare.status(), 401);
    let listed = api.get("/api/secrets").await;
    assert_eq!(listed["commands"][0]["name"], "demo/show", "{listed}");
    assert_eq!(listed["commands"][0]["roles"], json!(["demo/chief", "demo/holder", "demo/other"]));

    // the operator adds the secret for the holder role only; no answer carries the value
    let body = json!({ "name": "tok", "value": VALUE, "command": "demo/show", "variable": "DEMO_TOKEN", "roles": ["demo/holder"], "env": { "DEMO_URL": "https://example.org/" } });
    let (status, view) = api.post("/api/secrets", body.clone()).await;
    assert_eq!(status, 200, "{view}");
    assert!(!view.to_string().contains(VALUE) && !api.get("/api/secrets").await.to_string().contains(VALUE));
    assert_eq!(api.post("/api/secrets", body).await.0, 400, "same name twice");
    let (status, refused) = api.post("/api/secrets", json!({ "name": "bad", "value": VALUE, "command": "demo/show", "variable": "PATH", "roles": ["demo/holder"] })).await;
    assert_eq!(status, 400, "{refused}");

    // master → chief → holder: the holder's command receives it across the chief, and the
    // model reads an output where the value is gone, on both streams
    api.say("go chief1").await;
    let report = api.wait_event("the chief's report", |e| e["kind"] == "report" && e["agent_name"] == "chief1" && e["text"].as_str().is_some_and(|t| t.contains("Chief:"))).await;
    let text = report["text"].as_str().unwrap();
    assert!(text.contains("token=[secret DEMO_TOKEN] url=https://example.org/ len=23 path=set"), "{text}");
    assert!(text.contains("again [secret DEMO_TOKEN]"), "{text}");
    api.wait_event("the journal entry", |e| e["kind"] == "secret" && e["text"].as_str().is_some_and(|t| t.contains("secret tok given to demo/show of a-") && t.contains("(demo/holder)"))).await;

    // another role with the same command: it runs without
    api.say("go direct").await;
    let report = api.wait_event("the other role", |e| e["kind"] == "report" && e["agent_name"] == "direct" && e["text"].as_str().is_some_and(|t| t.contains("Output:"))).await;
    assert!(report["text"].as_str().unwrap().contains("token= url= len=0"), "{report}");

    // a role name of a pack that does not exist; a rule that asks for the secret by hand
    api.say("go forged").await;
    api.wait_event("the forged role", |e| e["text"].as_str().is_some_and(|t| t.contains("not an installed role"))).await;
    api.say("go byhand").await;
    let report = api.wait_event("the hand-written rule", |e| e["kind"] == "report" && e["agent_name"] == "byhand" && e["text"].as_str().is_some_and(|t| t.contains("Output:"))).await;
    assert!(report["text"].as_str().unwrap().contains("token= url= len=0"), "{report}");
    let tree = api.get("/api/tree").await;
    let byhand = tree["children"].as_array().unwrap().iter().find(|c| c["name"] == "byhand").unwrap();
    assert!(byhand["capabilities"]["exec"][0].get("secrets").is_none(), "{byhand}");

    // disabled: at the next run, nothing
    let (status, _) = api.post("/api/secrets/tok", json!({ "enabled": false })).await;
    assert_eq!(status, 200);
    api.say("go chief2").await;
    let report = api
        .wait_event("the second chief", |e| e["kind"] == "report" && e["agent_name"] == "chief2" && e["text"].as_str().is_some_and(|t| t.contains("Chief:")))
        .await;
    assert!(report["text"].as_str().unwrap().contains("token= url= len=0"), "{report}");
    api.wait_event("the refusal in the journal", |e| e["kind"] == "secret" && e["text"].as_str().is_some_and(|t| t.contains("it is disabled"))).await;

    // nowhere in the journal, nowhere in the logs of the agents, and closed on disk
    assert!(!api.journal().await.contains(VALUE));
    let logs = state.join("logs");
    for entry in std::fs::read_dir(&logs).into_iter().flatten().flatten() {
        assert!(!std::fs::read_to_string(entry.path()).unwrap_or_default().contains(VALUE), "{}", entry.path().display());
    }
    use std::os::unix::fs::PermissionsExt;
    let file = state.join("secrets/tok.json");
    assert_eq!(std::fs::metadata(&file).unwrap().permissions().mode() & 0o077, 0);
    assert!(std::fs::read_to_string(&file).unwrap().contains(VALUE));

    assert_eq!(api.post("/api/secrets/tok/delete", json!({})).await.0, 200);
    assert!(!Path::new(&file).exists());
    root.kill().await.unwrap();
}
