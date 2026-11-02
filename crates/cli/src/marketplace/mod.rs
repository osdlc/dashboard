//! `dashboard plugin install | list | update | remove | pack`: install plugin
//! packages from a marketplace index, a `.tar.gz`, a URL or a folder. See
//! docs/marketplace.md.
//!
//! Install checks the sha256, shows what the plugin runs and needs, asks,
//! then unpacks into a staging folder and moves it into
//! `<data home>/dashboard/plugins/<id>/`. `installed.json` there records
//! where each one came from.

use std::cmp::Ordering;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dashboard_config::packages::{Packages, data_dir, load_package, parse_package, valid_id};
use dashboard_contract::{Index, IndexEntry, PACKAGE_FILE, Package};

mod archive;
mod source;

use source::{Location, Origin, Record, load_index};

/// Set instead of `--index`.
const INDEX_ENV: &str = "DASHBOARD_PLUGIN_INDEX";

pub struct Options {
    /// The index: a path or URL to an `index.json`.
    pub index: Option<String>,
    /// Don't ask before installing or removing.
    pub yes: bool,
}

impl Options {
    /// `--index`, or else $DASHBOARD_PLUGIN_INDEX.
    fn index(&self) -> Result<Option<Location>> {
        match self.flag_index()? {
            Some(at) => Ok(Some(at)),
            None => std::env::var(INDEX_ENV)
                .ok()
                .filter(|s| !s.is_empty())
                .map(|s| Location::parse_index(&s))
                .transpose(),
        }
    }

    /// `--index` alone.
    fn flag_index(&self) -> Result<Option<Location>> {
        self.index.as_deref().map(Location::parse_index).transpose()
    }
}

/// The folders install writes to.
struct Places {
    /// Installed packages, one folder each.
    plugins: PathBuf,
    /// Unpacked archives wait here until confirmed. Next to `plugins`, so
    /// the final move is a rename, and outside it, so a running dashboard
    /// never loads a half-unpacked package.
    staging: PathBuf,
}

impl Places {
    fn new() -> Result<Self> {
        let plugins = data_dir()?;
        let staging = plugins.with_file_name("plugin-staging");
        Ok(Self { plugins, staging })
    }
}

/// An archive read and checked, ready to unpack.
struct Fetched {
    bytes: Vec<u8>,
    sha256: String,
    /// What was checked against, for the summary.
    checked: &'static str,
    from: String,
    /// The index and its entry, when installed by ID.
    index: Option<(String, IndexEntry)>,
}

pub async fn install(spec: &str, sha256: Option<&str>, o: &Options) -> Result<()> {
    let places = Places::new()?;
    let fetched = fetch(spec, sha256, o).await?;
    install_fetched(fetched, &places, o.yes)
}

/// Read what `spec` names: a folder, an archive, a URL or an ID in the index.
async fn fetch(spec: &str, sha256: Option<&str>, o: &Options) -> Result<Fetched> {
    let path = Path::new(spec);
    if !Location::is_web(spec) && path.exists() {
        let from = std::path::absolute(path)?.display().to_string();
        let bytes = if path.is_dir() {
            archive::pack(path)?.1
        } else {
            Location::parse(spec)?.fetch().await?
        };
        return checked(bytes, sha256, "--sha256", from, None);
    }
    if Location::is_web(spec) {
        let at = Location::parse(spec)?;
        if matches!(at, Location::Web(_)) && sha256.is_none() {
            bail!("pass --sha256 with a URL, so a changed download is refused");
        }
        let bytes = at.fetch().await?;
        return checked(bytes, sha256, "--sha256", at.to_string(), None);
    }
    if !valid_id(spec) {
        bail!("{spec:?} is not a folder, a .tar.gz, a URL or a package ID");
    }
    let Some(at) = o.index()? else {
        bail!("to install by ID, pass --index <path or URL of index.json> or set {INDEX_ENV}");
    };
    let index = load_index(&at).await?;
    let Some(entry) = index.get(spec) else {
        bail!("{spec} isn't in the index {at}");
    };
    if sha256.is_some() {
        bail!("--sha256 is for a file or URL; the index gives the hash for {spec}");
    }
    fetch_entry(&at, entry).await
}

/// Read the archive an index entry points at, checked against its hash.
async fn fetch_entry(at: &Location, entry: &IndexEntry) -> Result<Fetched> {
    let url = at.entry(&entry.url)?;
    let bytes = url.fetch().await?;
    checked(
        bytes,
        Some(&entry.sha256),
        "the index",
        url.to_string(),
        Some((at.to_string(), entry.clone())),
    )
}

/// Refuse the bytes if they don't match the hash given.
fn checked(
    bytes: Vec<u8>,
    expected: Option<&str>,
    by: &'static str,
    from: String,
    index: Option<(String, IndexEntry)>,
) -> Result<Fetched> {
    let sha256 = etag_write::etag(&bytes);
    if let Some(expected) = expected
        && !expected.eq_ignore_ascii_case(&sha256)
    {
        bail!(
            "sha256 of {from} doesn't match {by}: expected {expected}, got {sha256}. Nothing was installed."
        );
    }
    Ok(Fetched {
        bytes,
        sha256,
        checked: if expected.is_some() {
            by
        } else {
            "nothing (no hash given)"
        },
        from,
        index,
    })
}

fn install_fetched(f: Fetched, places: &Places, yes: bool) -> Result<()> {
    std::fs::create_dir_all(&places.staging)
        .with_context(|| format!("creating {}", places.staging.display()))?;
    let tmp = places.staging.join(format!("{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("unpacked"))?;
    let result = unpack_and_move(&f, places, &tmp, yes);
    let _ = std::fs::remove_dir_all(&tmp);
    // Only when empty: another install may be using it.
    let _ = std::fs::remove_dir(&places.staging);
    result
}

fn unpack_and_move(f: &Fetched, places: &Places, tmp: &Path, yes: bool) -> Result<()> {
    archive::unpack(&f.bytes, &tmp.join("unpacked"))?;
    let root = archive::package_root(&tmp.join("unpacked"))?;
    let text = std::fs::read_to_string(root.join(PACKAGE_FILE))?;
    let package = parse_package(&text).context("in the archive's dashboard-plugin.yaml")?;
    if let Some((at, entry)) = &f.index
        && (entry.id != package.id || entry.version != package.version)
    {
        bail!(
            "the index {at} lists {} {}, but the archive holds {} {}",
            entry.id,
            entry.version,
            package.id,
            package.version
        );
    }
    let target = places.plugins.join(&package.id);
    let current = load_package(&target).ok().map(|p| p.package.version);
    print!("{}", summary(&package, f, current.as_deref()));
    if !confirm(
        "A plugin runs as you, with your permissions. Install it?",
        yes,
    )? {
        println!("cancelled; nothing was installed");
        return Ok(());
    }

    std::fs::create_dir_all(&places.plugins)?;
    let old = tmp.join("old");
    if target.exists() {
        std::fs::rename(&target, &old)
            .with_context(|| format!("moving the old {} aside", target.display()))?;
    }
    if let Err(err) = std::fs::rename(&root, &target) {
        if old.exists() {
            let _ = std::fs::rename(&old, &target);
        }
        return Err(err).with_context(|| format!("moving the package to {}", target.display()));
    }
    let finish = || -> Result<()> {
        load_package(&target)?;
        Record::update(&places.plugins, |r| {
            r.plugins.insert(
                package.id.clone(),
                Origin {
                    version: package.version.clone(),
                    sha256: f.sha256.clone(),
                    from: f.from.clone(),
                    index: f.index.as_ref().map(|(at, _)| at.clone()),
                },
            );
        })
    };
    if let Err(err) = finish() {
        // Undo the move, so the folder and installed.json still agree.
        let _ = std::fs::remove_dir_all(&target);
        if old.exists() {
            let _ = std::fs::rename(&old, &target);
            return Err(err.context(format!(
                "{} was not updated; the version before is still in place",
                package.id
            )));
        }
        return Err(err.context(format!("{} was not installed", package.id)));
    }
    println!(
        "installed {} {} in {}\nadd it to a config with `package: {}`",
        package.id,
        package.version,
        target.display(),
        package.id
    );
    Ok(())
}

/// What the package is, runs and needs, shown before asking.
fn summary(p: &Package, f: &Fetched, current: Option<&str>) -> String {
    let mut out = format!("{} {} — {}\n  {}\n", p.id, p.version, p.name, p.description);
    let mut line = |label: &str, value: String| {
        if !value.is_empty() {
            out.push_str(&format!("  {label:9}{value}\n"));
        }
    };
    line("from", f.from.clone());
    line(
        "sha256",
        format!("{} (checked against {})", f.sha256, f.checked),
    );
    line("runs", p.run.join(" "));
    let needs: Vec<String> = p
        .runtime
        .iter()
        .chain(&p.binaries)
        .map(|r| {
            let found = if on_path(r) { "found" } else { "not found" };
            format!("{r} ({found})")
        })
        .collect();
    line("needs", needs.join(", "));
    line("reads", p.env.join(", "));
    line("calls", p.hosts.join(", "));
    if let Some(v) = current {
        line("replaces", format!("version {v}"));
    }
    out
}

/// Is the program a requirement names (`node>=22` → `node`) on PATH?
fn on_path(requirement: &str) -> bool {
    let name = requirement.split(">=").next().unwrap_or(requirement);
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| {
        dir.join(name).is_file() || (cfg!(windows) && dir.join(format!("{name}.exe")).is_file())
    })
}

/// Ask on stdin. `yes` answers for the user.
fn confirm(question: &str, yes: bool) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    if std::io::stdin().read_line(&mut answer)? == 0 {
        println!();
        bail!("no answer on stdin; pass --yes to go ahead without asking");
    }
    Ok(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Installed packages, and what the index offers when one is given.
pub async fn list(o: &Options) -> Result<()> {
    let plugins = data_dir()?;
    let found = Packages::load_from(std::slice::from_ref(&plugins));
    for err in &found.errors {
        eprintln!("skipped a plugin package: {err}");
    }
    let (record, _) = Record::read(&plugins)?;
    let index = match o.index()? {
        Some(at) => Some((load_index(&at).await?, at)),
        None => None,
    };
    println!("installed in {}:", plugins.display());
    if found.installed.is_empty() {
        println!("  (none)");
    }
    let id_width = found
        .installed
        .iter()
        .map(|p| p.package.id.len())
        .chain(
            index
                .iter()
                .flat_map(|(i, _)| i.plugins.iter().map(|e| e.id.len())),
        )
        .max()
        .unwrap_or(0);
    for p in &found.installed {
        let id = &p.package.id;
        let from = match record.plugins.get(id) {
            Some(origin) => format!("from {}", origin.from),
            None => "copied by hand".into(),
        };
        let update = index
            .as_ref()
            .and_then(|(i, _)| i.get(id))
            .filter(|e| newer(&e.version, &p.package.version))
            .map(|e| format!("  (update: {})", e.version))
            .unwrap_or_default();
        println!("  {id:id_width$}  {:8}  {from}{update}", p.package.version);
    }
    if let Some((index, at)) = &index {
        println!("available from {at}:");
        let available: Vec<&IndexEntry> = index
            .plugins
            .iter()
            .filter(|e| found.get(&e.id).is_none())
            .collect();
        if available.is_empty() {
            println!("  (nothing more)");
        }
        for e in available {
            println!(
                "  {:id_width$}  {:8}  {}",
                e.id,
                e.version,
                first_line(&e.description)
            );
        }
    }
    Ok(())
}

/// The start of a description, cut to fit a line.
fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    match line.char_indices().nth(70) {
        Some((i, _)) => format!("{}…", line[..i].trim_end()),
        None => line.to_string(),
    }
}

/// Install newer versions of the packages named, or of every package
/// installed from an index. Each uses the index it was installed from, or
/// `--index`. $DASHBOARD_PLUGIN_INDEX is only used for a package named here
/// that was not installed from an index.
pub async fn update(ids: &[String], o: &Options) -> Result<()> {
    let places = Places::new()?;
    let (record, _) = Record::read(&places.plugins)?;
    let named = !ids.is_empty();
    let ids: Vec<String> = if named {
        ids.to_vec()
    } else {
        record.plugins.keys().cloned().collect()
    };
    if ids.is_empty() {
        println!("nothing was installed with `dashboard plugin install`");
        return Ok(());
    }
    let mut indexes: Vec<(String, Index)> = vec![];
    let mut failed = vec![];
    for id in &ids {
        let Some(origin) = record.plugins.get(id) else {
            println!("{id}: not installed with `dashboard plugin install`; skipped");
            continue;
        };
        let at = match (o.flag_index(), &origin.index) {
            (Ok(Some(at)), _) => Ok(Some(at)),
            (Ok(None), Some(at)) => Location::parse_index(at).map(Some),
            (Ok(None), None) if named => o.index(),
            (Ok(None), None) => Ok(None),
            (Err(e), _) => Err(e),
        };
        let at = match at {
            Ok(Some(at)) => at,
            Ok(None) => {
                println!(
                    "{id}: installed from {}, not from an index; name it with --index to update it",
                    origin.from
                );
                continue;
            }
            Err(err) => {
                eprintln!("{id}: {err:#}");
                failed.push(id.as_str());
                continue;
            }
        };
        if let Err(err) = update_one(id, origin, &at, &mut indexes, &places, o.yes).await {
            eprintln!("{id}: {err:#}");
            failed.push(id.as_str());
        }
    }
    if !failed.is_empty() {
        bail!("not updated: {}", failed.join(", "));
    }
    Ok(())
}

/// Update one package from the index `at`, loading each index once.
async fn update_one(
    id: &str,
    origin: &Origin,
    at: &Location,
    indexes: &mut Vec<(String, Index)>,
    places: &Places,
    yes: bool,
) -> Result<()> {
    let key = at.to_string();
    if !indexes.iter().any(|(k, _)| *k == key) {
        indexes.push((key.clone(), load_index(at).await?));
    }
    let index = &indexes.iter().find(|(k, _)| *k == key).expect("loaded").1;
    let Some(entry) = index.get(id) else {
        println!(
            "{id}: not in the index {key} any more; kept {}",
            origin.version
        );
        return Ok(());
    };
    match compare(&entry.version, &origin.version) {
        Some(Ordering::Greater) => {}
        Some(_) => {
            println!("{id} {}: up to date", origin.version);
            return Ok(());
        }
        None => {
            println!(
                "{id}: can't tell if {} is newer than {}; install it again to change",
                entry.version, origin.version
            );
            return Ok(());
        }
    }
    let fetched = fetch_entry(at, entry).await?;
    install_fetched(fetched, places, yes)
}

/// Is version `a` newer than `b`?
fn newer(a: &str, b: &str) -> bool {
    compare(a, b) == Some(Ordering::Greater)
}

/// Order two versions as semver does: dotted numbers (missing ones are 0),
/// then a release after its pre-releases (`1.0.0-beta.2` < `1.0.0`), whose
/// dotted parts compare as numbers or text. `+build` is ignored. `None`
/// when either isn't in that shape.
fn compare(a: &str, b: &str) -> Option<Ordering> {
    fn split(v: &str) -> Option<(Vec<u64>, Option<&str>)> {
        let v = v.split('+').next().unwrap_or(v);
        let (core, pre) = match v.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (v, None),
        };
        let numbers = core
            .split('.')
            .map(|n| n.parse::<u64>().ok())
            .collect::<Option<Vec<u64>>>()?;
        Some((numbers, pre))
    }
    let (an, ap) = split(a)?;
    let (bn, bp) = split(b)?;
    let numbers = (0..an.len().max(bn.len())).fold(Ordering::Equal, |ord, i| {
        ord.then(an.get(i).unwrap_or(&0).cmp(bn.get(i).unwrap_or(&0)))
    });
    let pre = match (ap, bp) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            let (xs, ys): (Vec<&str>, Vec<&str>) = (x.split('.').collect(), y.split('.').collect());
            xs.iter()
                .zip(&ys)
                .map(|(x, y)| match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => x.cmp(y),
                })
                .fold(Ordering::Equal, Ordering::then)
                .then(xs.len().cmp(&ys.len()))
        }
    };
    Some(numbers.then(pre))
}

/// Delete an installed package's folder.
pub fn remove(id: &str, yes: bool) -> Result<()> {
    if !valid_id(id) {
        bail!("{id:?} is not a package ID");
    }
    let plugins = data_dir()?;
    let target = plugins.join(id);
    if !target.join(PACKAGE_FILE).is_file() {
        bail!("{id} isn't installed in {}", plugins.display());
    }
    if !confirm(&format!("Remove {id} ({})?", target.display()), yes)? {
        println!("cancelled; nothing was removed");
        return Ok(());
    }
    std::fs::remove_dir_all(&target).with_context(|| format!("removing {}", target.display()))?;
    let (record, _) = Record::read(&plugins)?;
    if record.plugins.contains_key(id) {
        Record::update(&plugins, |r| {
            r.plugins.remove(id);
        })?;
    }
    println!("removed {id}");
    Ok(())
}

/// Pack package folders into `<out>/packages/` and add them to
/// `<out>/index.json`, replacing entries with the same ID.
pub fn pack(dirs: &[PathBuf], out: &Path) -> Result<()> {
    let packages = out.join("packages");
    std::fs::create_dir_all(&packages)
        .with_context(|| format!("creating {}", packages.display()))?;
    let index_path = out.join("index.json");
    let (mut index, etag) = match std::fs::read(&index_path) {
        Ok(bytes) => (
            serde_json::from_slice::<Index>(&bytes)
                .with_context(|| format!("reading {}", index_path.display()))?,
            Some(etag_write::etag(&bytes)),
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Index::default(), None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", index_path.display())),
    };
    for dir in dirs {
        let (installed, bytes) =
            archive::pack(dir).with_context(|| format!("packing {}", dir.display()))?;
        let p = &installed.package;
        let file = format!("{}-{}.tar.gz", p.id, p.version);
        etag_write::write_if_changed(&packages.join(&file), &bytes)?;
        let sha256 = etag_write::etag(&bytes);
        println!("{} {}  packages/{file}  {sha256}", p.id, p.version);
        let entry = IndexEntry::new(p, format!("packages/{file}"), sha256);
        match index.plugins.iter_mut().find(|e| e.id == p.id) {
            Some(e) => *e = entry,
            None => index.plugins.push(entry),
        }
    }
    index.plugins.sort_by(|a, b| a.id.cmp(&b.id));
    let mut text = serde_json::to_string_pretty(&index)?;
    text.push('\n');
    etag_write::write_cas(&index_path, text.as_bytes(), etag.as_deref())?;
    println!("wrote {}", index_path.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(newer("0.2.0", "0.1.9"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0", "0.2.0"));
        assert!(newer("1.0.0", "1.0.0-beta"));
        assert!(!newer("1.0.0-beta", "1.0.0"));
        assert!(newer("1.0.0-beta.2", "1.0.0-beta.1"));
        assert!(!newer("1.0.0-beta.1", "1.0.0-beta.2"));
        assert!(newer("1.0.0-beta.11", "1.0.0-beta.2"));
        assert!(newer("1.0.0-beta", "1.0.0-alpha.5"));
        assert!(newer("1.0.0-alpha.1", "1.0.0-alpha"));
        assert!(!newer("1.0", "1.0.0"));
        assert!(!newer("1.0.0+b2", "1.0.0+b1"));
        // Not in that shape: never an update.
        assert!(!newer("nightly-2", "nightly-1"));
        assert_eq!(compare("x", "1.0"), None);
    }

    #[test]
    fn a_wrong_hash_is_refused() {
        let err = checked(
            b"x".to_vec(),
            Some("00"),
            "the index",
            "a.tar.gz".into(),
            None,
        )
        .err()
        .unwrap();
        assert!(err.to_string().contains("doesn't match the index"), "{err}");
        let ok = checked(
            b"x".to_vec(),
            Some(&etag_write::etag(b"x").to_uppercase()),
            "--sha256",
            "a".into(),
            None,
        )
        .unwrap();
        assert_eq!(ok.checked, "--sha256");
    }
}
