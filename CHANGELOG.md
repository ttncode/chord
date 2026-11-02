# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Changed

- The chord groups sit on the right of the panel's "N more" line instead of taking a line of their own.

## [0.3.1] - 2026-11-02

### Added

- `[groups]` in chord files names chord prefixes (`"/w" = "Window"`). The panel lists the groups that match what you typed on one small line, so you can find a chord without remembering its first letter.
- Starter chords `/th` and `/tl` move the current tab left and right in Chrome, Firefox, Antigravity and Windows Terminal.

## [0.3.0] - 2026-11-02

### Added

- Tray icon → **Check for updates**: finds the newest release on GitHub, and after you confirm, downloads its installer and installs it silently; chord starts again by itself.
- Loop breaker: if chord has to act 30 times within 2 seconds (Caps Lock sends or chord runs), another program is feeding its keys back to it. chord lets go of the keyboard at once, says which app was in front, and quits, instead of hanging the PC.

## [0.2.1] - 2026-11-02

### Fixed

- Pressing Caps Lock while a Remote Desktop window was in front could hang the PC for minutes (CPU at 100%, screen flickering): the Remote Desktop client re-sends Caps Lock to keep the light in sync, and chord treated each one as a new press. chord now ignores Caps Lock sent by other programs, and leaves Caps Lock alone while a Remote Desktop (`mstsc`, `msrdc`) or VM (Hyper-V, VMware, VirtualBox) window is in front, so chords reach the chord on the remote PC.

## [0.2.0] - 2026-11-01

### Added

- Installer (`chord-setup-<version>.exe`): per-user install to `%LOCALAPPDATA%\Programs\chord` without an admin prompt, Start menu shortcut, optional start at sign-in, upgrade in place, uninstall from Settings → Apps.
- App icon.
- Releases are built and published by GitHub Actions: installer, portable zip and `SHA256SUMS.txt`.

### Changed

- chord no longer opens a console window. Started from a terminal, it prints there; otherwise it lives only in the tray. A second copy shows a message instead of printing one.
- Chord files default to `%APPDATA%\chord\chords`, created from the built-in starter set on first start. A `chords` folder in the current folder (portable zip, development) still wins.

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

[Unreleased]: https://github.com/ttncode/chord/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/ttncode/chord/releases/tag/v0.3.1
[0.3.0]: https://github.com/ttncode/chord/releases/tag/v0.3.0
[0.2.1]: https://github.com/ttncode/chord/releases/tag/v0.2.1
[0.2.0]: https://github.com/ttncode/chord/releases/tag/v0.2.0
[0.1.0]: https://github.com/ttncode/chord/releases/tag/v0.1.0
