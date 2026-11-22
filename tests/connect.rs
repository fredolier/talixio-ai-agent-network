//! Se connecter à un fournisseur et activer ses modèles depuis l'interface : un vrai maître,
//! son API, et un faux fournisseur que le catalogue de test désigne.

use std::path::Path;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use base64::Engine;
use serde_json::{json, Value};
use sha2::Digest;

const TOKEN: &str = "jeton-de-test-0123456789";
const KEY: &str = "sk-cle-valide-0123456789";
const OAUTH_KEY: &str = "sk-cle-obtenue-par-autorisation";

#[derive(Default)]
struct Fake {
    /// Chemin, clé présentée, modèle demandé.
    seen: Mutex<Vec<(String, String, String)>>,
    /// Le défi annoncé au départ de l'autorisation : l'échange doit en présenter le secret.
    challenge: Mutex<String>,
    key_file: Mutex<String>,
}

async fn fake_provider(State(fake): State<Arc<Fake>>, method: Method, uri: Uri, headers: HeaderMap, body: Bytes) -> Response {
    let path = uri.path().to_string();
    let key = headers.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
    let asked: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    fake.seen.lock().unwrap().push((path.clone(), key.clone(), asked["model"].as_str().unwrap_or("").to_string()));
    let good = key == format!("Bearer {KEY}") || key == format!("Bearer {OAUTH_KEY}");
    match (method.as_str(), path.as_str()) {
        ("GET", "/v1/models") if good => Json(json!({ "data": [] })).into_response(),
        ("GET", "/v1/models") => StatusCode::UNAUTHORIZED.into_response(),
        // un fournisseur ouvert aux anonymes : il refuse toute clé, même fausse
        ("GET", "/ouvert/models") if key.is_empty() => Json(json!({ "data": [] })).into_response(),
        ("GET", "/ouvert/models") => StatusCode::FORBIDDEN.into_response(),
        // un fournisseur qui renvoie ailleurs : la clé ne doit pas suivre
        ("GET", "/ailleurs/models") => (StatusCode::TEMPORARY_REDIRECT, [("location", "/piege")]).into_response(),
        ("POST", "/oauth/keys") => {
            let verifier = asked["code_verifier"].as_str().unwrap_or("");
            let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(verifier.as_bytes()));
            if asked["code"] == "code-bon" && challenge == *fake.challenge.lock().unwrap() && asked["code_challenge_method"] == "S256" {
                Json(json!({ "key": OAUTH_KEY, "user_id": "u1" })).into_response()
            } else {
                StatusCode::FORBIDDEN.into_response()
            }
        }
        ("POST", "/v1/chat/completions") if good => {
            let messages = asked["messages"].as_array().unwrap();
            let is_root = messages[0]["content"].as_str().unwrap().contains("agent maître");
            let last = messages.last().unwrap();
            let text = last["content"].as_str().unwrap_or("");
            let call = |name: &str, args: Value| {
                json!({ "role": "assistant", "content": null, "tool_calls": [
                    { "id": "call_1", "type": "function", "function": { "name": name, "arguments": args.to_string() } }
                ] })
            };
            let say = |text: &str| json!({ "role": "assistant", "content": text });
            let message = match (is_root, last["role"].as_str().unwrap()) {
                (true, "user") if text.contains("délègue") => call("spawn_agent", json!({
                    "name": "second", "role": "essai", "instructions": "Réponds.", "model": "grand-1",
                    "objective": "Dis bonjour.", "success_criteria": "Bonjour est dit.", "capabilities": {}
                })),
                (true, "user") if text.contains("lis la clé") => call("read_file", json!({ "path": fake.key_file.lock().unwrap().as_str() })),
                (true, _) => say("C'est noté."),
                (false, "user") => call("finish_mission", json!({ "outcome": "achieved", "summary": "Bonjour.", "evidence": "dit" })),
                (false, _) => say("Fini."),
            };
            Json(json!({ "choices": [{ "message": message }], "usage": { "prompt_tokens": 10, "completion_tokens": 5 } })).into_response()
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    async fn get(&self, path: &str) -> Value {
        self.http.get(format!("{}{path}", self.base)).bearer_auth(TOKEN).send().await.unwrap().json().await.unwrap()
    }

    /// Le statut et le corps d'une demande de changement.
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self.http.post(format!("{}{path}", self.base)).bearer_auth(TOKEN).json(&body).send().await.unwrap();
        (r.status().as_u16(), r.json().await.unwrap_or(Value::Null))
    }

    async fn ok(&self, path: &str, body: Value) -> Value {
        let (status, body) = self.post(path, body).await;
        assert_eq!(status, 200, "{path} : {body}");
        body
    }

    /// Une demande refusée : la raison donnée.
    async fn refused(&self, path: &str, body: Value) -> String {
        let (status, body) = self.post(path, body).await;
        assert_eq!(status, 400, "{path} : {body}");
        body["error"].as_str().unwrap().to_string()
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

fn provider<'a>(models: &'a Value, id: &str) -> &'a Value {
    models["providers"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("fournisseur absent : {id}"))
}

#[tokio::test(flavor = "multi_thread")]
async fn connect_and_activate_from_the_interface() {
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path().canonicalize().unwrap();
    let state = tmp.join("state");
    let key_file = state.join("keys").join("faux.key");

    let fake = Arc::new(Fake::default());
    *fake.key_file.lock().unwrap() = key_file.to_str().unwrap().to_string();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fp = listener.local_addr().unwrap().port();
    let app = Router::new().fallback(fake_provider).with_state(fake.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    // le catalogue de test : c'est lui, et lui seul, qui dit où vont les clés
    let entry = |id: &str, endpoint: &str, extra: &str| {
        // un modèle configuré se rattache à son fournisseur par sa machine : celle du faux fournisseur est à « faux »
        let host = if id == "faux" { "127.0.0.1".to_string() } else { format!("hote-{id}.exemple.fr") };
        format!(
            "[[provider]]\nid = \"{id}\"\nname = \"Fournisseur {id}\"\ncountry = \"France\"\neurope = true\ntrust = \"4\"\nkind = \"Essai\"\n\
             billing = \"usage\"\napis = [\"openai\"]\nendpoint = \"{endpoint}\"\nhosts = [\"{host}\"]\n{extra}\n"
        )
    };
    let catalog = tmp.join("catalogue.toml");
    std::fs::write(
        &catalog,
        format!(
            "{}{}{}{}{}\
             [[model]]\nname = \"Grand\"\nid = \"grand-1\"\nprovider = \"faux\"\nnature = \"LLM\"\nrank = 7\n\
             [[model]]\nname = \"Sens\"\nid = \"sens-1\"\nprovider = \"faux\"\nnature = \"Embedding\"\nrank = 5\n\
             [[model]]\nname = \"Peintre\"\nid = \"peint-1\"\nprovider = \"faux\"\nnature = \"Image\"\nrank = 5\n",
            entry("faux", &format!("http://127.0.0.1:{fp}/v1"), "keys_url = \"https://exemple.fr/cles\""),
            entry("ailleurs", &format!("http://127.0.0.1:{fp}/ailleurs"), ""),
            entry("muet", &format!("http://127.0.0.1:{fp}/muet"), ""),
            entry("compte", "", ""),
            entry(
                "porte",
                &format!("http://127.0.0.1:{fp}/v1"),
                &format!("oauth_authorize = \"https://autorisation.exemple.fr/auth\"\noauth_exchange = \"http://127.0.0.1:{fp}/oauth/keys\""),
            ),
        ),
    )
    .unwrap();

    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    std::env::set_var("TALIXIO_TEST_CLE_CONNEXION", KEY);
    let config = tmp.join("talixio.toml");
    std::fs::write(
        &config,
        format!(
            "model = \"main\"\nlisten = \"127.0.0.1:{port}\"\nstate_dir = {state:?}\ncatalog_file = {catalog:?}\n\
             [timing]\nheartbeat_secs = 1\nsuspend_after_secs = 2\ndie_after_secs = 4\n\
             [models.main]\nprovider = \"openai\"\nbase_url = \"http://127.0.0.1:{fp}/v1\"\nmodel = \"principal\"\napi_key_env = \"TALIXIO_TEST_CLE_CONNEXION\"\n\
             [capabilities]\nfs_read = [{tmp:?}]\n[capabilities.max_calls]\nimage = 4\n[capabilities.spawn]\nmax_children = 2\nmax_depth = 1\n"
        ),
    )
    .unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;

    // sans jeton : aucune de ces routes ne répond
    for path in [
        "/api/providers/faux/connect", "/api/providers/faux/disconnect", "/api/providers/porte/oauth",
        "/api/providers/porte/oauth/finish", "/api/models/activate", "/api/models/grand-1/deactivate",
    ] {
        let denied = api.http.post(format!("{}{path}", api.base)).json(&json!({ "key": KEY })).send().await.unwrap();
        assert_eq!(denied.status(), 401, "{path}");
    }

    // ce que la page propose pour chaque fournisseur, sans rien dire de l'échange OAuth
    let models = api.get("/api/models").await;
    assert_eq!(provider(&models, "faux")["connect"], "key");
    assert_eq!(provider(&models, "faux")["keys_url"], "https://exemple.fr/cles");
    assert_eq!(provider(&models, "porte")["connect"], "oauth");
    assert_eq!(provider(&models, "compte")["connect"], "config");
    assert_eq!(provider(&models, "muet")["linked"], false);
    assert!(!models.to_string().contains("oauth/keys"));

    // refus : fournisseur inconnu, adresse dépendante du compte, clé mal formée, clé refusée par le fournisseur
    assert!(api.refused("/api/providers/inconnu/connect", json!({ "key": KEY })).await.contains("inconnu du catalogue"));
    assert!(api.refused("/api/providers/compte/connect", json!({ "key": KEY })).await.contains("configuration du maître"));
    for bad in ["", "court", "avec un espace dedans", "retour\r\nX-Injecte: 1", "clé-accentuée-1234"] {
        assert!(api.refused("/api/providers/faux/connect", json!({ "key": bad })).await.contains("clé invalide"), "{bad}");
    }
    assert!(api.refused("/api/providers/faux/connect", json!({ "key": "sk-mauvaise-cle-000" })).await.contains("refuse cette clé"));
    assert!(!key_file.exists(), "une clé refusée n'est pas gardée");
    assert!(api.refused("/api/models/activate", json!({ "provider": "faux", "model": "grand-1" })).await.contains("connectez d'abord"));

    // la bonne clé : essayée chez le fournisseur, puis rangée dans un fichier fermé
    let done = api.ok("/api/providers/faux/connect", json!({ "key": format!("  {KEY}\n") })).await;
    assert_eq!(done, json!({ "connected": "faux", "verified": true }));
    assert_eq!(std::fs::read_to_string(&key_file).unwrap(), KEY);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&key_file).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(state.join("connections.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let models = api.get("/api/models").await;
    assert_eq!(provider(&models, "faux")["linked"], true);
    assert_eq!(provider(&models, "faux")["connected"], true);

    // un fournisseur qui renvoie ailleurs ou ne liste rien : connecté sans vérification, et la clé n'a pas suivi
    assert_eq!(api.ok("/api/providers/ailleurs/connect", json!({ "key": KEY })).await["verified"], false);
    assert_eq!(api.ok("/api/providers/muet/connect", json!({ "key": KEY })).await["verified"], false);
    assert!(!fake.seen.lock().unwrap().iter().any(|(path, ..)| path == "/piege"));

    // activer : noms tenus, alias uniques
    for bad in ["../admin", "grand 1", "a?b", ""] {
        assert!(api.refused("/api/models/activate", json!({ "provider": "faux", "model": bad })).await.contains("nom de modèle invalide"), "{bad}");
    }
    assert!(api.refused("/api/models/activate", json!({ "provider": "faux", "model": "grand-1", "alias": "Pas_Bon" })).await.contains("alias invalide"));
    assert!(api.refused("/api/models/activate", json!({ "provider": "faux", "model": "grand-1", "alias": "main" })).await.contains("déjà l'alias"));
    let done = api.ok("/api/models/activate", json!({ "provider": "faux", "model": "grand-1" })).await;
    assert_eq!(done["alias"], "grand-1");
    assert_eq!(done["nature"], "llm");
    assert_eq!(api.ok("/api/models/activate", json!({ "provider": "faux", "model": "grand-1" })).await["alias"], "grand-1-2");
    assert_eq!(api.ok("/api/models/grand-1-2/deactivate", json!({})).await["deactivated"], "grand-1-2");
    assert!(api.refused("/api/models/main/deactivate", json!({})).await.contains("pas été activé depuis l'interface"));
    assert!(api.refused("/api/providers/compte/disconnect", json!({})).await.contains("pas été connecté depuis l'interface"));

    // un modèle de service s'active avec son nombre d'appels, borné
    assert!(api.refused("/api/models/activate", json!({ "provider": "faux", "model": "sens-1", "calls": 5_000_000 })).await.contains("au plus"));
    let done = api.ok("/api/models/activate", json!({ "provider": "faux", "model": "sens-1", "alias": "sens", "calls": 3 })).await;
    assert_eq!(done, json!({ "alias": "sens", "nature": "embedding", "default": false, "max_calls": 3 }));

    // un nombre d'appels fixé par la configuration ne se change pas d'ici, ni en plus ni en moins
    let done = api.ok("/api/models/activate", json!({ "provider": "faux", "model": "peint-1", "alias": "peintre", "calls": 1000 })).await;
    assert_eq!(done, json!({ "alias": "peintre", "nature": "image", "default": false, "max_calls": 4 }));
    api.ok("/api/models/peintre/deactivate", json!({})).await;
    api.ok("/api/models/activate", json!({ "provider": "faux", "model": "peint-1", "alias": "peintre", "calls": 0 })).await;
    api.ok("/api/models/peintre/deactivate", json!({})).await;
    // des appels donnés d'ici partent avec le dernier modèle de leur nature
    api.ok("/api/models/activate", json!({ "provider": "faux", "model": "sens-1", "alias": "sens-2", "calls": 9 })).await;
    assert_eq!(api.get("/api/tree").await["capabilities"]["max_calls"], json!({ "embedding": 9, "image": 4 }));
    api.ok("/api/models/sens/deactivate", json!({})).await;
    assert_eq!(api.get("/api/tree").await["capabilities"]["max_calls"], json!({ "embedding": 9, "image": 4 }));
    api.ok("/api/models/sens-2/deactivate", json!({})).await;
    assert_eq!(api.get("/api/tree").await["capabilities"]["max_calls"], json!({ "image": 4 }));
    api.ok("/api/models/activate", json!({ "provider": "faux", "model": "sens-1", "alias": "sens", "calls": 3 })).await;

    // sans redémarrage : le maître détient ces modèles, et peut les accorder
    let tree = api.get("/api/tree").await;
    assert_eq!(tree["capabilities"]["models"], json!(["main", "grand-1", "sens"]));
    assert_eq!(tree["capabilities"]["max_calls"], json!({ "embedding": 3, "image": 4 }));
    let models = api.get("/api/models").await;
    assert_eq!(models["activated"], json!(["grand-1", "sens"]));
    let grand = models["catalog"].as_array().unwrap().iter().find(|m| m["id"] == "grand-1").unwrap();
    assert_eq!(grand["aliases"], json!(["grand-1"]));
    assert!(models["models"].as_array().unwrap().iter().any(|m| m["alias"] == "sens" && m["nature"] == "embedding" && m["has_key"] == true));

    // un agent tourne sur le modèle activé à l'instant : le fournisseur reçoit la clé, du maître seul
    api.http.post(format!("{}/api/messages", api.base)).bearer_auth(TOKEN).json(&json!({ "text": "délègue" })).send().await.unwrap();
    api.wait_event("le compte rendu du second", |e| e["agent_name"] == "second" && e["text"].as_str().is_some_and(|t| t.contains("Bonjour."))).await;
    assert!(fake.seen.lock().unwrap().iter().any(|(path, key, model)| path == "/v1/chat/completions" && model == "grand-1" && key == &format!("Bearer {KEY}")));
    let tree = api.get("/api/tree").await;
    let second = &tree["children"][0];
    assert_eq!(second["model"], "grand-1");

    // tant qu'un agent le détient, le modèle ne se retire pas ; l'agent arrêté, si
    assert!(api.refused("/api/models/grand-1/deactivate", json!({})).await.contains("accordé à second"));
    assert!(api.refused("/api/providers/faux/disconnect", json!({})).await.contains("accordé à second"));
    assert!(key_file.exists());
    api.ok(&format!("/api/agents/{}/stop", second["id"].as_str().unwrap()), json!({})).await;
    let start = Instant::now();
    while api.post("/api/models/grand-1/deactivate", json!({})).await.0 != 200 {
        assert!(start.elapsed() < Duration::from_secs(15), "le modèle reste retenu par un agent arrêté");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(api.get("/api/tree").await["capabilities"]["models"], json!(["main", "sens"]));
    api.ok("/api/models/activate", json!({ "provider": "faux", "model": "grand-1" })).await;

    // la clé est hors d'atteinte des agents, même du maître qui lit tout le dossier
    api.http.post(format!("{}/api/messages", api.base)).bearer_auth(TOKEN).json(&json!({ "text": "lis la clé" })).send().await.unwrap();
    api.wait_event("le refus de lire la clé", |e| e["kind"] == "tool_error" && e["text"].as_str().unwrap().contains("read_file")).await;

    // la clé n'est nulle part dans ce que rend l'API
    for path in ["/api/models", "/api/tree", "/api/activity?after=0&limit=5000", "/api/roles"] {
        let shown = api.get(path).await.to_string();
        // (le journal garde le chemin que l'agent a demandé à lire : c'est sa demande, pas une fuite)
        assert!(!shown.contains(KEY) && (path.starts_with("/api/activity") || !shown.contains("faux.key")), "{path}");
    }

    // au redémarrage, tout est retrouvé : rien à ressaisir
    root.kill().await.unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    let tree = api.get("/api/tree").await;
    assert_eq!(tree["capabilities"]["models"], json!(["main", "grand-1", "sens"]));
    assert_eq!(tree["capabilities"]["max_calls"], json!({ "embedding": 3, "image": 4 }));
    let models = api.get("/api/models").await;
    assert_eq!(provider(&models, "faux")["linked"], true);
    assert_eq!(models["activated"], json!(["grand-1", "sens"]));

    // la configuration reprend un alias activé d'ici : elle gagne, et l'interface ne peut plus le retirer
    root.kill().await.unwrap();
    let mut text = std::fs::read_to_string(&config).unwrap();
    text.push_str(&format!("[models.grand-1]\nprovider = \"openai\"\nbase_url = \"http://127.0.0.1:{fp}/v1\"\nmodel = \"grand-1\"\napi_key_env = \"TALIXIO_TEST_CLE_CONNEXION\"\n"));
    std::fs::write(&config, text).unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    let models = api.get("/api/models").await;
    assert_eq!(models["activated"], json!(["sens"]));
    assert!(models["models"].as_array().unwrap().iter().any(|m| m["alias"] == "grand-1"));
    assert!(api.refused("/api/models/grand-1/deactivate", json!({})).await.contains("pas été activé depuis l'interface"));
    assert_eq!(api.get("/api/tree").await["capabilities"]["models"], json!(["grand-1", "main", "sens"]));

    // l'autorisation dans le navigateur : le retour ne vise que cette page, le code ne sert qu'une fois
    let back = format!("https://127.0.0.1:{port}/");
    assert!(api.refused("/api/providers/faux/oauth", json!({ "callback": back })).await.contains("collez une clé"));
    for bad in ["https://ailleurs.exemple.fr/", &format!("http://127.0.0.1:{port}/"), &format!("https://127.0.0.1:{port}/?x=1")] {
        assert!(api.refused("/api/providers/porte/oauth", json!({ "callback": bad })).await.contains("adresse de retour refusée"), "{bad}");
    }
    assert!(api.refused("/api/providers/porte/oauth/finish", json!({ "code": "code-bon" })).await.contains("aucune autorisation en cours"));
    let begin = || async {
        let url = api.ok("/api/providers/porte/oauth", json!({ "callback": back })).await["url"].as_str().unwrap().to_string();
        let url = reqwest::Url::parse(&url).unwrap();
        assert_eq!(url.host_str(), Some("autorisation.exemple.fr"));
        let pairs: std::collections::BTreeMap<String, String> = url.query_pairs().into_owned().collect();
        assert_eq!(pairs["callback_url"], back);
        assert_eq!(pairs["code_challenge_method"], "S256");
        *fake.challenge.lock().unwrap() = pairs["code_challenge"].clone();
    };
    begin().await;
    assert!(api.refused("/api/providers/porte/oauth/finish", json!({ "code": "code-faux" })).await.contains("a refusé l'autorisation"));
    assert!(api.refused("/api/providers/porte/oauth/finish", json!({ "code": "code-bon" })).await.contains("aucune autorisation en cours"));
    begin().await;
    assert_eq!(api.ok("/api/providers/porte/oauth/finish", json!({ "code": "code-bon" })).await, json!({ "connected": "porte" }));
    assert!(api.refused("/api/providers/porte/oauth/finish", json!({ "code": "code-bon" })).await.contains("aucune autorisation en cours"));
    assert_eq!(std::fs::read_to_string(state.join("keys").join("porte.key")).unwrap(), OAUTH_KEY);
    let shown = format!("{}{}", api.get("/api/models").await, api.get("/api/activity?after=0&limit=5000").await);
    assert!(!shown.contains(OAUTH_KEY) && shown.contains("connecté par autorisation dans le navigateur"));

    // déconnecter : les modèles activés d'ici sont retirés, la clé est effacée
    let done = api.ok("/api/providers/faux/disconnect", json!({})).await;
    assert_eq!(done["models"], json!(["sens"]));
    assert!(!key_file.exists());
    let tree = api.get("/api/tree").await;
    assert_eq!(tree["capabilities"]["models"], json!(["grand-1", "main"]));
    assert_eq!(tree["capabilities"]["max_calls"], json!({ "image": 4 }));
    assert!(api.get("/api/models").await["models"].as_array().unwrap().iter().any(|m| m["alias"] == "grand-1"), "le modèle de la configuration reste");
    let models = api.get("/api/models").await;
    assert_eq!(provider(&models, "faux")["linked"], false);
    assert_eq!(models["activated"], json!([]));
    assert!(api.refused("/api/providers/faux/disconnect", json!({})).await.contains("pas été connecté depuis l'interface"));
    assert!(api.refused("/api/models/activate", json!({ "provider": "faux", "model": "grand-1" })).await.contains("connectez d'abord"));

    root.kill().await.unwrap();
}

/// Le maître démarre sans modèle ni clé, et reçoit son modèle depuis l'interface, comme un enfant
/// reçoit le sien de son parent.
#[tokio::test(flavor = "multi_thread")]
async fn a_master_gets_its_model_from_the_interface() {
    let tmp_guard = tempfile::tempdir().unwrap();
    let tmp = tmp_guard.path().canonicalize().unwrap();
    let state = tmp.join("state");
    let fake = Arc::new(Fake::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let fp = listener.local_addr().unwrap().port();
    let app = Router::new().fallback(fake_provider).with_state(fake.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let catalog = tmp.join("catalogue.toml");
    std::fs::write(
        &catalog,
        format!(
            "[[provider]]\nid = \"faux\"\nname = \"Fournisseur faux\"\ncountry = \"France\"\neurope = true\ntrust = \"4\"\nkind = \"Essai\"\n\
             billing = \"usage\"\napis = [\"openai\"]\nendpoint = \"http://127.0.0.1:{fp}/v1\"\nhosts = [\"127.0.0.1\"]\n\
             [[provider]]\nid = \"ouvert\"\nname = \"Fournisseur ouvert\"\ncountry = \"France\"\neurope = true\ntrust = \"4\"\nkind = \"Essai\"\n\
             billing = \"usage\"\napis = [\"openai\"]\nendpoint = \"http://127.0.0.1:{fp}/ouvert\"\nhosts = [\"ouvert.exemple.fr\"]\nkeyless = true\n\
             [[model]]\nname = \"Grand\"\nid = \"grand-1\"\nprovider = \"faux\"\nnature = \"LLM\"\nrank = 7\n\
             [[model]]\nname = \"Petit\"\nid = \"petit-1\"\nprovider = \"faux\"\nnature = \"LLM\"\nrank = 3\n\
             [[model]]\nname = \"Sens\"\nid = \"sens-1\"\nprovider = \"faux\"\nnature = \"Embedding\"\nrank = 5\n"
        ),
    )
    .unwrap();
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let config = tmp.join("talixio.toml");
    // ni `model`, ni [models] : rien à fournir pour démarrer
    std::fs::write(&config, format!("listen = \"127.0.0.1:{port}\"\nstate_dir = {state:?}\ncatalog_file = {catalog:?}\n")).unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    let say = |text: &'static str| api.http.post(format!("{}/api/messages", api.base)).bearer_auth(TOKEN).json(&json!({ "text": text })).send();

    let models = api.get("/api/models").await;
    assert_eq!(models["default"], "");
    assert_eq!(models["default_fixed"], false);
    // sans modèle, le maître le dit et n'appelle personne
    say("bonjour").await.unwrap();
    api.wait_event("l'absence de modèle", |e| e["kind"] == "error" && e["text"].as_str().unwrap().contains("pas encore de modèle")).await;
    assert!(!fake.seen.lock().unwrap().iter().any(|(path, ..)| path.contains("chat")));

    // sans jeton, rien ; un alias inconnu ou mal formé, rien
    let denied = api.http.post(format!("{}/api/models/grand-1/default", api.base)).send().await.unwrap();
    assert_eq!(denied.status(), 401);
    assert!(api.refused("/api/models/grand-1/default", json!({})).await.contains("modèle activé"));

    // sans clé : seulement chez un fournisseur qui l'annonce, et alors aucune clé, même fausse, ne part
    assert!(api.refused("/api/providers/faux/connect", json!({ "key": "" })).await.contains("clé invalide"));
    assert_eq!(api.get("/api/models").await["providers"].as_array().unwrap().iter().find(|p| p["id"] == "ouvert").unwrap()["keyless"], true);
    assert_eq!(api.ok("/api/providers/ouvert/connect", json!({ "key": "" })).await, json!({ "connected": "ouvert", "verified": true }));
    assert!(!state.join("keys").join("ouvert.key").exists());
    assert!(fake.seen.lock().unwrap().iter().any(|(path, key, _)| path == "/ouvert/models" && key.is_empty()));

    // un modèle de service ne fait pas réfléchir le maître ; le premier modèle de texte activé devient le sien
    api.ok("/api/providers/faux/connect", json!({ "key": KEY })).await;
    assert_eq!(api.ok("/api/models/activate", json!({ "provider": "faux", "model": "sens-1", "calls": 5 })).await["default"], false);
    assert!(api.refused("/api/models/sens-1/default", json!({})).await.contains("tient une conversation"));
    assert_eq!(api.get("/api/models").await["default"], "");
    assert_eq!(api.ok("/api/models/activate", json!({ "provider": "faux", "model": "grand-1" })).await["default"], true);
    assert_eq!(api.get("/api/models").await["default"], "grand-1");
    say("bonjour encore").await.unwrap();
    api.wait_event("la réponse du maître", |e| e["text"].as_str().is_some_and(|t| t.contains("C'est noté."))).await;
    assert!(fake.seen.lock().unwrap().iter().any(|(path, _, model)| path.contains("chat") && model == "grand-1"));

    // un deuxième modèle ne prend pas la place ; on la lui donne
    assert_eq!(api.ok("/api/models/activate", json!({ "provider": "faux", "model": "petit-1" })).await["default"], false);
    assert_eq!(api.ok("/api/models/petit-1/default", json!({})).await, json!({ "default": "petit-1" }));
    say("et maintenant").await.unwrap();
    let start = Instant::now();
    while !fake.seen.lock().unwrap().iter().any(|(path, _, model)| path.contains("chat") && model == "petit-1") {
        assert!(start.elapsed() < Duration::from_secs(20), "le maître n'a pas changé de modèle");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // le choix tient après un redémarrage
    root.kill().await.unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    assert_eq!(api.get("/api/models").await["default"], "petit-1");

    // désactiver le modèle du maître : il n'en a plus, et le dit ; il ne retombe pas sur un autre en silence
    api.ok("/api/models/petit-1/deactivate", json!({})).await;
    assert_eq!(api.get("/api/models").await["default"], "");
    // il a déjà eu un modèle : un modèle activé ensuite ne reçoit pas sa conversation d'office
    assert_eq!(api.ok("/api/models/activate", json!({ "provider": "faux", "model": "petit-1" })).await["default"], false);
    assert_eq!(api.get("/api/models").await["default"], "");
    root.kill().await.unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    assert_eq!(api.get("/api/models").await["default"], "");
    api.ok("/api/models/grand-1/default", json!({})).await;

    // déconnecter le fournisseur emporte le modèle du maître
    api.ok("/api/providers/faux/disconnect", json!({})).await;
    assert_eq!(api.get("/api/models").await["default"], "");
    root.kill().await.unwrap();

    // une configuration qui fixe le modèle garde le dernier mot
    std::env::set_var("TALIXIO_TEST_CLE_FIXE", KEY);
    std::fs::write(
        &config,
        format!(
            "model = \"main\"\nlisten = \"127.0.0.1:{port}\"\nstate_dir = {state:?}\ncatalog_file = {catalog:?}\n\
             [models.main]\nprovider = \"openai\"\nbase_url = \"http://127.0.0.1:{fp}/v1\"\nmodel = \"principal\"\napi_key_env = \"TALIXIO_TEST_CLE_FIXE\"\n\
             [models.autre]\nprovider = \"openai\"\nbase_url = \"http://127.0.0.1:{fp}/v1\"\nmodel = \"second\"\napi_key_env = \"TALIXIO_TEST_CLE_FIXE\"\n"
        ),
    )
    .unwrap();
    let (mut root, api) = start_root(&config, &state, port).await;
    assert_eq!(api.get("/api/models").await["default_fixed"], true);
    assert!(api.refused("/api/models/autre/default", json!({})).await.contains("fixé par sa configuration"));
    assert_eq!(api.get("/api/models").await["default"], "main");
    root.kill().await.unwrap();
}
