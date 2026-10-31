# chord

Keyboard chords for Windows: hold Caps Lock, hold Space, type a few letters, release Space, and the matching action runs in the app in front. Chords live in TOML files you edit.

While chord mode is on, a Spotlight-style panel near the top of the screen lists the chords for the app in front and narrows as you type. The look follows macOS: rounded corners, a soft shadow, Apple's system colours, keycaps for the keys, and short fade-in and fade-out animations. It follows Windows' light or dark mode, and the animations turn off when Windows' "Animation effects" setting is off. A tray icon offers "Open chords folder", "Start with Windows" and "Quit". Windows only for now.

Inspired by [KeyChord/Chord](https://github.com/KeyChord/Chord) for macOS. This is an independent implementation; no code is shared.

## Run

In PowerShell:

```powershell
cd C:\workspace\chord
cargo test            # logic tests, no keyboard needed
cargo run --release   # uses the chords\ folder; or: cargo run --release -- --chords D:\my-chords
```

Quit with `Ctrl + Space + Esc`, the tray menu, or `Ctrl+C` in the console. Only one copy runs at a time.

**Start with Windows:** tray icon → "Start with Windows". It starts with `--background` (no console window) and the chords folder you're using now.

Don't click inside the console while it runs: Windows pauses console output while text is selected (QuickEdit), and chords stop printing until you press Esc there.

Inside chord mode:

| Key | Does |
|---|---|
| `Backspace` / `Shift+Backspace` | Delete the last letter / all letters |
| `Esc` | Clear the letters |
| `Caps Lock` | Run the typed chord and stay in chord mode; with nothing typed, run the last chord again (`du⇪⇪⇪` = up three folders) |
| `Shift` + letter | Run the typed letters plus this letter, keep the typed letters (`t` then `Shift`+`t`,`s`,`p` runs `tt`, `ts`, `tp`) |
| Ctrl / Alt / Win + key | Passed to the app as a normal shortcut |

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
| `shortcut` | Presses keys. Combos joined with `+`; several combos separated by spaces run one after another | `"ctrl+shift+p"`, `"ctrl+k ctrl+0"`, `"space f f"`. Media keys: `volumeup`, `volumedown`, `volumemute`, `playpause`, `nexttrack`, `prevtrack`, `printscreen` |
| `palette` | Opens the command palette (`Ctrl+Shift+P`), waits 0.4 s, types the text, presses Enter. If the palette opens slower, the text lands in your file: use a `shortcut` where one exists | `"Collapse Folders in Explorer"` |
| `focus` | Brings the app's window to the front (its next window when it's already in front). Optional `launch` runs when it isn't open | `focus = "windowsterminal.exe", launch = "wt"` |
| `mouse` | `click`, `right-click`, `double-click`, `middle-click`, `scroll-up`, `scroll-down`, `scroll-left`, `scroll-right`, `back`, `forward` at the pointer | `mouse = "click"` |
| `hints` | Blue labels on every clickable element of the window; type a label, release Space, it's clicked (`click`, `right-click`, `double-click`) | `hints = "click"` |
| `shell` | Runs the command exactly as written (`cmd /S /C "…"`), without a console window | `'start "" "C:\path\app.exe"'` |

Key names: `ctrl shift alt win`, `a`–`z`, `0`–`9`, `f1`–`f24`, `left right up down home end pageup pagedown insert delete`, `backspace enter esc tab space`, and `` ` - = [ ] \ ; ' , . / ``. Write `+` as `plus`.

Chord letters: `a`–`z`, `0`–`9` and `` ; = , - . / [ ] ' ``. Start global chords with a symbol (`/e`) so they never clash with app chords.

**Regex chords:** parts in `( )` are a regular expression, the rest is literal; `$1`…`$9` in `name`, `shell` and `palette` are filled from the match. Example: `'/o(\d+)' = { name = "Open port $1", shell = 'start http://localhost:$1' }`, then type `/o3000`. An exact chord beats a pattern.

**Which chord wins** when several files define it: a file matching the app *and* window title, then the app only, then a global file with a title, then a plain global file. If two files tie, the one whose file name sorts first wins and the console prints a warning.

**Finding an app's name:** run a chord that doesn't exist (e.g. `zz`) in that app. The console prints `no chord 'zz' for <process name> "<window title>"`.

**Mistakes** in a file are printed at startup and with each chord: a bad chord is skipped, a file that isn't valid TOML is skipped, everything else keeps working.

## Starter chords

| File | Chords |
|---|---|
| `global.toml` | Apps: `/a` Antigravity · `/t` Terminal · `/c` Chrome · `/f` Firefox · `/p` Postman · `/b` Bruno · `/q` DBX · `/k` Mattermost · `/o` Outlook · `/g` Everything · `/=` Notepad++ · `/e` new File Explorer. Window (via `window.ahk`): `/wh` `/wl` `/wk` `/wj` halves · `/wf` maximize · `/wc` center · `/w1` `/w2` `/w3` thirds · `/wn` minimize · `/wq` close. Desktops: `/dh` `/dl` `/dn` `/dx`. System: `/ss` screenshot · `/sv` clipboard · `/se` emoji · `/sl` lock · `/st` Task Manager · `/si` `/sw` `/sb` `/sd` `/sk` Settings pages · `/sn` notifications · `/sr` Run · `/sx` power menu · `/r` DeepL. Media: `/vj` `/vk` volume · `/vm` mute · `/vp` play · `/vh` `/vl` tracks. Mouse: `/mc` `/mr` `/md` `/mm` clicks · `/mh` `/mj` `/mk` `/ml` scroll · `/mp` `/mn` back / forward · `/mt` `/mb` top / bottom. Labels: `;` click · `'` right-click |
| `antigravity.toml` | `mf` `me` `ms` `mv` `mg` `mr` `mt` `mh` menus (File, Edit, Selection, View, Go, Run, Terminal, Help) · `gd` go to definition · `gf` go to file · `cp` command palette · `fe` focus Explorer · `ts` toggle sidebar · `ca` collapse folders · `tt` toggle terminal · `fs` search in files · `fg` source control · `fd` focus editor |
| `windows-terminal.toml` | `nt` new tab · `cx` close pane · `tn` / `tp` next / previous tab (set the tab switcher to "In order") · `sv` / `sh` split right / down |
| `lazyvim.toml` | `fe` explorer · `ff` find files · `fg` grep · `bd` delete buffer (Normal mode) |

LazyVim chords need Neovim to put "nvim" in the window title. Add this to your Neovim config (e.g. `lua/config/options.lua`):

```lua
vim.opt.title = true
```

## Hint labels

Type `;` in chord mode (keep Space held): blue labels appear on the buttons, links, tabs and fields of the window in front. Type a label's letters (the list narrows as you type), release Space, and that element is clicked. `Backspace` past the `;` hides the labels.

To click several things in a row, keep Space held: tap `Caps Lock` (or hold `Shift` on the label's last letter) to click and get fresh labels at once. A `Caps Lock` tap with nothing typed brings the labels back after a pick.

- Works through Windows UI Automation, so it covers most apps (Explorer, Settings, Antigravity, Chrome, Office). Custom-drawn apps and games may show few or no labels.
- In Chrome, Vimium's `f` is faster and more complete for web pages.
- The first `;` in an Electron app (Antigravity, Slack) can come back empty while the app builds its accessibility tree; try again.

## Manual test checklist

Run these on the real machine; the automated tests can't press real keys.

Chords from files:
- [ ] In Antigravity: `mf` opens the File menu, `fe` focuses the Explorer, `ts` hides/shows the sidebar, `ca` collapses all folders.
- [ ] In Windows Terminal: `nt` opens a new tab, `sv` splits the pane.
- [ ] Anywhere: `/wh` snaps the window left, `/dl` switches to the next desktop, `/t` opens Windows Terminal.
- [ ] In Neovim (with `vim.opt.title = true`): `fe` opens the LazyVim explorer.
- [ ] Change a chord's `shortcut` in a file, save, and run it again: the new shortcut is used without restarting.
- [ ] Write `shorcut` (typo) in one chord: the console names that chord, and the other chords still work.

Panel and tray:
- [ ] Holding Caps Lock + Space fades the panel in near the top centre, without taking focus from the app; typing narrows it and colours the typed keys blue; releasing Space fades it out.
- [ ] Switching Windows between light and dark mode changes the panel, the tray icon (⌘) and the tray menu to match.
- [ ] Tray icon → "Open chords folder" opens the folder; "Start with Windows" toggles a check mark and survives a reboot; "Quit" exits.
- [ ] Starting a second copy prints "chord is already running".

Chord mode keys:
- [ ] In Antigravity: `ts` then `Caps` toggles the sidebar and stays in chord mode; `Caps` again toggles it back.
- [ ] In Windows Terminal: `t` then `Shift`+`n` opens a new tab (`tn`… runs next tab) while the panel still shows `t`.

Windows and mouse:
- [ ] `/a` switches to Antigravity (starts it when closed); with two Antigravity windows, `/a` again switches to the other one.
- [ ] `/mc` clicks at the pointer, `/mj` scrolls down.

Hints:
- [ ] `;` in File Explorer shows labels on its buttons; typing one and releasing Space clicks it.
- [ ] Labels sit on the right elements at 125%/150% display scaling.
- [ ] `;` + label + `Caps Lock` clicks it and shows fresh labels; another `Caps Lock` with nothing typed shows labels again.

Robustness:
- [ ] After Win+L and unlocking, and after sleep, chords still work without restarting.

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
