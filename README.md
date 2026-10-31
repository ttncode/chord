# chord (Phase 0)

A console prototype of keyboard chords for Windows: hold Caps Lock, hold Space, type a few letters, release Space, and the matching shortcut fires in the app in front.

No window, tray icon or config file yet. The console prints what the tool sees and does. Windows only for now.

Inspired by [KeyChord/Chord](https://github.com/KeyChord/Chord) for macOS. This is an independent implementation; no code is shared.

## Run

In PowerShell:

```powershell
cd C:\workspace\chord
cargo test            # 26 logic tests, no keyboard needed
cargo run --release
```

Quit with `Ctrl + Space + Esc`, or `Ctrl+C` in the console.

Don't click inside the console while it runs: Windows pauses console output while text is selected (QuickEdit), and chords stop printing until you press Esc there.

## Chords in this build

| Where | Chord | Sends | Meaning |
|---|---|---|---|
| VS Code, Antigravity | `gd` | `F12` | Go to definition |
| VS Code, Antigravity | `gf` | `Ctrl+P` | Go to file |
| VS Code, Antigravity | `cp` | `Ctrl+Shift+P` | Command palette |
| Windows Terminal | `nt` | `Ctrl+Shift+T` | New tab |
| Any app | `/e` | `Win+E` | Open File Explorer |

Inside chord mode: `Backspace` deletes the last letter, `Esc` clears all of them. Holding Ctrl, Alt or Win lets normal shortcuts through.

The app is recognised by its process name (`code.exe`, `antigravity ide.exe`, `windowsterminal.exe`). If a chord prints `no chord ... for <name>`, that name is what to add in `src/chords.rs`.

## Manual test checklist

Run these on the real machine; the automated tests can't press real keys.

Chords:
- [ ] `gd` in VS Code jumps to a definition.
- [ ] `cp` in Antigravity opens the command palette.
- [ ] `nt` in Windows Terminal opens a new tab.
- [ ] `/e` in any app opens File Explorer.
- [ ] An unknown chord (`zz`) does nothing and prints `no chord`.

Caps Lock:
- [ ] Tapping Caps Lock alone still turns it on and off.
- [ ] After any chord, Caps Lock is in the same state as before.
- [ ] Release Space first, keep holding Caps for 2 seconds, then release: Caps Lock does not turn on.

Vietnamese Telex (switch to it with `Win + Space`):
- [ ] Outside chord mode, `Vieejt Nam` still becomes `Việt Nam`.
- [ ] Chord `dd` (prints `no chord`) produces no `đ` in the app.
- [ ] Type half a word (`Vie`), then do the chord `gd` in VS Code: note what happens to the half-typed word.

Normal typing:
- [ ] Holding Space alone types spaces as usual.
- [ ] Typing speed feels unchanged.
- [ ] `Ctrl + Space + Esc` always quits.

Known limits: chords do nothing while an app running as administrator is in front, and nothing works on the lock screen or UAC prompts.

## License

MIT, see [LICENSE](LICENSE).
