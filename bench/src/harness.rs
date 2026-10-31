//! The native-vs-plugin benchmark, run side: starts each benchmark widget
//! built in and as a plugin (TypeScript, Python, WASM) and measures startup,
//! render latency, memory and CPU. Build sizes and build times are measured by
//! `bench/build.sh`. See bench/README.md; the results are in
//! docs/modules.md.
//!
//! A built-in module runs in a child process of this program (`native-child`),
//! so each one is measured on its own, the way the dashboard would run it: in
//! process, on a tokio runtime. Plugins are spawned the way the host spawns
//! them and spoken to over stdin and stdout.
//!
//! No widget calls the internet: HTTP widgets read from a small local server
//! that serves the recorded samples, Hacker News reads its sample file, and
//! docker runs the fake docker program from the tests.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

const USAGE: &str = "\
usage: dashboard-bench [options]

  --only <ids>      widgets to measure, comma-separated (default: all 8)
  --impls <names>   native,ts,py,wasm,wasm-aot (default: all; wasm only for clock and textfile)
  --renders <n>     renders timed per widget (default 100)
  --starts <n>      cold starts timed per plugin (default 10)
  --idle <secs>     idle time measured after start, for idle CPU (default 20)
  --live <secs>     for widgets that refresh every 5 s or faster, also run them at
                    their real refresh this long and measure CPU (default 60; 0 skips)
  --out <file>      JSON results (default bench/results/runtime.json)";

/// One benchmark widget.
struct Widget {
    id: &'static str,
    /// The TypeScript and Python plugin entry files, under plugins/ext.
    ts: &'static str,
    py: &'static str,
    /// Built for WASI too, in bench/wasm.
    wasm: bool,
}

const WIDGETS: &[Widget] = &[
    Widget {
        id: "clock",
        ts: "clock-ts/main.ts",
        py: "clock-py/main.py",
        wasm: true,
    },
    Widget {
        id: "textfile",
        ts: "textfile-ts/main.ts",
        py: "textfile-py/main.py",
        wasm: true,
    },
    Widget {
        id: "git",
        ts: "git-ts/main.ts",
        py: "git-py/git_py.py",
        wasm: false,
    },
    Widget {
        id: "docker",
        ts: "docker-ts/main.ts",
        py: "docker-py/main.py",
        wasm: false,
    },
    Widget {
        id: "hackernews",
        ts: "hackernews-ts/main.ts",
        py: "hackernews-py/main.py",
        wasm: false,
    },
    Widget {
        id: "weather",
        ts: "weather-ts/main.ts",
        py: "weather-py/main.py",
        wasm: false,
    },
    Widget {
        id: "feedreader",
        ts: "feedreader-ts/main.ts",
        py: "feedreader-py/main.py",
        wasm: false,
    },
    Widget {
        id: "battery",
        ts: "battery-ts/main.ts",
        py: "battery-py/main.py",
        wasm: false,
    },
];

const IMPLS: &[&str] = &["native", "ts", "py", "wasm", "wasm-aot"];

/// The same config for every implementation. Paths are absolute, since
/// plugins and the native child all run in the repository root anyway.
fn config(id: &str, repo: &Path, port: u16) -> Value {
    let path = |p: &str| repo.join(p).to_string_lossy().into_owned();
    let local = format!("http://127.0.0.1:{port}");
    match id {
        // Named zones, not `local`: a WASI program can't see the machine's zone.
        "clock" => json!({ "zones": ["UTC", "America/New_York", "Europe/Berlin", "Asia/Tokyo"] }),
        "textfile" => json!({ "path": path("crates/modules-system/testdata/textfile/notes.md") }),
        "git" => json!({ "repos": [path(".")] }),
        "docker" => {
            json!({ "docker": path("crates/modules-ops/testdata/docker/docker"), "stats": true })
        }
        "hackernews" => {
            json!({ "sample": path("crates/modules-fun/testdata/hackernews/top.json") })
        }
        "weather" => json!({
            "latitude": 52.52,
            "longitude": 13.41,
            "name": "Berlin",
            "forecast_url": format!("{local}/v1/forecast"),
        }),
        "feedreader" => {
            json!({ "feeds": [format!("{local}/rss2.xml"), format!("{local}/atom.xml")] })
        }
        _ => json!({}),
    }
}

struct Options {
    only: Vec<String>,
    impls: Vec<String>,
    renders: usize,
    starts: usize,
    idle: Duration,
    live: Duration,
    out: PathBuf,
}

pub fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("native-child") {
        return native_child(&args[1..]);
    }
    let options = parse(&args)?;
    let repo: PathBuf = [env!("CARGO_MANIFEST_DIR"), ".."].iter().collect();
    let repo = repo.canonicalize()?;
    let port = serve_samples(&repo)?;

    let mut rows = vec![];
    for widget in WIDGETS {
        if !options.only.is_empty() && !options.only.iter().any(|o| o == widget.id) {
            continue;
        }
        for name in &options.impls {
            let Some(argv) = argv(widget, name, &repo) else {
                continue;
            };
            eprintln!("== {} {name}", widget.id);
            let config = config(widget.id, &repo, port);
            let row = if name == "native" {
                measure_native(widget.id, &config, &options)
            } else {
                measure_plugin(&argv, &repo, &config, &options)
            };
            match row {
                Ok(mut row) => {
                    row["widget"] = widget.id.into();
                    row["impl"] = name.as_str().into();
                    eprintln!("   {}", summary(&row));
                    rows.push(row);
                }
                Err(err) => eprintln!("   failed: {err:#}"),
            }
        }
    }

    let report = json!({
        "machine": machine(),
        "renders": options.renders,
        "starts": options.starts,
        "idle_secs": options.idle.as_secs_f64(),
        "live_secs": options.live.as_secs_f64(),
        "rows": rows,
    });
    if let Some(dir) = options.out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&options.out, serde_json::to_string_pretty(&report)? + "\n")?;
    print!("{}", table(&rows));
    eprintln!("wrote {}", options.out.display());
    Ok(())
}

fn parse(args: &[String]) -> Result<Options> {
    let mut o = Options {
        only: vec![],
        impls: IMPLS.iter().map(|s| s.to_string()).collect(),
        renders: 100,
        starts: 10,
        idle: Duration::from_secs(20),
        live: Duration::from_secs(60),
        out: [env!("CARGO_MANIFEST_DIR"), "results", "runtime.json"]
            .iter()
            .collect(),
    };
    let list = |s: &str| s.split(',').map(String::from).collect::<Vec<_>>();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = || {
            it.next()
                .with_context(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--only" => o.only = list(value()?),
            "--impls" => o.impls = list(value()?),
            "--renders" => o.renders = value()?.parse()?,
            "--starts" => o.starts = value()?.parse()?,
            "--idle" => o.idle = Duration::from_secs_f64(value()?.parse()?),
            "--live" => o.live = Duration::from_secs_f64(value()?.parse()?),
            "--out" => o.out = value()?.into(),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            _ => bail!("unknown argument {arg}\n{USAGE}"),
        }
    }
    for name in &o.impls {
        if !IMPLS.contains(&name.as_str()) {
            bail!("unknown implementation {name}\n{USAGE}");
        }
    }
    if o.renders == 0 || o.starts == 0 {
        bail!("--renders and --starts must be at least 1");
    }
    Ok(o)
}

/// The command that runs this implementation of the widget, or None when it
/// has none. Native returns an empty argv.
fn argv(widget: &Widget, name: &str, repo: &Path) -> Option<Vec<String>> {
    let ext = |p: &str| {
        repo.join("plugins/ext")
            .join(p)
            .to_string_lossy()
            .into_owned()
    };
    let wasm = |suffix: &str| {
        repo.join("bench/wasm/target/wasm32-wasip1/release")
            .join(format!("{}.{suffix}", widget.id))
            .to_string_lossy()
            .into_owned()
    };
    // The guest sees the repository at the same path, and the zone database.
    let dirs = |file: String, extra: &[&str]| {
        let root = repo.to_string_lossy();
        let mut v: Vec<String> = vec!["wasmtime".into(), "run".into()];
        v.extend(extra.iter().map(|s| s.to_string()));
        v.extend([
            "--dir".into(),
            format!("{root}::{root}"),
            "--dir".into(),
            "/usr/share/zoneinfo::/usr/share/zoneinfo".into(),
            file,
        ]);
        v
    };
    match name {
        "native" => Some(vec![]),
        "ts" => Some(vec!["node".into(), ext(widget.ts)]),
        "py" => Some(vec!["python3".into(), ext(widget.py)]),
        "wasm" if widget.wasm => Some(dirs(wasm("wasm"), &[])),
        "wasm-aot" if widget.wasm => Some(dirs(wasm("cwasm"), &["--allow-precompiled"])),
        _ => None,
    }
}

// ---- Plugins ----

/// How long to wait for one reply before giving up on a plugin.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);

/// A running plugin. Dropped without `finish` (after an error), it is killed
/// and waited for, so no plugin outlives the run.
struct Proc {
    child: Child,
    stdin: Option<ChildStdin>,
    /// Lines of stdout, read on their own thread so a reply can time out.
    lines: Receiver<String>,
    next_id: u64,
    /// Set once `finish` has reaped the process.
    reaped: bool,
}

/// What a finished process used, it and everything it started.
struct Usage {
    cpu: f64,
    max_rss_kb: u64,
}

impl Proc {
    fn spawn(argv: &[String], dir: &Path) -> Result<Self> {
        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("starting {argv:?}"))?;
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().expect("piped stdout"));
        let (tx, lines) = std::sync::mpsc::channel();
        // Ends when stdout closes, that is when the plugin exits.
        std::thread::spawn(move || {
            for line in stdout.lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            lines,
            next_id: 1,
            reaped: false,
        })
    }

    /// Send a request and wait for its reply. An error reply is an Err.
    fn call(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut req = json!({ "jsonrpc": "2.0", "id": id, "method": method });
        if let Some(params) = params {
            req["params"] = params;
        }
        let stdin = self.stdin.as_mut().context("stdin closed")?;
        writeln!(stdin, "{req}")?;
        stdin.flush()?;
        let deadline = Instant::now() + REPLY_TIMEOUT;
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            let line = match self.lines.recv_timeout(wait) {
                Ok(line) => line,
                Err(RecvTimeoutError::Timeout) => {
                    bail!("no reply to {method} in {REPLY_TIMEOUT:?}")
                }
                Err(RecvTimeoutError::Disconnected) => bail!("plugin exited"),
            };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue;
            };
            // Skip notifications (log) and anything else that isn't our reply.
            if msg.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(err) = msg.get("error") {
                bail!("{err}");
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    fn initialize(&mut self, config: &Value) -> Result<Value> {
        let params = json!({
            "contract_version": dashboard_contract::CONTRACT_VERSION,
            "host": { "name": "dashboard-bench", "version": env!("CARGO_PKG_VERSION") },
            "config": config,
        });
        let result = self.call("initialize", Some(params))?;
        result.get("manifest").cloned().context("no manifest")
    }

    fn rss_kb(&self) -> Option<u64> {
        rss_kb(self.child.id())
    }

    /// CPU seconds the process itself has used so far.
    fn cpu(&self) -> f64 {
        pid_cpu(self.child.id()).unwrap_or(0.0)
    }

    /// Close stdin, wait for the process to exit (killing it after 5 s) and
    /// return what it used.
    fn finish(mut self) -> Usage {
        self.stdin = None;
        let pid = self.child.id() as libc::pid_t;
        let before = children_cpu();
        let start = Instant::now();
        let mut status = 0;
        // SAFETY: rusage is plain data; wait4 fills it in.
        let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
        loop {
            // SAFETY: waiting on our own child, with valid pointers.
            let r = unsafe { libc::wait4(pid, &mut status, libc::WNOHANG, &mut ru) };
            if r == pid || r < 0 {
                break;
            }
            if start.elapsed() > Duration::from_secs(5) {
                let _ = self.child.kill();
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        // Reaped by wait4 (or gone): Drop must not signal the pid again.
        self.reaped = true;
        Usage {
            cpu: children_cpu() - before,
            max_rss_kb: max_rss_kb(&ru),
        }
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn measure_plugin(argv: &[String], repo: &Path, config: &Value, o: &Options) -> Result<Value> {
    // CPU is read from the live process before and after each phase, so the
    // interpreter's own start and exit don't blur the small numbers. Commands
    // the plugin runs (git, docker, pmset) only show up once it has exited:
    // they are the "tail" (exit total minus the last live reading) beyond what
    // a plugin that only started and exited leaves.

    // Cold starts: spawn to `initialize` reply, then exit.
    let mut starts = vec![];
    let mut start_cpu = vec![];
    let mut tails = vec![];
    let mut manifest = Value::Null;
    for _ in 0..o.starts {
        let t = Instant::now();
        let mut p = Proc::spawn(argv, repo)?;
        manifest = p.initialize(config)?;
        starts.push(ms(t.elapsed()));
        let live = p.cpu();
        let total = p.finish().cpu;
        start_cpu.push(live);
        tails.push(total - live);
    }
    let start_cpu = mean(&start_cpu);
    let tail = mean(&tails);

    // Renders, back to back.
    let mut p = Proc::spawn(argv, repo)?;
    p.initialize(config)?;
    let cpu0 = p.cpu();
    let t = Instant::now();
    let first = p.call("view/render", None);
    let first_ms = ms(t.elapsed());
    let mut errors = usize::from(first.is_err());
    let mut renders = vec![];
    for _ in 0..o.renders {
        let t = Instant::now();
        errors += usize::from(p.call("view/render", None).is_err());
        renders.push(ms(t.elapsed()));
    }
    let cpu1 = p.cpu();
    let rss = p.rss_kb();
    let used = p.finish();
    let commands = (used.cpu - cpu1 - tail).max(0.0);
    let render_cpu = (cpu1 - cpu0 + commands) / (o.renders + 1) as f64;

    // Idle: started, then left alone.
    let mut p = Proc::spawn(argv, repo)?;
    p.initialize(config)?;
    let cpu0 = p.cpu();
    std::thread::sleep(o.idle);
    let idle_cpu = (p.cpu() - cpu0) / o.idle.as_secs_f64();
    let idle_rss = p.rss_kb();
    p.finish();

    let interval = interval_secs(&manifest);
    let mut row = json!({
        "startup_ms": stats(&starts),
        "first_render_ms": first_ms,
        "render_ms": stats(&renders),
        "errors": errors,
        "rss_kb": rss,
        "idle_rss_kb": idle_rss,
        "max_rss_kb": used.max_rss_kb,
        "startup_cpu_ms": start_cpu * 1000.0,
        "render_cpu_ms": render_cpu * 1000.0,
        "idle_cpu_ms_per_s": idle_cpu * 1000.0,
        "interval_s": interval,
        "cpu_s_per_hour": per_hour(idle_cpu, render_cpu, interval),
    });
    if interval <= 5.0 && !o.live.is_zero() {
        let mut p = Proc::spawn(argv, repo)?;
        p.initialize(config)?;
        let cpu0 = p.cpu();
        live(o.live, interval, || {
            let _ = p.call("view/render", None);
        });
        let cpu1 = p.cpu();
        let commands = (p.finish().cpu - cpu1 - tail).max(0.0);
        let cpu = cpu1 - cpu0 + commands;
        row["live_cpu_s_per_hour"] = (cpu / o.live.as_secs_f64() * 3600.0).into();
    }
    Ok(row)
}

/// Render at the widget's refresh for `total`.
fn live(total: Duration, interval: f64, mut render: impl FnMut()) {
    let start = Instant::now();
    let mut next = start;
    while start.elapsed() < total {
        render();
        next += Duration::from_secs_f64(interval);
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
    }
}

// ---- Built-in modules ----

/// Run `native-child` once and read its JSON reply.
fn measure_native(id: &str, config: &Value, o: &Options) -> Result<Value> {
    let exe = std::env::current_exe()?;
    let out = Command::new(exe)
        .args([
            "native-child",
            id,
            &config.to_string(),
            &o.renders.to_string(),
            &o.idle.as_secs_f64().to_string(),
            &o.live.as_secs_f64().to_string(),
        ])
        .stderr(Stdio::inherit())
        .output()?;
    if !out.status.success() {
        bail!("native-child failed");
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// In a fresh process: init the built-in module, time its renders, then idle,
/// and print the numbers as JSON.
fn native_child(args: &[String]) -> Result<()> {
    let [id, config, renders, idle, live_secs] = args else {
        bail!("native-child <id> <config> <renders> <idle-secs> <live-secs>");
    };
    let config: Value = serde_json::from_str(config)?;
    let renders: usize = renders.parse()?;
    let idle = Duration::from_secs_f64(idle.parse()?);
    let live_for = Duration::from_secs_f64(live_secs.parse()?);
    // Multi-threaded, like the dashboard's own runtime.
    let rt = tokio::runtime::Runtime::new()?;
    let row = rt.block_on(async {
        // Let the runtime's threads settle before the baseline.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let rss_before = rss_kb(std::process::id());
        let t = Instant::now();
        let mut module = dashboard_modules::builtin(id).context("not in this build")?;
        let manifest = module.init(config.clone()).await?;
        let init_ms = ms(t.elapsed());
        let cpu0 = self_cpu();
        let t = Instant::now();
        let mut errors = usize::from(module.render().await.is_err());
        let first_ms = ms(t.elapsed());
        let mut times = vec![];
        for _ in 0..renders {
            let t = Instant::now();
            errors += usize::from(module.render().await.is_err());
            times.push(ms(t.elapsed()));
        }
        let render_cpu = (self_cpu() - cpu0) / (renders + 1) as f64;
        let rss_after = rss_kb(std::process::id());
        let cpu1 = self_cpu();
        tokio::time::sleep(idle).await;
        let idle_cpu = (self_cpu() - cpu1) / idle.as_secs_f64();
        let manifest = serde_json::to_value(&manifest)?;
        let interval = interval_secs(&manifest);
        let mut row = json!({
            "startup_ms": stats(&[init_ms]),
            "first_render_ms": first_ms,
            "render_ms": stats(&times),
            "errors": errors,
            "rss_kb": rss_after,
            "process_rss_before_kb": rss_before,
            "rss_delta_kb": rss_after.zip(rss_before).map(|(a, b)| a as i64 - b as i64),
            "render_cpu_ms": render_cpu * 1000.0,
            "idle_cpu_ms_per_s": idle_cpu * 1000.0,
            "interval_s": interval,
            "cpu_s_per_hour": per_hour(idle_cpu, render_cpu, interval),
        });
        if interval <= 5.0 && !live_for.is_zero() {
            let cpu = self_cpu();
            let start = Instant::now();
            let mut next = start;
            while start.elapsed() < live_for {
                let _ = module.render().await;
                next += Duration::from_secs_f64(interval);
                if let Some(wait) = next.checked_duration_since(Instant::now()) {
                    tokio::time::sleep(wait).await;
                }
            }
            row["live_cpu_s_per_hour"] =
                ((self_cpu() - cpu) / live_for.as_secs_f64() * 3600.0).into();
        }
        anyhow::Ok(row)
    })?;
    println!("{row}");
    Ok(())
}

// ---- Local sample server ----

/// Serve the recorded HTTP samples on 127.0.0.1, so HTTP widgets do real HTTP
/// with no internet. Returns the port.
fn serve_samples(repo: &Path) -> Result<u16> {
    let files: Vec<(&str, &str, Vec<u8>)> = [
        (
            "/v1/forecast",
            "application/json",
            "crates/modules-personal/testdata/weather/forecast.json",
        ),
        (
            "/rss2.xml",
            "application/rss+xml",
            "crates/modules-fun/testdata/feedreader/rss2.xml",
        ),
        (
            "/atom.xml",
            "application/atom+xml",
            "crates/modules-fun/testdata/feedreader/atom.xml",
        ),
    ]
    .into_iter()
    .map(|(path, kind, file)| Ok((path, kind, std::fs::read(repo.join(file))?)))
    .collect::<Result<_>>()?;
    let files: &'static [(&str, &str, Vec<u8>)] = Box::leak(files.into_boxed_slice());
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let _ = answer(stream, files);
            });
        }
    });
    Ok(port)
}

fn answer(mut stream: TcpStream, files: &[(&str, &str, Vec<u8>)]) -> Result<()> {
    let mut buf = vec![];
    let mut chunk = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let head = String::from_utf8_lossy(&buf);
    let target = head.split_whitespace().nth(1).unwrap_or("/");
    let path = target.split('?').next().unwrap_or(target);
    match files.iter().find(|(p, _, _)| *p == path) {
        Some((_, kind, body)) => {
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )?;
            stream.write_all(body)?;
        }
        None => write!(
            stream,
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
        )?,
    }
    Ok(())
}

// ---- Numbers ----

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// p50, p95 and max of a list of milliseconds.
fn stats(v: &[f64]) -> Value {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    let pick = |q: f64| v[((v.len() as f64 * q).ceil() as usize).clamp(1, v.len()) - 1];
    json!({ "p50": pick(0.5), "p95": pick(0.95), "max": pick(1.0), "n": v.len() })
}

/// CPU seconds per hour: idle use plus one render per refresh.
fn per_hour(idle_cpu_per_s: f64, render_cpu: f64, interval: f64) -> f64 {
    idle_cpu_per_s * 3600.0 + render_cpu * 3600.0 / interval
}

fn interval_secs(manifest: &Value) -> f64 {
    manifest["refresh"]["interval_ms"]
        .as_f64()
        .unwrap_or(5000.0)
        / 1000.0
}

fn cpu_of(ru: &libc::rusage) -> f64 {
    let t = |tv: libc::timeval| tv.tv_sec as f64 + tv.tv_usec as f64 / 1e6;
    t(ru.ru_utime) + t(ru.ru_stime)
}

fn usage(who: libc::c_int) -> libc::rusage {
    // SAFETY: rusage is plain data; getrusage fills it in.
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(who, &mut ru) };
    ru
}

/// CPU seconds used by finished children (and their own finished children).
fn children_cpu() -> f64 {
    cpu_of(&usage(libc::RUSAGE_CHILDREN))
}

/// CPU seconds used by this process and its finished children: a built-in
/// module's commands (git, docker, pmset) count too.
fn self_cpu() -> f64 {
    cpu_of(&usage(libc::RUSAGE_SELF)) + children_cpu()
}

/// CPU seconds a running process has used, not counting its children.
#[cfg(target_os = "macos")]
#[allow(deprecated)] // libc's mach_timebase_info: fine for this.
fn pid_cpu(pid: u32) -> Option<f64> {
    // SAFETY: plain data, filled in by the calls.
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let mut base: libc::mach_timebase_info = unsafe { std::mem::zeroed() };
    let ok = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V2,
            &mut info as *mut _ as *mut libc::rusage_info_t,
        ) == 0
            && libc::mach_timebase_info(&mut base) == 0
    };
    // The times are in Mach ticks, not nanoseconds, on Apple silicon.
    let ticks = (info.ri_user_time + info.ri_system_time) as f64;
    ok.then(|| ticks * base.numer as f64 / base.denom as f64 / 1e9)
}

/// CPU seconds a running process has used, not counting its children.
#[cfg(not(target_os = "macos"))]
fn pid_cpu(pid: u32) -> Option<f64> {
    // The first field is the time on CPU, in nanoseconds.
    let text = std::fs::read_to_string(format!("/proc/{pid}/schedstat")).ok()?;
    let ns: f64 = text.split_whitespace().next()?.parse().ok()?;
    Some(ns / 1e9)
}

fn max_rss_kb(ru: &libc::rusage) -> u64 {
    // Bytes on macOS, kilobytes on Linux.
    if cfg!(target_os = "macos") {
        ru.ru_maxrss as u64 / 1024
    } else {
        ru.ru_maxrss as u64
    }
}

/// Resident memory of a process now, from `ps`.
fn rss_kb(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

fn machine() -> Value {
    let run = |cmd: &str, args: &[&str]| {
        Command::new(cmd)
            .args(args)
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    };
    json!({
        "uname": run("uname", &["-mrs"]),
        "cpu": run("sysctl", &["-n", "machdep.cpu.brand_string"]),
        "load": run("sysctl", &["-n", "vm.loadavg"]),
        "node": run("node", &["--version"]),
        "python": run("python3", &["--version"]),
        "wasmtime": run("wasmtime", &["--version"]),
    })
}

fn summary(row: &Value) -> String {
    format!(
        "start p50 {:.1} ms, render p50 {:.3} / p95 {:.3} ms, rss {} KB, {:.2} cpu s/h, errors {}",
        row["startup_ms"]["p50"].as_f64().unwrap_or(0.0),
        row["render_ms"]["p50"].as_f64().unwrap_or(0.0),
        row["render_ms"]["p95"].as_f64().unwrap_or(0.0),
        row["rss_kb"],
        row["cpu_s_per_hour"].as_f64().unwrap_or(0.0),
        row["errors"],
    )
}

/// The results as a markdown table.
fn table(rows: &[Value]) -> String {
    let mut s = String::from(
        "| Widget | Impl | Startup p50 ms | First render ms | Render p50 ms | Render p95 ms | RSS MB | CPU ms/render | Idle CPU ms/s | Refresh s | CPU s/hour | Live CPU s/hour | Errors |\n\
         |---|---|---|---|---|---|---|---|---|---|---|---|---|\n",
    );
    let f = |v: &Value, digits: usize| match v.as_f64() {
        Some(x) => format!("{x:.digits$}"),
        None => "-".into(),
    };
    for r in rows {
        // For a built-in module, the memory it adds to the dashboard's process.
        let rss = if r["impl"] == "native" {
            &r["rss_delta_kb"]
        } else {
            &r["rss_kb"]
        };
        let mb = rss
            .as_f64()
            .map(|kb| json!(kb / 1024.0))
            .unwrap_or(Value::Null);
        s += &format!(
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            r["widget"].as_str().unwrap_or(""),
            r["impl"].as_str().unwrap_or(""),
            f(&r["startup_ms"]["p50"], 2),
            f(&r["first_render_ms"], 2),
            f(&r["render_ms"]["p50"], 3),
            f(&r["render_ms"]["p95"], 3),
            f(&mb, 1),
            f(&r["render_cpu_ms"], 3),
            f(&r["idle_cpu_ms_per_s"], 3),
            f(&r["interval_s"], 0),
            f(&r["cpu_s_per_hour"], 2),
            f(&r["live_cpu_s_per_hour"], 2),
            r["errors"],
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_picks_nearest_rank_percentiles() {
        let v: Vec<f64> = (1..=100).rev().map(f64::from).collect();
        let s = stats(&v);
        assert_eq!(s["p50"], 50.0);
        assert_eq!(s["p95"], 95.0);
        assert_eq!(s["max"], 100.0);
        assert_eq!(s["n"], 100);
    }

    #[test]
    fn stats_of_few_values() {
        let s = stats(&[3.0]);
        assert_eq!(
            (s["p50"].as_f64(), s["max"].as_f64()),
            (Some(3.0), Some(3.0))
        );
        let s = stats(&[2.0, 1.0]);
        assert_eq!(
            (s["p50"].as_f64(), s["p95"].as_f64()),
            (Some(1.0), Some(2.0))
        );
    }

    #[test]
    fn per_hour_adds_idle_and_one_render_per_refresh() {
        // 1 ms/s idle = 3.6 s; 2 ms per render every 5 s = 720 renders = 1.44 s.
        let h = per_hour(0.001, 0.002, 5.0);
        assert!((h - 5.04).abs() < 1e-9, "{h}");
        assert_eq!(per_hour(0.0, 0.0, 60.0), 0.0);
    }

    #[test]
    fn interval_defaults_to_five_seconds() {
        assert_eq!(
            interval_secs(&json!({ "refresh": { "interval_ms": 60000 } })),
            60.0
        );
        assert_eq!(interval_secs(&json!({})), 5.0);
    }
}
