#![cfg_attr(not(windows), allow(dead_code))]
// A tray app: no console window of its own. Started from a terminal, it prints there (see `win::attach_console`).
#![cfg_attr(not(test), windows_subsystem = "windows")]

/// `println!` that never panics: started without a terminal there is no console to write to.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stdout(), $($arg)*);
    }};
}

#[cfg(windows)]
mod autostart;
#[cfg(windows)]
mod canvas;
mod config;
mod hints;
mod keys;
mod machine;
mod style;
mod update;
#[cfg(windows)]
mod panel;
#[cfg(windows)]
mod uia;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod window;

struct Args {
    chords_dir: Option<std::path::PathBuf>,
    background: bool,
}

/// `--chords <dir>` (default: `config::default_dir`), `--background` (never print to the terminal
/// it was started from), `--debug-hints` (write what each label scan saw to `hints-debug.txt`).
fn args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args { chords_dir: None, background: false };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--chords" => {
                if let Some(value) = args.next() {
                    parsed.chords_dir = Some(value.into());
                }
            }
            "--background" => parsed.background = true,
            #[cfg(windows)]
            "--debug-hints" => uia::DEBUG.store(true, std::sync::atomic::Ordering::Relaxed),
            _ => {}
        }
    }
    parsed
}

fn main() {
    #[cfg(windows)]
    {
        let args = args();
        let in_terminal = !args.background && win::attach_console();
        if !win::claim_single_instance() {
            if in_terminal {
                say!("chord is already running (see the tray icon).");
            } else {
                win::message("chord is already running. Its icon is in the notification area of the taskbar.");
            }
            return;
        }
        let chords_dir = args.chords_dir.unwrap_or_else(|| {
            let per_user = std::env::var_os("APPDATA").map(|d| std::path::Path::new(&d).join("chord").join("chords"));
            let dir = config::default_dir(std::path::Path::new("chords"), per_user);
            if let Err(e) = config::create_with_starters(&dir) {
                win::message(&format!("chord could not create its chord files in {}: {e}", dir.display()));
            }
            dir
        });
        win::run(chords_dir);
    }
    #[cfg(not(windows))]
    say!("chord only runs on Windows");
}
