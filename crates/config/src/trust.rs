//! Which project files may be used. A `.dashboard.yaml` comes with whatever
//! folder you are in, and it can start programs (plugins, `token_command`,
//! built-ins that run commands). So, like `direnv allow`, a project file is
//! only read after `dashboard trust`, and only while its content is the one
//! that was trusted: any change to the file needs a new `dashboard trust`.
//!
//! The list is `<data home>/dashboard/trusted-projects`: one line per file,
//! `<sha256 of the content>  <absolute path>`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use config_layers::tiered_dirs::{Convention, ResolveOptions, TieredDirs, env_vars};
use sha2::{Digest, Sha256};

use crate::{TOOL, VENDOR};

/// The default list: `<data home>/dashboard/trusted-projects`.
pub fn default_path() -> Result<PathBuf> {
    let dirs = TieredDirs::from_env(
        VENDOR,
        &env_vars!("DASHBOARD"),
        ResolveOptions::new().with_convention(Convention::Xdg),
    )?;
    Ok(dirs
        .standalone(TOOL)?
        .durable()
        .into_path()
        .join("trusted-projects"))
}

/// The list of trusted project files.
#[derive(Debug, Clone)]
pub struct Trust {
    pub path: PathBuf,
}

impl Trust {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn open_default() -> Result<Self> {
        Ok(Self::new(default_path()?))
    }

    /// True when `project` is on the list with its current content.
    pub fn allows(&self, project: &Path) -> bool {
        std::fs::read(project).is_ok_and(|bytes| self.allows_bytes(project, &bytes))
    }

    /// True when `project` is on the list with `bytes` as its content. For a
    /// caller that reads the file once and uses the same bytes.
    pub fn allows_bytes(&self, project: &Path, bytes: &[u8]) -> bool {
        let key = (hash(bytes), absolute(project));
        self.entries().contains(&key)
    }

    /// Trust `project` as it is now. Replaces an older entry for the same file.
    pub fn allow(&self, project: &Path) -> Result<()> {
        let (hash, path) = key(project)?;
        let mut entries = self.entries();
        entries.retain(|(_, p)| p != &path);
        entries.push((hash, path));
        self.save(&entries)
    }

    /// Take `project` off the list. False when it wasn't on it.
    pub fn remove(&self, project: &Path) -> Result<bool> {
        let path = absolute(project);
        let mut entries = self.entries();
        let before = entries.len();
        entries.retain(|(_, p)| p != &path);
        if entries.len() == before {
            return Ok(false);
        }
        self.save(&entries)?;
        Ok(true)
    }

    /// The list's lines. A missing or unreadable list is empty.
    fn entries(&self) -> Vec<(String, PathBuf)> {
        let text = std::fs::read_to_string(&self.path).unwrap_or_default();
        text.lines()
            .filter_map(|line| {
                let (hash, path) = line.split_once("  ")?;
                Some((hash.to_string(), PathBuf::from(path)))
            })
            .collect()
    }

    fn save(&self, entries: &[(String, PathBuf)]) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let text: String = entries
            .iter()
            .map(|(hash, path)| format!("{hash}  {}\n", path.display()))
            .collect();
        // Written to a temporary file and renamed, so a crash mid-write
        // leaves the old list whole.
        etag_write::write_atomic(&self.path, text.as_bytes())
            .with_context(|| format!("writing {}", self.path.display()))
    }
}

/// The file's content hash and absolute path.
fn key(project: &Path) -> Result<(String, PathBuf)> {
    let bytes = std::fs::read(project).with_context(|| format!("reading {}", project.display()))?;
    Ok((hash(&bytes), absolute(project)))
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Symlinks resolved when the file exists, so one file has one entry.
fn absolute(path: &Path) -> PathBuf {
    std::fs::canonicalize(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_follows_the_content() {
        let dir = std::env::temp_dir().join(format!("dashboard-trust-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let project = dir.join(".dashboard.yaml");
        std::fs::write(&project, "screens: []\n").unwrap();
        let trust = Trust::new(dir.join("data/trusted-projects"));
        assert!(!trust.allows(&project));
        trust.allow(&project).unwrap();
        assert!(trust.allows(&project));
        // Trusting again keeps one entry.
        trust.allow(&project).unwrap();
        assert_eq!(trust.entries().len(), 1);
        // Any change needs a new `dashboard trust`.
        std::fs::write(&project, "screens: []\nmodules: {}\n").unwrap();
        assert!(!trust.allows(&project));
        trust.allow(&project).unwrap();
        assert!(trust.remove(&project).unwrap());
        assert!(!trust.allows(&project));
        assert!(!trust.remove(&project).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
