#![cfg_attr(not(windows), allow(dead_code))]

mod config;
mod keys;
mod machine;
#[cfg(windows)]
mod win;

/// `--chords <dir>`, default `chords` in the current directory.
fn chords_dir() -> std::path::PathBuf {
    let mut args = std::env::args().skip(1);
    let mut dir = "chords".into();
    while let Some(arg) = args.next() {
        if arg == "--chords" {
            if let Some(value) = args.next() {
                dir = value.into();
            }
        }
    }
    dir
}

fn main() {
    #[cfg(windows)]
    win::run(chords_dir());
    #[cfg(not(windows))]
    println!("chord only runs on Windows");
}
