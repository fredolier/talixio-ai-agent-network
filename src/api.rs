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

use crate::config::ensure_loopback;
use crate::link::{serve_child, MAX_FRAME};
use crate::node::{Input, Node};
use crate::secret_eq;

#[derive(Clone)]
struct App {
    node: Arc<Node>,
    /// Jeton de l'API. Absent chez les agents enfants : ils n'exposent pas d'API.
    api_token: Option<Arc<String>>,
}

/// Ouvre l'écoute. `api_token` présent : l'API JSON du maître est servie aussi.
pub async fn start_listener(node: Arc<Node>, addr: SocketAddr, api_token: Option<String>) -> Result<SocketAddr> {
    ensure_loopback(&addr)?;
    let listener = tokio::net::TcpListener::bind(addr).await.with_context(|| format!("écoute sur {addr}"))?;
    let local = listener.local_addr()?;
    let mut router = Router::new().route("/link", get(link));
    if api_token.is_some() {
        router = router
            .route("/api/health", get(|| async { Json(json!({ "ok": true })) }))
            .route("/api/tree", get(tree))
            .route("/api/activity", get(activity))
            .route("/api/events", get(events))
            .route("/api/messages", post(message))
            .route("/api/agents/{id}/stop", post(stop));
    }
    let app = router.with_state(App { node: node.clone(), api_token: api_token.map(Arc::new) });
    let _ = node.listen.set(local);
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("écoute interrompue : {e}");
        }
    });
    Ok(local)
}

async fn link(State(app): State<App>, upgrade: WebSocketUpgrade) -> Response {
    upgrade
        .max_message_size(MAX_FRAME)
        .max_frame_size(MAX_FRAME)
        .on_upgrade(move |socket| serve_child(app.node, socket))
}

fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

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
