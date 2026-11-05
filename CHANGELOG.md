# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/).

## [0.4.8] - 2026-11-05

### Fixed

- Labels now show on the content of Windows Settings and other UWP apps, not only on their title bar buttons.
- Overlapping elements (an avatar and its status badge, an icon spilling out of its button) get one label instead of two stacked on top of each other.

## [0.4.7] - 2026-11-05

### Fixed

- The pointer could still stay hidden after chord moved it: the mouse move landed on the spot already set, and Windows ignores a move that goes nowhere. It now steps 1 px aside first.

## [0.4.6] - 2026-11-05

### Fixed

- Arrow glide hides the chord list, so it no longer covers the pointer.
- The pointer no longer stays invisible after chord moves it (arrow glide, label clicks, scroll aiming): the move is sent as real mouse input, which also triggers hover effects.

## [0.4.5] - 2026-11-05

### Added

- Arrow keys in chord mode move the mouse pointer: a tap nudges it, holding glides it slowly at first, then up to full speed. Click there with `/mc`.
- Antigravity starter chord `fr`: reveal the current file in the Explorer tree.

## [0.4.4] - 2026-11-05

### Fixed

- Scroll chords (`j` `k` `h` `l`, `/mj`…) scroll the page even when the pointer rests on a header, a sidebar or another window: the pointer first moves onto the biggest scrolling area of the window in front.

## [0.4.3] - 2026-11-05

### Added

- `Tab` in chord mode opens search: let go of Caps Lock and Space, type words to find a chord by name or keys, `Down` / `Up` to pick, `Enter` to run, `Esc` to close.

### Removed

- `Down` / `Up` in chord mode from 0.4.2. Search replaces it.

## [0.4.2] - 2026-11-04

### Added

- `Down` / `Up` in chord mode highlight a chord in the list and scroll past "N more"; releasing Space runs the highlighted chord. Finds a chord whose letters you forgot.

## [0.4.1] - 2026-11-03

### Changed

- `--debug-hints` prints where a picked label clicks and what Windows reports under that point.

## [0.4.0] - 2026-11-03

### Added

- Starter chord `]` double-clicks a label: opens a file in File Explorer, where `;` only selects it.

### Fixed

- Labels reach items inside an open folder of a tree (GitHub's file tree: a folder's files got none), and the open folder's own label clicks its row instead of the middle of everything under it.
- A label on a list or tree row clicks near the row's name, not its middle: File Explorer's rows run past the visible columns, so the click missed.

## [0.3.9] - 2026-11-03

### Fixed

- `/ms` also labels navigation areas, so web sidebars Chrome doesn't report as scrollable (ChatGPT's chat list) can be picked; `j` `k` then scroll them.

## [0.3.8] - 2026-11-03

### Fixed

- `/ms` labels only areas at least 80 px on each side that hide a real part of their content. Chrome calls anything that overflows by a pixel scrollable, so prompt boxes, tables and message blocks got labels too.
- `--debug-hints` also lists every element that reports a scroll pattern when `/ms` runs.

## [0.3.7] - 2026-11-03

### Added

- `hints = "scroll"` labels the scrollable areas of the window and moves the pointer to the picked one, so `j` `k` scroll that area. Apps that report no scroll areas (Electron editors, chat apps) get labels on their clickable elements instead. Starter chord: `/ms`.

## [0.3.6] - 2026-11-03

### Added

- `hints = "middle-click"` middle-clicks the chosen label, so a link opens in a new background tab. Starter chord: `[`.

## [0.3.5] - 2026-11-02

### Changed

- After a `repeat` chord runs, tapping its last letter again runs it again, and tapping another letter runs the chord with that letter instead: `j j j k` scrolls down three times, then up; `/vk k k j` raises the volume three times, then lowers it.

## [0.3.4] - 2026-11-02

### Added

- `repeat = true` on a chord runs it when its last letter is pressed and keeps running it while the letter is held. Scrolling glides in small steps; other actions repeat 10 times a second.
- With nothing typed, `j` `k` `h` `l` scroll down, up, left and right; hold to keep scrolling. Volume (`/vj` `/vk`), tab (`/th` `/tl`, Windows Terminal `tn` `tp`), desktop (`/dh` `/dl`) and mouse scroll (`/mj` …) chords repeat while held.

## [0.3.3] - 2026-11-02

### Added

- Starter chord `/sh` opens the hidden tray icons (the `^` by the clock): `Win+B`, then `Enter`.
- `on = "taskbar"` on a `hints` chord labels the taskbar (app buttons, tray icons, clock) instead of the window in front. Starter chords: `/;` clicks, `/'` right-clicks.

## [0.3.2] - 2026-11-02

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

[0.4.8]: https://github.com/ttncode/chord/releases/tag/v0.4.8
[0.4.7]: https://github.com/ttncode/chord/releases/tag/v0.4.7
[0.4.6]: https://github.com/ttncode/chord/releases/tag/v0.4.6
[0.4.5]: https://github.com/ttncode/chord/releases/tag/v0.4.5
[0.4.4]: https://github.com/ttncode/chord/releases/tag/v0.4.4
[0.4.3]: https://github.com/ttncode/chord/releases/tag/v0.4.3
[0.4.2]: https://github.com/ttncode/chord/releases/tag/v0.4.2
[0.4.1]: https://github.com/ttncode/chord/releases/tag/v0.4.1
[0.4.0]: https://github.com/ttncode/chord/releases/tag/v0.4.0
[0.3.9]: https://github.com/ttncode/chord/releases/tag/v0.3.9
[0.3.8]: https://github.com/ttncode/chord/releases/tag/v0.3.8
[0.3.7]: https://github.com/ttncode/chord/releases/tag/v0.3.7
[0.3.6]: https://github.com/ttncode/chord/releases/tag/v0.3.6
[0.3.5]: https://github.com/ttncode/chord/releases/tag/v0.3.5
[0.3.4]: https://github.com/ttncode/chord/releases/tag/v0.3.4
[0.3.3]: https://github.com/ttncode/chord/releases/tag/v0.3.3
[0.3.2]: https://github.com/ttncode/chord/releases/tag/v0.3.2
[0.3.1]: https://github.com/ttncode/chord/releases/tag/v0.3.1
[0.3.0]: https://github.com/ttncode/chord/releases/tag/v0.3.0
[0.2.1]: https://github.com/ttncode/chord/releases/tag/v0.2.1
[0.2.0]: https://github.com/ttncode/chord/releases/tag/v0.2.0
[0.1.0]: https://github.com/ttncode/chord/releases/tag/v0.1.0
