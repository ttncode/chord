#![cfg_attr(not(windows), allow(dead_code))]

/// `println!` that never panics: with `--background` there is no console to write to.
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
#[cfg(windows)]
mod panel;
#[cfg(windows)]
mod uia;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod window;

struct Args {
    chords_dir: std::path::PathBuf,
    background: bool,
}

/// `--chords <dir>` (default `chords` in the current directory), `--background` (no console window),
/// `--debug-hints` (write what each label scan saw to `hints-debug.txt`).
fn args() -> Args {
    let mut args = std::env::args().skip(1);
    let mut parsed = Args { chords_dir: "chords".into(), background: false };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--chords" => {
                if let Some(value) = args.next() {
                    parsed.chords_dir = value.into();
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
        if !win::claim_single_instance() {
            say!("chord is already running (see the tray icon).");
            return;
        }
        if args.background {
            win::detach_console();
        }
        win::run(args.chords_dir);
    }
    #[cfg(not(windows))]
    say!("chord only runs on Windows");
}
