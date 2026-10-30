use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};
use dashboard_config::Config;
use dashboard_config::packages::Packages;
use dashboard_contract::{CatalogEntry, Manifest, Package, ServerMessage, View, rpc};
use dashboard_contract::{ClientMessage, UiSettings};
use dashboard_host::Host;
use runtime::{Runtime, handle_actions, known_modules, missing_packages, watch_config};
use tokio::sync::Mutex;
use tokio::sync::{mpsc, watch};

mod background;
mod plugin_check;
mod plugin_new;
mod runtime;
mod widgets;

#[derive(Parser)]
#[command(name = "dashboard", about = "Modular developer dashboard")]
struct Cli {
    /// Config file. Default: ~/.config/dashboard/config.yaml (or $DASHBOARD_MACHINE_CONFIG).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
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
    /// Stop the background web server.
    Stop,
    /// Show whether a background web server is running, and its URL.
    Status,
    /// Show where the config file is and whether it is valid.
    Config,
    /// List everything that can be added to a dashboard: built-in modules,
    /// bundled plugins and installed plugin packages.
    Widgets {
        /// Words to look for in the name, ID, description and tags (case-insensitive).
        query: Vec<String>,
        /// Only entries with this tag.
        #[arg(long)]
        tag: Option<String>,
        /// Print the full catalog entries, demo views included, as JSON.
        #[arg(long)]
        json: bool,
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
        /// Height of the printed tile. Default: what the tile needs.
        #[arg(long)]
        height: Option<u16>,
        /// Also print the last view as JSON.
        #[arg(long)]
        json: bool,
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
    /// Run the bundled cswap plugin over stdio (the dashboard starts it itself).
    #[command(hide = true)]
    Cswap,
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Tui => {
            // Logs would corrupt the TUI; keep them off until there's a log file.
            let (host, settings, actions) = start(&cli.config).await?;
            dashboard_tui::run(host, settings, actions).await
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
                        background::detach(cli.config.as_deref(), port, dev_origin.as_deref())?,
                        true,
                    ),
                };
                let verb = if started {
                    "started"
                } else {
                    "already running"
                };
                println!("dashboard: {} (pid {}, {verb})", record.url, record.pid);
                if !no_open {
                    background::open_browser(&record.url);
                }
                return Ok(());
            }
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .init();
            let (host, settings, actions) = start(&cli.config).await?;
            let on_ready = move |url: &str| {
                if record && let Err(err) = background::write_record(url) {
                    tracing::error!("{err:#}");
                }
                if !no_open {
                    background::open_browser(url);
                }
            };
            dashboard_web::serve(
                host,
                settings,
                actions,
                dashboard_web::WebConfig {
                    port,
                    dev_origin,
                    on_ready: Some(Box::new(on_ready)),
                },
            )
            .await
        }
        Command::Stop => {
            match background::stop()? {
                Some(r) => println!("stopped {} (pid {})", r.url, r.pid),
                None => println!("not running"),
            }
            Ok(())
        }
        Command::Status => {
            match background::running()? {
                Some(r) => println!("running: {} (pid {})", r.url, r.pid),
                None => println!("not running"),
            }
            Ok(())
        }
        Command::Config => {
            let loaded = dashboard_config::load(cli.config.as_deref())?;
            println!("{}", loaded.path.display());
            if !loaded.exists {
                println!("(no file; using the built-in demo, examples/demo.yaml)");
                return Ok(());
            }
            loaded.config.validate(&known_modules(&loaded.config))?;
            let packages = Packages::load(&loaded.path);
            for err in &packages.errors {
                eprintln!("skipped a plugin package: {err}");
            }
            let missing = missing_packages(&loaded.config, &packages);
            if !missing.is_empty() {
                anyhow::bail!("{}", missing.join("\n"));
            }
            println!("ok");
            Ok(())
        }
        Command::Widgets { query, tag, json } => {
            let path = match &cli.config {
                Some(p) => p.clone(),
                None => dashboard_config::default_path()?,
            };
            widgets::run(&path, &query.join(" "), tag.as_deref(), json)
        }
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
            json,
            argv,
        } => {
            tracing_subscriber::fmt()
                .with_writer(std::io::stderr)
                .without_time()
                .init();
            plugin_check::run(plugin_check::Options {
                config: plugin_config,
                renders,
                wait_ms,
                width,
                height,
                json,
                argv,
            })
            .await
        }
        Command::Plugin { command } => match command {
            PluginCommand::New { lang, name, dir } => plugin_new::run(lang, &name, dir.as_deref()),
            PluginCommand::Cswap => dashboard_cswap::run(),
        },
    }
}

/// Start modules from the config, follow config changes, and take renderer actions.
async fn start(
    config: &Option<PathBuf>,
) -> Result<(
    Host,
    watch::Receiver<UiSettings>,
    mpsc::UnboundedSender<ClientMessage>,
)> {
    let (runtime, settings) = Runtime::start(config.as_deref()).await?;
    let host = runtime.host.clone();
    let path = runtime.path().to_path_buf();
    let runtime = Arc::new(Mutex::new(runtime));
    watch_config(runtime.clone(), path);
    let actions = handle_actions(runtime);
    Ok((host, settings, actions))
}
