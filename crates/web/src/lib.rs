//! Web renderer: serves tile updates to the Vue app over a WebSocket.
//!
//! Binds to 127.0.0.1 only. Any page open in the browser can send requests
//! to localhost, and a DNS-rebinding page can even read the answers, so:
//!
//! - every request must name a loopback `Host`, and a browser request must
//!   come from an allowed `Origin`;
//! - every `/api` request but sign-in must carry the session token, made for
//!   each run: `Authorization: Bearer <token>`, or for the WebSocket, which
//!   can't set headers, the subprotocol `bearer.<token>`.
//!
//! The session token never goes in a URL. The link the server prints and
//! opens carries a one-time boot token (`?boot=`), which the page trades for
//! the session token at `POST /api/session` (see `dashboard_cli_kit::boot`).
//! The UI's static files (built by build.rs and embedded) are served without
//! the token.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::Result;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, Request, State};
use axum::http::{StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use dashboard_cli_kit::boot::{self, Redeem, SignIn};
use dashboard_contract::{
    CatalogEntry, CatalogResponse, ClientMessage, ServerMessage, UiSettings, Widget,
};
use dashboard_host::Host;
use futures_util::{SinkExt, StreamExt};
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{mpsc, watch};

pub type OnReady = Box<dyn FnOnce(&Ready) + Send>;

/// Where the server is listening, given to `on_ready` once the port is bound.
pub struct Ready {
    /// The page, without any token: `http://127.0.0.1:<port>/`.
    pub url: String,
    /// The session token. Keep it out of URLs and logs.
    pub token: String,
    /// The page with a fresh boot token: open this one.
    pub sign_in: String,
}

/// Reads the whole catalog, and problems reading it. Called for each
/// `/api/catalog` request, so newly installed packages show up.
pub type CatalogSource = Arc<dyn Fn() -> (Vec<CatalogEntry>, Vec<String>) + Send + Sync>;

pub struct WebConfig {
    pub port: u16,
    /// Extra allowed origin, e.g. the Vite dev server `http://localhost:5173`.
    pub dev_origin: Option<String>,
    /// Called once the port is bound.
    pub on_ready: Option<OnReady>,
    /// Backs `/api/catalog`. Without it the catalog is empty.
    pub catalog: Option<CatalogSource>,
}

#[derive(Clone)]
struct AppState {
    host: Host,
    settings: watch::Receiver<UiSettings>,
    actions: mpsc::UnboundedSender<ClientMessage>,
    token: Arc<str>,
    sign_in: Arc<Mutex<SignIn>>,
    allowed_origins: Arc<[String]>,
    allowed_hosts: Arc<[String]>,
    catalog: Option<CatalogSource>,
}

pub async fn serve(
    host: Host,
    settings: watch::Receiver<UiSettings>,
    actions: mpsc::UnboundedSender<ClientMessage>,
    config: WebConfig,
) -> Result<()> {
    let token = boot::new_token();
    let listener =
        tokio::net::TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, config.port))).await?;
    let port = listener.local_addr()?.port();

    let mut allowed_hosts = vec![
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
        format!("[::1]:{port}"),
    ];
    let mut allowed_origins: Vec<String> = allowed_hosts
        .iter()
        .map(|h| format!("http://{h}"))
        .collect();
    if let Some(origin) = &config.dev_origin {
        // The Vite proxy passes the browser's Host through.
        allowed_hosts.extend(origin.split_once("://").map(|(_, h)| h.to_string()));
        allowed_origins.push(origin.clone());
    }

    let sign_in = Arc::new(Mutex::new(SignIn::new()));
    let state = AppState {
        host,
        settings,
        actions,
        token: token.clone().into(),
        sign_in: sign_in.clone(),
        allowed_origins: allowed_origins.into(),
        allowed_hosts: allowed_hosts.into(),
        catalog: config.catalog,
    };
    let app = router(state);

    let url = format!("http://127.0.0.1:{port}/");
    let boot = {
        let mut s = sign_in.lock().unwrap();
        s.arm(Instant::now());
        s.token().to_string()
    };
    let sign_in = format!("{url}?boot={boot}");
    println!("dashboard: {sign_in}");
    println!("           signs one browser tab in; unused, it expires in a minute.");
    println!("           `dashboard open` makes a new link.");
    if let Some(origin) = &config.dev_origin {
        println!("dev:       {origin}/?boot={boot}");
    }
    if let Some(on_ready) = config.on_ready {
        on_ready(&Ready {
            url,
            token,
            sign_in,
        });
    }
    axum::serve(listener, app).await?;
    Ok(())
}

fn router(state: AppState) -> Router {
    let api = Router::new()
        .route("/api/tiles", get(tiles))
        .route("/api/catalog", get(catalog))
        .route("/api/image", get(image))
        .route("/api/ws", get(ws))
        .route("/api/boot", post(new_boot))
        .layer(middleware::from_fn_with_state(state.clone(), require_token));
    Router::new()
        .merge(api)
        .route("/api/session", post(session))
        .fallback(ui)
        .layer(middleware::from_fn_with_state(state.clone(), local_only))
        .with_state(state)
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

/// Refuses a `Host` that isn't this server on a loopback name (DNS
/// rebinding) and an `Origin` that isn't the page's own, before anything else.
async fn local_only(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| state.allowed_hosts.iter().any(|a| a == h));
    if !host_ok {
        return StatusCode::FORBIDDEN.into_response();
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

/// Refuses requests without the session token.
async fn require_token(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let headers = req.headers();
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "));
    let subprotocol = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.split(',').find_map(|p| p.trim().strip_prefix("bearer.")));
    if !bearer
        .or(subprotocol)
        .is_some_and(|t| boot::token_eq(t, &state.token))
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(req).await
}

#[derive(Deserialize)]
struct SessionRequest {
    boot: String,
}

#[derive(Serialize)]
struct SessionResponse {
    token: String,
}

/// Trade the boot token for the session token, once. The JSON body means a
/// page on another origin needs a CORS preflight, which is never answered.
async fn session(State(state): State<AppState>, Json(req): Json<SessionRequest>) -> Response {
    let result = state
        .sign_in
        .lock()
        .unwrap()
        .redeem(&req.boot, Instant::now());
    match result {
        Redeem::Ok => Json(SessionResponse {
            token: state.token.to_string(),
        })
        .into_response(),
        Redeem::Replayed => {
            tracing::warn!(
                "a used sign-in link was opened again (a second tab, or another program that \
                 read it); refused. `dashboard open` makes a new link"
            );
            StatusCode::UNAUTHORIZED.into_response()
        }
        Redeem::Refused => StatusCode::UNAUTHORIZED.into_response(),
    }
}

#[derive(Serialize)]
struct BootResponse {
    boot: String,
}

/// A new boot token, for `dashboard open`. The old one stops working.
async fn new_boot(State(state): State<AppState>) -> Json<BootResponse> {
    let boot = state
        .sign_in
        .lock()
        .unwrap()
        .renew(Instant::now())
        .to_string();
    Json(BootResponse { boot })
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
    let ext = std::path::Path::new(&q.path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    let mime = match ext.as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => return StatusCode::UNSUPPORTED_MEDIA_TYPE.into_response(),
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

fn shows_image(widget: &Widget, path: &str) -> bool {
    match widget {
        Widget::Image { path: p, .. } => p == path,
        Widget::Row { children } | Widget::Column { children } => {
            children.iter().any(|c| shows_image(c, path))
        }
        _ => false,
    }
}

#[derive(Deserialize)]
struct CatalogQuery {
    #[serde(default)]
    q: String,
    tag: Option<String>,
}

/// `GET /api/catalog?q=words&tag=t`: entries matching both, and every tag.
async fn catalog(State(state): State<AppState>, Query(q): Query<CatalogQuery>) -> Response {
    let Some(source) = state.catalog.clone() else {
        return Json(CatalogResponse::default()).into_response();
    };
    // Reading packages touches the disk.
    match tokio::task::spawn_blocking(move || source()).await {
        Ok((all, errors)) => {
            let tag = q.tag.as_deref().filter(|t| !t.is_empty());
            Json(CatalogResponse::new(&all, &q.q, tag, errors)).into_response()
        }
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

async fn ws(State(state): State<AppState>, upgrade: WebSocketUpgrade) -> Response {
    // The page offers `dashboard` and `bearer.<token>`; answer the first, so the
    // token isn't echoed back.
    upgrade
        .protocols(["dashboard"])
        .on_upgrade(move |socket| feed(socket, state.host, state.settings, state.actions))
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use dashboard_contract::ConfigInfo;
    use tower::ServiceExt;

    use super::*;

    fn state() -> AppState {
        let (_, settings) = watch::channel(UiSettings {
            theme: Default::default(),
            color_theme: Default::default(),
            color_themes: vec![],
            screens: vec![],
            modules: vec![],
            rotate: None,
            config: ConfigInfo::default(),
        });
        let (actions, _) = mpsc::unbounded_channel();
        let mut sign_in = SignIn::new();
        sign_in.arm(Instant::now());
        AppState {
            host: Host::new(),
            settings,
            actions,
            token: "session".into(),
            sign_in: Arc::new(Mutex::new(sign_in)),
            allowed_origins: vec!["http://127.0.0.1:1".to_string()].into(),
            allowed_hosts: vec!["127.0.0.1:1".to_string()].into(),
            catalog: None,
        }
    }

    async fn call(state: &AppState, req: Request<Body>) -> (StatusCode, String) {
        let res = router(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let body = axum::body::to_bytes(res.into_body(), 1 << 20)
            .await
            .unwrap();
        (status, String::from_utf8_lossy(&body).into_owned())
    }

    fn session(boot: &str, origin: &str) -> Request<Body> {
        Request::post("/api/session")
            .header(header::HOST, "127.0.0.1:1")
            .header(header::ORIGIN, origin)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(format!(r#"{{"boot":"{boot}"}}"#)))
            .unwrap()
    }

    fn tiles(host: &str, auth: Option<&str>) -> Request<Body> {
        let mut req = Request::get("/api/tiles").header(header::HOST, host);
        if let Some(auth) = auth {
            req = req.header(header::AUTHORIZATION, auth);
        }
        req.body(Body::empty()).unwrap()
    }

    #[tokio::test]
    async fn boot_token_signs_in_once() {
        let state = state();
        let boot = state.sign_in.lock().unwrap().token().to_string();
        let (status, _) = call(&state, session(&boot, "http://evil.example")).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
        let (status, body) = call(&state, session(&boot, "http://127.0.0.1:1")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, r#"{"token":"session"}"#);
        let (status, _) = call(&state, session(&boot, "http://127.0.0.1:1")).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn api_needs_the_token_and_a_loopback_host() {
        let state = state();
        let ok = Some("Bearer session");
        assert_eq!(
            call(&state, tiles("127.0.0.1:1", ok)).await.0,
            StatusCode::OK
        );
        let bad = call(&state, tiles("127.0.0.1:1", Some("Bearer nope"))).await;
        assert_eq!(bad.0, StatusCode::UNAUTHORIZED);
        let none = call(&state, tiles("127.0.0.1:1", None)).await;
        assert_eq!(none.0, StatusCode::UNAUTHORIZED);
        // DNS rebinding: the right token, but a foreign Host.
        let rebound = call(&state, tiles("evil.example:1", ok)).await;
        assert_eq!(rebound.0, StatusCode::FORBIDDEN);
        // The token in the query string no longer works.
        let query = Request::get("/api/tiles?token=session")
            .header(header::HOST, "127.0.0.1:1")
            .body(Body::empty())
            .unwrap();
        assert_eq!(call(&state, query).await.0, StatusCode::UNAUTHORIZED);
    }

    struct Picture(String);

    #[async_trait::async_trait]
    impl dashboard_host::Module for Picture {
        async fn init(
            &mut self,
            _: serde_json::Value,
        ) -> anyhow::Result<dashboard_contract::Manifest> {
            Ok(dashboard_contract::Manifest {
                id: "picture".into(),
                name: "picture".into(),
                contract_version: dashboard_contract::CONTRACT_VERSION.into(),
                refresh: dashboard_contract::Refresh::Poll {
                    interval_ms: 60_000,
                    min_interval_ms: None,
                },
                layout: dashboard_contract::LayoutHints {
                    size: dashboard_contract::Size::Medium,
                    priority: 0,
                    min_size: None,
                },
                commands: vec![],
                config_schema: None,
            })
        }
        async fn render(&mut self) -> anyhow::Result<dashboard_contract::View> {
            Ok(dashboard_contract::View {
                status: dashboard_contract::Status::Ok,
                summary: "a picture".into(),
                root: Widget::Column {
                    children: vec![Widget::Image {
                        path: self.0.clone(),
                        caption: None,
                    }],
                },
            })
        }
    }

    /// A picture a tile shows is served; any other file, or no token, is not.
    #[tokio::test]
    async fn serves_only_pictures_a_tile_shows() {
        let dir = std::env::temp_dir().join(format!("dashboard-web-image-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let shown = dir.join("shown.png");
        let other = dir.join("other.png");
        std::fs::write(&shown, b"png bytes").unwrap();
        std::fs::write(&other, b"secret").unwrap();

        let state = state();
        state
            .host
            .spawn(
                "picture",
                Box::new(Picture(shown.to_string_lossy().into_owned())),
                serde_json::Value::Null,
                Default::default(),
            )
            .await
            .unwrap();
        while state.host.snapshot().await.is_empty() {
            tokio::task::yield_now().await;
        }
        let get = |path: &std::path::Path, auth: bool| {
            let mut req = Request::get(format!(
                "/api/image?path={}",
                urlencode(&path.to_string_lossy())
            ))
            .header(header::HOST, "127.0.0.1:1");
            if auth {
                req = req.header(header::AUTHORIZATION, "Bearer session");
            }
            req.body(Body::empty()).unwrap()
        };
        assert_eq!(
            call(&state, get(&shown, true)).await,
            (StatusCode::OK, "png bytes".to_string())
        );
        assert_eq!(
            call(&state, get(&other, true)).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            call(&state, get(&shown, false)).await.0,
            StatusCode::UNAUTHORIZED
        );
        state.host.stop_all().await;
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn urlencode(s: &str) -> String {
        s.bytes()
            .map(|b| match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                    (b as char).to_string()
                }
                _ => format!("%{b:02X}"),
            })
            .collect()
    }

    /// The page's WebSocket carries the token as a subprotocol, since a
    /// browser can't set headers on one.
    #[tokio::test]
    async fn websocket_subprotocol_carries_the_token() {
        let state = state();
        let with = |protocols: &str| {
            Request::get("/api/tiles")
                .header(header::HOST, "127.0.0.1:1")
                .header(header::SEC_WEBSOCKET_PROTOCOL, protocols)
                .body(Body::empty())
                .unwrap()
        };
        let ok = call(&state, with("dashboard, bearer.session")).await;
        assert_eq!(ok.0, StatusCode::OK);
        let bad = call(&state, with("dashboard, bearer.nope")).await;
        assert_eq!(bad.0, StatusCode::UNAUTHORIZED);
        let none = call(&state, with("dashboard")).await;
        assert_eq!(none.0, StatusCode::UNAUTHORIZED);
    }
}
