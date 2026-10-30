//! Web renderer: serves tile updates to the Vue app over a WebSocket.
//!
//! Binds to 127.0.0.1 only. Every `/api` request must carry the per-session
//! token, and browser requests must come from an allowed `Origin`, because any
//! page open in the browser can send requests to localhost. The UI's static
//! files (built by build.rs and embedded) are served without the token.

use std::net::{Ipv4Addr, SocketAddr};

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use dashboard_contract::{ClientMessage, ServerMessage, UiSettings, Widget};
use dashboard_host::Host;
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, watch};

pub type OnReady = Box<dyn FnOnce(&str) + Send>;

pub struct WebConfig {
    pub port: u16,
    /// Extra allowed origin, e.g. the Vite dev server `http://localhost:5173`.
    pub dev_origin: Option<String>,
    /// Called with the page URL (token included) once the port is bound.
    pub on_ready: Option<OnReady>,
}

#[derive(Clone)]
struct AppState {
    host: Host,
    settings: watch::Receiver<UiSettings>,
    actions: mpsc::UnboundedSender<ClientMessage>,
    token: String,
    allowed_origins: Vec<String>,
}

pub async fn serve(
    host: Host,
    settings: watch::Receiver<UiSettings>,
    actions: mpsc::UnboundedSender<ClientMessage>,
    config: WebConfig,
) -> Result<()> {
    let token = new_token()?;
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, config.port))).await?;
    let port = listener.local_addr()?.port();

    let mut allowed_origins = vec![
        format!("http://127.0.0.1:{port}"),
        format!("http://localhost:{port}"),
    ];
    allowed_origins.extend(config.dev_origin.clone());

    let state = AppState {
        host,
        settings,
        actions,
        token: token.clone(),
        allowed_origins,
    };
    let api = Router::new()
        .route("/api/tiles", get(tiles))
        .route("/api/ws", get(ws))
        .route("/api/image", get(image))
        .layer(middleware::from_fn_with_state(state.clone(), guard));
    let app = Router::new().merge(api).fallback(ui).with_state(state);

    let url = format!("http://127.0.0.1:{port}/?token={token}");
    println!("dashboard: {url}");
    if let Some(origin) = &config.dev_origin {
        println!("dev:       {origin}/?token={token}");
    }
    if let Some(on_ready) = config.on_ready {
        on_ready(&url);
    }
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(RustEmbed)]
#[folder = "../../web/dist"]
struct Ui;

/// Serves the embedded Vue build. Unknown paths without an extension get
/// index.html so client-side routes work.
async fn ui(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    let (path, file) = match Ui::get(path) {
        Some(file) => (path, file),
        None if !path.contains('.') => match Ui::get("index.html") {
            Some(file) => ("index.html", file),
            None => return StatusCode::NOT_FOUND.into_response(),
        },
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    // Vite fingerprints everything under assets/, so those can be cached forever.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    (
        [
            (header::CONTENT_TYPE, file.metadata.mimetype().to_string()),
            (header::CACHE_CONTROL, cache.to_string()),
        ],
        file.data,
    )
        .into_response()
}

#[derive(Deserialize)]
struct TokenQuery {
    token: Option<String>,
}

/// Rejects requests without the session token or from a foreign origin.
async fn guard(
    State(state): State<AppState>,
    Query(q): Query<TokenQuery>,
    req: Request,
    next: Next,
) -> Response {
    if q.token.as_deref() != Some(state.token.as_str()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    // Non-browser clients (curl) send no Origin; browsers always do for WebSocket and fetch.
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        let ok = origin
            .to_str()
            .is_ok_and(|o| state.allowed_origins.iter().any(|a| a == o));
        if !ok {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    next.run(req).await
}

async fn tiles(State(state): State<AppState>) -> impl IntoResponse {
    Json(state.host.snapshot().await)
}

/// Larger pictures are refused; they would be slow to send and to draw.
const MAX_IMAGE_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Deserialize)]
struct ImageQuery {
    path: String,
}

/// Sends a picture that an `image` widget shows now. Only those: a path no
/// tile's current view names is refused, so this can't read other files.
async fn image(State(state): State<AppState>, Query(q): Query<ImageQuery>) -> Response {
    let shown = state
        .host
        .snapshot()
        .await
        .iter()
        .any(|t| shows_image(&t.view.root, &q.path));
    if !shown {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(mime) = image_type(&q.path) else {
        return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response();
    };
    match tokio::fs::metadata(&q.path).await {
        Ok(m) if m.is_file() && m.len() <= MAX_IMAGE_BYTES => {}
        Ok(m) if m.is_file() => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        _ => return StatusCode::NOT_FOUND.into_response(),
    }
    match tokio::fs::read(&q.path).await {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "private, no-cache"),
            ],
            bytes,
        )
            .into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

fn image_type(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path)
        .extension()?
        .to_str()?
        .to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => return None,
    })
}

fn shows_image(widget: &Widget, path: &str) -> bool {
    match widget {
        Widget::Image { path: p, .. } => p == path,
        Widget::Row { children } | Widget::Column { children } => {
            children.iter().any(|c| shows_image(c, path))
        }
        _ => false,
    }
}

async fn ws(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(move |socket| feed(socket, state.host, state.settings, state.actions))
}

async fn feed(
    socket: WebSocket,
    host: Host,
    mut settings: watch::Receiver<UiSettings>,
    actions: mpsc::UnboundedSender<ClientMessage>,
) {
    let (mut sink, mut stream) = socket.split();
    let mut rx = host.subscribe();

    let current = settings.borrow_and_update().clone();
    if send(&mut sink, ServerMessage::Settings { settings: current })
        .await
        .is_err()
    {
        return;
    }

    for tile in host.snapshot().await {
        if send(&mut sink, ServerMessage::Tile { tile }).await.is_err() {
            return;
        }
    }

    loop {
        tokio::select! {
            update = rx.recv() => match update {
                Ok(tile) => {
                    if send(&mut sink, ServerMessage::Tile { tile }).await.is_err() {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => return,
            },
            changed = settings.changed() => {
                if changed.is_err() {
                    return;
                }
                let current = settings.borrow_and_update().clone();
                if send(&mut sink, ServerMessage::Settings { settings: current }).await.is_err() {
                    return;
                }
            },
            msg = stream.next() => match msg {
                Some(Ok(Message::Text(text))) => match serde_json::from_str::<ClientMessage>(&text) {
                    Ok(action) => {
                        let _ = actions.send(action);
                    }
                    Err(err) => tracing::warn!("ignoring bad client message: {err}"),
                },
                Some(Ok(_)) => {}
                _ => return,
            },
        }
    }
}

async fn send(
    sink: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    msg: ServerMessage,
) -> Result<()> {
    let text = serde_json::to_string(&msg)?;
    sink.send(Message::Text(text.into())).await?;
    Ok(())
}

fn new_token() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("getrandom: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pictures_in_the_view_are_served() {
        let view = Widget::Column {
            children: vec![Widget::Image {
                path: "/pics/a.png".into(),
                caption: None,
            }],
        };
        assert!(shows_image(&view, "/pics/a.png"));
        assert!(!shows_image(&view, "/pics/b.png"));
        assert!(!shows_image(&view, "/etc/passwd"));
        assert_eq!(image_type("/pics/A.JPG"), Some("image/jpeg"));
        assert_eq!(image_type("/pics/a.svg"), None);
    }
}
