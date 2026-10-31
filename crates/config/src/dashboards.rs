//! Saved dashboards: named configs kept as
//! `<config home>/dashboard/dashboards/<name>.yaml`, e.g.
//! `~/.config/dashboard/dashboards/work.yaml`. A dashboard file has the same
//! keys as `config.yaml`. `dashboard --dashboard work` lays it over
//! `config.yaml` (see `layers`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::TOOL;

/// `<config home>/dashboard/dashboards`. Moved by `XDG_CONFIG_HOME` or
/// `DASHBOARD_CONFIG_DIR`.
pub fn default_dir() -> Result<PathBuf> {
    Ok(crate::tiered_dirs()?
        .standalone(TOOL)?
        .config()
        .into_path()
        .join("dashboards"))
}

/// A saved dashboard found on disk.
#[derive(Debug, Clone, PartialEq)]
pub struct Saved {
    pub name: String,
    pub path: PathBuf,
    /// Its `description:`, if it has one and the file reads.
    pub description: Option<String>,
}

/// The folder of saved dashboards.
#[derive(Debug, Clone)]
pub struct Registry {
    pub dir: PathBuf,
}

impl Registry {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The registry in the user's config home.
    pub fn open_default() -> Result<Self> {
        Ok(Self::new(default_dir()?))
    }

    /// Every `*.yaml` in the folder, sorted by name, and a problem for each
    /// one that doesn't read as a config (it is still listed). A missing
    /// folder is an empty list.
    pub fn list(&self) -> (Vec<Saved>, Vec<String>) {
        let mut saved = vec![];
        let mut errors = vec![];
        let Ok(read) = std::fs::read_dir(&self.dir) else {
            return (saved, errors);
        };
        for path in read.filter_map(|e| e.ok()).map(|e| e.path()) {
            if path.extension().is_none_or(|e| e != "yaml") || !path.is_file() {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            if check_name(name).is_err() {
                errors.push(format!(
                    "{}: not a dashboard name (use lowercase letters, digits and dashes)",
                    path.display()
                ));
                continue;
            }
            let description = match read_config(&path) {
                Ok(c) => c.description,
                Err(err) => {
                    errors.push(format!("{err:#}"));
                    None
                }
            };
            saved.push(Saved {
                name: name.to_string(),
                path,
                description,
            });
        }
        saved.sort_by(|a, b| slug_collate::compare(&a.name, &b.name));
        (saved, errors)
    }

    /// Where dashboard `name` is kept, whether or not it exists.
    pub fn path(&self, name: &str) -> Result<PathBuf> {
        check_name(name)?;
        Ok(self.dir.join(format!("{name}.yaml")))
    }

    /// The file of an existing dashboard. The error names the ones there are.
    pub fn find(&self, name: &str) -> Result<PathBuf> {
        let path = self.path(name)?;
        if path.is_file() {
            return Ok(path);
        }
        let (saved, _) = self.list();
        let names: Vec<&str> = saved.iter().map(|s| s.name.as_str()).collect();
        if names.is_empty() {
            bail!(
                "no dashboard {name:?}: {} has none yet (add one with `dashboard import`)",
                self.dir.display()
            );
        }
        bail!(
            "no dashboard {name:?} in {} (there is: {})",
            self.dir.display(),
            names.join(", ")
        )
    }

    /// Save `text` as dashboard `name`. It must parse as a config. An
    /// existing dashboard is only replaced when `replace` is set.
    pub fn save(&self, name: &str, text: &str, replace: bool) -> Result<PathBuf> {
        let path = self.path(name)?;
        crate::parse(text).with_context(|| format!("dashboard {name:?}"))?;
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("creating {}", self.dir.display()))?;
        let result = if replace {
            etag_write::write_atomic(&path, text.as_bytes())
        } else {
            // No etag: refused if the file is already there.
            etag_write::write_cas(&path, text.as_bytes(), None).map(|_| ())
        };
        match result {
            Ok(()) => Ok(path),
            Err(etag_write::WriteError::ExistsWithoutEtag { .. }) => bail!(
                "dashboard {name:?} already exists ({}); pass --force to replace it, or --name to pick another",
                path.display()
            ),
            Err(err) => Err(err).with_context(|| format!("writing {}", path.display())),
        }
    }
}

/// A dashboard name is a slug: lowercase letters, digits and dashes.
pub fn check_name(name: &str) -> Result<()> {
    if name.is_empty() || slug_collate::slugify(name) != name {
        bail!(
            "dashboard name {name:?}: use lowercase letters, digits and dashes, e.g. {:?}",
            name_for(name)
        );
    }
    Ok(())
}

/// A dashboard name made from a file name or title, e.g. `My Work.yaml` gives
/// `my-work`. `dashboard` when nothing usable is left.
pub fn name_for(hint: &str) -> String {
    let stem = hint
        .strip_suffix(".yaml")
        .or_else(|| hint.strip_suffix(".yml"))
        .unwrap_or(hint);
    let slug = slug_collate::slugify(stem);
    if slug.is_empty() {
        "dashboard".into()
    } else {
        slug
    }
}

fn read_config(path: &Path) -> Result<crate::Config> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    crate::parse(&text).with_context(|| format!("in {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("dashboard-registry-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn names_are_slugs() {
        assert!(check_name("work").is_ok());
        assert!(check_name("on-call-2").is_ok());
        assert!(check_name("Work").is_err());
        assert!(check_name("../x").is_err());
        assert!(check_name("").is_err());
        assert_eq!(name_for("My Work.yaml"), "my-work");
        assert_eq!(name_for("???"), "dashboard");
    }

    #[test]
    fn save_list_find() {
        let dir = temp("save");
        let reg = Registry::new(&dir);
        assert!(reg.list().0.is_empty());
        assert!(reg.find("work").is_err());

        reg.save("work", "description: Day job\ntheme: dark\n", false)
            .unwrap();
        reg.save("home", "theme: light\n", false).unwrap();
        // Not a config: refused, nothing written.
        assert!(reg.save("bad", "themes: dark\n", false).is_err());
        assert!(!dir.join("bad.yaml").exists());
        // Already there: refused unless replacing.
        let err = reg.save("work", "theme: auto\n", false).unwrap_err();
        assert!(format!("{err:#}").contains("--force"), "{err:#}");
        reg.save("work", "description: Day job\ntheme: auto\n", true)
            .unwrap();

        std::fs::write(dir.join("broken.yaml"), "theme: [\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "not a dashboard").unwrap();
        let (saved, errors) = reg.list();
        let names: Vec<&str> = saved.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["broken", "home", "work"]);
        assert_eq!(saved[2].description.as_deref(), Some("Day job"));
        assert_eq!(errors.len(), 1, "{errors:?}");

        assert_eq!(reg.find("home").unwrap(), dir.join("home.yaml"));
        let err = reg.find("play").unwrap_err().to_string();
        assert!(err.contains("broken, home, work"), "{err}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn default_dir_follows_the_config_home() {
        // Test binaries get a scratch root from tiered-dirs; the folder is
        // `dashboard/dashboards` under its config home.
        let dir = default_dir().unwrap();
        assert!(dir.ends_with("dashboard/dashboards"), "{}", dir.display());
    }
}
