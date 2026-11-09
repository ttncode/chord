# chord

Keyboard chords for Windows: hold Caps Lock + Space, type letters, release Space, and an action from a TOML chord file runs in the app in front. One native Rust binary.

## Setup

1. Install Rust 1.85+ (edition 2024) on Windows.
2. Without Visual Studio Build Tools, use the GNU toolchain in the project folder: `rustup override set stable-x86_64-pc-windows-gnu`.
3. `cargo run --release` loads chord files from `chords\` in the current folder (else `%APPDATA%\chord\chords`) and prints to the terminal it runs in.

## Commands

| Task | Command | Evidence |
| --- | --- | --- |
| Test (all) | `cargo test` | executed: 131 tests pass |
| Test (single) | `cargo test <name>`, e.g. `cargo test hint_prefix` | executed |
| Check the Windows build from Linux/WSL | `cargo check --target x86_64-pc-windows-gnu` | executed |
| Run | `cargo run --release` | executed on Windows by the maintainer |
| Run with another chord folder | `cargo run --release -- --chords D:\my-chords` | `src/main.rs` `args` |
| Debug hint labels | `cargo run --release -- --debug-hints` (writes `hints-debug.txt`) | `src/main.rs` `args`, `src/uia.rs` |
| Installer (local) | `cargo build --release`, then `iscc /DVersion=<version> installer\chord.iss` → `target\installer\` | `unverified: needs Inno Setup 6; CI runs it` |
| Release | bump `version` in `Cargo.toml`, add the `CHANGELOG.md` section, push tag `v<version>`; CI makes a draft release to test and publish | `.github/workflows/release.yml` |

## Architecture

Three threads: the **hook thread** runs the low-level keyboard hook and the pure `Machine`; the **main thread** runs effects (SendInput, chord lookup, UI Automation scans); the **GUI thread** owns the panel, the hint overlay and the tray icon. They talk only through a bounded channel and posted messages. See [docs/architecture/overview.md](docs/architecture/overview.md).

| Module | Holds | Tested on any OS |
| --- | --- | --- |
| `machine.rs` | key state machine: chord mode, buffer, Caps/Space/Shift rules, lost-release recovery | yes |
| `config.rs` | chord file parsing, regex chords, which file wins | yes |
| `keys.rs` | shortcut text → virtual-key combos | yes |
| `hints.rs` | label strings, which elements get a label | yes |
| `style.rs` | colours, sizes, easing (pure) | yes |
| `layout.rs` | `window` chord layouts: frames, placement, stepping on repeat (pure) | yes |
| `update.rs` | tray "Check for updates": release parsing and version compare (pure), download and silent install | parsing yes |
| `win.rs` | hook, effect loop, actions | no (`cfg(windows)`) |
| `panel.rs`, `canvas.rs` | layered panel, overlay, tray; per-pixel-alpha drawing | no |
| `uia.rs` | UI Automation scan for clickable elements | no |
| `window.rs`, `autostart.rs` | window focus, window moves, mouse input; Start with Windows | no |

## Conventions

- Logic that can be tested without a keyboard goes in a pure module with unit tests; Windows calls stay in the `cfg(windows)` modules. `main.rs` carries `#![cfg_attr(not(windows), allow(dead_code))]` so `cargo test` runs on Linux.
- The hook callback must never block: it decides block/pass and hands effects over with `try_send`. Anything slow belongs to the main thread.
- Every injected key or mouse event carries `dwExtraInfo = TAG` (`src/win.rs:55`); the hook ignores only those, plus any injected Caps Lock (remote desktop and VM clients inject it to sync the light; reacting loops forever).
- Print with `say!` (never `println!`): chord is a GUI-subsystem app and often has no console. Messages a user must see without a terminal go through `win::message`.
- Bug fixes start with a failing test when the logic is in a pure module.

## Gotchas

- Caps Lock light: the keyboard flips it below the hook; chord resyncs it with two tagged toggles 60 ms apart.
- `installer/chord.iss` `AppId` must never change: upgrades and the uninstaller find the installed copy by it.
- CI builds with `--locked`: commit `Cargo.lock` after changing dependencies or the version.
- Never pass an empty string to `DrawTextW` (access violation).
- Electron apps (Postman, Antigravity) drop their page from the window's UI Automation tree after a minimize; read from `Chrome_RenderWidgetHostHWND`.
- Details and the rest: [docs/architecture/risks.md](docs/architecture/risks.md).

## Pull requests

- Commits: Conventional Commits (`feat:`, `fix:`, `docs:`, `style:`), as in `git log`.
- Before review: `cargo test` and `cargo check --target x86_64-pc-windows-gnu` pass; Windows-only changes get a run through the manual checklist in `README.md`.
