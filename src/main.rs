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
mod config;
mod keys;
mod machine;
#[cfg(windows)]
mod panel;
#[cfg(windows)]
mod win;
#[cfg(windows)]
mod window;

struct Args {
    chords_dir: std::path::PathBuf,
    background: bool,
}

/// `--chords <dir>` (default `chords` in the current directory), `--background` (no console window).
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
