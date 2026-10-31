//! Shortcut text such as `"ctrl+shift+p"` or `"ctrl+k ctrl+0"` → virtual-key combos.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub vk: u16,
    /// Sent with KEYEVENTF_EXTENDEDKEY (arrows, navigation block, Win).
    pub extended: bool,
}

/// Modifiers first (ctrl, shift, alt, win), then the one non-modifier key.
pub type Combo = Vec<Key>;

const MODIFIERS: [(&str, Key); 4] = [
    ("ctrl", Key { vk: 0x11, extended: false }),
    ("shift", Key { vk: 0x10, extended: false }),
    ("alt", Key { vk: 0x12, extended: false }),
    ("win", Key { vk: 0x5B, extended: true }),
];

fn main_key(name: &str) -> Option<Key> {
    let plain = |vk| Some(Key { vk, extended: false });
    let ext = |vk| Some(Key { vk, extended: true });
    match name {
        n if n.len() == 1 && n.as_bytes()[0].is_ascii_lowercase() => plain(n.as_bytes()[0].to_ascii_uppercase() as u16),
        n if n.len() == 1 && n.as_bytes()[0].is_ascii_digit() => plain(n.as_bytes()[0] as u16),
        n if n.starts_with('f') => match n[1..].parse::<u16>() {
            Ok(i @ 1..=24) => plain(0x6F + i),
            _ => None,
        },
        "left" => ext(0x25),
        "up" => ext(0x26),
        "right" => ext(0x27),
        "down" => ext(0x28),
        "home" => ext(0x24),
        "end" => ext(0x23),
        "pageup" => ext(0x21),
        "pagedown" => ext(0x22),
        "insert" => ext(0x2D),
        "delete" => ext(0x2E),
        "backspace" => plain(0x08),
        "enter" => plain(0x0D),
        "esc" => plain(0x1B),
        "tab" => plain(0x09),
        "space" => plain(0x20),
        "`" => plain(0xC0),
        "-" => plain(0xBD),
        "=" | "plus" => plain(0xBB),
        "[" => plain(0xDB),
        "]" => plain(0xDD),
        "\\" => plain(0xDC),
        ";" => plain(0xBA),
        "'" => plain(0xDE),
        "," => plain(0xBC),
        "." => plain(0xBE),
        "/" => plain(0xBF),
        "printscreen" => ext(0x2C),
        "volumemute" => ext(0xAD),
        "volumedown" => ext(0xAE),
        "volumeup" => ext(0xAF),
        "nexttrack" => ext(0xB0),
        "prevtrack" => ext(0xB1),
        "playpause" => ext(0xB3),
        _ => None,
    }
}

pub fn parse_shortcut(text: &str) -> Result<Vec<Combo>, String> {
    let combos = text
        .split_whitespace()
        .map(|combo| parse_combo(&combo.to_lowercase()))
        .collect::<Result<Vec<_>, _>>()?;
    if combos.is_empty() {
        return Err("empty shortcut".into());
    }
    Ok(combos)
}

fn parse_combo(combo: &str) -> Result<Combo, String> {
    let mut modifiers = [false; 4];
    let mut main = None;
    for name in combo.split('+') {
        if let Some(i) = MODIFIERS.iter().position(|(m, _)| *m == name) {
            modifiers[i] = true;
        } else if let Some(key) = main_key(name) {
            if main.replace(key).is_some() {
                return Err(format!("\"{combo}\" has more than one non-modifier key"));
            }
        } else {
            return Err(format!("unknown key \"{name}\" in \"{combo}\""));
        }
    }
    let main = main.ok_or_else(|| format!("\"{combo}\" has no non-modifier key"))?;
    let mut keys: Combo = MODIFIERS.iter().zip(modifiers).filter(|(_, on)| *on).map(|((_, k), _)| *k).collect();
    keys.push(main);
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(vk: u16) -> Key {
        Key { vk, extended: false }
    }
    fn ext(vk: u16) -> Key {
        Key { vk, extended: true }
    }

    #[test]
    fn single_key() {
        assert_eq!(parse_shortcut("f12"), Ok(vec![vec![k(0x7B)]]));
    }

    #[test]
    fn combo_orders_modifiers() {
        assert_eq!(parse_shortcut("shift+ctrl+p"), Ok(vec![vec![k(0x11), k(0x10), k(0x50)]]));
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(parse_shortcut("Ctrl+Shift+P"), parse_shortcut("ctrl+shift+p"));
    }

    #[test]
    fn sequence() {
        let combos = parse_shortcut("ctrl+k ctrl+0").unwrap();
        assert_eq!(combos.len(), 2);
        assert_eq!(combos[1], vec![k(0x11), k(0x30)]);
    }

    #[test]
    fn win_and_arrows_are_extended() {
        assert_eq!(parse_shortcut("win+left"), Ok(vec![vec![ext(0x5B), ext(0x25)]]));
    }

    #[test]
    fn plus_and_backtick() {
        assert_eq!(parse_shortcut("alt+shift+plus").unwrap()[0].last(), Some(&k(0xBB)));
        assert_eq!(parse_shortcut("ctrl+`").unwrap()[0].last(), Some(&k(0xC0)));
    }

    #[test]
    fn media_keys_are_extended() {
        assert_eq!(parse_shortcut("volumeup").unwrap(), vec![vec![ext(0xAF)]]);
        assert_eq!(parse_shortcut("volumedown").unwrap(), vec![vec![ext(0xAE)]]);
        assert_eq!(parse_shortcut("volumemute").unwrap(), vec![vec![ext(0xAD)]]);
        assert_eq!(parse_shortcut("playpause").unwrap(), vec![vec![ext(0xB3)]]);
        assert_eq!(parse_shortcut("nexttrack").unwrap(), vec![vec![ext(0xB0)]]);
        assert_eq!(parse_shortcut("prevtrack").unwrap(), vec![vec![ext(0xB1)]]);
        assert_eq!(parse_shortcut("printscreen").unwrap(), vec![vec![ext(0x2C)]]);
    }

    #[test]
    fn unknown_key_is_error() {
        let err = parse_shortcut("ctrl+foo").unwrap_err();
        assert!(err.contains("unknown key \"foo\""), "{err}");
    }

    #[test]
    fn modifiers_only_is_error() {
        assert!(parse_shortcut("ctrl+shift").is_err());
    }

    #[test]
    fn two_main_keys_is_error() {
        assert!(parse_shortcut("a+b").is_err());
    }

    #[test]
    fn empty_is_error() {
        assert!(parse_shortcut("").is_err());
        assert!(parse_shortcut("   ").is_err());
    }
}
