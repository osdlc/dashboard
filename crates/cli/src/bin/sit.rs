//! `sit`: the short name for `sitroom`. Runs the `sitroom` in the same folder
//! (or the one on `PATH`) with the same arguments, so `sit serve` is
//! `sitroom serve`. It only uses std, so it stays a small file.

use std::env;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let mut cmd = Command::new(sitroom());
    cmd.args(env::args_os().skip(1));
    run(cmd)
}

/// `sitroom` next to this program, else the bare name (looked up on `PATH`).
fn sitroom() -> PathBuf {
    let name = format!("sitroom{}", env::consts::EXE_SUFFIX);
    env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(&name)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Becomes `sitroom`: the same process, so signals, the terminal and the exit
/// code are all its own.
#[cfg(unix)]
fn run(mut cmd: Command) -> ExitCode {
    use std::os::unix::process::CommandExt;
    let err = cmd.exec();
    fail(&err)
}

/// Windows can't replace a process: run `sitroom` and pass its exit code on.
#[cfg(not(unix))]
fn run(mut cmd: Command) -> ExitCode {
    match cmd.status() {
        Ok(status) => ExitCode::from(
            status
                .code()
                .map_or(1, |code| u8::try_from(code).unwrap_or(1)),
        ),
        Err(err) => fail(&err),
    }
}

fn fail(err: &std::io::Error) -> ExitCode {
    eprintln!("sit: couldn't run sitroom: {err}");
    ExitCode::from(127)
}
