//! The native-vs-plugin benchmark. The harness reads process CPU and memory
//! through unix calls (getrusage, wait4, /proc or proc_pid_rusage), so it only
//! builds on unix; elsewhere this program just says so. See bench/README.md.

#[cfg(unix)]
mod harness;

#[cfg(unix)]
fn main() -> anyhow::Result<()> {
    harness::main()
}

#[cfg(not(unix))]
fn main() {
    eprintln!("dashboard-bench runs on unix only (macOS, Linux)");
    std::process::exit(1);
}
