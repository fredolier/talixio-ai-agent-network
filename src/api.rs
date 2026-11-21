//! Les points d'entrée réseau d'un agent : le lien pour ses enfants, et, chez le maître, l'API JSON.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::{Stream, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio_stream::wrappers::BroadcastStream;

use crate::link::{serve_child, MAX_FRAME};
use crate::node::{Input, Node};
use crate::secret_eq;

#[derive(Clone)]
struct App {
    node: Arc<Node>,
    /// Jeton de l'API. Absent chez les agents enfants : ils n'exposent pas d'API.
    api_token: Option<Arc<String>>,
    /// Places pour les connexions qui n'ont pas encore prouvé qui elles sont.
    handshakes: Arc<tokio::sync::Semaphore>,
}

/// Présentations simultanées au plus : un inconnu ne peut pas occuper le parent.
const MAX_HANDSHAKES: usize = 8;
/// Taille maximale d'un message envoyé au maître par l'API, en caractères.
const MAX_MESSAGE_CHARS: usize = 100_000;

/// Ouvre l'écoute, en TLS. `api_token` présent : l'API JSON du maître est servie aussi.
pub async fn start_listener(
    node: Arc<Node>,
    addr: SocketAddr,
    api_token: Option<String>,
    identity: crate::tls::Identity,
) -> Result<SocketAddr> {
    let listener = std::net::TcpListener::bind(addr).with_context(|| format!("écoute sur {addr}"))?;
    listener.set_nonblocking(true)?;
    let local = listener.local_addr()?;
    let mut router = Router::new().route("/link", get(link));
    if api_token.is_some() {
        router = router
            .route("/", get(|| async { asset("text/html; charset=utf-8", include_bytes!("../web/dist/index.html")) }))
            .route("/app.js", get(|| async { asset("text/javascript; charset=utf-8", include_bytes!("../web/dist/app.js")) }))
            .route("/app.css", get(|| async { asset("text/css; charset=utf-8", include_bytes!("../web/dist/app.css")) }))
            .route("/sans.woff2", get(|| async { asset("font/woff2", include_bytes!("../web/dist/sans.woff2")) }))
            .route("/mono.woff2", get(|| async { asset("font/woff2", include_bytes!("../web/dist/mono.woff2")) }))
            .route("/icon.svg", get(|| async { asset("image/svg+xml", include_bytes!("../web/dist/icon.svg")) }))
            .route("/manifest.webmanifest", get(|| async { asset("application/manifest+json", include_bytes!("../web/dist/manifest.webmanifest")) }))
            .route("/api/health", get(|| async { Json(json!({ "ok": true })) }))
            .route("/api/tree", get(tree))
            .route("/api/releases", get(releases))
            .route("/api/models", get(models))
            .route("/api/roles", get(roles))
            .route("/api/activity", get(activity))
            .route("/api/events", get(events))
            .route("/api/messages", post(message))
            .route("/api/agents/{id}/stop", post(stop));
    }
    let app = router.with_state(App {
        node: node.clone(),
        api_token: api_token.map(Arc::new),
        handshakes: Arc::new(tokio::sync::Semaphore::new(MAX_HANDSHAKES)),
    });
    let _ = node.listen.set(local);
    let _ = node.cert.set(identity.fingerprint.clone());
    let tls = axum_server::tls_rustls::RustlsConfig::from_config(identity.server);
    tokio::spawn(async move {
        if let Err(e) = axum_server::from_tcp_rustls(listener, tls).serve(app.into_make_service()).await {
            eprintln!("écoute interrompue : {e}");
        }
    });
    Ok(local)
}

/// Ce que la page a le droit de faire : charger ses propres fichiers et appeler sa propre API.
/// Ni script ni style en ligne, aucune ressource d'ailleurs, jamais dans un cadre.
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; connect-src 'self'; \
                   manifest-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

/// Un fichier de l'interface web, embarqué dans l'exécutable. Rien de secret : servi sans jeton.
fn asset(content_type: &'static str, body: &'static [u8]) -> Response {
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CONTENT_SECURITY_POLICY, CSP),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::REFERRER_POLICY, "no-referrer"),
            (header::CACHE_CONTROL, "no-cache"),
            (header::STRICT_TRANSPORT_SECURITY, "max-age=31536000"),
        ],
        body,
    )
        .into_response()
}

/// Les modèles que le maître peut appeler, par fournisseur. Jamais de clé, ni où elle est rangée.
async fn models(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize(&app, &headers) {
        return response;
    }
    let catalog = app.node.providers.as_ref().map(|p| p.catalog().to_vec()).unwrap_or_default();
    Json(json!({ "default": app.node.model, "models": catalog })).into_response()
}

/// Le répertoire des rôles : chaque agent créé dans la grappe, avec ce qu'on lui a accordé.
async fn roles(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize(&app, &headers) {
        return response;
    }
    let store = app.node.store.clone();
    let roles = tokio::task::spawn_blocking(move || store.roles()).await.unwrap_or_default();
    Json(json!({ "roles": roles })).into_response()
}

/// Les notes de version, pour l'interface : la version qui tourne, puis toutes les notes.
async fn releases(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Err(response) = authorize(&app, &headers) {
        return response;
    }
    Json(json!({ "version": crate::releases::VERSION, "releases": crate::releases::all() })).into_response()
}

async fn link(State(app): State<App>, headers: HeaderMap, upgrade: WebSocketUpgrade) -> Response {
    // un agent n'envoie jamais d'en-tête Origin : c'est une page web qui essaie de se connecter
    if headers.contains_key(header::ORIGIN) {
        return error(StatusCode::FORBIDDEN, "lien réservé aux agents");
    }
    let Ok(permit) = app.handshakes.clone().try_acquire_owned() else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "trop de connexions en attente");
    };
    upgrade
        .max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| serve_child(app.node, socket, permit))
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

#[allow(clippy::result_large_err)]
fn authorize(app: &App, headers: &HeaderMap) -> Result<(), Response> {
    let expected = app.api_token.as_deref().ok_or_else(|| error(StatusCode::NOT_FOUND, "pas d'API ici"))?;
    let given = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if secret_eq(given, expected) {
        Ok(())
    } else {
        Err(error(StatusCode::UNAUTHORIZED, "jeton manquant ou invalide"))
    }
}

async fn tree(State(app): State<App>, headers: HeaderMap) -> Response {
    if let Err(denied) = authorize(&app, &headers) {
        return denied;
    }
    Json(app.node.snapshot()).into_response()
}

#[derive(Deserialize)]
struct After {
    #[serde(default)]
    after: u64,
    limit: Option<usize>,
}

async fn activity(State(app): State<App>, headers: HeaderMap, Query(q): Query<After>) -> Response {
    if let Err(denied) = authorize(&app, &headers) {
        return denied;
    }
    let log = app.node.activity.as_ref().expect("le maître tient le journal");
    Json(json!({ "events": log.after(q.after, q.limit.unwrap_or(500).min(5000)) })).into_response()
}

/// Le journal en continu (Server-Sent Events) : d'abord ce qui suit `after`, puis le direct.
async fn events(State(app): State<App>, headers: HeaderMap, Query(q): Query<After>) -> Response {
    if let Err(denied) = authorize(&app, &headers) {
        return denied;
    }
    let log = app.node.activity.as_ref().expect("le maître tient le journal");
    let live = BroadcastStream::new(log.live.subscribe());
    let replay = log.after(q.after, usize::MAX);
    let mut last = replay.last().map(|e| e.id).unwrap_or(q.after);
    let stream = futures_util::stream::iter(replay).chain(live.filter_map(move |event| {
        let fresh = event.ok().filter(|e| e.id > last);
        if let Some(event) = &fresh {
            last = event.id;
        }
        async move { fresh }
    }));
    let stream: std::pin::Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>> = Box::pin(stream.map(|event| {
        Ok(Event::default().id(event.id.to_string()).event(event.kind.clone()).data(serde_json::to_string(&event).unwrap_or_default()))
    }));
    Sse::new(stream).keep_alive(KeepAlive::default()).into_response()
}

async fn message(State(app): State<App>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    if let Err(denied) = authorize(&app, &headers) {
        return denied;
    }
    let Some(text) = body["text"].as_str().map(str::trim).filter(|t| !t.is_empty()) else {
        return error(StatusCode::BAD_REQUEST, "champ « text » manquant");
    };
    if text.chars().count() > MAX_MESSAGE_CHARS {
        return error(StatusCode::PAYLOAD_TOO_LARGE, "message trop long");
    }
    let _ = app.node.inbox.send(Input::User(text.to_string()));
    (StatusCode::ACCEPTED, Json(json!({ "accepted": true }))).into_response()
}

async fn stop(State(app): State<App>, headers: HeaderMap, Path(id): Path<String>) -> Response {
    if let Err(denied) = authorize(&app, &headers) {
        return denied;
    }
    match app.node.stop_descendant(&id) {
        Ok(()) => Json(json!({ "stopping": id })).into_response(),
        Err(e) => error(StatusCode::NOT_FOUND, &e),
    }
}
