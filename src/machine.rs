//! Chord-mode state machine. Pure logic: no Windows calls, so it is unit-tested anywhere.
//! The hook feeds it every physical key event and obeys the returned `Decision`.

pub const VK_BACK: u16 = 0x08;
pub const VK_CAPITAL: u16 = 0x14;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_LWIN: u16 = 0x5B;
pub const VK_RWIN: u16 = 0x5C;
pub const VK_LCONTROL: u16 = 0xA2;
pub const VK_RCONTROL: u16 = 0xA3;
pub const VK_LMENU: u16 = 0xA4;
pub const VK_RMENU: u16 = 0xA5;
pub const VK_OEM_2: u16 = 0xBF;

const CHORD_CAP: usize = 32;

/// Fixed-size buffer so the hook never allocates.
#[derive(Clone, Copy)]
pub struct Chord {
    bytes: [u8; CHORD_CAP],
    len: usize,
}

impl Chord {
    pub const fn new() -> Self {
        Self { bytes: [0; CHORD_CAP], len: 0 }
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

/// Bytes past `len` are stale after Backspace, so equality looks at the live part only.
impl PartialEq for Chord {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for Chord {}

impl std::fmt::Debug for Chord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    ToggleCaps,
    ResyncCapsLight,
    Enter,
    Show(Chord),
    Fire(Chord),
    Cancel,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub block: bool,
    pub effect: Option<Effect>,
}

const PASS: Decision = Decision { block: false, effect: None };

pub struct Machine {
    /// Keys whose press we swallowed; their release must be swallowed too, and only theirs.
    blocked: [bool; 256],
    /// Ctrl/Alt/Win currently down. In chord mode they mean "the user is typing a normal shortcut".
    modifiers: [bool; 256],
    caps_held: bool,
    /// Caps took part in a chord, so its release must not toggle Caps Lock.
    caps_used: bool,
    space_held: bool,
    chord_mode: bool,
    buffer: Chord,
}

impl Machine {
    pub const fn new() -> Self {
        Self {
            blocked: [false; 256],
            modifiers: [false; 256],
            caps_held: false,
            caps_used: false,
            space_held: false,
            chord_mode: false,
            buffer: Chord::new(),
        }
    }

    pub fn key(&mut self, vk: u16, down: bool) -> Decision {
        let i = (vk & 0xFF) as usize;
        if is_modifier(vk) {
            self.modifiers[i] = down;
            return PASS;
        }
        if down { self.press(vk, i) } else { self.release(vk, i) }
    }

    fn press(&mut self, vk: u16, i: usize) -> Decision {
        match vk {
            VK_SPACE => {
                let repeat = self.space_held;
                self.space_held = true;
                if self.chord_mode {
                    return self.block(i, None);
                }
                if self.caps_held && !repeat {
                    self.chord_mode = true;
                    self.caps_used = true;
                    self.buffer = Chord::new();
                    return self.block(i, Some(Effect::Enter));
                }
                return PASS;
            }
            VK_CAPITAL => {
                self.caps_held = true;
                // Auto-repeat must not reset it: a chord earlier in this same hold still counts.
                if !self.blocked[i] {
                    self.caps_used = self.chord_mode;
                }
                return self.block(i, None);
            }
            VK_ESCAPE if self.ctrl_held() && self.space_held => {
                return self.block(i, Some(Effect::Quit));
            }
            _ => {}
        }

        if !self.chord_mode || self.ctrl_alt_win_held() {
            return PASS;
        }
        if self.blocked[i] {
            return self.block(i, None);
        }
        match vk {
            VK_BACK => {
                self.buffer.len = self.buffer.len.saturating_sub(1);
                self.block(i, Some(Effect::Show(self.buffer)))
            }
            VK_ESCAPE => {
                self.buffer = Chord::new();
                self.block(i, Some(Effect::Show(self.buffer)))
            }
            _ => match char_of(vk) {
                Some(c) if self.buffer.len < CHORD_CAP => {
                    self.buffer.bytes[self.buffer.len] = c;
                    self.buffer.len += 1;
                    self.block(i, Some(Effect::Show(self.buffer)))
                }
                Some(_) => self.block(i, None),
                None => PASS,
            },
        }
    }

    fn release(&mut self, vk: u16, i: usize) -> Decision {
        if vk == VK_SPACE {
            self.space_held = false;
            if self.chord_mode {
                self.chord_mode = false;
                self.blocked[i] = false;
                let effect = if self.buffer.len == 0 { Effect::Cancel } else { Effect::Fire(self.buffer) };
                return Decision { block: true, effect: Some(effect) };
            }
        }
        if !self.blocked[i] {
            return PASS;
        }
        self.blocked[i] = false;
        if vk == VK_CAPITAL {
            self.caps_held = false;
            // The physical press flips the Caps light below the hook, even though we swallowed it.
            let effect = if self.caps_used { Effect::ResyncCapsLight } else { Effect::ToggleCaps };
            return Decision { block: true, effect: Some(effect) };
        }
        Decision { block: true, effect: None }
    }

    fn block(&mut self, i: usize, effect: Option<Effect>) -> Decision {
        self.blocked[i] = true;
        Decision { block: true, effect }
    }

    fn ctrl_held(&self) -> bool {
        self.modifiers[VK_LCONTROL as usize] || self.modifiers[VK_RCONTROL as usize]
    }

    fn ctrl_alt_win_held(&self) -> bool {
        [VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|&vk| self.modifiers[vk as usize])
    }
}

fn is_modifier(vk: u16) -> bool {
    matches!(vk, 0xA0..=0xA5 | VK_LWIN | VK_RWIN)
}

/// Every character `char_of` can produce; chord names in files must use only these.
pub const CHORD_CHARS: &str = "abcdefghijklmnopqrstuvwxyz0123456789;=,-./[]'";

/// Chord letters are matched by virtual key (US layout), never translated, so IMEs and dead keys can't interfere.
fn char_of(vk: u16) -> Option<u8> {
    match vk {
        0x41..=0x5A => Some(vk as u8 + 32),
        0x30..=0x39 => Some(vk as u8),
        0xBA => Some(b';'),
        0xBB => Some(b'='),
        0xBC => Some(b','),
        0xBD => Some(b'-'),
        0xBE => Some(b'.'),
        VK_OEM_2 => Some(b'/'),
        0xDB => Some(b'['),
        0xDD => Some(b']'),
        0xDE => Some(b'\''),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: u16 = 0x47;
    const D: u16 = 0x44;

    fn pass() -> Decision {
        Decision { block: false, effect: None }
    }
    fn block() -> Decision {
        Decision { block: true, effect: None }
    }
    fn block_with(e: Effect) -> Decision {
        Decision { block: true, effect: Some(e) }
    }
    fn chord(s: &str) -> Chord {
        let mut c = Chord::new();
        c.bytes[..s.len()].copy_from_slice(s.as_bytes());
        c.len = s.len();
        c
    }
    fn press(m: &mut Machine, vk: u16) -> Decision {
        m.key(vk, true)
    }
    fn release(m: &mut Machine, vk: u16) -> Decision {
        m.key(vk, false)
    }
    /// Caps held, Space held: chord mode is on.
    fn in_chord_mode() -> Machine {
        let mut m = Machine::new();
        press(&mut m, VK_CAPITAL);
        press(&mut m, VK_SPACE);
        m
    }

    #[test]
    fn caps_tap_toggles_caps_on_release() {
        let mut m = Machine::new();
        assert_eq!(press(&mut m, VK_CAPITAL), block());
        assert_eq!(release(&mut m, VK_CAPITAL), block_with(Effect::ToggleCaps));
    }

    #[test]
    fn caps_then_space_enters_chord_mode() {
        let mut m = Machine::new();
        press(&mut m, VK_CAPITAL);
        assert_eq!(press(&mut m, VK_SPACE), block_with(Effect::Enter));
    }

    #[test]
    fn caps_release_after_chord_mode_resyncs_the_light_instead_of_toggling() {
        let mut m = in_chord_mode();
        assert_eq!(release(&mut m, VK_CAPITAL), block_with(Effect::ResyncCapsLight));
    }

    #[test]
    fn letters_in_chord_mode_are_blocked_and_shown() {
        let mut m = in_chord_mode();
        assert_eq!(press(&mut m, G), block_with(Effect::Show(chord("g"))));
        assert_eq!(release(&mut m, G), block());
        assert_eq!(press(&mut m, D), block_with(Effect::Show(chord("gd"))));
    }

    #[test]
    fn slash_is_buffered() {
        let mut m = in_chord_mode();
        assert_eq!(press(&mut m, VK_OEM_2), block_with(Effect::Show(chord("/"))));
    }

    #[test]
    fn space_release_fires_the_buffer() {
        let mut m = in_chord_mode();
        press(&mut m, G);
        press(&mut m, D);
        assert_eq!(release(&mut m, VK_SPACE), block_with(Effect::Fire(chord("gd"))));
    }

    #[test]
    fn space_release_with_empty_buffer_cancels() {
        let mut m = in_chord_mode();
        assert_eq!(release(&mut m, VK_SPACE), block_with(Effect::Cancel));
    }

    #[test]
    fn chord_mode_survives_caps_release_while_space_is_held() {
        let mut m = in_chord_mode();
        release(&mut m, VK_CAPITAL);
        assert_eq!(press(&mut m, G), block_with(Effect::Show(chord("g"))));
    }

    #[test]
    fn chord_mode_ends_after_fire() {
        let mut m = in_chord_mode();
        press(&mut m, G);
        release(&mut m, VK_SPACE);
        release(&mut m, VK_CAPITAL);
        assert_eq!(press(&mut m, D), pass());
    }

    #[test]
    fn typing_outside_chord_mode_passes() {
        let mut m = Machine::new();
        assert_eq!(press(&mut m, VK_SPACE), pass());
        assert_eq!(release(&mut m, VK_SPACE), pass());
        assert_eq!(press(&mut m, G), pass());
        assert_eq!(release(&mut m, G), pass());
    }

    #[test]
    fn backspace_removes_last_letter() {
        let mut m = in_chord_mode();
        press(&mut m, G);
        press(&mut m, D);
        assert_eq!(press(&mut m, VK_BACK), block_with(Effect::Show(chord("g"))));
    }

    #[test]
    fn escape_clears_the_buffer() {
        let mut m = in_chord_mode();
        press(&mut m, G);
        assert_eq!(press(&mut m, VK_ESCAPE), block_with(Effect::Show(chord(""))));
    }

    #[test]
    fn key_repeat_is_not_appended() {
        let mut m = in_chord_mode();
        press(&mut m, G);
        assert_eq!(press(&mut m, G), block());
        assert_eq!(release(&mut m, VK_SPACE), block_with(Effect::Fire(chord("g"))));
    }

    #[test]
    fn release_of_key_blocked_in_chord_mode_stays_blocked_after_mode_ends() {
        let mut m = in_chord_mode();
        press(&mut m, G);
        release(&mut m, VK_SPACE);
        assert_eq!(release(&mut m, G), block());
    }

    #[test]
    fn release_never_seen_pressed_passes() {
        let mut m = in_chord_mode();
        assert_eq!(release(&mut m, G), pass());
    }

    #[test]
    fn letters_pass_while_ctrl_is_held_in_chord_mode() {
        let mut m = in_chord_mode();
        assert_eq!(press(&mut m, VK_LCONTROL), pass());
        assert_eq!(press(&mut m, G), pass());
        assert_eq!(release(&mut m, G), pass());
        release(&mut m, VK_LCONTROL);
        assert_eq!(press(&mut m, D), block_with(Effect::Show(chord("d"))));
    }

    #[test]
    fn ctrl_space_escape_quits() {
        let mut m = Machine::new();
        press(&mut m, VK_LCONTROL);
        press(&mut m, VK_SPACE);
        assert_eq!(press(&mut m, VK_ESCAPE), block_with(Effect::Quit));
    }

    #[test]
    fn caps_key_repeat_after_chord_does_not_toggle_caps() {
        let mut m = in_chord_mode();
        release(&mut m, VK_SPACE);
        press(&mut m, VK_CAPITAL); // auto-repeat while Caps is still held
        assert_eq!(release(&mut m, VK_CAPITAL), block_with(Effect::ResyncCapsLight));
    }

    #[test]
    fn held_space_does_not_start_chord_mode_when_caps_comes_later() {
        let mut m = Machine::new();
        press(&mut m, VK_SPACE);
        press(&mut m, VK_CAPITAL);
        assert_eq!(press(&mut m, VK_SPACE), pass()); // auto-repeat of the same Space hold
    }

    #[test]
    fn buffer_stops_growing_when_full() {
        let mut m = in_chord_mode();
        for _ in 0..CHORD_CAP {
            press(&mut m, G);
            release(&mut m, G);
        }
        assert_eq!(press(&mut m, D), block());
    }
}
