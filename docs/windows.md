# Windows

The CLI (`dashboard.exe`) builds for Windows and compiles there with no warnings. It has only
been cross-compiled from macOS so far, never run on Windows. "Needs a Windows test" below lists
what to check on a real machine.

## Build

On Windows, the normal build works: `cargo build -p dashboard` (with pnpm on the PATH for the
web app). From macOS or Linux, cross-compile with [cargo-xwin](https://github.com/rust-cross/cargo-xwin),
which downloads the MSVC headers and libraries:

```sh
rustup target add x86_64-pc-windows-msvc
cargo install cargo-xwin
# macOS: clang and lld from Homebrew's llvm
PATH="/opt/homebrew/opt/llvm/bin:$PATH" XWIN_ACCEPT_LICENSE=1 \
  cargo xwin build --target x86_64-pc-windows-msvc --profile small -p dashboard
# one bundle:
PATH="/opt/homebrew/opt/llvm/bin:$PATH" XWIN_ACCEPT_LICENSE=1 \
  cargo xwin build --target x86_64-pc-windows-msvc --profile small -p dashboard \
  --no-default-features --features bundle-system
```

The binary is `target/x86_64-pc-windows-msvc/small/dashboard.exe`. `cargo xwin clippy` and
`cargo xwin test --no-run` work the same way, for checking the Windows-only code.

## Size

`--profile small`, the same commit, macOS arm64 vs Windows x86_64 (MSVC):

| Build | macOS arm64 | Windows x86_64 | Windows vs macOS |
|---|---|---|---|
| every bundle (default) | 7.6 MB (7,562,432 bytes) | 8.5 MB (8,486,400 bytes) | +12% |
| `bundle-system` only | 3.5 MB (3,542,688 bytes) | 3.9 MB (3,895,296 bytes) | +10% |
| `bundle-dev` only | 6.0 MB (5,968,240 bytes) | 6.4 MB (6,432,256 bytes) | +8% |
| `bundle-ops` only | 6.0 MB (6,017,872 bytes) | 6.5 MB (6,496,768 bytes) | +8% |
| `bundle-fun` only | 6.4 MB (6,415,744 bytes) | 7.2 MB (7,161,344 bytes) | +12% |
| `bundle-agents` only | 6.1 MB (6,051,024 bytes) | 6.5 MB (6,506,496 bytes) | +8% |
| `bundle-personal` only | 6.4 MB (6,432,144 bytes) | 7.2 MB (7,164,416 bytes) | +11% |
| no bundles | 3.3 MB (3,339,184 bytes) | 3.7 MB (3,655,168 bytes) | +9% |

The Windows binary is 8 to 12% larger in every build (a different CPU architecture as well as a
different OS). The gap between "system only" and "every bundle" (4.0 MB on macOS, 4.6 MB on
Windows) is mostly reqwest and its TLS stack (see [modules.md](modules.md)).

## Files

The config and data folders follow Windows' own layout (the XDG names are a Linux and macOS
thing):

| What | Where |
|---|---|
| Config | `%APPDATA%\dashboard\config.yaml` (or `--config`, or `DASHBOARD_MACHINE_CONFIG`) |
| Installed plugins | `%APPDATA%\dashboard\plugins` |
| Background server record | `%APPDATA%\run\dashboard\serve.json` |
| Background server log | `%APPDATA%\logs\dashboard\serve.log` |

In a module's `config:`, `~/` (and `~\`) means `%USERPROFILE%` when `HOME` is not set.

## How it differs from macOS and Linux

**Starting plugins.** A plugin's program is looked up the way a shell does it: in `PATH`, with
each extension in `PATHEXT`. So `plugin: [npx, tsx, main.ts]` finds `npx.cmd`, and a package's
`run: [bin/tool]` finds `bin\tool.exe`. `python3` (and `python`) is the first `python3` or
`python` in `PATH`, so an activated venv, uv or conda Python is used. When that is only the
Microsoft Store alias (in `...\Microsoft\WindowsApps`), `py -3` is used instead if the Python
launcher is installed. `node` needs nothing special. The same lookup finds the editor for
the TUI's "edit config" (`notepad` when `EDITOR` is not set). `EDITOR` may be a full path with
spaces, such as `C:\Program Files\Notepad++\notepad++.exe`, but then without arguments.

**Stopping plugins.** Each plugin runs in a job object. When the dashboard stops or restarts a
plugin, everything the plugin started stops too (for example the `node` behind an `npx.cmd`
shim). `cmdrunner` does the same, so a timeout stops the whole command. `dashboard stop`
uses `taskkill /T /F`, which ends the background server and its plugins.

**Background server.** `serve --detach` starts the server with a hidden console of its own,
so its plugins and commands don't each open a console window.

**Uptime checks.** A `curl:` line can't go through `sh`, and `cmd /C` would change its quotes
and `%` signs, so on Windows the dashboard splits the line into words itself and runs
`curl.exe` (part of Windows 10 and later) directly:

- `'...'` keeps the text as it is; `"..."` groups words and still expands variables;
  `\"` is a quote inside `"..."`.
- `$VAR`, `${VAR}` and `%VAR%` outside single quotes become the variable's value. An unset
  `$VAR` is empty; an unset `%VAR%`, or a `%` that is not a variable name (`%20` in a URL),
  stays as it is.
- A backslash is kept, so `--cacert C:\certs\ca.pem` works.
- Pipes, `$(...)` and other shell features don't work. Use `cmdrunner` for those.

**cmdrunner.** The command runs with `cmd /S /C`, given exactly as written in the config (in
an extra pair of quotes that `/S` removes, so a quoted program path keeps its quotes).

**Log tail.** Lines may end in `\r\n`. A UTF-16 file (with a byte-order mark, as PowerShell 5
and some Windows tools write) is decoded; so is UTF-8 with a byte-order mark. The file is
opened so the program writing it can keep writing, rename or delete it.

**Links.** The TUI and `serve` open links with `rundll32 url.dll,FileProtocolHandler`, which
uses the default browser and doesn't go through `cmd` (where `&` in a URL would break).

**cswap.** The bundled cswap plugin runs `cswap`, or `cswap.cmd` when only that exists, and
asks Python for UTF-8 output so names and emails are not mangled by the console code page.
Whether `cswap` itself works on Windows is up to cswap.

## Modules with no Windows source yet

These build and load on Windows but have nothing to read there:

- `battery`: reads `pmset` (macOS) or `/sys` (Linux). It says it is not available on Windows.
- `security`: checks macOS and Linux settings only. It shows no checks on Windows.
- `gpu`: uses `nvidia-smi` when it is on the PATH (it is installed with the NVIDIA driver), and
  macOS or Linux sources otherwise.
- `antigravity` (agents): finds its process with `ps` and `lsof`.

The `sysinfo` modules (cpu, memory, network, disk, processes) support Windows.

## Needs a Windows test

None of this has been run on Windows. To check on a real machine (or a Windows CI runner):

1. `dashboard.exe tui` and `dashboard.exe serve` start, draw and quit cleanly, in Windows
   Terminal and in the old console host (`conhost`).
2. `dashboard.exe serve --detach`, then `status` and `stop`: no console window appears, the
   record and log land in the folders above, and `stop` ends the server and its plugins
   (Task Manager shows no `python`, `node` or `cmd` left over).
3. Plugins: `plugin: [python3, x.py]` with the python.org installer (uses `py -3`), with
   only the Microsoft Store Python, and from an activated venv (uses the venv's `python`); `plugin: [npx, tsx, main.ts]` (a `.cmd` shim); a package
   with `run: [bin/tool]` and `bin\tool.exe`.
4. Restarting a plugin (edit its config while it runs) leaves no orphan processes.
5. `uptime` with `url:` and with a `curl:` line that uses quotes and `%VAR%`/`$VAR`.
6. `cmdrunner` with a command that has quotes and `&&`, one that starts with a quoted program
   (`"C:\Program Files\Git\cmd\git.exe" "--version"`), and a timeout that has to stop a
   child (`ping -n 60 127.0.0.1`).
7. `log` on a file another program is writing, on a UTF-16 file, and on a CRLF file.
8. Clicking a link in the TUI and the web view opens the default browser.
9. `~/` paths in module config, with and without `HOME` set.
10. The bundled cswap plugin with cswap installed (`uv tool install`), and the TUI's
    "edit config" with and without `EDITOR` set (also set to a path under `Program Files`).
11. The Windows-only unit test `process::tests::kill_tree_ends_grandchildren` in
    `dashboard-host` (`cargo test -p dashboard-host`), which can't run on macOS.
12. The test suite as a whole (`cargo test --workspace`): some tests start `sh`, `python3` or
    `node` and may need to skip on Windows.
