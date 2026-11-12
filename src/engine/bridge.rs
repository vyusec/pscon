use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use crate::core::{AppEvent, BridgeEvent, Config, EventBus, ScanEvent, ScanSnapshot};
use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::net::TcpListener;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct BridgeHandle {
    pub addr: SocketAddr,
    pub token: String,
    pub events: EventBus,
    pub connected: Arc<AtomicBool>,
}

#[derive(Clone)]
struct BridgeState {
    token: String,
    events: EventBus,
    allowed_origins: Vec<String>,
    connected: Arc<AtomicBool>,
    connections: Arc<AtomicUsize>,
}

pub async fn start(config: &Config, events: EventBus) -> Result<BridgeHandle> {
    let token = config
        .bridge_token()
        .or_else(|| {
            std::fs::read_to_string(crate::core::paths::bridge_token_path())
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_else(generate_token);
    persist_token(&token)?;
    let addr: SocketAddr = format!("{}:{}", config.bridge.bind, config.bridge.port)
        .parse()
        .context("invalid bridge bind address")?;
    let listener = TcpListener::bind(addr)
        .await
        .with_context(|| format!("binding bridge on {addr}"))?;
    let addr = listener.local_addr()?;
    let connected = Arc::new(AtomicBool::new(false));
    let state = BridgeState {
        token: token.clone(),
        events: events.clone(),
        allowed_origins: config.bridge.allowed_origins.clone(),
        connected: connected.clone(),
        connections: Arc::new(AtomicUsize::new(0)),
    };
    let app = Router::new()
        .route("/health", get(health))
        .route("/ws", get(ws_handler))
        .route("/ingest", post(ingest))
        .route("/ingest/resource", post(ingest_resource))
        .layer(axum::extract::DefaultBodyLimit::max(64 * 1024 * 1024))
        .layer(tower_http::cors::CorsLayer::permissive())
        .with_state(state);
    info!(%addr, "pscon bridge listening");
    tokio::spawn(async move {
        if let Err(err) = axum::serve(listener, app).await {
            warn!("bridge server stopped: {err}");
        }
    });
    Ok(BridgeHandle {
        addr,
        token,
        events,
        connected,
    })
}

async fn health() -> impl IntoResponse {
    Json(json!({
        "status": "ok",
        "tool": "pscon",
        "protocol": 1,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[derive(Debug, Deserialize)]
struct WsQuery {
    token: Option<String>,
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(query): Query<WsQuery>,
    State(state): State<BridgeState>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !token_matches(&state, query.token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, "invalid bridge token").into_response();
    }
    if !origin_allowed(&state, &headers) {
        return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
    }
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: BridgeState) {
    let mut events = state.events.subscribe();
    let session = Uuid::new_v4().to_string();
    let ack = json!({
        "type": "hello_ack",
        "protocol": 1,
        "session_id": session,
        "tool": "pscon",
        "version": env!("CARGO_PKG_VERSION"),
    });
    if socket
        .send(Message::Text(ack.to_string().into()))
        .await
        .is_err()
    {
        return;
    }
    state.connections.fetch_add(1, Ordering::SeqCst);
    state.connected.store(true, Ordering::SeqCst);
    state.events.status(true, "extension connected");
    loop {
        let message = tokio::select! {
            message = socket.next() => match message {
                Some(message) => message,
                None => break,
            },
            event = events.recv() => {
                match event {
                    Ok(AppEvent::Scan(event)) => {
                        if let Some(progress) = scan_progress(event)
                            && socket.send(Message::Text(progress.to_string().into())).await.is_err()
                        {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    _ => {},
                }
                continue;
            }
        };
        let Ok(message) = message else { break };
        match message {
            Message::Text(text) => {
                let value: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(err) => {
                        let _ = socket
                            .send(Message::Text(
                                json!({"type":"error","error":format!("invalid JSON: {err}")})
                                    .to_string()
                                    .into(),
                            ))
                            .await;
                        continue;
                    }
                };
                if value.get("type").and_then(|v| v.as_str()) == Some("ping") {
                    if socket
                        .send(Message::Text(json!({"type":"pong"}).to_string().into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                } else {
                    handle_client_message(&state, &value).await;
                }
            }
            Message::Ping(payload) => {
                let _ = socket.send(Message::Pong(payload)).await;
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    if state.connections.fetch_sub(1, Ordering::SeqCst) == 1 {
        state.connected.store(false, Ordering::SeqCst);
        state.events.status(false, "extension disconnected");
        state.events.bridge(BridgeEvent::Disconnected);
    }
}

fn scan_progress(event: ScanEvent) -> Option<serde_json::Value> {
    let (scan_id, stage, message, progress) = match event {
        ScanEvent::Started { scan_id, target } => (
            scan_id,
            "starting".into(),
            format!("Scanning {target}"),
            0.0,
        ),
        ScanEvent::Stage {
            scan_id,
            stage,
            message,
            progress,
        } => (scan_id, stage, message, progress),
        ScanEvent::Completed { scan_id, summary } => (
            scan_id,
            "complete".into(),
            format!(
                "Complete: {} endpoints, {} secrets",
                summary.endpoints, summary.secrets
            ),
            1.0,
        ),
        ScanEvent::Failed { scan_id, error } => (scan_id, "failed".into(), error, 0.0),
        _ => return None,
    };
    Some(
        json!({"type":"scan_progress", "scan_id":scan_id, "stage":stage, "message":message, "progress":progress}),
    )
}

async fn handle_client_message(state: &BridgeState, value: &serde_json::Value) {
    let kind = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match kind {
        "hello" => {
            let extension_version = value
                .get("extension_version")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let browser = value
                .get("browser")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let tab_url = value
                .get("tab_url")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            state.events.bridge(BridgeEvent::Hello {
                extension_version,
                browser,
                tab_url,
            });
        }
        "scan_event" => {
            if let Some(event) = value.get("event")
                && let Ok(scan_event) = serde_json::from_value(event.clone())
            {
                state.events.scan(scan_event);
            }
        }
        "scan_progress" => {
            let scan_id = value
                .get("scan_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let stage = value
                .get("stage")
                .and_then(|v| v.as_str())
                .unwrap_or("extension")
                .to_string();
            let message = value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            let progress = value
                .get("progress")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0) as f32;
            state.events.bridge(BridgeEvent::ScanProgress {
                scan_id,
                stage,
                message,
                progress,
            });
        }
        "ping" => {
            // The HTTP handler cannot write back through this path; the WS loop
            // answers pings at the protocol level. Extension-side pings are harmless.
        }
        "log" => {
            let message = value
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            state.events.bridge(BridgeEvent::Log {
                level: "info".into(),
                message,
            });
        }
        _ => {}
    }
}

#[derive(Debug, Deserialize)]
struct ResourceUpload {
    url: String,
    status: u16,
    content_type: Option<String>,
    body_base64: String,
}

async fn ingest(
    State(state): State<BridgeState>,
    headers: HeaderMap,
    Json(snapshot): Json<ScanSnapshot>,
) -> impl IntoResponse {
    if !bearer_matches(&state, &headers) {
        return (StatusCode::UNAUTHORIZED, "invalid bridge token").into_response();
    }
    state.events.bridge(BridgeEvent::Snapshot { snapshot });
    Json(json!({"ok": true})).into_response()
}

async fn ingest_resource(
    State(state): State<BridgeState>,
    headers: HeaderMap,
    Json(upload): Json<ResourceUpload>,
) -> impl IntoResponse {
    if !bearer_matches(&state, &headers) {
        return (StatusCode::UNAUTHORIZED, "invalid bridge token").into_response();
    }
    state.events.bridge(BridgeEvent::Resource {
        url: upload.url,
        status: upload.status,
        content_type: upload.content_type,
        body_base64: upload.body_base64,
    });
    Json(json!({"ok": true})).into_response()
}

fn token_matches(state: &BridgeState, supplied: Option<&str>) -> bool {
    supplied
        .map(|token| constant_time_eq(token.as_bytes(), state.token.as_bytes()))
        .unwrap_or(false)
}

fn bearer_matches(state: &BridgeState, headers: &HeaderMap) -> bool {
    let Some(value) = headers.get(axum::http::header::AUTHORIZATION) else {
        return false;
    };
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_eq(token.as_bytes(), state.token.as_bytes())
}

fn origin_allowed(state: &BridgeState, headers: &HeaderMap) -> bool {
    let Some(origin) = headers.get(axum::http::header::ORIGIN) else {
        return true;
    };
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    state
        .allowed_origins
        .iter()
        .any(|allowed| origin_matches(allowed, origin))
}

fn origin_matches(allowed: &str, origin: &str) -> bool {
    if allowed == "*" {
        return true;
    }
    if let Some(prefix) = allowed.strip_suffix('*') {
        origin.starts_with(prefix)
    } else {
        allowed == origin
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

fn generate_token() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

fn persist_token(token: &str) -> Result<()> {
    let path = crate::core::paths::bridge_token_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, token)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
