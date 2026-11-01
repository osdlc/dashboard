use std::io;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use dashboard_cli_kit::Failure;
use dashboard_cli_kit::output::{Reporter, write_json};
use dashboard_config::layers::Sources;
use dashboard_config::packages::Packages;
use dashboard_config::{Config, Part};
use dashboard_contract::{CatalogEntry, Index, Manifest, Package, ServerMessage, View, rpc};
use dashboard_runtime::{load_valid, missing_packages, start};
use serde_json::json;

mod background;
mod dashboards;
mod marketplace;
mod plugin_check;
mod plugin_new;
mod widgets;

const NAME: &str = "dashboard";

#[derive(Parser)]
#[command(
    name = NAME,
    version,
    about = "Modular developer dashboard",
    after_long_help = exit_codes()
)]
struct Cli {
    /// Config file. Default: ~/.config/dashboard/config.yaml (or $DASHBOARD_MACHINE_CONFIG).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Lay this saved dashboard over the config, e.g. `--dashboard work`.
    /// See `dashboard dashboards list`.
    #[arg(long, global = true, value_name = "NAME")]
    dashboard: Option<String>,
    /// Don't read a `.dashboard.yaml` from this folder or the ones above it.
    #[arg(long, global = true)]
    no_project: bool,
    /// Print one JSON document on stdout (`schemaVersion`, `kind`), or an
    /// error document (`kind: "error"`) on failure. Commands that run until
    /// stopped refuse it.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

/// The exit codes, for the end of `--help`: every code in
/// `dashboard_cli_kit::exit` (docs/cli.md).
fn exit_codes() -> String {
    use dashboard_cli_kit::exit;
    format!(
        "Exit codes: {} ok, {} failed, {} the command can't run as typed, \
         {} a program it needs is not installed, {} a service refused the request or gave \
         an unusable answer, {} no repository or remote to work on, {} a service could not \
         be reached or limited the request.",
        exit::OK,
        exit::ERROR,
        exit::USAGE,
        exit::MISSING_TOOL,
        exit::SERVICE,
        exit::REPOSITORY,
        exit::UNREACHABLE,
    )
}

#[derive(Subcommand)]
enum Command {
    /// Show the dashboard in the terminal.
    Tui,
    /// Serve the web view on 127.0.0.1 and open it in the browser.
    Serve {
        #[arg(long, default_value_t = 7878)]
        port: u16,
        /// Also allow this origin (for the Vite dev server), e.g. http://localhost:5173
        #[arg(long)]
        dev_origin: Option<String>,
        /// Run in the background and return. `dashboard stop` stops it.
        #[arg(short, long)]
        detach: bool,
        /// Don't open a browser.
        #[arg(long)]
        no_open: bool,
        /// Write the background record once listening (set by --detach).
        #[arg(long, hide = true)]
        record: bool,
    },
    /// Stop the running web server.
    Stop,
    /// Show whether a web server is running, and its URL.
    Status,
    /// Open the running web server in the browser with a new sign-in link.
    Open {
        /// Print the link instead of opening a browser.
        #[arg(long)]
        no_open: bool,
    },
    /// Show the config files in use and whether they are valid.
    Config,
    /// Saved dashboards: `dashboard dashboards list`.
    Dashboards {
        #[command(subcommand)]
        command: DashboardsCommand,
    },
    /// Save a dashboard from a file or an https URL, and list what it needs
    /// and the programs it runs.
    Import {
        /// A dashboard file, or an https URL of one.
        from: String,
        /// Save it under this name. Default: from the file name.
        #[arg(long)]
        name: Option<String>,
        /// Replace a saved dashboard of the same name.
        #[arg(long)]
        force: bool,
    },
    /// Allow the project file (`.dashboard.yaml`) here or above to be used.
    /// It can run programs, so read it first. Any change to it needs a new
    /// `dashboard trust`.
    Trust {
        /// The project file. Default: the one found from this folder upward.
        file: Option<PathBuf>,
        /// Take it off the trusted list instead.
        #[arg(long)]
        remove: bool,
    },
    /// Write a saved dashboard to FILE, or to stdout.
    Export {
        name: String,
        file: Option<PathBuf>,
        /// Replace FILE if it exists.
        #[arg(long)]
        force: bool,
    },
    /// List everything that can be added to a dashboard: built-in modules,
    /// bundled plugins and installed plugin packages.
    Widgets {
        /// Words to look for in the name, ID, description and tags (case-insensitive).
        query: Vec<String>,
        /// Only entries with this tag.
        #[arg(long)]
        tag: Option<String>,
    },
    /// Print a JSON Schema for the config file or part of the contract.
    Schema {
        #[arg(value_enum)]
        which: SchemaKind,
    },
    /// Run a stdio plugin the way the dashboard does, check what it sends,
    /// and print its tile. For plugin authors:
    /// `dashboard plugin-check -- python3 my_plugin.py`
    PluginCheck {
        /// The tile's `config:` block passed to `initialize`, as JSON.
        #[arg(long = "plugin-config", value_name = "JSON")]
        plugin_config: Option<String>,
        /// How many times to call `view/render`.
        #[arg(long, default_value_t = 3)]
        renders: u32,
        /// Wait between renders in ms. Default: the plugin's interval, at most 1s.
        #[arg(long)]
        wait_ms: Option<u64>,
        /// Width of the printed tile.
        #[arg(long, default_value_t = 80)]
        width: u16,
        /// Height of the printed tile. Default: what the whole view needs. On a
        /// dashboard a small tile is 5 rows, medium and wide 10, large 15, tall 20.
        #[arg(long)]
        height: Option<u16>,
        /// The plugin's program and arguments, after `--`.
        #[arg(required = true, last = true)]
        argv: Vec<String>,
    },
    /// Plugin tools: `dashboard plugin new --lang ts my-plugin`.
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
}

#[derive(Subcommand)]
enum DashboardsCommand {
    /// List saved dashboards (`*` marks the one chosen with --dashboard).
    List,
}

#[derive(Subcommand)]
enum PluginCommand {
    /// Make a new plugin folder from a template, with the SDK copied in.
    New {
        #[arg(long, value_enum)]
        lang: plugin_new::Lang,
        /// The plugin's ID: lowercase letters, digits and dashes.
        name: String,
        /// Where to put it. Default: ./NAME
        dir: Option<PathBuf>,
    },
    /// Install a plugin package: by ID from an index, or from a .tar.gz,
    /// a URL or a folder. Checks the sha256, shows what it runs and needs,
    /// and asks first.
    Install {
        /// A package ID, a path to a .tar.gz or a folder, or a URL.
        package: String,
        /// The sha256 the file or URL must have. Needed for a web URL.
        #[arg(long)]
        sha256: Option<String>,
        #[command(flatten)]
        index: IndexArg,
        /// Don't ask.
        #[arg(short, long)]
        yes: bool,
    },
    /// List installed packages, and what the index offers.
    List {
        #[command(flatten)]
        index: IndexArg,
    },
    /// Install newer versions: of the packages named, or of every package
    /// installed from an index. Each uses the index it came from, unless
    /// --index names another.
    Update {
        ids: Vec<String>,
        #[command(flatten)]
        index: IndexArg,
        /// Don't ask.
        #[arg(short, long)]
        yes: bool,
    },
    /// Delete an installed package.
    Remove {
        id: String,
        /// Don't ask.
        #[arg(short, long)]
        yes: bool,
    },
    /// Pack package folders as .tar.gz files into OUT/packages and list
    /// them in OUT/index.json. For making an index.
    Pack {
        #[arg(required = true)]
        dirs: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    /// `plugin <id>` runs a bundled plugin over stdio (the dashboard starts it
    /// itself; see dashboard_runtime::run_bundled).
    #[command(external_subcommand)]
    Bundled(Vec<String>),
}

#[derive(clap::Args)]
struct IndexArg {
    /// The marketplace index: a path or https:// URL of an index.json.
    /// Default: $DASHBOARD_PLUGIN_INDEX.
    #[arg(long, value_name = "PATH|URL")]
    index: Option<String>,
}

#[derive(Clone, Copy, ValueEnum)]
enum SchemaKind {
    Config,
    Manifest,
    View,
    InitializeParams,
    InitializeResult,
    ServerMessage,
    /// One entry of `dashboard widgets --json`.
    CatalogEntry,
    /// A plugin package's `dashboard-plugin.yaml`.
    Package,
    /// A marketplace `index.json`.
    Index,
}

fn main() -> ExitCode {
    // Before parsing, so a usage error can still answer with a JSON document.
    let json = std::env::args_os()
        .take_while(|a| a != "--")
        .any(|a| a == "--json");
    let reporter = Reporter { name: NAME, json };
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            return ExitCode::from(reporter.clap_error(&err, &mut io::stdout(), &mut io::stderr()));
        }
    };
    let reporter = Reporter {
        name: NAME,
        json: cli.json,
    };
    let result = tokio::runtime::Runtime::new()
        .map_err(anyhow::Error::from)
        .and_then(|rt| {
            let result = rt.block_on(run(cli));
            // Dropping the tasks kills the plugins they own; don't wait long
            // on a blocking call that won't finish.
            rt.shutdown_timeout(std::time::Duration::from_secs(2));
            result
        });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            let err: &(dyn std::error::Error + 'static) = err.as_ref();
            ExitCode::from(reporter.fail(err, &mut io::stdout(), &mut io::stderr()))
        }
    }
}

/// Print `dashboard: warning: <message>` on stderr.
pub fn warn(message: &str) {
    Reporter {
        name: NAME,
        json: false,
    }
    .warn(message);
}

/// The usage error for `--json` on a command that has no JSON form.
fn no_json(command: &str, why: &str) -> anyhow::Error {
    Failure::usage(format!(
        "`{NAME} {command}` {why}, so it has no --json output."
    ))
    .hint("Leave out --json.")
    .into()
}

/// Print a `--json` document on stdout.
fn print_json<T: serde::Serialize>(kind: &str, body: &T) -> Result<()> {
    write_json(&mut io::stdout().lock(), kind, body)?;
    Ok(())
}

async fn run(cli: Cli) -> Result<()> {
    let json = cli.json;
    let sources = Sources {
        machine: cli.config.clone(),
        dashboard: cli.dashboard.clone(),
        project_from: if cli.no_project {
            None
        } else {
            std::env::current_dir().ok()
        },
        registry: None,
        trust: None,
    };
    let global_args = cli_args(&cli);
    match cli.command {
        Command::Tui if json => Err(no_json("tui", "takes over the terminal")),
        Command::Serve { .. } if json => Err(no_json("serve", "runs until stopped")),
        Command::Schema { .. } if json => {
            Err(no_json("schema", "prints a JSON Schema, not a document"))
        }
        Command::Plugin { .. } if json => Err(no_json("plugin", "has no JSON form yet")),
        Command::Dashboards { .. } if json => Err(no_json("dashboards", "has no JSON form yet")),
        Command::Import { .. } if json => Err(no_json("import", "has no JSON form yet")),
        Command::Export { .. } if json => Err(no_json("export", "has no JSON form yet")),
        Command::Trust { .. } if json => Err(no_json("trust", "has no JSON form yet")),
        Command::Tui => {
            // Logs would corrupt the TUI; keep them off until there's a log file.
            // Plugins run in process groups of their own, so a closed terminal
            // (SIGHUP) or `kill` reaches only us: stop cleanly so dropping the
            // host ends them. Listen from the start, since plugin inits can
            // take a while. Once the TUI runs, Ctrl-C is a key, not a signal.
            let shutdown = shutdown_signal();
            tokio::pin!(shutdown);
            let (host, settings, actions, _) = tokio::select! {
                started = start(sources) => started?,
                () = &mut shutdown => return Ok(()),
            };
            tokio::select! {
                result = dashboard_tui::run(host, settings, actions) => result,
                () = &mut shutdown => {
                    dashboard_tui::restore_terminal();
                    Ok(())
                }
            }
        }
        Command::Serve {
            port,
            dev_origin,
            detach,
            no_open,
            record,
        } => {
            if detach {
                let (record, started) = match background::running()? {
                    Some(r) => (r, false),
                    None => (
                        background::detach(&global_args, port, dev_origin.as_deref())?,
                        true,
                    ),
                };
                let verb = if started {
                    "started"
                } else {
                    "already running"
                };
                let link = background::sign_in_link(&record)?;
                println!("dashboard: {link} (pid {}, {verb})", record.pid);
                println!("           signs one browser tab in; unused, it expires in a minute.");
                if !no_open {
                    background::open_browser(&link);
                }
                return Ok(());
            }
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();
            // Listen for signals before the plugins start, so one that comes
            // during their inits still stops them.
            let shutdown = shutdown_signal();
            tokio::pin!(shutdown);
            let (host, settings, actions, layer_paths) = tokio::select! {
                started = start(sources) => started?,
                () = &mut shutdown => return Ok(()),
            };
            let on_ready = move |ready: &dashboard_web::Ready| {
                // A foreground server is recorded too, unless another one is,
                // so `dashboard open` can sign in another tab.
                let write = record || matches!(background::running(), Ok(None));
                if write && let Err(err) = background::write_record(&ready.url, &ready.token) {
                    tracing::error!("{err:#}");
                }
                if !no_open {
                    background::open_browser(&ready.sign_in);
                }
            };
            let served = dashboard_web::serve(
                host,
                settings,
                actions,
                dashboard_web::WebConfig {
                    port,
                    dev_origin,
                    on_ready: Some(Box::new(on_ready)),
                    catalog: Some(Arc::new(dashboard_runtime::catalog_source(layer_paths))),
                },
            );
            // Stopped by Ctrl-C or `dashboard stop`: remove the record on the
            // way out, so a later run never trusts it once the pid is reused.
            let result = tokio::select! {
                result = served => result,
                () = &mut shutdown => Ok(()),
            };
            background::forget();
            result
        }
        Command::Stop => {
            let stopped = background::stop()?;
            if json {
                return print_json("stop", &server_json("stopped", stopped.as_ref()));
            }
            match stopped {
                Some(r) => println!("stopped {} (pid {})", r.url, r.pid),
                None => println!("not running"),
            }
            Ok(())
        }
        Command::Status => {
            let running = background::running()?;
            if json {
                return print_json("status", &server_json("running", running.as_ref()));
            }
            match running {
                Some(r) => println!("running: {} (pid {})", r.url, r.pid),
                None => println!("not running"),
            }
            Ok(())
        }
        Command::Open { no_open } => {
            let Some(record) = background::running()? else {
                return Err(Failure::new("not-running", "no web server is running.")
                    .hint("Start one with `dashboard serve`.")
                    .into());
            };
            let link = background::sign_in_link(&record)?;
            if json {
                return print_json("open", &json!({ "url": link, "pid": record.pid }));
            }
            println!("{link}");
            if !no_open {
                background::open_browser(&link);
            }
            Ok(())
        }
        Command::Config => {
            let loaded = load_valid(&sources)?;
            let packages = Packages::load_for(&loaded.paths());
            let (missing, skipped) = if loaded.exists {
                let missing = missing_packages(&loaded.config, &packages)
                    .into_iter()
                    .map(|(id, message)| loaded.locate(&Part::Module(id), &message))
                    .collect();
                (missing, packages.errors)
            } else {
                (vec![], vec![])
            };
            for err in &skipped {
                warn(&format!("skipped a plugin package: {err}"));
            }
            if !json {
                // The files first, so a missing package can be traced to one.
                if loaded.exists {
                    for layer in &loaded.layers {
                        println!("{:9}  {}", layer.kind.label(), layer.path.display());
                    }
                    if let Some(path) = &loaded.untrusted {
                        println!(
                            "{:9}  {} (not trusted, not used; see `dashboard trust`)",
                            "project",
                            path.display()
                        );
                    }
                } else {
                    if let Some(notice) = loaded.untrusted_notice() {
                        warn(&notice);
                    }
                    println!("{}", loaded.machine.display());
                    println!("(no file; using the built-in demo, examples/demo.yaml)");
                }
            }
            if !missing.is_empty() {
                return Err(anyhow::anyhow!("{}", missing.join("; ")));
            }
            if json {
                let layers: Vec<_> = loaded
                    .layers
                    .iter()
                    .map(|l| json!({ "kind": l.kind.label(), "path": l.path }))
                    .collect();
                return print_json(
                    "config",
                    &json!({
                        "path": loaded.write_path(),
                        "exists": loaded.exists,
                        "layers": layers,
                        "untrustedProject": loaded.untrusted,
                        "skippedPackages": skipped,
                    }),
                );
            }
            if loaded.exists {
                println!("ok");
            }
            Ok(())
        }
        Command::Dashboards { command } => match command {
            DashboardsCommand::List => dashboards::list(&sources),
        },
        Command::Import { from, name, force } => {
            dashboards::import(&sources, &from, name.as_deref(), force)
        }
        Command::Export { name, file, force } => {
            dashboards::export(&sources, &name, file.as_deref(), force)
        }
        Command::Widgets { query, tag } => {
            // Packages next to every file in use; just the machine file's if
            // the files don't load.
            let paths = match dashboard_config::layers::load(&sources) {
                Ok(loaded) => loaded.paths(),
                Err(_) => vec![match &cli.config {
                    Some(p) => p.clone(),
                    None => dashboard_config::default_path()?,
                }],
            };
            widgets::run(&paths, &query.join(" "), tag.as_deref(), json)
        }
        Command::Trust { file, remove } => trust(&sources, file, remove),
        Command::Schema { which } => {
            let schema = match which {
                SchemaKind::Config => schemars::schema_for!(Config),
                SchemaKind::Manifest => schemars::schema_for!(Manifest),
                SchemaKind::View => schemars::schema_for!(View),
                SchemaKind::InitializeParams => schemars::schema_for!(rpc::InitializeParams),
                SchemaKind::InitializeResult => schemars::schema_for!(rpc::InitializeResult),
                SchemaKind::ServerMessage => schemars::schema_for!(ServerMessage),
                SchemaKind::CatalogEntry => schemars::schema_for!(CatalogEntry),
                SchemaKind::Package => schemars::schema_for!(Package),
                SchemaKind::Index => schemars::schema_for!(Index),
            };
            println!("{}", serde_json::to_string_pretty(&schema)?);
            Ok(())
        }
        Command::PluginCheck {
            plugin_config,
            renders,
            wait_ms,
            width,
            height,
            argv,
        } => {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .without_time()
                .init();
            let checked = plugin_check::run(plugin_check::Options {
                config: plugin_config,
                renders,
                wait_ms,
                width,
                height,
                json,
                argv,
            });
            // The plugin is in its own process group, so Ctrl-C or a closed
            // terminal reaches only us: stop cleanly so dropping it kills it.
            tokio::select! {
                result = checked => result,
                () = shutdown_signal() => Err(anyhow::anyhow!("interrupted")),
            }
        }
        Command::Plugin { command } => match command {
            PluginCommand::New { lang, name, dir } => plugin_new::run(lang, &name, dir.as_deref()),
            PluginCommand::Install {
                package,
                sha256,
                index,
                yes,
            } => {
                let o = marketplace::Options {
                    index: index.index,
                    yes,
                };
                marketplace::install(&package, sha256.as_deref(), &o).await
            }
            PluginCommand::List { index } => {
                let o = marketplace::Options {
                    index: index.index,
                    yes: false,
                };
                marketplace::list(&o).await
            }
            PluginCommand::Update { ids, index, yes } => {
                let o = marketplace::Options {
                    index: index.index,
                    yes,
                };
                marketplace::update(&ids, &o).await
            }
            PluginCommand::Remove { id, yes } => marketplace::remove(&id, yes),
            PluginCommand::Pack { dirs, out } => marketplace::pack(&dirs, &out),
            PluginCommand::Bundled(args) => match args.as_slice() {
                [id] => dashboard_runtime::run_bundled(id)
                    .unwrap_or_else(|| Err(anyhow::anyhow!("no bundled plugin {id:?}"))),
                _ => Err(anyhow::anyhow!("usage: dashboard plugin <bundled-id>")),
            },
        },
    }
}

/// `dashboard trust [FILE] [--remove]`.
fn trust(sources: &Sources, file: Option<PathBuf>, remove: bool) -> Result<()> {
    let file = match file {
        Some(f) => f,
        None => {
            let here = std::env::current_dir()?;
            dashboard_config::layers::find_project(&here).ok_or_else(|| {
                anyhow::anyhow!(
                    "no {} in {} or above",
                    dashboard_config::layers::PROJECT_FILE,
                    here.display()
                )
            })?
        }
    };
    let list = sources.trust()?;
    if remove {
        if list.remove(&file)? {
            println!("no longer trusted: {}", file.display());
        } else {
            println!("was not trusted: {}", file.display());
        }
        return Ok(());
    }
    // Check it before trusting it, and say what it will run.
    let text = std::fs::read_to_string(&file)
        .map_err(|e| anyhow::anyhow!("reading {}: {e}", file.display()))?;
    let config = dashboard_config::parse(&text)
        .map_err(|e| anyhow::anyhow!("{e:#} in {}", file.display()))?;
    list.allow(&file)?;
    println!("trusted: {}", file.display());
    for line in dashboards::commands(&config) {
        println!("  runs {line}");
    }
    println!("{}", dashboards::RUNS_NOTE);
    Ok(())
}

/// Resolves on Ctrl-C, on SIGTERM (what `dashboard stop` sends), or on SIGHUP
/// (the terminal closed).
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut hangup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::hangup()),
        ) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
            _ = hangup.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

/// The global options to pass on to a background `serve`, with the config
/// path made absolute.
fn cli_args(cli: &Cli) -> Vec<std::ffi::OsString> {
    let mut args = vec![];
    if let Some(config) = &cli.config {
        args.push("--config".into());
        args.push(
            std::path::absolute(config)
                .unwrap_or_else(|_| config.clone())
                .into(),
        );
    }
    if let Some(name) = &cli.dashboard {
        args.push("--dashboard".into());
        args.push(name.into());
    }
    if cli.no_project {
        args.push("--no-project".into());
    }
    args
}

/// The `--json` body of `status` and `stop`: whether a server was found, and which.
fn server_json(flag: &str, record: Option<&background::Record>) -> serde_json::Value {
    match record {
        Some(r) => json!({ flag: true, "url": r.url, "pid": r.pid }),
        None => json!({ flag: false }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Every subcommand, at any depth, has its own one-line description: not
    /// empty, and sharing no sentence with another command's.
    #[test]
    fn every_subcommand_has_its_own_about() {
        fn walk(cmd: &clap::Command, path: &str, seen: &mut Vec<(String, String)>) {
            for sub in cmd.get_subcommands() {
                let path = format!("{path} {}", sub.get_name());
                let about = sub.get_about().map(|a| a.to_string()).unwrap_or_default();
                assert!(!about.trim().is_empty(), "`{path}` has no description");
                for sentence in about.split(". ").map(|s| s.trim().trim_end_matches('.')) {
                    if sentence.is_empty() {
                        continue;
                    }
                    if let Some((other, _)) = seen.iter().find(|(_, s)| s == sentence) {
                        panic!("`{path}` and `{other}` share \"{sentence}\"");
                    }
                    seen.push((path.clone(), sentence.to_string()));
                }
                walk(sub, &path, seen);
            }
        }
        walk(&Cli::command(), NAME, &mut vec![]);
    }
}
