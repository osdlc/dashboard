//! Packing a package folder into a `.tar.gz`, and unpacking one safely.
//!
//! Unpacking takes only plain files and folders. Every path goes through
//! path-guard, so an entry like `../../.bashrc` or `/etc/x` is refused
//! (zip-slip), and links and device files are refused too.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dashboard_config::packages::{Installed, load_package};
use dashboard_contract::PACKAGE_FILE;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use path_guard::Guard;
use tar::EntryType;

/// The largest download or archive read: 64 MiB.
pub const MAX_ARCHIVE: u64 = 64 << 20;
/// The most an archive may unpack to: 256 MiB.
const MAX_UNPACKED: u64 = 256 << 20;
/// The most files and folders an archive may hold.
const MAX_ENTRIES: usize = 10_000;

/// Pack the package in `dir` as `<id>/...`. The same files give the same
/// bytes: sorted names, no times, no owners. Leaves out dotfiles, Python
/// caches and `node_modules`; refuses links.
pub fn pack(dir: &Path) -> Result<(Installed, Vec<u8>)> {
    let installed = load_package(dir)?;
    let mut files = vec![];
    walk(&installed.dir, Path::new(""), &mut files)?;
    files.sort();

    let mut tar = tar::Builder::new(GzEncoder::new(vec![], Compression::default()));
    for rel in &files {
        let path = installed.dir.join(rel);
        let data = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(if executable(&path) { 0o755 } else { 0o644 });
        header.set_mtime(0);
        let name = Path::new(&installed.package.id).join(rel);
        tar.append_data(&mut header, &name, data.as_slice())
            .with_context(|| format!("packing {}", path.display()))?;
    }
    let bytes = tar.into_inner()?.finish()?;
    Ok((installed, bytes))
}

/// Files under `dir`, relative to the package root, skipping what isn't
/// part of a package.
fn walk(root: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let dir = root.join(rel);
    for entry in std::fs::read_dir(&dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.')
            || name_str == "__pycache__"
            || name_str == "node_modules"
            || name_str.ends_with(".pyc")
        {
            continue;
        }
        let kind = entry.file_type()?;
        let path = rel.join(&name);
        if kind.is_symlink() {
            bail!(
                "{} is a link; a package holds only plain files",
                root.join(&path).display()
            );
        } else if kind.is_dir() {
            walk(root, &path, out)?;
        } else if kind.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_: &Path) -> bool {
    false
}

/// Unpack a `.tar.gz` into `dest`, an empty folder.
pub fn unpack(bytes: &[u8], dest: &Path) -> Result<()> {
    let dest = std::fs::canonicalize(dest)?;
    let guard = Guard::std();
    let mut archive = tar::Archive::new(GzDecoder::new(bytes));
    let mut total = 0u64;
    for (count, entry) in archive.entries().context("not a .tar.gz")?.enumerate() {
        if count >= MAX_ENTRIES {
            bail!("the archive has more than {MAX_ENTRIES} entries");
        }
        let mut entry = entry.context("reading the archive")?;
        let kind = entry.header().entry_type();
        // Extended headers carry metadata for the next entry; tar applies them.
        if matches!(kind, EntryType::XGlobalHeader | EntryType::XHeader) {
            continue;
        }
        let raw = entry.path_bytes();
        let raw = std::str::from_utf8(&raw).context("an archive path isn't UTF-8")?;
        let name = clean(raw);
        if name.is_empty() {
            continue;
        }
        let path = guard
            .resolve(&dest, &name)
            .with_context(|| format!("unsafe path in the archive: {raw:?}"))?;
        match kind {
            EntryType::Directory => std::fs::create_dir_all(&path)?,
            EntryType::Regular | EntryType::Continuous => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                // create_new: never write through something already there.
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .with_context(|| format!("creating {}", path.display()))?;
                let left = MAX_UNPACKED - total;
                let written = std::io::copy(&mut (&mut entry).take(left + 1), &mut file)?;
                total += written;
                if total > MAX_UNPACKED {
                    bail!(
                        "the archive unpacks to more than {} MiB",
                        MAX_UNPACKED >> 20
                    );
                }
                file.flush()?;
                set_mode(&path, entry.header().mode().unwrap_or(0o644))?;
            }
            _ => bail!("{raw:?}: links and special files aren't allowed in a package"),
        }
    }
    Ok(())
}

/// `./a/b/` → `a/b`. Other odd shapes are left for path-guard to refuse.
fn clean(raw: &str) -> String {
    let mut s = raw;
    while let Some(rest) = s.strip_prefix("./") {
        s = rest;
    }
    if s == "." {
        s = "";
    }
    s.trim_end_matches('/').to_string()
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if mode & 0o111 != 0 { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> Result<()> {
    Ok(())
}

/// The package folder inside an unpacked archive: `dest` itself, or its
/// one folder.
pub fn package_root(dest: &Path) -> Result<PathBuf> {
    if dest.join(PACKAGE_FILE).is_file() {
        return Ok(dest.to_path_buf());
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(dest)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    match entries.as_slice() {
        [one] if one.join(PACKAGE_FILE).is_file() => Ok(one.clone()),
        _ => bail!("no {PACKAGE_FILE} at the top of the archive or in its one folder"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dashboard-archive-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn hello() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/plugins/hello")
    }

    /// A .tar.gz with one entry whose header names `name`, written raw so
    /// the tar crate's own path checks don't stop it.
    fn hostile(name: &str, kind: EntryType) -> Vec<u8> {
        let mut header = tar::Header::new_old();
        let field = &mut header.as_old_mut().name;
        field[..name.len()].copy_from_slice(name.as_bytes());
        header.set_entry_type(kind);
        header.set_size(2);
        header.set_mode(0o644);
        if kind == EntryType::Symlink {
            header.set_size(0);
            header.set_link_name("/etc/passwd").unwrap();
        }
        header.set_cksum();
        let mut tar = tar::Builder::new(GzEncoder::new(vec![], Compression::default()));
        let data: &[u8] = if kind == EntryType::Symlink {
            b""
        } else {
            b"hi"
        };
        tar.append(&header, data).unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    }

    #[test]
    fn packs_the_same_bytes_and_unpacks_them() {
        let (first, a) = pack(&hello()).unwrap();
        let (_, b) = pack(&hello()).unwrap();
        assert_eq!(first.package.id, "hello");
        assert_eq!(a, b, "packing is repeatable");

        let dest = scratch("roundtrip");
        unpack(&a, &dest).unwrap();
        let root = package_root(&dest).unwrap();
        assert!(root.ends_with("hello"));
        for name in [PACKAGE_FILE, "hello.py"] {
            assert_eq!(
                std::fs::read(root.join(name)).unwrap(),
                std::fs::read(hello().join(name)).unwrap(),
                "{name}"
            );
        }
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn refuses_paths_that_leave_the_folder() {
        for name in [
            "../evil.txt",
            "a/../../evil.txt",
            "/tmp/evil.txt",
            "~/evil.txt",
        ] {
            let dest = scratch("slip");
            let err = unpack(&hostile(name, EntryType::Regular), &dest).unwrap_err();
            assert!(
                format!("{err:#}").contains("unsafe path"),
                "{name}: {err:#}"
            );
            assert!(!dest.parent().unwrap().join("evil.txt").exists());
            std::fs::remove_dir_all(&dest).unwrap();
        }
    }

    #[test]
    fn refuses_links() {
        let dest = scratch("link");
        let err = unpack(&hostile("x/passwd", EntryType::Symlink), &dest).unwrap_err();
        assert!(format!("{err:#}").contains("links"), "{err:#}");
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn refuses_what_isnt_a_package() {
        let dest = scratch("notgz");
        assert!(unpack(b"not an archive", &dest).is_err());
        assert!(package_root(&dest).is_err());
        std::fs::remove_dir_all(&dest).unwrap();
    }

    #[test]
    fn cleans_names() {
        assert_eq!(clean("./a/b/"), "a/b");
        assert_eq!(clean("./"), "");
        assert_eq!(clean("a"), "a");
        assert_eq!(clean("../a"), "../a");
    }
}
