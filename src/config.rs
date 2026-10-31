//! Chord files: TOML text → validated chords, and chord lookup by app, window title and letters.

use std::collections::BTreeMap;

use crate::keys::{Combo, parse_shortcut};
use crate::machine::CHORD_CHARS;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Shortcut(Vec<Combo>),
    Shell(String),
    Palette(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chord {
    pub name: String,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChordFile {
    pub file: String,
    pub name: String,
    /// Lowercase process file name; `None` for a global file.
    pub app: Option<String>,
    /// Lowercase substring the window title must contain.
    pub title: Option<String>,
    pub chords: BTreeMap<String, Chord>,
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
        if !matches!(key.as_str(), "name" | "app" | "title" | "chords") {
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
    match table.get("chords") {
        Some(toml::Value::Table(entries)) => {
            for (key, value) in entries {
                match parse_chord(key, value) {
                    Ok(chord) => {
                        chords.insert(key.clone(), chord);
                    }
                    Err(msg) => err(format!("chord '{key}': {msg}")),
                }
            }
        }
        Some(_) => err("'chords' must be a table".into()),
        None => err("missing [chords]".into()),
    }

    (Some(ChordFile { file: file.into(), name, app, title, chords }), errors)
}

pub struct Resolved<'a> {
    pub file: &'a ChordFile,
    pub chord: &'a Chord,
    /// Other files in the same priority tier that also define this chord (they lose).
    pub shadowed: Vec<&'a str>,
}

/// Priority: app + title, app only, title only, global. Within a tier the first file by name wins.
pub fn resolve<'a>(files: &'a [ChordFile], exe: &str, title: &str, chord: &str) -> Option<Resolved<'a>> {
    let exe = exe.to_lowercase();
    let title = title.to_lowercase();
    let tier = |f: &ChordFile| {
        let title_match = f.title.as_ref().map(|t| title.contains(t.as_str()));
        match (&f.app, title_match) {
            (Some(app), Some(true)) if *app == exe => Some(0),
            (Some(app), None) if *app == exe => Some(1),
            (None, Some(true)) => Some(2),
            (None, None) => Some(3),
            _ => None,
        }
    };
    let mut hits: Vec<_> = files
        .iter()
        .filter_map(|f| Some((tier(f)?, f, f.chords.get(chord)?)))
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.file.cmp(&b.1.file)));
    let &(best, file, chord) = hits.first()?;
    let shadowed = hits[1..].iter().filter(|h| h.0 == best).map(|h| h.1.file.as_str()).collect();
    Some(Resolved { file, chord, shadowed })
}

/// Every chord usable in this window as `(chord, chord name)`, sorted by chord; each chord's winner per `resolve`.
pub fn available(files: &[ChordFile], exe: &str, title: &str) -> Vec<(String, String)> {
    let keys: std::collections::BTreeSet<&String> = files.iter().flat_map(|f| f.chords.keys()).collect();
    keys.into_iter()
        .filter_map(|key| resolve(files, exe, title, key).map(|r| (key.clone(), r.chord.name.clone())))
        .collect()
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

fn parse_chord(key: &str, value: &toml::Value) -> Result<Chord, String> {
    if key.is_empty() || !key.chars().all(|c| CHORD_CHARS.contains(c)) {
        return Err(format!("only these characters are allowed: {CHORD_CHARS}"));
    }
    let toml::Value::Table(fields) = value else {
        return Err("must be a table like { name = \"…\", shortcut = \"…\" }".into());
    };
    let mut name = None;
    let mut actions = Vec::new();
    for (field, value) in fields {
        let toml::Value::String(s) = value else {
            return Err(format!("'{field}' must be a string"));
        };
        match field.as_str() {
            "name" => name = Some(s.clone()),
            "shortcut" => actions.push(Action::Shortcut(parse_shortcut(s)?)),
            "shell" => actions.push(Action::Shell(s.clone())),
            "palette" => actions.push(Action::Palette(s.clone())),
            other => return Err(format!("unknown field '{other}'")),
        }
    }
    let name = name.ok_or("missing 'name'")?;
    match <[Action; 1]>::try_from(actions) {
        Ok([action]) => Ok(Chord { name, action }),
        Err(_) => Err("needs exactly one of 'shortcut', 'shell', 'palette'".into()),
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

    #[test]
    fn missing_dir_is_error_not_panic() {
        let (files, errors) = load_dir(std::path::Path::new("/no/such/dir"));
        assert!(files.is_empty());
        assert_eq!(errors.len(), 1);
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
}
