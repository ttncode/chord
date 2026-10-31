//! Hard-coded Phase 0 chords. Replaced by TOML files in Phase 1.

pub const VK_CONTROL: u16 = 0x11;
pub const VK_SHIFT: u16 = 0x10;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_F12: u16 = 0x7B;

/// Keys pressed in order, released in reverse.
pub type Keys = &'static [u16];

const EDITOR: &[(&str, Keys)] = &[
    ("gd", &[VK_F12]),
    ("gf", &[VK_CONTROL, 0x50]),
    ("cp", &[VK_CONTROL, VK_SHIFT, 0x50]),
];

const TERMINAL: &[(&str, Keys)] = &[("nt", &[VK_CONTROL, VK_SHIFT, 0x54])];

const GLOBAL: &[(&str, Keys)] = &[("/e", &[VK_LWIN, 0x45])];

/// `exe` is the lowercase file name of the foreground process.
pub fn lookup(exe: &str, chord: &str) -> Option<Keys> {
    let app: &[(&str, Keys)] = match exe {
        "code.exe" | "antigravity ide.exe" => EDITOR,
        "windowsterminal.exe" => TERMINAL,
        _ => &[],
    };
    app.iter()
        .chain(GLOBAL)
        .find(|(c, _)| *c == chord)
        .map(|(_, keys)| *keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gd_in_vscode_is_f12() {
        assert_eq!(lookup("code.exe", "gd"), Some(&[VK_F12][..]));
    }

    #[test]
    fn vscode_chords_work_in_antigravity() {
        assert_eq!(lookup("antigravity ide.exe", "cp"), Some(&[VK_CONTROL, VK_SHIFT, 0x50][..]));
    }

    #[test]
    fn nt_in_windows_terminal_is_ctrl_shift_t() {
        assert_eq!(lookup("windowsterminal.exe", "nt"), Some(&[VK_CONTROL, VK_SHIFT, 0x54][..]));
    }

    #[test]
    fn app_chord_does_not_leak_into_other_apps() {
        assert_eq!(lookup("windowsterminal.exe", "gd"), None);
    }

    #[test]
    fn global_chord_works_in_any_app() {
        assert_eq!(lookup("notepad.exe", "/e"), Some(&[VK_LWIN, 0x45][..]));
    }

    #[test]
    fn unknown_chord_is_none() {
        assert_eq!(lookup("code.exe", "zz"), None);
    }
}
