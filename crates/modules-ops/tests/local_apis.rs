//! The Pi-hole and Transmission modules against fake servers on 127.0.0.1
//! that answer with the sample replies in `testdata/`. They check the parts
//! the unit tests can't: Pi-hole's login and session header, and
//! Transmission's 409 session-id handshake. No real service is called.

#![cfg(all(unix, feature = "pihole", feature = "transmission"))]

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use dashboard_contract::{Status, Widget};
use dashboard_host::Module;
use dashboard_modules_ops::{pihole::Pihole, transmission::Transmission};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// One request, as the fake server sees it.
struct Request {
    method: String,
    path: String,
    /// Header lines, lowercased.
    headers: Vec<String>,
    body: String,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        let prefix = format!("{}:", name.to_lowercase());
        self.headers
            .iter()
            .find(|h| h.starts_with(&prefix))
            .map(|h| h[prefix.len()..].trim())
    }
}

/// What the fake server sends back: status line, extra headers, body.
type Reply = (&'static str, Vec<String>, String);

/// Starts a server that answers every request with `handler`, and returns its
/// base URL. It runs until the test ends.
async fn serve(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let handler = Arc::new(handler);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            tokio::spawn(async move {
                let mut raw = Vec::new();
                let mut buf = [0u8; 4096];
                let head_end = loop {
                    if let Some(at) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                        break at + 4;
                    }
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                    }
                };
                let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
                let mut lines = head.lines();
                let mut first = lines.next().unwrap_or("").split_whitespace();
                let method = first.next().unwrap_or("").to_string();
                let path = first.next().unwrap_or("/").to_string();
                let headers: Vec<String> = lines
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_lowercase())
                    .collect();
                let len: usize = headers
                    .iter()
                    .find_map(|h| h.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                while raw.len() < head_end + len {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => raw.extend_from_slice(&buf[..n]),
                    }
                }
                let body = String::from_utf8_lossy(&raw[head_end..]).into_owned();
                let (status, extra, body) = handler(&Request {
                    method,
                    path,
                    headers,
                    body,
                });
                let mut out = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for h in extra {
                    out.push_str(&h);
                    out.push_str("\r\n");
                }
                out.push_str("\r\n");
                out.push_str(&body);
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.shutdown().await;
            });
        }
    });
    base
}

fn sample(path: &str) -> String {
    let p: PathBuf = [env!("CARGO_MANIFEST_DIR"), "testdata", path]
        .iter()
        .collect();
    std::fs::read_to_string(p).unwrap()
}

/// A fake Pi-hole. With `password` set, the API wants the session header
/// from the last login; without, logins give no session and the API is open.
/// It counts logins and logouts.
#[derive(Default)]
struct FakePihole {
    password: Option<&'static str>,
    logins: AtomicUsize,
    logouts: AtomicUsize,
    /// The session the API accepts now. Clearing it expires the session.
    sid: Mutex<Option<String>>,
}

impl FakePihole {
    fn reply(&self, req: &Request) -> Reply {
        let ok = |file: &str| ("200 OK", vec![], sample(file));
        let denied = (
            "401 Unauthorized",
            vec![],
            json!({ "error": { "key": "unauthorized", "message": "Unauthorized" } }).to_string(),
        );
        if req.path == "/api/auth" && req.method == "DELETE" {
            self.logouts.fetch_add(1, Ordering::SeqCst);
            *self.sid.lock().unwrap() = None;
            return ("204 No Content", vec![], String::new());
        }
        if req.method == "POST" && req.path == "/api/auth" {
            let n = self.logins.fetch_add(1, Ordering::SeqCst) + 1;
            let Some(password) = self.password else {
                let open = json!({ "session": { "valid": true, "totp": false, "sid": null, "validity": -1 } });
                return ("200 OK", vec![], open.to_string());
            };
            if !req.body.contains(&format!("\"{password}\"")) {
                return denied;
            }
            // Header values reach the handler lowercased.
            let sid = format!("sid{n}");
            *self.sid.lock().unwrap() = Some(sid.clone());
            let auth = json!({ "session": { "valid": true, "totp": false, "sid": sid, "validity": 1800 } });
            return ("200 OK", vec![], auth.to_string());
        }
        if self.password.is_some() && req.header("x-ftl-sid") != self.sid.lock().unwrap().as_deref()
        {
            return denied;
        }
        if self.password.is_some() && req.header("x-ftl-sid").is_none() {
            return denied;
        }
        match req.path.split('?').next().unwrap() {
            "/api/stats/summary" => ok("pihole/summary.json"),
            "/api/dns/blocking" => ok("pihole/blocking.json"),
            "/api/stats/top_domains" if req.path.contains("blocked=true") => {
                ok("pihole/top_blocked.json")
            }
            _ => ("404 Not Found", vec![], "{}".into()),
        }
    }
}

async fn serve_pihole(password: Option<&'static str>) -> (String, Arc<FakePihole>) {
    let fake = Arc::new(FakePihole {
        password,
        ..Default::default()
    });
    let f = fake.clone();
    (serve(move |req| f.reply(req)).await, fake)
}

fn count(n: &AtomicUsize) -> usize {
    n.load(Ordering::SeqCst)
}

/// A token_command that prints `secret` and adds a line to a file each time
/// it runs, so tests can count the runs. Returns the command and the file.
fn counting_command(name: &str, secret: &str) -> (serde_json::Value, PathBuf) {
    let file = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-runs"));
    let _ = std::fs::remove_file(&file);
    let script = format!("echo run >> '{}'; echo {secret}", file.display());
    (json!(["sh", "-c", script]), file)
}

fn runs(file: &PathBuf) -> usize {
    std::fs::read_to_string(file)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

#[tokio::test]
async fn pihole_logs_in_once_and_keeps_the_session() {
    let (base, fake) = serve_pihole(Some("hunter2")).await;
    let (command, file) = counting_command("pihole", "hunter2");
    let mut m = Pihole::default();
    m.init(json!({ "url": base, "token_command": command }))
        .await
        .unwrap();
    let v = m.render().await.unwrap();
    assert_eq!(v.status, Status::Ok);
    assert_eq!(v.summary, "blocking on · 20.5% of 48.2k queries blocked");
    assert_eq!(m.render().await.unwrap().summary, v.summary);
    assert_eq!(
        count(&fake.logins),
        1,
        "the second render reuses the session"
    );
    assert_eq!(runs(&file), 1, "the password command runs only to log in");

    // The session expires: the next render logs in again, once.
    *fake.sid.lock().unwrap() = None;
    assert_eq!(m.render().await.unwrap().summary, v.summary);
    assert_eq!(count(&fake.logins), 2);
    assert_eq!(runs(&file), 2);

    // A reload ends the session before starting over.
    m.init(json!({ "url": base, "token_command": command }))
        .await
        .unwrap();
    assert_eq!(count(&fake.logouts), 1);
}

#[tokio::test]
async fn pihole_without_a_password_on_the_server_logs_in_once() {
    let (base, fake) = serve_pihole(None).await;
    let mut m = Pihole::default();
    m.init(json!({ "url": base, "token_command": ["echo", "unused"] }))
        .await
        .unwrap();
    m.render().await.unwrap();
    m.render().await.unwrap();
    assert_eq!(
        count(&fake.logins),
        1,
        "no session is needed, so no new login"
    );
}

#[tokio::test]
async fn pihole_without_a_password_says_how_to_set_one() {
    let (base, _) = serve_pihole(Some("hunter2")).await;
    let mut m = Pihole::default();
    m.init(json!({ "url": base, "token_env": "DASHBOARD_TEST_NO_SUCH_PASSWORD" }))
        .await
        .unwrap();
    let err = m.render().await.unwrap_err().to_string();
    assert!(err.contains("PIHOLE_PASSWORD"), "{err}");

    let mut m = Pihole::default();
    m.init(json!({ "url": base, "token_command": ["echo", "wrong"] }))
        .await
        .unwrap();
    let err = m.render().await.unwrap_err().to_string();
    assert!(err.contains("rejected the password"), "{err}");
}

/// A fake Transmission that answers 409 with a session id until it gets one
/// back, and counts the 409s. With `auth` set, it wants that Authorization
/// header (lowercased).
#[derive(Default)]
struct FakeTransmission {
    auth: Option<&'static str>,
    conflicts: AtomicUsize,
}

impl FakeTransmission {
    fn reply(&self, req: &Request) -> Reply {
        if req.method != "POST" || req.path != "/transmission/rpc" {
            return ("404 Not Found", vec![], "{}".into());
        }
        if self.auth.is_some() && req.header("authorization") != self.auth {
            return (
                "401 Unauthorized",
                vec![],
                "<h1>401: Unauthorized</h1>".into(),
            );
        }
        if req.header("x-transmission-session-id") != Some("abc123") {
            self.conflicts.fetch_add(1, Ordering::SeqCst);
            return (
                "409 Conflict",
                vec!["X-Transmission-Session-Id: abc123".into()],
                "<h1>409: Conflict</h1>".into(),
            );
        }
        assert!(req.body.contains("\"torrent-get\""), "{}", req.body);
        ("200 OK", vec![], sample("transmission/torrent-get.json"))
    }
}

async fn serve_transmission(auth: Option<&'static str>) -> (String, Arc<FakeTransmission>) {
    let fake = Arc::new(FakeTransmission {
        auth,
        ..Default::default()
    });
    let f = fake.clone();
    (serve(move |req| f.reply(req)).await, fake)
}

#[tokio::test]
async fn transmission_does_the_session_handshake_once() {
    let (base, fake) = serve_transmission(None).await;
    let mut m = Transmission::default();
    m.init(json!({ "url": base, "limit": 3 })).await.unwrap();
    let v = m.render().await.unwrap();
    assert_eq!(v.status, Status::Error);
    assert!(
        v.summary.starts_with("1 downloading · 1 seeding"),
        "{}",
        v.summary
    );
    let Widget::Column { children } = &v.root else {
        panic!("column")
    };
    let Widget::List { items, .. } = &children[1] else {
        panic!("list")
    };
    assert_eq!(items.len(), 3);
    assert_eq!(m.render().await.unwrap().summary, v.summary);
    assert_eq!(
        count(&fake.conflicts),
        1,
        "later renders send the kept session id straight away"
    );
}

#[tokio::test]
async fn transmission_sends_basic_auth_and_reads_the_password_once() {
    // base64("me:s3cret")
    let (base, _) = serve_transmission(Some("basic bwu6cznjcmv0")).await;
    let (command, file) = counting_command("transmission", "s3cret");
    let mut m = Transmission::default();
    m.init(json!({ "url": base, "username": "me", "token_command": command }))
        .await
        .unwrap();
    m.render().await.unwrap();
    m.render().await.unwrap();
    assert_eq!(runs(&file), 1, "the password is kept between renders");

    let mut m = Transmission::default();
    m.init(json!({ "url": base, "username": "me", "token_command": ["echo", "wrong"] }))
        .await
        .unwrap();
    let err = m.render().await.unwrap_err().to_string();
    assert!(err.contains("rejected the login"), "{err}");
}

#[tokio::test]
async fn unreachable_service_is_an_error() {
    // Port 9 (discard) on localhost: nothing listens there in the test run.
    let mut m = Transmission::default();
    m.init(json!({ "url": "http://127.0.0.1:9" }))
        .await
        .unwrap();
    let err = format!("{:#}", m.render().await.unwrap_err());
    assert!(err.contains("reaching Transmission"), "{err}");
}
