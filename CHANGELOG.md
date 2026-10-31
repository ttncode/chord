# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-01

First release. Windows x64; tested on Windows 10 22H2.

### Added

- Chord mode: hold Caps Lock, hold Space, type a chord, release Space. Caps Lock and Space keep working as usual on their own, and the Caps Lock light stays in sync.
- Chord files in TOML, per app (process name and optional window title), re-read on every chord. Actions: `shortcut` (including media keys), `shell`, `palette`, `focus` with `launch`, `mouse`, and `hints`. Regex chords with `$1`–`$9`.
- Chord mode keys: Caps Lock runs the typed chord and stays in chord mode (or repeats the last chord), Shift + letter runs and keeps the typed letters, Backspace and Esc edit the letters.
- Hint labels: `;` (click) and `'` (right-click) put Vimium-style labels on the clickable elements of the window in front, through UI Automation. Tested in File Explorer, Windows Terminal and Postman (an Electron app, including after a minimize); labels only the top open modal.
- Panel listing the chords for the app in front, narrowing as you type; macOS-style look with light and dark mode and fade animations that respect Windows' animation setting.
- Tray icon: open the chords folder, Start with Windows, Quit. Single instance.
- Recovery from lost key releases, lock/unlock and sleep; the keyboard hook is reinstalled every 5 minutes.
- Starter chords for Antigravity, Windows Terminal, LazyVim, and global chords for apps, window layout, desktops, system settings, media and the mouse.
- `--chords <folder>`, `--background` and `--debug-hints` options.

### Known limits

- Chords do nothing while an app running as administrator is in front.
- Elements an app draws without telling UI Automation (custom-drawn apps, Postman's ☰ and window buttons) get no label.
- `chord.exe` is not signed; Windows SmartScreen may warn on first run.

[0.1.0]: https://github.com/ttncode/chord/releases/tag/v0.1.0
