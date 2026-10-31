//! Runs the TypeScript and Python feed reader plugins (`plugins/ext/feedreader-ts`,
//! `plugins/ext/feedreader-py`) through the real plugin host on the samples in
//! `crates/modules-fun/testdata/feedreader`, and checks each view against the
//! expected one. The native module's tests check the same files, so all three
//! show the same view for the same input. A plugin whose interpreter isn't
//! installed, or is too old, is skipped.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;

use dashboard_contract::check::{check_manifest, check_view};
use dashboard_contract::{Status, View, Widget};
use dashboard_host::{Module, Plugin};
use serde_json::json;

/// Single-feed samples: `<name>.xml`, expected `<name>.view.json`.
const SAMPLES: &[&str] = &["rss2", "atom", "rdf", "rss091", "demo"];

fn repo(path: &str) -> PathBuf {
    [env!("CARGO_MANIFEST_DIR"), "../..", path].iter().collect()
}

fn dir() -> PathBuf {
    repo("crates/modules-fun/testdata/feedreader")
}

fn data(name: &str) -> String {
    dir().join(name).to_string_lossy().into_owned()
}

/// `<name>.view.json`, with `$DIR` replaced by the samples folder (a feed with
/// no title is named by its path).
fn expected(name: &str) -> View {
    let text = std::fs::read_to_string(dir().join(format!("{name}.view.json"))).unwrap();
    let dir = serde_json::to_string(&data("")).unwrap();
    let dir = dir[1..dir.len() - 1].trim_end_matches('/');
    serde_json::from_str(&text.replace("$DIR", dir)).unwrap()
}

/// Whether the command runs and succeeds: checks the interpreter is there and
/// new enough, not just installed.
fn runs(argv: &[&str]) -> bool {
    Command::new(argv[0])
        .args(&argv[1..])
        .stdin(std::process::Stdio::null())
        .output()
        .is_ok_and(|out| out.status.success())
}

/// (id, argv) of each plugin whose interpreter is installed and new enough.
fn plugins() -> Vec<(&'static str, Vec<String>)> {
    let path = |p: &str| repo(p).to_string_lossy().into_owned();
    let mut out = vec![];
    // The SDK copy does nothing when run, so this checks that node can run a
    // .ts file (it strips types from Node 22.18 and 23.6 on).
    if runs(&[
        "node",
        &path("plugins/ext/feedreader-ts/dashboard-plugin.ts"),
    ]) {
        out.push((
            "feedreader-ts",
            vec!["node".into(), path("plugins/ext/feedreader-ts/main.ts")],
        ));
    }
    if runs(&[
        "python3",
        "-c",
        "import sys; sys.exit(sys.version_info < (3, 9))",
    ]) {
        out.push((
            "feedreader-py",
            vec!["python3".into(), path("plugins/ext/feedreader-py/main.py")],
        ));
    }
    out
}

fn problems(found: Vec<dashboard_contract::check::Finding>) -> Vec<String> {
    found
        .into_iter()
        .filter(|f| f.problem)
        .map(|f| f.text)
        .collect()
}

async fn render(argv: &[String], config: serde_json::Value) -> anyhow::Result<View> {
    let mut plugin = Plugin::new("test", argv.to_vec());
    plugin.init(config).await?;
    plugin.render().await
}

#[tokio::test]
async fn plugins_match_the_expected_views() {
    for (id, argv) in plugins() {
        for name in SAMPLES {
            let mut plugin = Plugin::new("test", argv.clone());
            let manifest = plugin
                .init(json!({ "feeds": [data(&format!("{name}.xml"))] }))
                .await
                .expect("initialize");
            assert_eq!(manifest.id, id);
            assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
            let view = plugin.render().await.expect("view/render");
            assert_eq!(problems(check_view(&view)), Vec::<String>::new());
            assert_eq!(view, expected(name), "{id} on {name}");
        }
        // Three good feeds and three that fail, cut to 8 items.
        let feeds: Vec<String> = [
            "rss2.xml",
            "atom.xml",
            "rdf.xml",
            "broken.xml",
            "notafeed.xml",
            "missing.xml",
        ]
        .iter()
        .map(|n| data(n))
        .collect();
        let view = render(&argv, json!({ "feeds": feeds, "max_items": 8 }))
            .await
            .unwrap();
        assert_eq!(view, expected("mixed"), "{id} on mixed");
    }
}

/// Removes its folder when dropped, so a failed test leaves nothing behind.
struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn plugins_parse_the_xml_cases_like_the_native_module() {
    // The native module checks the same table in its own tests.
    let text = std::fs::read_to_string(dir().join("xml-cases.json")).unwrap();
    let cases: Vec<serde_json::Value> = serde_json::from_str(&text).unwrap();
    let tmp =
        TempDir(std::env::temp_dir().join(format!("dashboard-feedreader-{}", std::process::id())));
    std::fs::create_dir_all(&tmp.0).unwrap();
    let mut feeds = vec![data("demo.xml")];
    for (i, case) in cases.iter().enumerate() {
        let path = tmp.0.join(format!("{i}.xml"));
        std::fs::write(&path, case["xml"].as_str().unwrap()).unwrap();
        feeds.push(path.to_string_lossy().into_owned());
    }
    for (id, argv) in plugins() {
        let view = render(&argv, json!({ "feeds": feeds })).await.unwrap();
        let Widget::List { items, .. } = view.root else {
            panic!("{id}: not a list");
        };
        for (case, path) in cases.iter().zip(&feeds[1..]) {
            let failed = items
                .iter()
                .find(|i| i.title == format!("Could not read {path}"));
            let got = failed.map_or("ok", |i| i.subtitle.as_deref().unwrap());
            assert_eq!(got, case["result"].as_str().unwrap(), "{id}: {case}");
        }
    }
}

/// Answers `GET /feed` with the rss2 sample and anything else with 404, for
/// `n` requests, on a thread.
fn serve(n: usize) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let body = std::fs::read(data("rss2.xml")).unwrap();
    std::thread::spawn(move || {
        for _ in 0..n {
            let Ok((mut sock, _)) = listener.accept() else {
                return;
            };
            let mut buf = vec![0; 4096];
            let len = sock.read(&mut buf).unwrap_or(0);
            let found = buf[..len].starts_with(b"GET /feed ");
            let head = match found {
                true => format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n", body.len()),
                false => "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n".into(),
            };
            let _ = sock.write_all(format!("{head}Connection: close\r\n\r\n").as_bytes());
            if found {
                let _ = sock.write_all(&body);
            }
        }
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn plugins_fetch_over_http() {
    for (id, argv) in plugins() {
        let base = serve(2);
        let feeds = [format!("{base}/feed"), format!("{base}/gone")];
        let view = render(&argv, json!({ "feeds": feeds })).await.unwrap();
        assert_eq!(view.status, Status::Warn, "{id}");
        assert_eq!(view.summary, "7 items from 1 feed · 1 failed", "{id}");
        let Widget::List { items, .. } = view.root else {
            panic!("{id}: not a list");
        };
        assert_eq!(items[0].title, format!("Could not read {base}/gone"));
        assert_eq!(items[0].subtitle.as_deref(), Some("HTTP 404"), "{id}");
        assert_eq!(items[1].title, "An item with a permalink guid", "{id}");
    }
}

#[tokio::test]
async fn plugins_reject_bad_config() {
    for (id, argv) in plugins() {
        for bad in [
            json!({}),
            json!({ "feeds": [] }),
            json!({ "feeds": "x" }),
            json!({ "feeds": [1] }),
            json!({ "feeds": ["x"], "max_items": 0 }),
            json!({ "feeds": ["x"], "max_items": 2.5 }),
            json!({ "feeds": ["x"], "max_items": -1 }),
            json!({ "feeds": ["x"], "maxitems": 3 }),
        ] {
            let err = Plugin::new("test", argv.clone())
                .init(bad.clone())
                .await
                .unwrap_err();
            let err = format!("{err:#}");
            assert!(err.contains("code -32002"), "{id} on {bad}: {err}");
        }
    }
}

#[tokio::test]
async fn no_readable_feed_is_an_upstream_error() {
    for (id, argv) in plugins() {
        let config = json!({ "feeds": [data("missing.xml"), data("broken.xml")] });
        let err = format!("{:#}", render(&argv, config).await.unwrap_err());
        assert!(err.contains("code -32001"), "{id}: {err}");
        assert!(
            err.contains("missing.xml: could not read the file"),
            "{id}: {err}"
        );
    }
}

#[test]
fn sdk_copies_match_the_sdk() {
    // Each package carries its own copy of the SDK, so it runs wherever it is
    // installed. Keep them in step with sdk/.
    for (copy, sdk) in [
        (
            "plugins/ext/feedreader-ts/dashboard-plugin.ts",
            "sdk/typescript/dashboard-plugin.ts",
        ),
        (
            "plugins/ext/feedreader-py/dashboard_plugin.py",
            "sdk/python/dashboard_plugin.py",
        ),
    ] {
        let a = std::fs::read_to_string(repo(copy)).unwrap();
        let b = std::fs::read_to_string(repo(sdk)).unwrap();
        assert!(a == b, "{copy} differs from {sdk}: copy it again");
    }
}
