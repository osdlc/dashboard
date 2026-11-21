//! `sit` runs the `sitroom` beside it with the same arguments.

use std::process::Command;

fn run(bin: &str, args: &[&str]) -> (Option<i32>, String) {
    let out = Command::new(bin)
        .args(args)
        .output()
        .expect("running the binary");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
    )
}

#[test]
fn sit_is_sitroom() {
    let sit = env!("CARGO_BIN_EXE_sit");
    let sitroom = env!("CARGO_BIN_EXE_sitroom");
    assert_eq!(run(sit, &["--version"]), run(sitroom, &["--version"]));
    // A failing command fails the same way.
    let (code, _) = run(sit, &["no-such-command"]);
    assert_eq!(code, run(sitroom, &["no-such-command"]).0);
    assert_ne!(code, Some(0));
}
