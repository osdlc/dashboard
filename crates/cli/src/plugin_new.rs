//! `dashboard plugin new --lang ts|py NAME [DIR]`: write a working plugin to start from.
//! The templates and the SDK files are built into the binary, so the new folder needs
//! nothing else.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Lang {
    /// TypeScript, run by Node 24 or Bun.
    Ts,
    /// Python 3.9 or later.
    Py,
}

const TS_MAIN: &str = include_str!("../templates/plugin.ts");
const TS_SDK: &str = include_str!("../../../sdk/typescript/dashboard-plugin.ts");
const TS_PACKAGE: &str = "{\n  \"private\": true,\n  \"type\": \"module\"\n}\n";
const PY_MAIN: &str = include_str!("../templates/plugin.py");
const PY_SDK: &str = include_str!("../../../sdk/python/dashboard_plugin.py");

impl Lang {
    /// (file name, contents) for each file, the main file first.
    fn files(self) -> Vec<(&'static str, &'static str)> {
        match self {
            Lang::Ts => vec![
                ("main.ts", TS_MAIN),
                ("dashboard-plugin.ts", TS_SDK),
                // Tells Node the .ts files are ES modules, so it doesn't guess and warn.
                ("package.json", TS_PACKAGE),
            ],
            Lang::Py => vec![("main.py", PY_MAIN), ("dashboard_plugin.py", PY_SDK)],
        }
    }

    /// The program that runs the main file.
    pub fn runner(self) -> &'static str {
        match self {
            Lang::Ts => "node",
            Lang::Py => "python3",
        }
    }
}

/// Writes the plugin into `dir` (default `./NAME`) and returns the main file's path.
pub fn create(lang: Lang, name: &str, dir: Option<&Path>) -> Result<PathBuf> {
    check_name(name)?;
    let dir = dir.map_or_else(|| PathBuf::from(name), Path::to_path_buf);
    if dir.exists() && dir.read_dir()?.next().is_some() {
        bail!("{} already exists and is not empty", dir.display());
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    for (file, text) in lang.files() {
        let text = text
            .replace("__ID__", name)
            .replace("__NAME__", &display_name(name));
        let path = dir.join(file);
        std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(dir.join(lang.files()[0].0))
}

/// Runs `plugin new` and prints what to do next.
pub fn run(lang: Lang, name: &str, dir: Option<&Path>) -> Result<()> {
    let main = create(lang, name, dir)?;
    let folder = main.parent().unwrap_or(Path::new("."));
    println!("created {}", folder.display());
    for (file, _) in lang.files() {
        println!("  {file}");
    }
    let runner = lang.runner();
    println!(
        "\ntry it:\n  dashboard plugin-check -- {runner} {}",
        main.display()
    );
    // Plugins run in the config file's folder, so the config gets the full path.
    let full = std::path::absolute(&main)?;
    println!(
        "\nadd it to your config:\n  modules:\n    {name}:\n      plugin: [{runner}, {}]",
        full.display()
    );
    Ok(())
}

/// A module ID: lowercase letters, digits and dashes, starting with a letter.
fn check_name(name: &str) -> Result<()> {
    let ok = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !ok {
        bail!("{name:?}: use lowercase letters, digits and dashes, starting with a letter");
    }
    Ok(())
}

/// `my-plugin` -> `My plugin`.
fn display_name(name: &str) -> String {
    let words = name.replace('-', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(check_name("my-plugin2").is_ok());
        assert!(check_name("My").is_err());
        assert!(check_name("2x").is_err());
        assert!(check_name("a b").is_err());
        assert_eq!(display_name("my-plugin"), "My plugin");
    }

    fn have(program: &str) -> bool {
        std::process::Command::new(program)
            .arg("--version")
            .output()
            .is_ok()
    }

    /// Each template, written out and run through the real host, passes the
    /// same checks as `dashboard plugin-check`. Skipped when the runner is missing.
    #[tokio::test]
    async fn scaffolds_pass_the_checks() {
        use dashboard_contract::check::{check_manifest, check_view};
        use dashboard_host::{Module, Plugin};

        let problems = |found: Vec<dashboard_contract::check::Finding>| -> Vec<String> {
            found
                .into_iter()
                .filter(|f| f.problem)
                .map(|f| f.text)
                .collect()
        };
        let root =
            std::env::temp_dir().join(format!("dashboard-plugin-new-{}", std::process::id()));
        for lang in [Lang::Ts, Lang::Py] {
            if !have(lang.runner()) {
                continue;
            }
            let dir = root.join(format!("{lang:?}"));
            let main = create(lang, "my-plugin", Some(&dir)).unwrap();
            assert!(create(lang, "my-plugin", Some(&dir)).is_err(), "not empty");

            let argv = vec![lang.runner().into(), main.display().to_string()];
            let mut plugin = Plugin::new("test", argv);
            let manifest = plugin
                .init(serde_json::json!({ "greeting": "hi" }))
                .await
                .unwrap();
            assert_eq!(manifest.id, "my-plugin");
            assert_eq!(manifest.name, "My plugin");
            assert_eq!(problems(check_manifest(&manifest)), Vec::<String>::new());
            let view = plugin.render().await.unwrap();
            assert_eq!(problems(check_view(&view)), Vec::<String>::new());
            assert!(view.summary.starts_with("hi"), "{lang:?}: {}", view.summary);
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// The types in both SDKs match the contract's JSON Schemas. After a contract
    /// change, run `node sdk/gen-types.mjs`. Skipped without Node.
    #[test]
    fn sdk_types_are_up_to_date() {
        use dashboard_contract::{Manifest, View, rpc};

        if !have("node") {
            return;
        }
        let dir =
            std::env::temp_dir().join(format!("dashboard-sdk-schemas-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, schema) in [
            ("manifest", schemars::schema_for!(Manifest)),
            ("view", schemars::schema_for!(View)),
            (
                "initialize-params",
                schemars::schema_for!(rpc::InitializeParams),
            ),
        ] {
            let text = serde_json::to_string_pretty(&schema).unwrap();
            std::fs::write(dir.join(format!("{name}.json")), text).unwrap();
        }
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/../../sdk/gen-types.mjs");
        let out = std::process::Command::new("node")
            .arg(script)
            .arg("--check")
            .arg("--schemas")
            .arg(&dir)
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            out.status.success(),
            "SDK types are out of date; run `node sdk/gen-types.mjs`\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}
