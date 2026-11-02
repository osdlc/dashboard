//! Where packages and indexes come from (a path or a URL), and the record
//! of what was installed from where.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use dashboard_config::packages::market_index_path;
use dashboard_contract::Index;
use dashboard_contract::marketplace::INDEX_FORMAT;
use reqwest::Url;
use serde::{Deserialize, Serialize};

use super::archive::MAX_ARCHIVE;

/// A file on disk or on the web.
#[derive(Debug, Clone, PartialEq)]
pub enum Location {
    Path(PathBuf),
    Web(Url),
}

impl Location {
    /// `https://…` and `http://…` are web URLs, `file://…` and anything else
    /// a path. A relative path is made absolute.
    pub fn parse(s: &str) -> Result<Self> {
        if s.starts_with("https://") || s.starts_with("http://") {
            return Ok(Self::Web(Url::parse(s).with_context(|| format!("{s:?}"))?));
        }
        let path = if s.starts_with("file://") {
            Url::parse(s)
                .ok()
                .and_then(|u| u.to_file_path().ok())
                .with_context(|| format!("{s:?} is not a file URL"))?
        } else {
            PathBuf::from(s)
        };
        Ok(Self::Path(std::path::absolute(&path)?))
    }

    /// An index: like `parse`, but not plain `http://`. The index supplies
    /// each package's hash, so one that could be changed on the way would
    /// make the check worthless.
    pub fn parse_index(s: &str) -> Result<Self> {
        if s.starts_with("http://") {
            bail!("the index {s} must be https://, a file:// URL or a path, not http://");
        }
        Self::parse(s)
    }

    pub fn is_web(s: &str) -> bool {
        ["https://", "http://", "file://"]
            .iter()
            .any(|p| s.starts_with(p))
    }

    /// `rel` read against this file: a URL stays as it is, a relative path
    /// is next to this file.
    pub fn join(&self, rel: &str) -> Result<Self> {
        if Self::is_web(rel) {
            return Self::parse(rel);
        }
        match self {
            Self::Path(p) => Ok(Self::Path(p.parent().unwrap_or(Path::new("/")).join(rel))),
            Self::Web(u) => Ok(Self::Web(u.join(rel)?)),
        }
    }

    /// A package's `url` in this index. Never plain `http://`, and an index
    /// on the web only points at the web, never at a local file.
    pub fn entry(&self, rel: &str) -> Result<Self> {
        let at = self.join(rel)?;
        match (&at, self) {
            (Self::Web(u), _) if u.scheme() != "https" => {
                bail!(
                    "{u}: a package in an index must be https://, not {}",
                    u.scheme()
                )
            }
            (Self::Path(p), Self::Web(_)) => {
                bail!(
                    "the index {self} is on the web, so it can't point at the local file {}",
                    p.display()
                )
            }
            _ => Ok(at),
        }
    }

    /// Read it all, refusing anything over the size limit.
    pub async fn fetch(&self) -> Result<Vec<u8>> {
        match self {
            Self::Path(p) => {
                let len = std::fs::metadata(p)
                    .with_context(|| format!("reading {}", p.display()))?
                    .len();
                if len > MAX_ARCHIVE {
                    bail!("{} is over {} MiB", p.display(), MAX_ARCHIVE >> 20);
                }
                Ok(std::fs::read(p).with_context(|| format!("reading {}", p.display()))?)
            }
            Self::Web(url) => {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(60))
                    .build()?;
                let mut resp = client
                    .get(url.clone())
                    .send()
                    .await
                    .and_then(|r| r.error_for_status())
                    .with_context(|| format!("downloading {url}"))?;
                let mut out = vec![];
                while let Some(chunk) = resp.chunk().await? {
                    out.extend_from_slice(&chunk);
                    if out.len() as u64 > MAX_ARCHIVE {
                        bail!("{url} is over {} MiB", MAX_ARCHIVE >> 20);
                    }
                }
                Ok(out)
            }
        }
    }
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(p) => write!(f, "{}", p.display()),
            Self::Web(u) => write!(f, "{u}"),
        }
    }
}

/// Read and check an index.
pub async fn load_index(at: &Location) -> Result<Index> {
    let bytes = at.fetch().await?;
    let index: Index =
        serde_json::from_slice(&bytes).with_context(|| format!("reading the index {at}"))?;
    if index.format != INDEX_FORMAT {
        bail!(
            "the index {at} is format {}; this dashboard reads format {INDEX_FORMAT}",
            index.format
        );
    }
    if let Err(err) = cache_index(&bytes) {
        eprintln!("could not keep a copy of the index for the web editor: {err:#}");
    }
    Ok(index)
}

/// Keep the last index read at `<data home>/dashboard/marketplace/index.json`,
/// where the web editor's catalog lists the packages that aren't installed.
fn cache_index(bytes: &[u8]) -> Result<()> {
    let path = market_index_path()?;
    let dir = path.parent().expect("index path has a folder");
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = dir.join(".index.json.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, &path).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

/// `installed.json` in the plugins folder: what `plugin install` put there,
/// so `update` knows where each one came from.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Record {
    #[serde(default)]
    pub plugins: BTreeMap<String, Origin>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Origin {
    pub version: String,
    /// sha256 of the archive it came from.
    pub sha256: String,
    /// The archive, folder or URL it was installed from.
    pub from: String,
    /// The index it was found in, when installed by ID.
    #[serde(default)]
    pub index: Option<String>,
}

const RECORD_FILE: &str = "installed.json";

impl Record {
    /// The record in `plugins`, and its etag for the write back. A missing
    /// file is an empty record.
    pub fn read(plugins: &Path) -> Result<(Self, Option<String>)> {
        let path = plugins.join(RECORD_FILE);
        match std::fs::read(&path) {
            Ok(bytes) => {
                let record = serde_json::from_slice(&bytes)
                    .with_context(|| format!("reading {}", path.display()))?;
                Ok((record, Some(etag_write::etag(&bytes))))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((Self::default(), None)),
            Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
        }
    }

    /// Change the record on disk. Fails, writing nothing, if another
    /// install changed it at the same time.
    pub fn update(plugins: &Path, change: impl FnOnce(&mut Self)) -> Result<()> {
        let (mut record, etag) = Self::read(plugins)?;
        change(&mut record);
        let mut text = serde_json::to_string_pretty(&record)?;
        text.push('\n');
        etag_write::write_cas(&plugins.join(RECORD_FILE), text.as_bytes(), etag.as_deref())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn locations() {
        let index = Location::parse("/srv/market/index.json").unwrap();
        assert_eq!(
            index.join("packages/a.tar.gz").unwrap(),
            Location::Path("/srv/market/packages/a.tar.gz".into())
        );
        assert_eq!(
            index.join("file:///x/a.tar.gz").unwrap(),
            Location::Path("/x/a.tar.gz".into())
        );
        let web = Location::parse("https://example.com/m/index.json").unwrap();
        assert_eq!(
            web.join("p/a.tar.gz").unwrap().to_string(),
            "https://example.com/m/p/a.tar.gz"
        );
        assert!(matches!(
            index.join("https://example.com/a.tar.gz").unwrap(),
            Location::Web(_)
        ));
        assert_eq!(
            web.join("/p/a.tar.gz").unwrap().to_string(),
            "https://example.com/p/a.tar.gz"
        );
        assert!(
            Location::parse("relative/index.json")
                .unwrap()
                .to_string()
                .starts_with('/')
        );
    }

    #[cfg(unix)]
    #[test]
    fn index_entries_stay_checked() {
        // The index gives the hash, so it can't come over plain http.
        assert!(Location::parse_index("http://example.com/index.json").is_err());
        assert!(Location::parse_index("https://example.com/index.json").is_ok());
        let web = Location::parse_index("https://example.com/m/index.json").unwrap();
        assert!(web.entry("p/a.tar.gz").is_ok());
        assert!(web.entry("http://example.com/a.tar.gz").is_err());
        // A web index never points at a local file.
        assert!(web.entry("file:///etc/passwd").is_err());
        let local = Location::parse_index("/srv/market/index.json").unwrap();
        assert!(local.entry("/srv/other/a.tar.gz").is_ok());
        assert!(local.entry("https://example.com/a.tar.gz").is_ok());
        assert!(local.entry("http://example.com/a.tar.gz").is_err());
    }
}
