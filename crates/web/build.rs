//! Builds the Vue app in `web/` so it can be embedded in the binary.
//!
//! Reruns only when UI sources change. Set `DASHBOARD_SKIP_UI_BUILD=1` to skip
//! (a placeholder page is embedded instead); the same happens if pnpm is missing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

const WATCHED: &[&str] = &[
    "src",
    "public",
    "index.html",
    "package.json",
    "pnpm-lock.yaml",
    "vite.config.ts",
    "tsconfig.json",
    "tsconfig.app.json",
    "tsconfig.node.json",
];

fn main() {
    let web = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../web");
    let dist = web.join("dist");

    for path in WATCHED {
        println!("cargo:rerun-if-changed={}", web.join(path).display());
    }
    println!("cargo:rerun-if-env-changed=DASHBOARD_SKIP_UI_BUILD");

    if env::var_os("DASHBOARD_SKIP_UI_BUILD").is_some() {
        placeholder(&dist, "UI build skipped (DASHBOARD_SKIP_UI_BUILD is set).");
        return;
    }
    if Command::new("pnpm").arg("--version").output().is_err() {
        println!("cargo:warning=pnpm not found; embedding a placeholder UI");
        placeholder(&dist, "UI not built: pnpm was not found when compiling.");
        return;
    }

    run(&web, &["install", "--prefer-offline"]);
    run(&web, &["build"]);
}

fn run(dir: &Path, args: &[&str]) {
    let out = Command::new("pnpm")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("running pnpm");
    if !out.status.success() {
        panic!(
            "`pnpm {}` failed in {}\n{}\n{}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

fn placeholder(dist: &Path, message: &str) {
    // rust-embed needs the folder to exist. Don't overwrite a real build.
    if dist.join("index.html").exists() {
        return;
    }
    fs::create_dir_all(dist).unwrap();
    fs::write(
        dist.join("index.html"),
        format!("<!doctype html><title>Dashboard</title><p>{message}</p>"),
    )
    .unwrap();
}
