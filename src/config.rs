//! Chord files: TOML text → validated chords, and chord lookup by app, window title and letters.

use std::collections::BTreeMap;

use crate::keys::{Combo, parse_shortcut};
use crate::machine::CHORD_CHARS;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Shortcut(Vec<Combo>),
    Shell(String),
    Palette(String),
    /// Bring the app's window to the front (next window if it's already in front); `launch` runs if it isn't open.
    Focus { exe: String, launch: Option<String> },
    Mouse(Mouse),
    /// Label the clickable elements in the window; the letters typed after the chord pick one.
    Hints(Hint),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    pub click: Mouse,
    /// Label the taskbar instead of the window in front.
    pub taskbar: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mouse {
    Click,
    RightClick,
    DoubleClick,
    MiddleClick,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    /// The mouse's side buttons.
    Back,
    Forward,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
    pub name: String,
    pub action: Action,
    /// Runs as soon as its last letter is pressed, and again and again while that letter is held.
    pub repeat: bool,
}

#[derive(Debug, Clone)]
pub struct ChordFile {
    pub file: String,
    pub name: String,
    /// Lowercase process file name; `None` for a global file.
    pub app: Option<String>,
    /// Lowercase substring the window title must contain.
    pub title: Option<String>,
    pub chords: BTreeMap<String, Chord>,
    /// Chords whose key has `( … )` regex parts, compiled; the chord itself is in `chords` under the same key.
    pub patterns: Vec<(String, regex::Regex)>,
    /// Chord prefix → group name (`"/w"` → `"Window"`), shown in the panel as a reminder.
    pub groups: BTreeMap<String, String>,
}

/// `None` only when the TOML itself can't be parsed. Every error starts with `"{file}: "`.
pub fn parse_file(file: &str, text: &str) -> (Option<ChordFile>, Vec<String>) {
    // Notepad saves UTF-8 with a BOM, which the TOML parser rejects.
    let text = text.strip_prefix('\u{FEFF}').unwrap_or(text);
    let table = match text.parse::<toml::Table>() {
        Ok(table) => table,
        Err(e) => return (None, vec![format!("{file}: {}", e.to_string().trim_end())]),
    };

    let mut errors = Vec::new();
    let mut err = |msg: String| errors.push(format!("{file}: {msg}"));

    for key in table.keys() {
        if !matches!(key.as_str(), "name" | "app" | "title" | "groups" | "chords") {
            err(format!("unknown field '{key}'"));
        }
    }
    let string = |key: &str, err: &mut dyn FnMut(String)| match table.get(key) {
        None => None,
        Some(toml::Value::String(s)) => Some(s.clone()),
        Some(_) => {
            err(format!("'{key}' must be a string"));
            None
        }
    };
    let name = string("name", &mut err).unwrap_or_else(|| {
        err("missing 'name'".into());
        String::new()
    });
    let app = string("app", &mut err).map(|s| s.to_lowercase());
    let title = string("title", &mut err).map(|s| s.to_lowercase());

    let mut chords = BTreeMap::new();
    let mut patterns = Vec::new();
    match table.get("chords") {
        Some(toml::Value::Table(entries)) => {
            for (key, value) in entries {
                match parse_chord(key, value) {
                    Ok((chord, pattern)) => {
                        chords.insert(key.clone(), chord);
                        patterns.extend(pattern.map(|p| (key.clone(), p)));
                    }
                    Err(msg) => err(format!("chord '{key}': {msg}")),
                }
            }
        }
        Some(_) => err("'chords' must be a table".into()),
        None => err("missing [chords]".into()),
    }

    let mut groups = BTreeMap::new();
    match table.get("groups") {
        Some(toml::Value::Table(entries)) => {
            for (key, value) in entries {
                match value {
                    toml::Value::String(name) => {
                        groups.insert(key.clone(), name.clone());
                    }
                    _ => err(format!("group '{key}' must be a string")),
                }
            }
        }
        Some(_) => err("'groups' must be a table".into()),
        None => {}
    }

    (Some(ChordFile { file: file.into(), name, app, title, chords, patterns, groups }), errors)
}

pub struct Resolved<'a> {
    pub file: &'a ChordFile,
    /// With pattern captures already filled in.
    pub chord: Chord,
    /// Other files in the same priority tier that also define this chord (they lose).
    pub shadowed: Vec<&'a str>,
}

/// Priority: app + title, app only, title only, global. Within a tier the first file by name wins.
pub fn resolve<'a>(files: &'a [ChordFile], exe: &str, title: &str, chord: &str) -> Option<Resolved<'a>> {
    let (exe, title) = (exe.to_lowercase(), title.to_lowercase());
    let tier = |f: &ChordFile| tier(f, &exe, &title);
    // Within a tier an exact chord (kind 0) beats a pattern (kind 1).
    let lookup = |f: &ChordFile| -> Option<(u8, Chord)> {
        if let Some(found) = f.chords.get(chord) {
            return Some((0, found.clone()));
        }
        f.patterns.iter().find_map(|(key, re)| Some((1, fill_chord(&f.chords[key], &re.captures(chord)?))))
    };
    let mut hits: Vec<_> = files
        .iter()
        .filter_map(|f| {
            let (kind, found) = lookup(f)?;
            Some(((tier(f)?, kind), f, found))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.file.cmp(&b.1.file)));
    let mut hits = hits.into_iter();
    let (best, file, chord) = hits.next()?;
    let shadowed = hits.filter(|h| h.0 == best).map(|h| h.1.file.as_str()).collect();
    Some(Resolved { file, chord, shadowed })
}

/// How strongly `f` applies to this window (0 best), `None` if it doesn't. `exe` and `title` are lowercase.
fn tier(f: &ChordFile, exe: &str, title: &str) -> Option<u8> {
    let title_match = f.title.as_ref().map(|t| title.contains(t.as_str()));
    match (&f.app, title_match) {
        (Some(app), Some(true)) if app == exe => Some(0),
        (Some(app), None) if app == exe => Some(1),
        (None, Some(true)) => Some(2),
        (None, None) => Some(3),
        _ => None,
    }
}

/// Every group usable in this window as `(prefix, group name)`, sorted by prefix; the best tier names a prefix.
pub fn available_groups(files: &[ChordFile], exe: &str, title: &str) -> Vec<(String, String)> {
    let (exe, title) = (exe.to_lowercase(), title.to_lowercase());
    let mut applying: Vec<_> = files.iter().filter_map(|f| Some((tier(f, &exe, &title)?, f))).collect();
    applying.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.file.cmp(&b.1.file)));
    let mut groups = BTreeMap::new();
    for (_, f) in applying {
        for (key, name) in &f.groups {
            groups.entry(key.clone()).or_insert_with(|| name.clone());
        }
    }
    groups.into_iter().collect()
}

/// Every chord usable in this window as `(chord, chord name)`, sorted by chord; each chord's winner per `resolve`.
pub fn available(files: &[ChordFile], exe: &str, title: &str) -> Vec<(String, String)> {
    let keys: std::collections::BTreeSet<&String> = files.iter().flat_map(|f| f.chords.keys()).collect();
    keys.into_iter()
        .filter_map(|key| resolve(files, exe, title, key).map(|r| (key.clone(), r.chord.name.clone())))
        .collect()
}

/// The hint chord a chord starts with (`;` in `;aj`): its length and what it labels. The rest is a label.
pub fn hint_prefix(files: &[ChordFile], exe: &str, title: &str, chord: &str) -> Option<(usize, Hint)> {
    chord.char_indices().map(|(i, c)| i + c.len_utf8()).find_map(|end| {
        match resolve(files, exe, title, &chord[..end])?.chord.action {
            Action::Hints(hint) => Some((end, hint)),
            _ => None,
        }
    })
}

/// The starter chord files, built into the program so an installed copy can create them.
pub const STARTERS: [(&str, &str); 4] = [
    ("global.toml", include_str!("../chords/global.toml")),
    ("antigravity.toml", include_str!("../chords/antigravity.toml")),
    ("windows-terminal.toml", include_str!("../chords/windows-terminal.toml")),
    ("lazyvim.toml", include_str!("../chords/lazyvim.toml")),
];

/// Where chord files live when `--chords` isn't given: a `chords` folder in the current folder
/// (portable zip, development) wins; otherwise the per-user folder, e.g. `%APPDATA%\chord\chords`.
/// Remote desktop and VM windows: their own machine runs its own chord, and they sync Caps Lock
/// back to this one, so chord here must leave Caps alone while they are in front.
pub fn remote_client(exe: &str) -> bool {
    // shortcut: fixed list of the common clients; make it a chord-file setting if users need others.
    ["mstsc.exe", "msrdc.exe", "vmconnect.exe", "vmware.exe", "vmplayer.exe", "virtualboxvm.exe"].contains(&exe)
}

pub fn default_dir(local: &std::path::Path, per_user: Option<std::path::PathBuf>) -> std::path::PathBuf {
    match per_user {
        Some(dir) if !local.is_dir() => dir,
        _ => local.to_path_buf(),
    }
}

/// Creates `dir` with the starter files when it doesn't exist yet. An existing folder is never
/// touched, so deleted or edited starters stay that way.
pub fn create_with_starters(dir: &std::path::Path) -> std::io::Result<()> {
    if dir.exists() {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    for (name, text) in STARTERS {
        std::fs::write(dir.join(name), text)?;
    }
    Ok(())
}

/// Every `*.toml` directly in `dir`, sorted by file name. A missing directory is an error, not a panic.
pub fn load_dir(dir: &std::path::Path) -> (Vec<ChordFile>, Vec<String>) {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => return (vec![], vec![format!("{}: {e}", dir.display())]),
    };
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "toml"))
        .collect();
    paths.sort();

    let mut files = Vec::new();
    let mut errors = Vec::new();
    for path in paths {
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let (file, file_errors) = parse_file(&name, &text);
                files.extend(file);
                errors.extend(file_errors);
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }
    (files, errors)
}

/// Arguments for `cmd` that run a `shell` action exactly as written.
/// `/S` makes cmd strip only the outer quotes we add, never quotes inside the command.
pub fn cmd_line(command: &str) -> String {
    format!("/S /C \"{command}\"")
}

/// Chord key → anchored regex when it has `( … )` parts. Text outside parentheses is literal
/// and limited to chord letters; text inside is regex syntax.
fn compile_pattern(key: &str) -> Result<Option<regex::Regex>, String> {
    if !key.contains('(') {
        if key.is_empty() || !key.chars().all(|c| CHORD_CHARS.contains(c)) {
            return Err(format!("only these characters are allowed: {CHORD_CHARS}"));
        }
        return Ok(None);
    }
    let mut pattern = String::from("^");
    let mut depth = 0;
    for c in key.chars() {
        match c {
            '(' => depth += 1,
            ')' if depth > 0 => depth -= 1,
            _ if depth == 0 && !CHORD_CHARS.contains(c) => {
                return Err(format!("'{c}' outside ( ) must be one of {CHORD_CHARS}"));
            }
            _ => {}
        }
        if depth == 0 && c != ')' {
            pattern.push_str(&regex::escape(&c.to_string()));
        } else {
            pattern.push(c);
        }
    }
    pattern.push('$');
    regex::Regex::new(&pattern).map(Some).map_err(|e| format!("bad pattern: {e}"))
}

/// `$1`…`$9` in `text` → the pattern's captures.
fn fill(text: &str, captures: &regex::Captures) -> String {
    (1..captures.len()).rev().fold(text.to_string(), |text, n| {
        text.replace(&format!("${n}"), captures.get(n).map_or("", |m| m.as_str()))
    })
}

fn fill_chord(chord: &Chord, captures: &regex::Captures) -> Chord {
    let action = match &chord.action {
        Action::Shell(s) => Action::Shell(fill(s, captures)),
        Action::Palette(s) => Action::Palette(fill(s, captures)),
        other => other.clone(),
    };
    Chord { name: fill(&chord.name, captures), action, repeat: chord.repeat }
}

fn parse_chord(key: &str, value: &toml::Value) -> Result<(Chord, Option<regex::Regex>), String> {
    let pattern = compile_pattern(key)?;
    let toml::Value::Table(fields) = value else {
        return Err("must be a table like { name = \"…\", shortcut = \"…\" }".into());
    };
    let mut name = None;
    let mut actions = Vec::new();
    let mut launch = None;
    let mut on = None;
    let mut repeat = false;
    for (field, value) in fields {
        if field == "repeat" {
            repeat = value.as_bool().ok_or("'repeat' must be true or false")?;
            continue;
        }
        let toml::Value::String(s) = value else {
            return Err(format!("'{field}' must be a string"));
        };
        match field.as_str() {
            "name" => name = Some(s.clone()),
            "shortcut" => actions.push(Action::Shortcut(parse_shortcut(s)?)),
            "shell" => actions.push(Action::Shell(s.clone())),
            "palette" => actions.push(Action::Palette(s.clone())),
            "focus" => actions.push(Action::Focus { exe: s.to_lowercase(), launch: None }),
            "launch" => launch = Some(s.clone()),
            "on" => on = Some(s.clone()),
            "hints" => {
                let click = match s.as_str() {
                    "click" => Mouse::Click,
                    "right-click" => Mouse::RightClick,
                    "double-click" => Mouse::DoubleClick,
                    other => return Err(format!("hints \"{other}\" must be click, right-click or double-click")),
                };
                actions.push(Action::Hints(Hint { click, taskbar: false }));
            }
            "mouse" => actions.push(Action::Mouse(match s.as_str() {
                "click" => Mouse::Click,
                "right-click" => Mouse::RightClick,
                "double-click" => Mouse::DoubleClick,
                "middle-click" => Mouse::MiddleClick,
                "scroll-up" => Mouse::ScrollUp,
                "scroll-down" => Mouse::ScrollDown,
                "scroll-left" => Mouse::ScrollLeft,
                "scroll-right" => Mouse::ScrollRight,
                "back" => Mouse::Back,
                "forward" => Mouse::Forward,
                other => return Err(format!(
                    "mouse \"{other}\" must be click, right-click, double-click, middle-click, scroll-up, scroll-down, scroll-left, scroll-right, back or forward"
                )),
            })),
            other => return Err(format!("unknown field '{other}'")),
        }
    }
    let name = name.ok_or("missing 'name'")?;
    if let Some(command) = launch {
        match actions.as_mut_slice() {
            [Action::Focus { launch, .. }] => *launch = Some(command),
            _ => return Err("'launch' only goes together with 'focus'".into()),
        }
    }
    if let Some(target) = on {
        match (actions.as_mut_slice(), target.as_str()) {
            ([Action::Hints(hint)], "taskbar") => hint.taskbar = true,
            ([Action::Hints(_)], other) => return Err(format!("on \"{other}\" must be taskbar")),
            _ => return Err("'on' only goes together with 'hints'".into()),
        }
    }
    match <[Action; 1]>::try_from(actions) {
        Ok([action]) => Ok((Chord { name, action, repeat }, pattern)),
        Err(_) => Err("needs exactly one of 'shortcut', 'shell', 'palette', 'focus', 'mouse', 'hints'".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> ChordFile {
        let (file, errors) = parse_file("x.toml", text);
        assert_eq!(errors, Vec::<String>::new());
        file.expect("file")
    }

    #[test]
    fn parses_all_action_kinds() {
        let f = ok(r#"
            name = "Test"
            [chords]
            gd = { name = "Go to definition", shortcut = "f12" }
            "/t" = { name = "Terminal", shell = "wt" }
            ca = { name = "Collapse", palette = "Collapse Folders in Explorer" }
        "#);
        assert_eq!(f.chords.len(), 3);
        assert_eq!(f.chords["gd"].action, Action::Shortcut(parse_shortcut("f12").unwrap()));
        assert_eq!(f.chords["/t"].action, Action::Shell("wt".into()));
        assert_eq!(f.chords["ca"].action, Action::Palette("Collapse Folders in Explorer".into()));
        assert_eq!(f.chords["gd"].name, "Go to definition");
    }

    #[test]
    fn app_and_title_lowercased() {
        let f = ok(r#"
            name = "T"
            app = "Antigravity IDE.exe"
            title = "NVim"
            [chords]
        "#);
        assert_eq!(f.app.as_deref(), Some("antigravity ide.exe"));
        assert_eq!(f.title.as_deref(), Some("nvim"));
    }

    #[test]
    fn bad_chord_skipped_others_kept() {
        let (f, errors) = parse_file("x.toml", r#"
            name = "T"
            [chords]
            gd = { name = "ok", shortcut = "f12" }
            xx = { name = "typo", shorcut = "f1" }
            yy = { name = "two", shortcut = "f1", shell = "wt" }
            zz = { name = "none" }
            "A!" = { name = "letters", shortcut = "f1" }
            bb = { name = "bad key", shortcut = "ctrl+foo" }
        "#);
        let f = f.expect("file");
        assert_eq!(f.chords.keys().collect::<Vec<_>>(), vec!["gd"]);
        assert_eq!(errors.len(), 5, "{errors:?}");
        for key in ["xx", "yy", "zz", "A!", "bb"] {
            assert!(errors.iter().any(|e| e.contains(&format!("'{key}'"))), "no error for {key}: {errors:?}");
        }
    }

    #[test]
    fn syntax_error_rejects_file() {
        let (f, errors) = parse_file("x.toml", "name = ");
        assert!(f.is_none());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].starts_with("x.toml: "), "{errors:?}");
    }

    #[test]
    fn unknown_top_level_key_is_error() {
        let (_, errors) = parse_file("x.toml", "nmae = \"X\"\n[chords]\n");
        assert!(errors.iter().any(|e| e.contains("nmae")), "{errors:?}");
    }

    #[test]
    fn cmd_line_keeps_inner_quotes() {
        let command = r#""C:\Program Files\X\x.exe" --open "a b""#;
        assert_eq!(cmd_line(command), format!("/S /C \"{command}\""));
    }

    #[test]
    fn utf8_bom_is_ignored() {
        ok("\u{FEFF}name = \"T\"\n[chords]\n");
    }

    /// One-chord file; `header` holds optional `app = …` / `title = …` lines.
    fn file(name: &str, header: &str, chord: &str) -> ChordFile {
        let text = format!("name = \"{name}\"\n{header}\n[chords]\n\"{chord}\" = {{ name = \"{name}\", shell = \"x\" }}\n");
        let (f, errors) = parse_file(name, &text);
        assert!(errors.is_empty(), "{errors:?}");
        f.unwrap()
    }

    fn winner<'a>(files: &'a [ChordFile], exe: &str, title: &str, chord: &str) -> Option<&'a str> {
        resolve(files, exe, title, chord).map(|r| r.file.file.as_str())
    }

    #[test]
    fn app_beats_global() {
        let files = [file("ed.toml", "app = \"code.exe\"", "gd"), file("g.toml", "", "gd")];
        assert_eq!(winner(&files, "code.exe", "", "gd"), Some("ed.toml"));
        assert_eq!(winner(&files, "notepad.exe", "", "gd"), Some("g.toml"));
    }

    #[test]
    fn app_case_insensitive() {
        let files = [file("a.toml", "app = \"Antigravity IDE.exe\"", "gd")];
        assert_eq!(winner(&files, "antigravity ide.exe", "", "gd"), Some("a.toml"));
        assert_eq!(winner(&files, "Antigravity IDE.exe", "", "gd"), Some("a.toml"));
    }

    #[test]
    fn title_beats_app_only() {
        let files = [
            file("vim.toml", "app = \"windowsterminal.exe\"\ntitle = \"nvim\"", "fe"),
            file("wt.toml", "app = \"windowsterminal.exe\"", "fe"),
        ];
        assert_eq!(winner(&files, "windowsterminal.exe", "README.md - NVIM", "fe"), Some("vim.toml"));
        assert_eq!(winner(&files, "windowsterminal.exe", "pwsh", "fe"), Some("wt.toml"));
    }

    #[test]
    fn title_unicode_case_insensitive() {
        let files = [file("v.toml", "title = \"việt\"", "/v")];
        assert_eq!(winner(&files, "notepad.exe", "VIỆT NAM", "/v"), Some("v.toml"));
    }

    #[test]
    fn global_title_beats_plain_global() {
        let files = [file("a.toml", "", "/e"), file("b.toml", "title = \"notes\"", "/e")];
        assert_eq!(winner(&files, "notepad.exe", "My Notes", "/e"), Some("b.toml"));
        assert_eq!(winner(&files, "notepad.exe", "Other", "/e"), Some("a.toml"));
    }

    #[test]
    fn same_tier_duplicate_reports_shadowed() {
        let files = [file("a.toml", "", "/e"), file("b.toml", "", "/e")];
        let r = resolve(&files, "notepad.exe", "", "/e").unwrap();
        assert_eq!(r.file.file, "a.toml");
        assert_eq!(r.shadowed, vec!["b.toml"]);
    }

    #[test]
    fn unknown_chord_is_none() {
        let files = [file("a.toml", "", "/e")];
        assert!(resolve(&files, "notepad.exe", "", "zz").is_none());
    }

    #[test]
    fn groups_come_from_files_that_apply_app_first() {
        let parse = |name: &str, text: &str| {
            let (f, errors) = parse_file(name, text);
            assert!(errors.is_empty(), "{errors:?}");
            f.unwrap()
        };
        let files = [
            parse("ag.toml", "name = \"A\"\napp = \"code.exe\"\n[groups]\n\"/w\" = \"Code windows\"\nm = \"Menu\"\n[chords]\n"),
            parse("g.toml", "name = \"G\"\n[groups]\n\"/w\" = \"Window\"\n\"/d\" = \"Desktop\"\n[chords]\n"),
            parse("np.toml", "name = \"N\"\napp = \"notepad.exe\"\n[groups]\nx = \"Other\"\n[chords]\n"),
        ];
        let pairs = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(available_groups(&files, "code.exe", ""), pairs(&[("/d", "Desktop"), ("/w", "Code windows"), ("m", "Menu")]));
        assert_eq!(available_groups(&files, "cmd.exe", ""), pairs(&[("/d", "Desktop"), ("/w", "Window")]));
    }

    #[test]
    fn group_name_must_be_a_string() {
        let (_, errors) = parse_file("x.toml", "name = \"X\"\n[groups]\n\"/w\" = 1\n[chords]\n");
        assert_eq!(errors, vec!["x.toml: group '/w' must be a string"]);
    }

    #[test]
    fn available_lists_winning_chords_sorted() {
        let files = [
            file("ag.toml", "app = \"code.exe\"", "gd"),
            file("g.toml", "", "gd"),
            file("h.toml", "", "/e"),
            file("other.toml", "app = \"notepad.exe\"", "xx"),
        ];
        let list: Vec<(String, String)> = available(&files, "code.exe", "");
        assert_eq!(
            list,
            vec![("/e".to_string(), "h.toml".to_string()), ("gd".to_string(), "ag.toml".to_string())]
        );
    }

    fn global(chords: &str) -> ChordFile {
        let (f, errors) = parse_file("g.toml", &format!("name = \"G\"\n[chords]\n{chords}\n"));
        assert!(errors.is_empty(), "{errors:?}");
        f.unwrap()
    }

    #[test]
    fn pattern_chord_matches_and_fills_captures() {
        let files = [global(r#"'/g(\d+)' = { name = "Go to line $1", shell = "echo $1" }"#)];
        let r = resolve(&files, "x.exe", "", "/g42").unwrap();
        assert_eq!(r.chord.name, "Go to line 42");
        assert_eq!(r.chord.action, Action::Shell("echo 42".into()));
    }

    #[test]
    fn pattern_literal_part_is_literal() {
        let files = [global(r#"'.(a+)' = { name = "x", shell = "x" }"#)];
        assert!(resolve(&files, "x.exe", "", ".aa").is_some());
        assert!(resolve(&files, "x.exe", "", "za").is_none());
        assert!(resolve(&files, "x.exe", "", ".ab").is_none());
    }

    #[test]
    fn exact_chord_beats_pattern() {
        let files = [global(
            r#"'/g(\d+)' = { name = "pattern", shell = "p" }
"/g1" = { name = "exact", shell = "e" }"#,
        )];
        assert_eq!(resolve(&files, "x.exe", "", "/g1").unwrap().chord.name, "exact");
    }

    #[test]
    fn bad_patterns_are_errors() {
        let (_, errors) = parse_file(
            "g.toml",
            "name = \"G\"\n[chords]\n'/g(\\d+' = { name = \"x\", shell = \"x\" }\n'Q(a)' = { name = \"x\", shell = \"x\" }\n",
        );
        assert_eq!(errors.len(), 2, "{errors:?}");
    }

    #[test]
    fn focus_and_mouse_actions_parse() {
        let f = global(
            r#""/a" = { name = "A", focus = "Antigravity IDE.exe", launch = "start x" }
"/b" = { name = "B", focus = "wt.exe" }
"/c" = { name = "C", mouse = "double-click" }"#,
        );
        assert_eq!(
            f.chords["/a"].action,
            Action::Focus { exe: "antigravity ide.exe".into(), launch: Some("start x".into()) }
        );
        assert_eq!(f.chords["/b"].action, Action::Focus { exe: "wt.exe".into(), launch: None });
        assert_eq!(f.chords["/c"].action, Action::Mouse(Mouse::DoubleClick));
    }

    #[test]
    fn mouse_side_buttons_and_horizontal_scroll_parse() {
        let f = global(
            r#""/1" = { name = "1", mouse = "back" }
"/2" = { name = "2", mouse = "forward" }
"/3" = { name = "3", mouse = "scroll-left" }
"/4" = { name = "4", mouse = "scroll-right" }"#,
        );
        assert_eq!(f.chords["/1"].action, Action::Mouse(Mouse::Back));
        assert_eq!(f.chords["/2"].action, Action::Mouse(Mouse::Forward));
        assert_eq!(f.chords["/3"].action, Action::Mouse(Mouse::ScrollLeft));
        assert_eq!(f.chords["/4"].action, Action::Mouse(Mouse::ScrollRight));
    }

    #[test]
    fn launch_without_focus_and_unknown_mouse_are_errors() {
        let (_, errors) = parse_file(
            "g.toml",
            "name = \"G\"\n[chords]\n\"/a\" = { name = \"x\", shell = \"x\", launch = \"y\" }\n\"/b\" = { name = \"x\", mouse = \"tickle\" }\n",
        );
        assert_eq!(errors.len(), 2, "{errors:?}");
    }

    #[test]
    fn hints_action_parses() {
        let f = global(r#"";" = { name = "Click", hints = "click" }"#);
        assert_eq!(f.chords[";"].action, Action::Hints(Hint { click: Mouse::Click, taskbar: false }));
    }

    #[test]
    fn repeat_parses_and_must_be_true_or_false() {
        let f = global(r#""j" = { name = "Down", mouse = "scroll-down", repeat = true }
"/e" = { name = "Explorer", shortcut = "win+e" }"#);
        assert!(f.chords["j"].repeat);
        assert!(!f.chords["/e"].repeat);
        let (_, errors) = parse_file("g.toml", "name = \"G\"\n[chords]\n\"j\" = { name = \"x\", shell = \"x\", repeat = \"yes\" }\n");
        assert_eq!(errors, vec!["g.toml: chord 'j': 'repeat' must be true or false"]);
    }

    #[test]
    fn hints_on_taskbar_parses_and_on_needs_hints() {
        let f = global(r#""/b" = { name = "Taskbar", hints = "click", on = "taskbar" }"#);
        assert_eq!(f.chords["/b"].action, Action::Hints(Hint { click: Mouse::Click, taskbar: true }));
        let (_, errors) = parse_file(
            "g.toml",
            "name = \"G\"\n[chords]\n\"/a\" = { name = \"x\", shell = \"x\", on = \"taskbar\" }\n\"/b\" = { name = \"x\", hints = \"click\", on = \"desk\" }\n",
        );
        assert_eq!(
            errors,
            vec![
                "g.toml: chord '/a': 'on' only goes together with 'hints'",
                "g.toml: chord '/b': on \"desk\" must be taskbar",
            ]
        );
    }

    #[test]
    fn hint_prefix_finds_the_hint_chord_at_the_start() {
        let files = vec![global(
            r#"";" = { name = "Click", hints = "click" }
"'" = { name = "Right", hints = "right-click" }
"/e" = { name = "Explorer", shortcut = "win+e" }"#,
        )];
        let find = |chord| hint_prefix(&files, "x.exe", "", chord);
        let hint = |click| Hint { click, taskbar: false };
        assert_eq!(find(";"), Some((1, hint(Mouse::Click))));
        assert_eq!(find(";aj"), Some((1, hint(Mouse::Click))));
        assert_eq!(find("'s"), Some((1, hint(Mouse::RightClick))));
        assert_eq!(find("/e"), None);
        assert_eq!(find(""), None);
    }

    #[test]
    fn missing_dir_is_error_not_panic() {
        let (files, errors) = load_dir(std::path::Path::new("/no/such/dir"));
        assert!(files.is_empty());
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn local_chords_folder_wins_over_the_per_user_one() {
        let local = std::env::temp_dir().join(format!("chord-test-local-{}", std::process::id()));
        let per_user = std::path::PathBuf::from("per-user");
        assert_eq!(default_dir(&local, Some(per_user.clone())), per_user);
        std::fs::create_dir_all(&local).unwrap();
        assert_eq!(default_dir(&local, Some(per_user)), local);
        assert_eq!(default_dir(&local, None), local);
        std::fs::remove_dir_all(&local).unwrap();
    }

    #[test]
    fn starters_are_written_once_into_a_new_folder() {
        let dir = std::env::temp_dir().join(format!("chord-test-starters-{}", std::process::id())).join("chords");
        create_with_starters(&dir).unwrap();
        let (files, errors) = load_dir(&dir);
        assert_eq!((files.len(), errors.len()), (4, 0));
        std::fs::remove_file(dir.join("lazyvim.toml")).unwrap();
        create_with_starters(&dir).unwrap();
        assert!(!dir.join("lazyvim.toml").exists(), "an existing folder must not be refilled");
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn starter_files_have_no_errors() {
        let dir = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/chords"));
        let (files, errors) = load_dir(dir);
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(files.len(), 4);
        let gd = resolve(&files, "antigravity ide.exe", "", "gd").unwrap();
        assert_eq!(gd.chord.action, Action::Shortcut(parse_shortcut("f12").unwrap()));
    }

    #[test]
    fn remote_clients_are_recognised_by_exe() {
        for exe in ["mstsc.exe", "msrdc.exe", "vmconnect.exe", "vmware.exe", "vmplayer.exe", "virtualboxvm.exe"] {
            assert!(remote_client(exe), "{exe}");
        }
        for exe in ["", "explorer.exe", "chrome.exe", "mstsc.exe.bak"] {
            assert!(!remote_client(exe), "{exe}");
        }
    }
}
