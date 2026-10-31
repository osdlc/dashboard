//! Starting child processes the same way on every OS.
//!
//! On Windows a program name is looked up the way a shell does it: in `PATH`,
//! with each `PATHEXT` extension, so `npx` finds `npx.cmd`. `python3` is the
//! first `python3` or `python` in `PATH`, or `py -3` when that is only the
//! Microsoft Store alias. A child can also be put in a job object
//! ([`KillTree`]), so stopping it stops everything it started too (`npx` ->
//! `node`, `cmd` -> `curl`).

use std::path::{Path, PathBuf};

/// The command line to start: `argv` with the program resolved for this OS.
/// A relative program path (`./bin/x`) is made absolute against `dir`, the
/// child's folder, the same on every OS.
pub fn resolve_argv(argv: &[String], dir: Option<&Path>) -> Vec<String> {
    let lookup = cfg!(windows).then(Lookup::from_env);
    resolve(argv, dir, lookup.as_ref())
}

/// Where Windows looks for a program: the `PATH` folders and the `PATHEXT`
/// extensions, and a way to ask whether a file exists (swapped in tests).
struct Lookup<'a> {
    path: Vec<PathBuf>,
    exts: Vec<String>,
    exists: &'a dyn Fn(&Path) -> bool,
}

impl Lookup<'static> {
    fn from_env() -> Self {
        let path = std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default();
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        Self {
            path,
            exts: split_exts(&exts),
            exists: &|p: &Path| p.is_file(),
        }
    }
}

fn split_exts(pathext: &str) -> Vec<String> {
    pathext
        .split(';')
        .map(str::trim)
        .filter(|e| e.starts_with('.') && e.len() > 1)
        .map(str::to_ascii_lowercase)
        .collect()
}

/// `lookup` is `Some` on Windows only.
fn resolve(argv: &[String], dir: Option<&Path>, lookup: Option<&Lookup>) -> Vec<String> {
    let Some((program, args)) = argv.split_first() else {
        return vec![];
    };
    let mut program = program.clone();
    let mut extra: Vec<String> = vec![];
    let as_path = Path::new(&program);
    let is_path = as_path.components().count() > 1;
    if is_path
        && as_path.is_relative()
        && let Some(dir) = dir
    {
        // Absolute even when `dir` is relative: the child also runs in `dir`,
        // which would apply it a second time.
        let joined = dir.join(&program);
        let joined = match joined.has_root() {
            true => joined,
            false => std::path::absolute(&joined).unwrap_or(joined),
        };
        program = joined.display().to_string();
    }
    if let Some(lookup) = lookup {
        if program == "python3" || program == "python" {
            // A `python` early in PATH (a venv, uv, conda) wins. Else `py`, the
            // launcher from the python.org installer: `python3` there is often
            // only the Microsoft Store alias (in `...\Microsoft\WindowsApps`).
            let first = python_on_path(&program, lookup);
            let found = first
                .clone()
                .filter(|p| !is_store_alias(p))
                .map(|p| (p, None))
                .or_else(|| Some((on_path("py", lookup)?, Some("-3"))))
                .or_else(|| first.map(|p| (p, None)));
            if let Some((path, flag)) = found {
                program = path.display().to_string();
                extra.extend(flag.map(String::from));
            }
        } else if Path::new(&program).extension().is_none() {
            let found = if is_path {
                with_ext(Path::new(&program), lookup)
            } else {
                on_path(&program, lookup)
            };
            if let Some(path) = found {
                program = path.display().to_string();
            }
        }
    }
    std::iter::once(program)
        .chain(extra)
        .chain(args.iter().cloned())
        .collect()
}

/// The first `python3` or `python` in `PATH` (`name` first within a folder).
fn python_on_path(name: &str, lookup: &Lookup) -> Option<PathBuf> {
    let other = if name == "python" {
        "python3"
    } else {
        "python"
    };
    lookup.path.iter().find_map(|dir| {
        with_ext(&dir.join(name), lookup).or_else(|| with_ext(&dir.join(other), lookup))
    })
}

/// The Microsoft Store's `python` stubs live in `...\Microsoft\WindowsApps`.
fn is_store_alias(path: &Path) -> bool {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_ascii_lowercase())
        .collect();
    parts
        .windows(2)
        .any(|w| w[0] == "microsoft" && w[1] == "windowsapps")
}

/// `name` in the first `PATH` folder that has it with one of the extensions.
fn on_path(name: &str, lookup: &Lookup) -> Option<PathBuf> {
    lookup
        .path
        .iter()
        .find_map(|dir| with_ext(&dir.join(name), lookup))
}

/// `base` plus the first extension that names an existing file.
fn with_ext(base: &Path, lookup: &Lookup) -> Option<PathBuf> {
    lookup.exts.iter().find_map(|ext| {
        let mut name = base.as_os_str().to_owned();
        name.push(ext);
        let path = PathBuf::from(name);
        (lookup.exists)(&path).then_some(path)
    })
}

/// Windows: a job object holding a child. Dropping it ends the child and
/// every process it started, which killing the child alone would leave
/// running (a `.cmd` shim's `node`, `cmd /C`'s `curl`). The child joins the
/// job just after it starts (std and tokio can't start it suspended), so a
/// process it starts in that moment is not in the job.
#[cfg(windows)]
pub struct KillTree(windows_sys::Win32::Foundation::HANDLE);

// SAFETY: a job handle is a plain kernel handle; it can be used and closed
// from any thread.
#[cfg(windows)]
unsafe impl Send for KillTree {}
#[cfg(windows)]
unsafe impl Sync for KillTree {}

#[cfg(windows)]
impl KillTree {
    /// Put `child` in a new job. `None` if that fails (the child still runs;
    /// only its own process is killed then).
    pub fn new(child: &tokio::process::Child) -> Option<Self> {
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };
        let process = child.raw_handle()? as HANDLE;
        // SAFETY: plain Win32 calls on handles we own. The job is closed by
        // Drop (or here, on failure); the process handle stays the child's.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let job = Self(job);
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let set = SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if set == 0 || AssignProcessToJobObject(job.0, process) == 0 {
                return None;
            }
            Some(job)
        }
    }
}

#[cfg(windows)]
impl Drop for KillTree {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateJobObjectW and is closed once.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    /// Compares the program as a path, so `\\` and `/` both match on Windows.
    #[track_caller]
    fn same(got: Vec<String>, want: &[&str]) {
        assert_eq!(got.len(), want.len(), "{got:?}");
        assert_eq!(Path::new(&got[0]), Path::new(want[0]), "{got:?}");
        assert_eq!(got[1..], argv(&want[1..]), "{got:?}");
    }

    /// A fake Windows: `PATH` is `/a` then `/b`, and only `files` exist.
    fn windows<'a>(files: &'a dyn Fn(&Path) -> bool) -> Lookup<'a> {
        Lookup {
            path: vec!["/a".into(), "/b".into()],
            exts: split_exts(".COM;.EXE;.BAT;.CMD"),
            exists: files,
        }
    }

    #[test]
    fn other_systems_only_make_relative_paths_absolute() {
        let dir = Path::new("/cfg");
        same(
            resolve(&argv(&["node", "x.mjs"]), Some(dir), None),
            &["node", "x.mjs"],
        );
        same(
            resolve(&argv(&["bin/tool", "--fast"]), Some(dir), None),
            &["/cfg/bin/tool", "--fast"],
        );
        same(
            resolve(&argv(&["/usr/bin/x"]), Some(dir), None),
            &["/usr/bin/x"],
        );
        same(resolve(&argv(&["bin/tool"]), None, None), &["bin/tool"]);
        assert!(resolve(&[], None, None).is_empty());
    }

    #[test]
    fn windows_finds_cmd_shims_on_path() {
        let files = |p: &Path| p == Path::new("/b/npx.cmd") || p == Path::new("/a/node.exe");
        let w = windows(&files);
        same(
            resolve(&argv(&["npx", "tsx", "main.ts"]), None, Some(&w)),
            &["/b/npx.cmd", "tsx", "main.ts"],
        );
        same(resolve(&argv(&["node"]), None, Some(&w)), &["/a/node.exe"]);
        // Not found, or given with an extension: left as it is.
        same(resolve(&argv(&["deno"]), None, Some(&w)), &["deno"]);
        same(resolve(&argv(&["npx.cmd"]), None, Some(&w)), &["npx.cmd"]);
    }

    #[test]
    fn windows_adds_the_extension_to_a_path() {
        let files = |p: &Path| p == Path::new("/pkg/bin/tool.exe");
        let w = windows(&files);
        same(
            resolve(&argv(&["bin/tool"]), Some(Path::new("/pkg")), Some(&w)),
            &["/pkg/bin/tool.exe"],
        );
    }

    #[test]
    fn relative_dir_still_gives_an_absolute_program() {
        let got = resolve(&argv(&["./bin/tool"]), Some(Path::new("cfg")), None);
        assert!(Path::new(&got[0]).is_absolute(), "{got:?}");
        assert!(Path::new(&got[0]).ends_with("cfg/bin/tool"), "{got:?}");
    }

    #[test]
    fn windows_python3_prefers_the_first_python_on_path() {
        // A venv's python comes first in PATH: it wins over `py -3`.
        let venv = |p: &Path| {
            ["/a/python.exe", "/b/py.exe", "/b/python3.exe"]
                .iter()
                .any(|f| p == Path::new(f))
        };
        same(
            resolve(&argv(&["python3", "main.py"]), None, Some(&windows(&venv))),
            &["/a/python.exe", "main.py"],
        );
        let no_py = |p: &Path| p == Path::new("/b/python.exe");
        same(
            resolve(&argv(&["python3", "main.py"]), None, Some(&windows(&no_py))),
            &["/b/python.exe", "main.py"],
        );
        let none = |_: &Path| false;
        same(
            resolve(&argv(&["python3", "main.py"]), None, Some(&windows(&none))),
            &["python3", "main.py"],
        );
    }

    #[test]
    fn windows_python3_skips_the_store_alias_for_py() {
        let store = "/Users/me/AppData/Local/Microsoft/WindowsApps";
        let lookup = |files: &'static [&'static str]| {
            move |p: &Path| files.iter().any(|f| p == Path::new(f))
        };
        let with_py = lookup(&[
            "/Users/me/AppData/Local/Microsoft/WindowsApps/python3.exe",
            "/b/py.exe",
        ]);
        let mut w = windows(&with_py);
        w.path = vec![store.into(), "/b".into()];
        same(
            resolve(&argv(&["python3", "main.py"]), None, Some(&w)),
            &["/b/py.exe", "-3", "main.py"],
        );
        // Only the alias: use it, it may still work (or says how to install).
        let alias_only = lookup(&["/Users/me/AppData/Local/Microsoft/WindowsApps/python3.exe"]);
        let mut w = windows(&alias_only);
        w.path = vec![store.into(), "/b".into()];
        same(
            resolve(&argv(&["python3", "main.py"]), None, Some(&w)),
            &[
                "/Users/me/AppData/Local/Microsoft/WindowsApps/python3.exe",
                "main.py",
            ],
        );
    }

    /// Dropping the tree ends what the child started: `ping` here holds the
    /// stdout pipe open, so reading to the end only finishes once it is gone.
    #[cfg(windows)]
    #[tokio::test]
    async fn kill_tree_ends_grandchildren() {
        use tokio::io::AsyncReadExt;
        let mut child = tokio::process::Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1"])
            .stdout(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let tree = KillTree::new(&child).expect("a job object");
        let mut out = child.stdout.take().unwrap();
        drop(tree);
        let mut buf = vec![];
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            out.read_to_end(&mut buf),
        );
        assert!(read.await.is_ok(), "ping still running");
    }

    #[test]
    fn pathext_is_cleaned_up() {
        assert_eq!(split_exts(".EXE; .Cmd;;bad;."), [".exe", ".cmd"]);
    }
}
