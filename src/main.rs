#![cfg_attr(not(windows), allow(dead_code))]

mod chords;
mod config;
mod keys;
mod machine;
#[cfg(windows)]
mod win;

fn main() {
    #[cfg(windows)]
    win::run();
    #[cfg(not(windows))]
    println!("chord only runs on Windows");
}
