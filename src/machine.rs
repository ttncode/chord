//! Chord-mode state machine. Pure logic: no Windows calls, so it is unit-tested anywhere.
//! The hook feeds it every physical key event and obeys the returned `Decision`.

pub const VK_BACK: u16 = 0x08;
pub const VK_TAB: u16 = 0x09;
pub const VK_RETURN: u16 = 0x0D;
pub const VK_CAPITAL: u16 = 0x14;
pub const VK_ESCAPE: u16 = 0x1B;
pub const VK_SPACE: u16 = 0x20;
pub const VK_LEFT: u16 = 0x25;
pub const VK_UP: u16 = 0x26;
pub const VK_RIGHT: u16 = 0x27;
pub const VK_DOWN: u16 = 0x28;
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
    /// Run this chord but stay in chord mode (Caps tap, or Shift + last letter).
    FireKeep(Chord),
    Cancel,
    Quit,
    /// The newest letter typed in chord mode was let go: a held `repeat` chord stops.
    KeyUp,
    /// Search is open (Tab in chord mode) and this is its text.
    Search(Chord),
    /// Down or Up in search: move the highlight.
    Move { down: bool },
    /// Enter in search: run the highlighted chord.
    Pick,
    /// Arrows held in chord mode changed (`GLIDE_*` bits; 0 = none): the pointer glides that way.
    Glide(u8),
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
    /// Search swallows every key until Enter or Esc, even with Space and Caps let go; `buffer` holds its text.
    search: bool,
    /// Arrows held in chord mode, as `GLIDE_*` bits.
    arrows: u8,
    buffer: Chord,
    /// Last chord run with Caps or Shift, repeated by a Caps tap on an empty buffer.
    last: Chord,
    last_space_ms: u32,
    last_caps_ms: u32,
    /// Only the most recently pressed key auto-repeats, so silence proves a lost release only for it.
    last_down: u16,
    last_event_ms: u32,
    /// The letter typed last; only its release ends a held `repeat` chord.
    last_letter: u16,
}

/// A held key that was pressed last auto-repeats well within this; longer silence means its release was lost.
const STALE_MS: u32 = 1_500;
/// No key at all for this long in chord mode: assume Space's release was lost after typing.
const IDLE_MS: u32 = 15_000;
const MODIFIER_KEYS: [u16; 8] = [0xA0, 0xA1, VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN];

impl Machine {
    pub const fn new() -> Self {
        Self {
            blocked: [false; 256],
            modifiers: [false; 256],
            caps_held: false,
            caps_used: false,
            space_held: false,
            chord_mode: false,
            search: false,
            arrows: 0,
            buffer: Chord::new(),
            last: Chord::new(),
            last_space_ms: 0,
            last_caps_ms: 0,
            last_down: 0,
            last_event_ms: 0,
            last_letter: 0,
        }
    }

    /// `now_ms` is the event's timestamp (KBDLLHOOKSTRUCT.time).
    pub fn key_at(&mut self, vk: u16, down: bool, now_ms: u32) -> Decision {
        let idle = now_ms.wrapping_sub(self.last_event_ms) > IDLE_MS;
        let caps_lost = self.last_down == VK_CAPITAL && now_ms.wrapping_sub(self.last_caps_ms) > STALE_MS;
        if down && self.caps_held && vk == VK_SPACE && (caps_lost || idle) {
            self.caps_held = false;
            self.blocked[VK_CAPITAL as usize] = false;
        }
        let space_silent = self.last_down == VK_SPACE && now_ms.wrapping_sub(self.last_space_ms) > STALE_MS;
        let space_lost = down && self.chord_mode && vk != VK_SPACE && (space_silent || idle);
        if space_lost {
            self.chord_mode = false;
            self.space_held = false;
            self.blocked[VK_SPACE as usize] = false;
        }
        match vk {
            VK_SPACE => self.last_space_ms = now_ms,
            VK_CAPITAL => self.last_caps_ms = now_ms,
            _ => {}
        }
        self.last_event_ms = now_ms;
        if down {
            self.last_down = vk;
        }
        // A forgotten search must not eat the keyboard: after a long silence the key goes through.
        if down && self.search && idle {
            self.search = false;
            let decision = self.key(vk, down);
            return Decision { block: decision.block, effect: Some(Effect::Cancel) };
        }
        let decision = self.key(vk, down);
        if space_lost {
            return Decision { block: decision.block, effect: Some(Effect::Cancel) };
        }
        decision
    }

    /// Clears Ctrl/Alt/Win/Shift whose release we missed; `is_down` reads the real key state.
    pub fn sync_modifiers(&mut self, is_down: impl Fn(u16) -> bool) {
        for vk in MODIFIER_KEYS {
            if self.modifiers[vk as usize] && !is_down(vk) {
                self.modifiers[vk as usize] = false;
            }
        }
    }

    /// Back to idle, as after a fresh start (used after lock/unlock and sleep).
    pub fn reset(&mut self) {
        *self = Self::new();
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
        if self.search && !self.ctrl_alt_win_held() {
            return self.search_press(vk, i);
        }
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
                if self.blocked[i] {
                    return self.block(i, None);
                }
                self.caps_used = self.chord_mode;
                if !self.chord_mode {
                    return self.block(i, None);
                }
                if self.buffer.len > 0 {
                    self.last = self.buffer;
                    self.buffer = Chord::new();
                }
                let effect = (self.last.len > 0).then_some(Effect::FireKeep(self.last));
                return self.block(i, effect);
            }
            VK_ESCAPE if self.ctrl_held() && self.space_held => {
                return self.block(i, Some(Effect::Quit));
            }
            _ => {}
        }

        if !self.chord_mode || self.ctrl_alt_win_held() {
            return PASS;
        }
        if let Some(bit) = arrow_bit(vk) {
            // Auto-repeat changes nothing: the glide runs until the arrow is let go.
            let effect = (self.arrows & bit == 0).then_some(Effect::Glide(self.arrows | bit));
            self.arrows |= bit;
            return self.block(i, effect);
        }
        if self.blocked[i] {
            return self.block(i, None);
        }
        match vk {
            VK_BACK => {
                self.buffer.len = if self.shift_held() { 0 } else { self.buffer.len.saturating_sub(1) };
                self.block(i, Some(Effect::Show(self.buffer)))
            }
            VK_ESCAPE => {
                self.buffer = Chord::new();
                self.block(i, Some(Effect::Show(self.buffer)))
            }
            VK_TAB => {
                self.chord_mode = false;
                self.arrows = 0;
                self.search = true;
                self.buffer = Chord::new();
                self.block(i, Some(Effect::Search(self.buffer)))
            }
            _ => match char_of(vk) {
                Some(c) if self.shift_held() && self.buffer.len < CHORD_CAP => {
                    let mut fired = self.buffer;
                    fired.bytes[fired.len] = c;
                    fired.len += 1;
                    self.last = fired;
                    self.block(i, Some(Effect::FireKeep(fired)))
                }
                Some(c) if self.buffer.len < CHORD_CAP => {
                    self.buffer.bytes[self.buffer.len] = c;
                    self.buffer.len += 1;
                    self.last_letter = vk;
                    self.block(i, Some(Effect::Show(self.buffer)))
                }
                Some(_) => self.block(i, None),
                None => PASS,
            },
        }
    }

    fn release(&mut self, vk: u16, i: usize) -> Decision {
        if let Some(bit) = arrow_bit(vk).filter(|&bit| self.arrows & bit != 0) {
            self.arrows &= !bit;
            self.blocked[i] = false;
            return Decision { block: true, effect: Some(Effect::Glide(self.arrows)) };
        }
        if vk == VK_SPACE {
            self.space_held = false;
            if self.chord_mode {
                self.chord_mode = false;
                self.arrows = 0;
                self.blocked[i] = false;
                let effect = if self.buffer.len == 0 { Effect::Cancel } else { Effect::Fire(self.buffer) };
                return Decision { block: true, effect: Some(effect) };
            }
        }
        if !self.blocked[i] {
            return PASS;
        }
        self.blocked[i] = false;
        if self.chord_mode && vk == self.last_letter {
            return Decision { block: true, effect: Some(Effect::KeyUp) };
        }
        if vk == VK_CAPITAL {
            self.caps_held = false;
            // The physical press flips the Caps light below the hook, even though we swallowed it.
            let effect = if self.caps_used { Effect::ResyncCapsLight } else { Effect::ToggleCaps };
            return Decision { block: true, effect: Some(effect) };
        }
        Decision { block: true, effect: None }
    }

    /// No blocked check here: auto-repeat types and moves like in any search box.
    fn search_press(&mut self, vk: u16, i: usize) -> Decision {
        let effect = match vk {
            VK_ESCAPE | VK_CAPITAL => {
                self.search = false;
                // Caps here is the start of a new chord: its release must not toggle Caps Lock.
                if vk == VK_CAPITAL {
                    self.caps_held = true;
                    self.caps_used = true;
                }
                Effect::Cancel
            }
            VK_RETURN => {
                self.search = false;
                Effect::Pick
            }
            VK_DOWN | VK_UP => Effect::Move { down: vk == VK_DOWN },
            VK_BACK => {
                self.buffer.len = if self.shift_held() { 0 } else { self.buffer.len.saturating_sub(1) };
                Effect::Search(self.buffer)
            }
            _ => match char_of(vk).or((vk == VK_SPACE).then_some(b' ')) {
                Some(c) if self.buffer.len < CHORD_CAP => {
                    self.buffer.bytes[self.buffer.len] = c;
                    self.buffer.len += 1;
                    Effect::Search(self.buffer)
                }
                _ => return self.block(i, None),
            },
        };
        self.block(i, Some(effect))
    }

    fn block(&mut self, i: usize, effect: Option<Effect>) -> Decision {
        self.blocked[i] = true;
        Decision { block: true, effect }
    }

    fn shift_held(&self) -> bool {
        self.modifiers[0xA0] || self.modifiers[0xA1]
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

pub const GLIDE_LEFT: u8 = 1;
pub const GLIDE_RIGHT: u8 = 2;
pub const GLIDE_UP: u8 = 4;
pub const GLIDE_DOWN: u8 = 8;

fn arrow_bit(vk: u16) -> Option<u8> {
    match vk {
        VK_LEFT => Some(GLIDE_LEFT),
        VK_RIGHT => Some(GLIDE_RIGHT),
        VK_UP => Some(GLIDE_UP),
        VK_DOWN => Some(GLIDE_DOWN),
        _ => None,
    }
}

/// Unit direction (x right, y down) of the held `GLIDE_*` arrows; opposite arrows cancel out.
pub fn glide_direction(arrows: u8) -> (f64, f64) {
    let axis = |minus: u8, plus: u8| f64::from(arrows & plus != 0) - f64::from(arrows & minus != 0);
    let (x, y) = (axis(GLIDE_LEFT, GLIDE_RIGHT), axis(GLIDE_UP, GLIDE_DOWN));
    let length = (x * x + y * y).sqrt();
    if length == 0.0 { (0.0, 0.0) } else { (x / length, y / length) }
}

/// Pixels per second after holding arrows for `held_ms`: slow at first for precise aim, then a
/// quadratic ramp to full speed for crossing the screen (QMK's kinetic mouse keys).
pub fn glide_speed(held_ms: u64) -> f64 {
    const START: f64 = 120.0;
    const MAX: f64 = 2400.0;
    const RAMP_MS: f64 = 1200.0;
    let t = (held_ms as f64 / RAMP_MS).min(1.0);
    START + (MAX - START) * t * t
}

fn is_modifier(vk: u16) -> bool {
    matches!(vk, 0xA0..=0xA5 | VK_LWIN | VK_RWIN)
}

/// The chord to try after `buffer` grew by one letter. `ran` is (typed letters, chord) of the last
/// `repeat` chord run: one more letter re-runs that chord with its last letter swapped for the new
/// one, so `j j k` scrolls down, down, up instead of looking up `jj` and `jjk`.
/// The highlighted row after Down (`down`) or Up in a list of `len` rows; it stops at either end.
pub fn step(selected: Option<usize>, len: usize, down: bool) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match (selected, down) {
        (None, true) => 0,
        (None, false) => last,
        (Some(i), true) => (i + 1).min(last),
        (Some(i), false) => i.saturating_sub(1),
    })
}

/// The first row to draw when `room` rows fit, so the highlighted one is on screen.
pub fn first_shown(selected: Option<usize>, room: usize) -> usize {
    match selected {
        Some(i) if room > 0 && i >= room => i + 1 - room,
        _ => 0,
    }
}

pub fn repeat_candidate(ran: Option<(&str, &str)>, buffer: &str) -> String {
    if let Some((typed, chord)) = ran {
        if let Some(letter) = buffer.strip_prefix(typed).filter(|rest| rest.chars().count() == 1) {
            let mut swapped = chord.to_string();
            swapped.pop();
            return swapped + letter;
        }
    }
    buffer.to_string()
}

/// Every character `char_of` can produce; chord names in files must use only these.
/// Effects (Caps Lock sends, chord runs) within `STORM_WINDOW_MS` that mean a feedback loop, not a
/// person: like AutoHotkey's #MaxHotkeysPerInterval. A person tops out near 10 per second.
pub const STORM_MAX: usize = 30;
pub const STORM_WINDOW_MS: u64 = 2_000;

/// Counts effects over a sliding window; `hit` says when they come faster than a person can type.
#[derive(Default)]
pub struct Storm {
    times: std::collections::VecDeque<u64>,
}

impl Storm {
    pub fn hit(&mut self, now_ms: u64) -> bool {
        while self.times.front().is_some_and(|t| now_ms.saturating_sub(*t) > STORM_WINDOW_MS) {
            self.times.pop_front();
        }
        self.times.push_back(now_ms);
        self.times.len() >= STORM_MAX
    }
}

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
    fn tab_opens_search_that_stays_open_after_space_and_caps_are_let_go() {
        let mut m = in_chord_mode();
        assert_eq!(press(&mut m, VK_TAB), block_with(Effect::Search(chord(""))));
        assert_eq!(release(&mut m, VK_TAB), block());
        assert_eq!(release(&mut m, VK_SPACE), block());
        assert_eq!(release(&mut m, VK_CAPITAL), block_with(Effect::ResyncCapsLight));
        assert_eq!(press(&mut m, G), block_with(Effect::Search(chord("g"))));
        assert_eq!(press(&mut m, G), block_with(Effect::Search(chord("gg"))));
        assert_eq!(press(&mut m, VK_SPACE), block_with(Effect::Search(chord("gg "))));
        assert_eq!(release(&mut m, VK_SPACE), block());
        assert_eq!(press(&mut m, VK_BACK), block_with(Effect::Search(chord("gg"))));
        assert_eq!(press(&mut m, VK_DOWN), block_with(Effect::Move { down: true }));
        assert_eq!(press(&mut m, VK_DOWN), block_with(Effect::Move { down: true }));
        assert_eq!(press(&mut m, VK_UP), block_with(Effect::Move { down: false }));
        assert_eq!(press(&mut m, VK_RETURN), block_with(Effect::Pick));
        assert_eq!(release(&mut m, VK_RETURN), block());
        assert_eq!(press(&mut m, D), pass());
    }

    #[test]
    fn esc_closes_search_and_ctrl_shortcuts_pass_through_it() {
        let mut m = in_chord_mode();
        press(&mut m, VK_TAB);
        press(&mut m, VK_LCONTROL);
        assert_eq!(press(&mut m, G), pass());
        release(&mut m, VK_LCONTROL);
        assert_eq!(press(&mut m, VK_ESCAPE), block_with(Effect::Cancel));
        assert_eq!(press(&mut m, G), pass());
    }

    #[test]
    fn caps_in_search_closes_it_and_starts_a_new_chord() {
        let mut m = in_chord_mode();
        press(&mut m, VK_TAB);
        release(&mut m, VK_SPACE);
        release(&mut m, VK_CAPITAL);
        assert_eq!(press(&mut m, VK_CAPITAL), block_with(Effect::Cancel));
        assert_eq!(press(&mut m, VK_SPACE), block_with(Effect::Enter));
    }

    #[test]
    fn a_forgotten_search_lets_keys_through_after_a_long_silence() {
        let mut m = Machine::new();
        m.key_at(VK_CAPITAL, true, 1_000);
        m.key_at(VK_SPACE, true, 1_100);
        m.key_at(VK_TAB, true, 1_200);
        assert_eq!(m.key_at(G, true, 1_300), block_with(Effect::Search(chord("g"))));
        assert_eq!(m.key_at(G, true, 1_300 + IDLE_MS + 1), Decision { block: false, effect: Some(Effect::Cancel) });
    }

    #[test]
    fn arrows_in_chord_mode_glide_until_let_go_and_space_release_stops_them() {
        let mut m = in_chord_mode();
        assert_eq!(press(&mut m, VK_RIGHT), block_with(Effect::Glide(GLIDE_RIGHT)));
        assert_eq!(press(&mut m, VK_RIGHT), block());
        assert_eq!(press(&mut m, VK_DOWN), block_with(Effect::Glide(GLIDE_RIGHT | GLIDE_DOWN)));
        assert_eq!(release(&mut m, VK_RIGHT), block_with(Effect::Glide(GLIDE_DOWN)));
        assert_eq!(release(&mut m, VK_SPACE), block_with(Effect::Cancel));
        assert_eq!(release(&mut m, VK_DOWN), block());
        assert_eq!(press(&mut m, VK_DOWN), pass());
    }

    #[test]
    fn glide_goes_diagonal_at_the_same_speed_and_ramps_up() {
        assert_eq!(glide_direction(GLIDE_RIGHT), (1.0, 0.0));
        assert_eq!(glide_direction(GLIDE_LEFT | GLIDE_RIGHT), (0.0, 0.0));
        let (x, y) = glide_direction(GLIDE_UP | GLIDE_LEFT);
        assert!((x + 0.7071).abs() < 1e-3 && (y + 0.7071).abs() < 1e-3);
        assert_eq!(glide_speed(0), 120.0);
        assert!(glide_speed(300) < glide_speed(900));
        assert_eq!(glide_speed(5_000), 2400.0);
    }

    #[test]
    fn the_highlight_starts_at_either_end_and_stops_there() {
        assert_eq!(step(None, 5, true), Some(0));
        assert_eq!(step(None, 5, false), Some(4));
        assert_eq!(step(Some(0), 5, true), Some(1));
        assert_eq!(step(Some(4), 5, true), Some(4));
        assert_eq!(step(Some(0), 5, false), Some(0));
        assert_eq!(step(None, 0, true), None);
    }

    #[test]
    fn the_list_scrolls_just_enough_to_show_the_highlight() {
        assert_eq!(first_shown(None, 10), 0);
        assert_eq!(first_shown(Some(9), 10), 0);
        assert_eq!(first_shown(Some(10), 10), 1);
        assert_eq!(first_shown(Some(25), 10), 16);
        assert_eq!(first_shown(Some(3), 0), 0);
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
        assert_eq!(release(&mut m, G), block_with(Effect::KeyUp));
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
    fn lost_space_release_ends_chord_mode_on_next_key() {
        let mut m = Machine::new();
        m.key_at(VK_CAPITAL, true, 0);
        m.key_at(VK_SPACE, true, 10);
        m.key_at(VK_CAPITAL, false, 20);
        // Space up never arrived; a held Space would auto-repeat, so 2 s of silence means it was released.
        assert_eq!(
            m.key_at(G, true, 2_010),
            Decision { block: false, effect: Some(Effect::Cancel) }
        );
        assert_eq!(m.key_at(D, true, 2_020), pass());
    }

    #[test]
    fn pause_after_typing_keeps_chord_mode() {
        // Space stops auto-repeating once a letter is pressed, so silence after typing proves nothing.
        let mut m = Machine::new();
        m.key_at(VK_CAPITAL, true, 0);
        m.key_at(VK_SPACE, true, 10);
        m.key_at(VK_CAPITAL, false, 20);
        m.key_at(G, true, 100);
        m.key_at(G, false, 150);
        assert_eq!(m.key_at(D, true, 3_000), block_with(Effect::Show(chord("gd"))));
    }

    #[test]
    fn caps_repeat_after_a_pause_still_fires() {
        let mut m = Machine::new();
        m.key_at(VK_CAPITAL, true, 0);
        m.key_at(VK_SPACE, true, 10);
        m.key_at(VK_CAPITAL, false, 20);
        m.key_at(G, true, 100);
        m.key_at(G, false, 150);
        m.key_at(VK_CAPITAL, true, 200);
        m.key_at(VK_CAPITAL, false, 250);
        assert_eq!(m.key_at(VK_CAPITAL, true, 4_000), block_with(Effect::FireKeep(chord("g"))));
    }

    #[test]
    fn long_idle_in_chord_mode_gives_up() {
        let mut m = Machine::new();
        m.key_at(VK_CAPITAL, true, 0);
        m.key_at(VK_SPACE, true, 10);
        m.key_at(G, true, 100);
        m.key_at(G, false, 150);
        assert_eq!(
            m.key_at(D, true, 30_000),
            Decision { block: false, effect: Some(Effect::Cancel) }
        );
    }

    #[test]
    fn space_auto_repeat_keeps_chord_mode_alive() {
        let mut m = Machine::new();
        m.key_at(VK_CAPITAL, true, 0);
        m.key_at(VK_SPACE, true, 10);
        m.key_at(VK_SPACE, true, 1_400);
        m.key_at(VK_SPACE, true, 2_800);
        assert_eq!(m.key_at(G, true, 2_900), block_with(Effect::Show(chord("g"))));
    }

    #[test]
    fn lost_caps_release_does_not_start_chord_mode() {
        let mut m = Machine::new();
        m.key_at(VK_CAPITAL, true, 0);
        // Caps up never arrived; Space much later is a normal space.
        assert_eq!(m.key_at(VK_SPACE, true, 5_000), pass());
    }

    #[test]
    fn stuck_modifier_is_cleared_by_sync() {
        let mut m = in_chord_mode();
        press(&mut m, VK_LCONTROL); // its release gets lost
        m.sync_modifiers(|_| false);
        assert_eq!(press(&mut m, G), block_with(Effect::Show(chord("g"))));
    }

    #[test]
    fn reset_forgets_everything() {
        let mut m = in_chord_mode();
        press(&mut m, G);
        m.reset();
        assert_eq!(press(&mut m, D), pass());
        assert_eq!(release(&mut m, G), pass());
    }

    /// Chord mode with Caps already released (only Space held).
    fn space_only_chord_mode() -> Machine {
        let mut m = in_chord_mode();
        release(&mut m, VK_CAPITAL);
        m
    }

    #[test]
    fn caps_in_chord_mode_fires_and_clears() {
        let mut m = space_only_chord_mode();
        press(&mut m, G);
        release(&mut m, G);
        press(&mut m, D);
        release(&mut m, D);
        assert_eq!(press(&mut m, VK_CAPITAL), block_with(Effect::FireKeep(chord("gd"))));
        release(&mut m, VK_CAPITAL);
        assert_eq!(press(&mut m, G), block_with(Effect::Show(chord("g"))));
    }

    #[test]
    fn caps_with_empty_buffer_repeats_last_chord() {
        let mut m = space_only_chord_mode();
        press(&mut m, G);
        press(&mut m, D);
        press(&mut m, VK_CAPITAL);
        release(&mut m, VK_CAPITAL);
        assert_eq!(press(&mut m, VK_CAPITAL), block_with(Effect::FireKeep(chord("gd"))));
    }

    #[test]
    fn caps_repeat_while_held_does_not_fire_again() {
        let mut m = space_only_chord_mode();
        press(&mut m, G);
        press(&mut m, VK_CAPITAL);
        assert_eq!(press(&mut m, VK_CAPITAL), block());
    }

    #[test]
    fn space_release_after_caps_fire_does_not_fire_again() {
        let mut m = space_only_chord_mode();
        press(&mut m, G);
        press(&mut m, VK_CAPITAL);
        release(&mut m, VK_CAPITAL);
        assert_eq!(release(&mut m, VK_SPACE), block_with(Effect::Cancel));
    }

    #[test]
    fn shift_letter_fires_buffer_plus_letter_and_keeps_buffer() {
        let mut m = space_only_chord_mode();
        press(&mut m, 0x54); // t
        release(&mut m, 0x54);
        assert_eq!(press(&mut m, 0xA0), pass()); // left shift
        assert_eq!(press(&mut m, 0x54), block_with(Effect::FireKeep(chord("tt"))));
        release(&mut m, 0x54);
        assert_eq!(press(&mut m, 0x53), block_with(Effect::FireKeep(chord("ts")))); // s
        release(&mut m, 0xA0);
        assert_eq!(press(&mut m, 0x50), block_with(Effect::Show(chord("tp")))); // p
    }

    #[test]
    fn shift_backspace_clears_the_buffer() {
        let mut m = space_only_chord_mode();
        press(&mut m, G);
        press(&mut m, D);
        press(&mut m, 0xA1); // right shift
        assert_eq!(press(&mut m, VK_BACK), block_with(Effect::Show(chord(""))));
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

    #[test]
    fn letter_release_in_chord_mode_reports_key_up() {
        let mut m = in_chord_mode();
        press(&mut m, 0x4A);
        assert_eq!(press(&mut m, 0x4A), block(), "auto-repeat adds nothing");
        assert_eq!(release(&mut m, 0x4A), block_with(Effect::KeyUp));
    }

    #[test]
    fn only_the_newest_letter_reports_key_up() {
        let mut m = in_chord_mode();
        press(&mut m, G);
        press(&mut m, D);
        assert_eq!(release(&mut m, G), block(), "rollover: g let go after d was pressed");
        assert_eq!(release(&mut m, D), block_with(Effect::KeyUp));
    }

    #[test]
    fn tapping_again_after_a_repeat_chord_reruns_it_or_swaps_its_last_letter() {
        let ran = Some(("j", "j"));
        assert_eq!(repeat_candidate(None, "j"), "j");
        assert_eq!(repeat_candidate(ran, "jj"), "j");
        assert_eq!(repeat_candidate(ran, "jk"), "k");
        assert_eq!(repeat_candidate(Some(("jk", "k")), "jkk"), "k");
        assert_eq!(repeat_candidate(Some(("/vk", "/vk")), "/vkj"), "/vj");
        assert_eq!(repeat_candidate(ran, "jkx"), "jkx", "only one letter after the last run");
    }

    #[test]
    fn storm_ignores_fast_human_typing() {
        let mut storm = Storm::default();
        // 10 per second for 10 s: faster than anyone keeps up.
        assert!((0..100).all(|i| !storm.hit(i * 100)));
    }

    #[test]
    fn storm_trips_on_a_burst_inside_the_window() {
        let mut storm = Storm::default();
        let tripped: Vec<bool> = (0..STORM_MAX as u64).map(|i| storm.hit(1_000 + i * 10)).collect();
        assert_eq!(tripped.iter().filter(|t| **t).count(), 1);
        assert!(tripped[STORM_MAX - 1]);
    }

    #[test]
    fn storm_forgets_hits_older_than_the_window() {
        let mut storm = Storm::default();
        for i in 0..STORM_MAX as u64 - 1 {
            storm.hit(i);
        }
        assert!(!storm.hit(STORM_WINDOW_MS + 100));
    }
}
