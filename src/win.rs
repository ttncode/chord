//! Windows glue: low-level keyboard hook, SendInput, foreground app lookup.
//!
//! Threads: the hook thread only runs `Machine` and hands effects over with `try_send`;
//! it never blocks, because Windows silently drops a hook that takes longer than ~1 s.
//! The main thread does everything slow (printing, SendInput, process lookup).

use std::cell::RefCell;
use std::io::Write;
use std::os::windows::process::CommandExt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{RecvTimeoutError, SyncSender, sync_channel};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT,
    WPARAM,
};
use windows::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
};
use windows::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle};
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    CreateMutexW, GetCurrentThread, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    GetAsyncKeyState, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, FindWindowW, GetCursorPos, GetForegroundWindow, GetWindowRect, GetMessageW, GetWindowTextW, GetWindowThreadProcessId,
    HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG, PostThreadMessageW, SetTimer, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_SYSKEYDOWN, WM_TIMER,
};
use windows::core::{HSTRING, PWSTR, w};

use crate::config::{self, Action, Hint, Mouse};
use crate::{hints, uia};
use crate::panel;
use crate::layout::Layout;
use crate::window;
use crate::keys::{Key, parse_shortcut};
use crate::machine::{Effect, Machine, STORM_MAX, STORM_WINDOW_MS, Storm, VK_CAPITAL, VK_SPACE, repeat_candidate, step, glide_direction, glide_speed};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CAPS: [Key; 1] = [Key { vk: VK_CAPITAL, extended: false }];

/// Marks our own SendInput events so the hook skips exactly those (and not other tools' injected input).
const TAG: usize = 0x4348_4F52;

static EFFECTS: OnceLock<SyncSender<Effect>> = OnceLock::new();
static HOOK: AtomicIsize = AtomicIsize::new(0);
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
/// Set when a feedback loop is detected: from then on the hook passes every key untouched.
static STOPPED: AtomicBool = AtomicBool::new(false);
const WM_RESET_HOOK: u32 = WM_APP + 10;
const REINSTALL_EVERY_MS: u32 = 5 * 60 * 1000;

thread_local! {
    static MACHINE: RefCell<Machine> = const { RefCell::new(Machine::new()) };
}

/// False if another chord process already holds the keyboard hook.
pub fn claim_single_instance() -> bool {
    unsafe {
        // The handle is leaked on purpose: the mutex must live as long as the process.
        let created = CreateMutexW(None, true, w!("Local\\chord-ttncode-single-instance"));
        created.is_ok() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Prints into the terminal chord was started from, if any. False when there is none (Start menu,
/// Explorer, sign-in), so the caller knows nobody will see console output.
pub fn attach_console() -> bool {
    unsafe {
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            return false;
        }
        // A GUI program starts without standard handles; point stdout and stderr at the console.
        if let Ok(out) = CreateFileW(
            w!("CONOUT$"),
            (GENERIC_READ | GENERIC_WRITE).0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(0),
            None,
        ) {
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, out);
            let _ = SetStdHandle(STD_ERROR_HANDLE, out);
        }
        true
    }
}

/// A message box, for when there is no terminal to print to.
pub fn message(text: &str) {
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), w!("chord"), MB_OK | MB_ICONINFORMATION);
    }
}

/// One chord-mode session: the app it started in and the chords usable there, read once.
struct Session {
    app: String,
    title: String,
    files: Vec<config::ChordFile>,
    chords: Vec<(String, String)>,
    groups: Vec<(String, String)>,
}

impl Session {
    fn start(dir: &Path) -> Self {
        let (exe, title) = foreground();
        let (files, _) = config::load_dir(dir);
        let chords = config::available(&files, &exe, &title);
        let groups = config::available_groups(&files, &exe, &title);
        Self { chords, groups, files, app: exe, title }
    }

    fn view(&self, buffer: &str) -> panel::View {
        let matching = self.chords.iter().filter(|(chord, _)| match chord.split_once('(') {
            // Pattern chords: compare only the literal part before the first `(`.
            Some((prefix, _)) => prefix.starts_with(buffer) || buffer.starts_with(prefix),
            None => chord.starts_with(buffer),
        });
        // A group stays listed until its whole prefix is typed: by then its chords are the rows.
        let groups = self.groups.iter().filter(|(prefix, _)| prefix.starts_with(buffer) && prefix != buffer);
        panel::View {
            app: self.app.clone(),
            typed: buffer.to_string(),
            rows: matching.cloned().collect(),
            groups: groups.cloned().collect(),
            search: false,
            selected: None,
        }
    }

    fn search_view(&self, query: &str, selected: Option<usize>) -> panel::View {
        let rows = config::search(&self.chords, query);
        panel::View { app: self.app.clone(), typed: query.to_string(), rows, groups: Vec::new(), search: true, selected }
    }

    /// The action of `chord` if it is a `repeat` chord here.
    fn repeat_action(&self, chord: &str) -> Option<Action> {
        let found = config::resolve(&self.files, &self.app, &self.title, chord)?;
        found.chord.repeat.then_some(found.chord.action)
    }

    /// Length of the hint chord `chord` starts with (`;` in `;aj`), and its click kind.
    fn hint_prefix(&self, chord: &str) -> Option<(usize, Hint)> {
        config::hint_prefix(&self.files, &self.app, &self.title, chord)
    }
}

/// A `repeat` chord whose last letter is held: it runs again at `next` until the letter is let go.
struct Held {
    action: Action,
    next: Instant,
}

/// Scrolling repeats fast in small steps so it glides; other actions repeat like a held key.
const SCROLL_DELAY: Duration = Duration::from_millis(200);
const SCROLL_EVERY: Duration = Duration::from_millis(15);
const SCROLL_STEP: i32 = 40;
const REPEAT_DELAY: Duration = Duration::from_millis(400);
const REPEAT_EVERY: Duration = Duration::from_millis(100);

impl Held {
    fn start(chord: &str, action: Action) -> Self {
        say!("  -> {chord} (held)");
        without_shift(|| run_action(&action));
        let delay = if matches!(action, Action::Mouse(_)) { SCROLL_DELAY } else { REPEAT_DELAY };
        Self { action, next: Instant::now() + delay }
    }

    fn tick(&mut self) {
        let smooth = match &self.action {
            Action::Mouse(m) => window::scroll(*m, SCROLL_STEP),
            _ => false,
        };
        if !smooth {
            without_shift(|| run_action(&self.action));
        }
        // From now, not from the missed deadline: a slow action must not queue a burst of catch-up runs.
        self.next = Instant::now() + if smooth { SCROLL_EVERY } else { REPEAT_EVERY };
    }
}

/// The pointer gliding while arrows are held in chord mode.
struct Glide {
    direction: (f64, f64),
    started: Instant,
    last: Instant,
    /// Kept in fractions: at low speed one tick moves less than a pixel.
    at: (f64, f64),
}

const GLIDE_EVERY: Duration = Duration::from_millis(8);
/// A tap moves this far at once, so short taps nudge the pointer by a few pixels.
const GLIDE_NUDGE: f64 = 3.0;

impl Glide {
    fn start(direction: (f64, f64)) -> Self {
        let mut p = POINT::default();
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        let now = Instant::now();
        let mut glide = Self { direction, started: now, last: now, at: (f64::from(p.x), f64::from(p.y)) };
        glide.step(GLIDE_NUDGE);
        glide
    }

    fn tick(&mut self) {
        let now = Instant::now();
        // shortcut: speed is in physical pixels, so it feels faster on a low-DPI screen; scale by DPI if it bothers.
        let speed = glide_speed(now.duration_since(self.started).as_millis() as u64);
        let distance = speed * now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.step(distance);
    }

    fn step(&mut self, distance: f64) {
        self.at.0 += self.direction.0 * distance;
        self.at.1 += self.direction.1 * distance;
        let (x, y) = (self.at.0.round() as i32, self.at.1.round() as i32);
        let mut p = POINT::default();
        window::move_pointer(x, y);
        unsafe {
            let _ = GetCursorPos(&mut p);
        }
        // Stopped at a screen edge (or the real mouse moved): carry on from where the pointer is.
        if (p.x - x).abs() > 1 || (p.y - y).abs() > 1 {
            self.at = (f64::from(p.x), f64::from(p.y));
        }
    }
}

/// Hint labels on screen: the letters typed after `key` pick a target.
struct Hints {
    key: String,
    kind: Mouse,
    area: RECT,
    targets: Vec<(hints::Rect, String)>,
    /// Targets are scroll areas: each label sits where the pointer will go.
    areas: bool,
}

impl Hints {
    fn scan(hwnd: HWND, key: &str, kind: Mouse) -> Option<Self> {
        if uia::DEBUG.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = std::fs::write("hints-debug.txt", "");
        }
        // Electron apps (Postman, Antigravity) build their accessibility tree only once asked, and
        // drop it again after a while unused: a read can fail or come back empty while it rebuilds.
        let areas = std::cell::Cell::new(false);
        let read = || {
            areas.set(false);
            if kind == Mouse::Point {
                // Apps that scroll on their own report no scroll areas: label what's clickable instead,
                // and the pane holding the picked element is the one the wheel scrolls.
                let found = uia::scrollable_rects(hwnd).map(hints::areas);
                if matches!(&found, Ok(rects) if !rects.is_empty()) {
                    areas.set(true);
                    return found;
                }
            }
            uia::clickable_rects(hwnd).map(hints::targets)
        };
        let mut result = read();
        for wait in [300, 600] {
            if matches!(&result, Ok(rects) if !rects.is_empty()) {
                break;
            }
            std::thread::sleep(Duration::from_millis(wait));
            result = read();
        }
        let rects = match result {
            Ok(rects) if !rects.is_empty() => rects,
            Ok(_) => {
                say!("  -> no clickable elements found");
                return None;
            }
            Err(e) => {
                say!("  ! could not read this window's buttons: {e}");
                return None;
            }
        };
        let mut area = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut area);
        }
        let areas = areas.get();
        // Labels change after a drop, and so do their widths: check again until nothing drops.
        // Scroll area labels sit apart (see `spot`), so none drop.
        let mut rects = rects;
        let labels = loop {
            let labels = hints::labels(rects.len());
            let kept = hints::unstack(rects.clone(), &panel::hint_sizes(area, &labels));
            if areas || kept.len() == rects.len() {
                break labels;
            }
            rects = kept;
        };
        let targets = rects.into_iter().zip(labels).collect::<Vec<_>>();
        Some(Self { key: key.to_string(), kind, area, targets, areas })
    }

    fn show(&self, typed: &str) {
        // Labels sit on each element's top-left corner, so they don't hide its text.
        let visible = self
            .targets
            .iter()
            .filter(|(_, label)| label.starts_with(typed))
            .map(|&(r, ref label)| {
                let (x, y) = if self.areas { self.spot(r) } else { (r.0, r.1) };
                (POINT { x, y }, label.clone())
            })
            .collect();
        panel::show_hints(self.area, visible, typed);
    }

    fn spot(&self, r: hints::Rect) -> (i32, i32) {
        hints::spot(r, &self.targets.iter().map(|&(r, _)| r).collect::<Vec<_>>())
    }

    fn click(&self, label: &str) {
        match self.targets.iter().find(|(_, l)| l == label) {
            Some(&(r, _)) => {
                let (x, y) = if self.areas { self.spot(r) } else { ((r.0 + r.2) / 2, (r.1 + r.3) / 2) };
                if uia::DEBUG.load(std::sync::atomic::Ordering::Relaxed) {
                    say!("  -> {label} at ({x}, {y}): {}", uia::describe_at(x, y));
                }
                window::move_pointer(x, y);
                without_shift(|| window::mouse(self.kind));
            }
            None => say!("  -> no hint '{label}'"),
        }
    }
}

pub fn run(chords_dir: PathBuf) {
    unsafe {
        // Physical pixels everywhere, so UI Automation rectangles, the cursor and the overlay agree.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let (tx, rx) = sync_channel(64);
    EFFECTS.set(tx).expect("run() called twice");
    std::thread::spawn(hook_thread);
    panel::start(chords_dir.clone());

    // If chord was killed (or crashed) while Caps Lock was held, the keyboard left the light flipped.
    resync_caps_light();
    say!("chord running. Chord files: {}", chords_dir.display());
    let (files, errors) = config::load_dir(&chords_dir);
    for f in &files {
        say!("  loaded {} \"{}\" ({} chords)", f.file, f.name, f.chords.len());
    }
    print_errors(&errors);
    say!("  Hold Caps Lock, then hold Space, type a chord, release Space.");
    say!("  Emergency exit: Ctrl + Space + Esc (or Ctrl+C here).");

    let mut session: Option<Session> = None;
    let mut hints: Option<Hints> = None;
    let mut buffer = String::new();
    let mut storm = Storm::default();
    let started = Instant::now();
    let mut held: Option<Held> = None;
    // (typed letters, chord) of the repeat chord run last, so releasing Space doesn't run it once more.
    let mut ran: Option<(String, String)> = None;
    // Open search: its text, the highlighted row, and the window in front when it opened.
    let mut search: Option<(String, Option<usize>, HWND)> = None;
    let mut glide: Option<Glide> = None;
    loop {
        let effect = match (&mut held, &mut glide) {
            (None, Some(g)) => match rx.recv_timeout(GLIDE_EVERY) {
                Ok(effect) => effect,
                Err(RecvTimeoutError::Timeout) => {
                    g.tick();
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            (Some(h), _) => match rx.recv_timeout(h.next.saturating_duration_since(Instant::now())) {
                Ok(effect) => effect,
                Err(RecvTimeoutError::Timeout) => {
                    h.tick();
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
            // Duration::MAX makes recv_timeout wait like recv.
            (None, None) => match rx.recv_timeout(if search.is_some() { Duration::from_millis(250) } else { Duration::MAX }) {
                Ok(effect) => effect,
                Err(RecvTimeoutError::Timeout) => {
                    // Search swallows the keyboard, so it closes once the user moves to another window.
                    if search.as_ref().is_some_and(|(_, _, w)| *w != unsafe { GetForegroundWindow() }) {
                        search = None;
                        panel::hide();
                        reset_hook();
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => break,
            },
        };
        held = None;
        if !matches!(effect, Effect::Glide(_)) {
            glide = None;
        }
        let ours = matches!(effect, Effect::ToggleCaps | Effect::ResyncCapsLight | Effect::Fire(_) | Effect::FireKeep(_));
        if ours && storm.hit(started.elapsed().as_millis() as u64) {
            stop_for_loop();
        }
        match effect {
            Effect::KeyUp => {}
            Effect::Enter => {
                window::put_down();
                ran = None;
                search = None;
                buffer.clear();
                end_hints(&mut hints);
                let s = Session::start(&chords_dir);
                panel::show(s.view(""));
                session = Some(s);
                show("");
            }
            Effect::Show(chord) => {
                let typed_letter = chord.as_str().len() > buffer.len();
                buffer = chord.as_str().to_string();
                show(&buffer);
                if let Some(h) = &hints {
                    match buffer.strip_prefix(h.key.as_str()) {
                        Some(typed) => {
                            h.show(typed);
                            continue;
                        }
                        None => end_hints(&mut hints),
                    }
                }
                let Some(s) = &session else { continue };
                let candidate = repeat_candidate(ran.as_ref().map(|(t, c)| (t.as_str(), c.as_str())), &buffer);
                if let Some(action) = s.repeat_action(&candidate).filter(|_| typed_letter) {
                    panel::hide();
                    // Tap only: a held key would flicker the window through its layouts, or click 10 times a second.
                    if matches!(action, Action::Window(_) | Action::Mouse(Mouse::Click | Mouse::RightClick | Mouse::DoubleClick | Mouse::MiddleClick)) {
                        say!("  -> {candidate}");
                        without_shift(|| run_action(&action));
                    } else {
                        held = Some(Held::start(&candidate, action));
                    }
                    ran = Some((buffer.clone(), candidate));
                    continue;
                }
                // Exactly a hint chord: show its labels.
                hints = match s.hint_prefix(&buffer) {
                    Some((len, kind)) if len == buffer.len() => open_hints(&buffer, "", kind),
                    _ => None,
                };
                if hints.is_none() {
                    panel::show(s.view(&buffer));
                }
            }
            Effect::FireKeep(chord) => {
                // A Caps tap ran (and cleared) the typed letters; Shift + letter keeps them.
                if chord.as_str() == buffer {
                    buffer.clear();
                }
                let Some(s) = &session else { continue };
                match s.hint_prefix(chord.as_str()) {
                    // Caps repeat or Shift on a hint chord or a label pick: click the label if labels
                    // are up, then show fresh labels for the next pick (the click may change the window).
                    Some((_, kind)) => {
                        if click_hint(&mut hints, chord.as_str()) {
                            std::thread::sleep(Duration::from_millis(150));
                        }
                        let key = s.hint_prefix(&buffer).map_or(0, |(len, _)| len);
                        hints = open_hints(&buffer[..key], &buffer[key..], kind);
                        if hints.is_none() {
                            panel::show(s.view(&buffer));
                        }
                    }
                    None => {
                        end_hints(&mut hints);
                        fire(&chords_dir, chord.as_str());
                        panel::show(s.view(&buffer));
                    }
                }
                show(&buffer);
            }
            Effect::Search(query) => {
                window::put_down();
                let Some(s) = &session else { continue };
                let window = search.take().map_or_else(|| unsafe { GetForegroundWindow() }, |(_, _, w)| w);
                let mut view = s.search_view(query.as_str(), None);
                view.selected = (!view.rows.is_empty()).then_some(0);
                search = Some((query.as_str().to_string(), view.selected, window));
                panel::show(view);
            }
            Effect::Move { down } => {
                let (Some(s), Some((query, selected, _))) = (&session, &mut search) else { continue };
                let mut view = s.search_view(query, None);
                *selected = step(*selected, view.rows.len(), down);
                view.selected = *selected;
                panel::show(view);
            }
            Effect::Glide(arrows) => {
                let direction = glide_direction(arrows);
                match &mut glide {
                    _ if direction == (0.0, 0.0) => glide = None,
                    // A second arrow turns the glide without losing its speed.
                    Some(g) => g.direction = direction,
                    None => {
                        // The list would cover the pointer it is moving.
                        panel::hide_now();
                        glide = Some(Glide::start(direction));
                    }
                }
            }
            Effect::Pick => {
                panel::hide();
                let (Some(s), Some((query, Some(i), _))) = (&session, search.take()) else { continue };
                let Some((chord, _)) = s.search_view(&query, None).rows.into_iter().nth(i) else { continue };
                if s.hint_prefix(&chord).is_some() {
                    // shortcut: label chords need Space held to type the label, so search only shows their keys.
                    say!("  -> {chord} shows labels: hold Caps Lock + Space and type it");
                } else {
                    fire(&chords_dir, &chord);
                }
                window::put_down();
            }
            Effect::Cancel => {
                window::put_down();
                search = None;
                panel::hide();
                end_hints(&mut hints);
                say!("  (cancelled)");
            }
            Effect::Fire(chord) => {
                panel::hide();
                let hint_chord = session.as_ref().is_some_and(|s| s.hint_prefix(chord.as_str()).is_some());
                if ran.take().is_some_and(|(typed, _)| typed == chord.as_str()) {
                    // Already ran while its letter was held.
                } else if !click_hint(&mut hints, chord.as_str()) {
                    if hint_chord {
                        say!("  -> no labels on screen to pick from");
                    } else {
                        fire(&chords_dir, chord.as_str());
                    }
                }
                // Space is up: a window carried to another desktop lands there.
                window::put_down();
            }
            Effect::ToggleCaps => send_combo(&CAPS),
            Effect::ResyncCapsLight => resync_caps_light(),
            Effect::Quit => {
                window::put_down();
                say!("\nemergency exit");
                panel::remove_tray();
                std::process::exit(0);
            }
        }
    }
}

/// Hides the panel, then labels the window in front (or the taskbar); `key` is the typed part before the label.
fn open_hints(key: &str, typed: &str, hint: Hint) -> Option<Hints> {
    panel::hide_now();
    let hwnd = match hint.taskbar {
        // shortcut: main taskbar only; other monitors' taskbars are Shell_SecondaryTrayWnd, add them if asked.
        true => unsafe { FindWindowW(w!("Shell_TrayWnd"), None) }.ok()?,
        false => unsafe { GetForegroundWindow() },
    };
    let hints = Hints::scan(hwnd, key, hint.click)?;
    hints.show(typed);
    Some(hints)
}

fn end_hints(hints: &mut Option<Hints>) {
    if hints.take().is_some() {
        panel::hide_hints();
    }
}

/// True when hints were showing: the chord was a hint pick (clicked or not) and must not run as a chord.
fn click_hint(hints: &mut Option<Hints>, chord: &str) -> bool {
    let Some(h) = hints.take() else { return false };
    panel::hide_hints();
    match chord.strip_prefix(h.key.as_str()) {
        Some("") => say!("  -> no hint typed"),
        Some(label) => h.click(label),
        None => return false,
    }
    true
}

fn show(buffer: &str) {
    let mut out = std::io::stdout();
    let _ = write!(out, "\r\x1b[2K[chord] {buffer}");
    let _ = out.flush();
}

fn print_errors(errors: &[String]) {
    for e in errors {
        say!("  ! {e}");
    }
}

// shortcut: files are re-read when chord mode starts and on every fire; cache and watch them if that ever shows up as lag.
fn fire(dir: &Path, chord: &str) {
    let (exe, title) = foreground();
    let (files, errors) = config::load_dir(dir);
    let Some(found) = config::resolve(&files, &exe, &title, chord) else {
        say!("  -> no chord '{chord}' for {exe} \"{title}\"");
        print_errors(&errors);
        return;
    };
    say!("  -> {} ({exe})", found.chord.name);
    print_errors(&errors);
    if !found.shadowed.is_empty() {
        say!("  ! '{chord}' is also in {}; {} wins", found.shadowed.join(", "), found.file.file);
    }
    without_shift(|| run_action(&found.chord.action));
}

fn run_action(action: &Action) {
    match action {
        Action::Shortcut(combos) => {
            for (i, combo) in combos.iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(Duration::from_millis(30));
                }
                send_combo(combo);
            }
        }
        Action::Palette(text) => {
            let shortcut = |s| parse_shortcut(s).expect("built-in shortcut").remove(0);
            send_combo(&shortcut("ctrl+shift+p"));
            // shortcut: fixed wait for the palette; if it opens later, the text lands in the editor. Upgrade to a UI Automation wait in the hints phase.
            std::thread::sleep(Duration::from_millis(400));
            type_text(text);
            std::thread::sleep(Duration::from_millis(50));
            send_combo(&shortcut("enter"));
        }
        Action::Shell(command) => run_shell(command),
        Action::Focus { exe, launch } => {
            if !window::focus_app(exe, exe_of) {
                match launch {
                    Some(command) => run_shell(command),
                    None => say!("  ! {exe} has no open window"),
                }
            }
        }
        Action::Mouse(action) => {
            if matches!(action, Mouse::ScrollUp | Mouse::ScrollDown | Mouse::ScrollLeft | Mouse::ScrollRight) {
                aim_wheel();
            }
            window::mouse(*action)
        }
        Action::Window(layout @ (Layout::PreviousDesktop | Layout::NextDesktop)) => {
            let keys = if *layout == Layout::NextDesktop { "win+ctrl+right" } else { "win+ctrl+left" };
            window::move_to_desktop(|| send_combo(&parse_shortcut(keys).expect("built-in shortcut").remove(0)));
        }
        Action::Window(layout) => window::snap(*layout),
        // Hint chords never get here: `run` handles them while typing.
        Action::Hints(_) => {}
    }
}

/// The wheel scrolls what is under the pointer, not what has focus: a pointer resting on a page header,
/// a sidebar or another window scrolls nothing. Moves it onto the biggest scroll area of the window in
/// front, unless it is already on one.
fn aim_wheel() {
    // (window, pointer) last checked: tapping `j` again doesn't scan again.
    static CHECKED: std::sync::Mutex<Option<(isize, (i32, i32))>> = std::sync::Mutex::new(None);
    let mut p = POINT::default();
    let hwnd = unsafe {
        let _ = GetCursorPos(&mut p);
        GetForegroundWindow()
    };
    let mut checked = CHECKED.lock().unwrap_or_else(|e| e.into_inner());
    if *checked == Some((hwnd.0 as isize, (p.x, p.y))) {
        return;
    }
    // An empty read may be a browser still building its tree: check again next time.
    let Ok(rects) = uia::scrollable_rects(hwnd) else { return };
    if rects.is_empty() {
        return;
    }
    let (x, y) = hints::aim(rects, (p.x, p.y)).unwrap_or((p.x, p.y));
    window::move_pointer(x, y);
    *checked = Some((hwnd.0 as isize, (x, y)));
}

fn run_shell(command: &str) {
    let spawned = std::process::Command::new("cmd")
        .raw_arg(config::cmd_line(command))
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    if let Err(e) = spawned {
        say!("  ! could not run `{command}`: {e}");
    }
}

/// The Shift trick leaves Shift physically held; lift it (tagged, so the hook ignores it) so `gd`
/// sends F12, not Shift+F12, and a hint click isn't a Shift+click. Pressed again afterwards.
fn without_shift(action: impl FnOnce()) {
    let held: Vec<u16> = [0xA0u16, 0xA1].into_iter().filter(|&vk| unsafe { GetAsyncKeyState(vk as i32) } < 0).collect();
    let send = |vk: u16, up: bool| {
        let flags = if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) };
        unsafe { SendInput(&[key_input(vk, 0, flags)], size_of::<INPUT>() as i32) };
    };
    held.iter().for_each(|&vk| send(vk, true));
    action();
    held.iter().for_each(|&vk| send(vk, false));
}

/// Asks the hook thread to forget all key state and reinstall the hook (after unlock or sleep).
pub fn reset_hook() {
    let thread = HOOK_THREAD.load(Ordering::Acquire);
    if thread != 0 {
        unsafe {
            let _ = PostThreadMessageW(thread, WM_RESET_HOOK, WPARAM(0), LPARAM(0));
        }
    }
}

/// Windows silently drops a hook that is ever too slow; reinstalling now and then revives it.
fn install_hook() {
    unsafe {
        let module = GetModuleHandleW(None).expect("GetModuleHandleW");
        let old = HOOK.load(Ordering::Acquire);
        if old != 0 {
            let _ = UnhookWindowsHookEx(HHOOK(old as *mut _));
        }
        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), HINSTANCE(module.0), 0) {
            Ok(h) => HOOK.store(h.0 as isize, Ordering::Release),
            Err(e) => {
                say!("could not install keyboard hook: {e}");
                std::process::exit(1);
            }
        }
    }
}

fn hook_thread() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        HOOK_THREAD.store(GetCurrentThreadId(), Ordering::Release);
        install_hook();
        let _ = SetTimer(HWND::default(), 0, REINSTALL_EVERY_MS, None);
        // The hook only runs while this thread pumps messages.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            match msg.message {
                WM_TIMER => install_hook(),
                WM_RESET_HOOK => {
                    MACHINE.with_borrow_mut(|m| m.reset());
                    install_hook();
                }
                _ => {}
            }
        }
    }
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 && !STOPPED.load(Ordering::Relaxed) {
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        // Remote desktop and VM clients sync Caps Lock by injecting it: handling that would toggle
        // Caps again, they sync again, and the loop floods input until the machine hangs.
        let leave_caps = event.vkCode == VK_CAPITAL as u32
            && ((event.flags & LLKHF_INJECTED).0 != 0
                || (down && exe_of(unsafe { GetForegroundWindow() }).is_some_and(|e| config::remote_client(&e))));
        if event.dwExtraInfo != TAG && !leave_caps {
            // A panic must not unwind into Windows; on panic the key just passes through.
            let decision = catch_unwind(AssertUnwindSafe(|| {
                MACHINE.with_borrow_mut(|m| {
                    let vk = event.vkCode as u16;
                    if down && (vk == VK_SPACE || vk == VK_CAPITAL) {
                        m.sync_modifiers(|k| unsafe { GetAsyncKeyState(k as i32) } < 0);
                    }
                    m.key_at(vk, down, event.time)
                })
            }));
            if let Ok(decision) = decision {
                if let (Some(effect), Some(tx)) = (decision.effect, EFFECTS.get()) {
                    // Never block here: if the main thread is stuck, drop the effect, not the hook.
                    let _ = tx.try_send(effect);
                }
                if decision.block {
                    return LRESULT(1);
                }
            }
        }
    }
    unsafe { CallNextHookEx(HHOOK::default(), code, wparam, lparam) }
}

fn key_input(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: TAG,
            },
        },
    }
}

/// Another program keeps feeding our own input back to us. Let go of the keyboard first, so the loop
/// ends even while the message waits for OK, then quit.
fn stop_for_loop() -> ! {
    STOPPED.store(true, Ordering::SeqCst);
    window::put_down();
    let (exe, _) = foreground();
    say!("\nstopped: feedback loop with {exe}");
    panel::remove_tray();
    message(&format!(
        "chord stopped itself: it had to act {STORM_MAX} times in {} seconds while {exe} was in front, \
         faster than anyone types. Another program is sending chord's keys back to it.\n\n\
         Start chord again from the Start menu. If this keeps happening, quit the other program or tell \
         the chord project which one it was.",
        STORM_WINDOW_MS / 1000
    ));
    std::process::exit(1);
}

/// Toggles Caps Lock twice: the state ends where it was, and Windows rewrites the light each time.
fn resync_caps_light() {
    send_combo(&CAPS);
    // Back to back, the keyboard can drop the second light command while the first is still in
    // flight, leaving the light on with Caps off.
    std::thread::sleep(Duration::from_millis(60));
    send_combo(&CAPS);
}

/// Presses `keys` in order, then releases them in reverse, as one SendInput call.
fn send_combo(keys: &[Key]) {
    let flags = |k: &Key| if k.extended { KEYEVENTF_EXTENDEDKEY } else { KEYBD_EVENT_FLAGS(0) };
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|k| key_input(k.vk, 0, flags(k)))
        .chain(keys.iter().rev().map(|k| key_input(k.vk, 0, flags(k) | KEYEVENTF_KEYUP)))
        .collect();
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// Types text as Unicode characters, so the keyboard layout and IME can't change it.
fn type_text(text: &str) {
    let inputs: Vec<INPUT> = text
        .encode_utf16()
        .flat_map(|unit| {
            [
                key_input(0, unit, KEYEVENTF_UNICODE),
                key_input(0, unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP),
            ]
        })
        .collect();
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// Lowercase process file name (e.g. "antigravity ide.exe") and window title of the foreground window.
fn foreground() -> (String, String) {
    unsafe {
        let hwnd = GetForegroundWindow();
        let mut buf = [0u16; 512];
        let len = GetWindowTextW(hwnd, &mut buf).max(0) as usize;
        let title = String::from_utf16_lossy(&buf[..len]);
        (exe_of(hwnd).unwrap_or_default(), title)
    }
}

fn exe_of(hwnd: HWND) -> Option<String> {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let result =
            QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        result.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        Some(path.rsplit('\\').next()?.to_lowercase())
    }
}
