# chord

Keyboard chords for Windows: hold Caps Lock, hold Space, type a few letters, release Space, and the matching action runs in the app in front. Chords live in TOML files you edit.

No window or tray icon yet; the console prints what the tool sees and does. Windows only for now.

Inspired by [KeyChord/Chord](https://github.com/KeyChord/Chord) for macOS. This is an independent implementation; no code is shared.

## Run

In PowerShell:

```powershell
cd C:\workspace\chord
cargo test            # logic tests, no keyboard needed
cargo run --release   # uses the chords\ folder; or: cargo run --release -- --chords D:\my-chords
```

Quit with `Ctrl + Space + Esc`, or `Ctrl+C` in the console.

Don't click inside the console while it runs: Windows pauses console output while text is selected (QuickEdit), and chords stop printing until you press Esc there.

Inside chord mode: `Backspace` deletes the last letter, `Esc` clears all of them. Holding Ctrl, Alt or Win lets normal shortcuts through.

## Chord files

Every `*.toml` file in `chords\` is loaded. Files are read again each time a chord fires, so a saved change applies to the next chord, without restarting.

```toml
name = "Antigravity"
app = "antigravity ide.exe"   # process name; leave out for chords that work in every app
# title = "nvim"              # optional: the window title must contain this

[chords]
gd = { name = "Go to definition", shortcut = "f12" }
ca = { name = "Collapse folders", palette = "Collapse Folders in Explorer" }
"/t" = { name = "Open Windows Terminal", shell = "wt" }
```

Each chord has a `name` and exactly one action:

| Action | What it does | Example |
|---|---|---|
| `shortcut` | Presses keys. Combos joined with `+`; several combos separated by spaces run one after another | `"ctrl+shift+p"`, `"ctrl+k ctrl+0"`, `"space f f"` |
| `palette` | Opens the command palette (`Ctrl+Shift+P`), waits 0.4 s, types the text, presses Enter. If the palette opens slower, the text lands in your file: use a `shortcut` where one exists | `"Collapse Folders in Explorer"` |
| `shell` | Runs the command exactly as written (`cmd /S /C "…"`), without a console window | `'start "" "C:\path\app.exe"'` |

Key names: `ctrl shift alt win`, `a`–`z`, `0`–`9`, `f1`–`f24`, `left right up down home end pageup pagedown insert delete`, `backspace enter esc tab space`, and `` ` - = [ ] \ ; ' , . / ``. Write `+` as `plus`.

Chord letters: `a`–`z`, `0`–`9` and `` ; = , - . / [ ] ' ``. Start global chords with a symbol (`/e`) so they never clash with app chords.

**Which chord wins** when several files define it: a file matching the app *and* window title, then the app only, then a global file with a title, then a plain global file. If two files tie, the one whose file name sorts first wins and the console prints a warning.

**Finding an app's name:** run a chord that doesn't exist (e.g. `zz`) in that app. The console prints `no chord 'zz' for <process name> "<window title>"`.

**Mistakes** in a file are printed at startup and with each chord: a bad chord is skipped, a file that isn't valid TOML is skipped, everything else keeps working.

## Starter chords

| File | Chords |
|---|---|
| `global.toml` | `/e` File Explorer · `/wh` `/wl` `/wk` `/wj` snap window left / right / maximize / restore · `/dh` `/dl` previous / next desktop · `/dn` new desktop · `/dx` close desktop · `/a` open Antigravity · `/t` open Windows Terminal |
| `antigravity.toml` | `mf` `me` `ms` `mv` `mg` `mr` `mt` `mh` menus (File, Edit, Selection, View, Go, Run, Terminal, Help) · `gd` go to definition · `gf` go to file · `cp` command palette · `fe` focus Explorer · `ts` toggle sidebar · `ca` collapse folders · `tt` toggle terminal · `fs` search in files · `fg` source control · `fd` focus editor |
| `windows-terminal.toml` | `nt` new tab · `cx` close pane · `tn` / `tp` next / previous tab (set the tab switcher to "In order") · `sv` / `sh` split right / down |
| `lazyvim.toml` | `fe` explorer · `ff` find files · `fg` grep · `bd` delete buffer (Normal mode) |

LazyVim chords need Neovim to put "nvim" in the window title. Add this to your Neovim config (e.g. `lua/config/options.lua`):

```lua
vim.opt.title = true
```

## Manual test checklist

Run these on the real machine; the automated tests can't press real keys.

Chords from files:
- [ ] In Antigravity: `mf` opens the File menu, `fe` focuses the Explorer, `ts` hides/shows the sidebar, `ca` collapses all folders.
- [ ] In Windows Terminal: `nt` opens a new tab, `sv` splits the pane.
- [ ] Anywhere: `/wh` snaps the window left, `/dl` switches to the next desktop, `/t` opens Windows Terminal.
- [ ] In Neovim (with `vim.opt.title = true`): `fe` opens the LazyVim explorer.
- [ ] Change a chord's `shortcut` in a file, save, and run it again: the new shortcut is used without restarting.
- [ ] Write `shorcut` (typo) in one chord: the console names that chord, and the other chords still work.

Caps Lock:
- [ ] Tapping Caps Lock alone still turns it on and off.
- [ ] After any chord, the Caps Lock light and the actual state match what they were before.

Vietnamese Telex (switch to it with `Win + Space`):
- [ ] Outside chord mode, `Vieejt Nam` still becomes `Việt Nam`.
- [ ] Chord `dd` produces no `đ` in the app.

Normal typing:
- [ ] Holding Space alone types spaces as usual.
- [ ] `Ctrl + Space + Esc` always quits.

Known limits: chords do nothing while an app running as administrator is in front, and nothing works on the lock screen or UAC prompts.

## License

MIT, see [LICENSE](LICENSE).
