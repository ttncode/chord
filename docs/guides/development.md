# Development

Commands: see [AGENTS.md](../../AGENTS.md#commands).

## Adding a chord action

1. Add the variant to `Action` and its TOML field to the parser in `src/config.rs`, with a parse test there.
2. If it needs new key names, add them to `main_key` in `src/keys.rs`, with a test.
3. Run it in `run_action` in `src/win.rs`.
4. Document the field in the README's action table and, if useful, add a starter chord in `chords/`. `starter_files_have_no_errors` checks the starter files parse.

## Debugging

| Want | Do |
| --- | --- |
| See which app and title a chord resolves against | type an unknown chord (`zz`); the console prints `no chord 'zz' for <exe> "<title>"` |
| See why a button gets no label | run with `--debug-hints`, press `;`, read `hints-debug.txt`: windows tried, child windows, open dialogs, every element with offscreen/enabled/focusable/invokable flags |
| Reproduce a key-timing bug | write a `machine.rs` test with `key_at(vk, down, ms)` timestamps |
| Test what unit tests can't | the manual checklist at the end of `README.md`, on a real Windows machine |

## Rules that will surprise you

| Rule | Where enforced |
| --- | --- |
| Windows-only modules don't compile under `cargo test` on Linux; check them with `cargo check --target x86_64-pc-windows-gnu` | `src/main.rs` `#[cfg(windows)]` mods |
| Never `println!`: use `say!`, which ignores a missing console | `src/main.rs` |
| Never block in the hook callback | `src/win.rs:733` |
| UI Automation runs on the main thread (COM MTA); GDI and windows on the GUI thread only | `src/uia.rs:56`, `src/panel.rs:103` |
| Starter chords must parse: a test loads every file in `chords/` | `src/config.rs` `starter_files_have_no_errors` |
