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
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::mpsc::{SyncSender, sync_channel};
use std::time::Duration;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT,
    WPARAM,
};
use windows::Win32::System::Console::FreeConsole;
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
    CallNextHookEx, GetForegroundWindow, GetWindowRect, SetCursorPos, GetMessageW, GetWindowTextW, GetWindowThreadProcessId,
    HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, PostThreadMessageW, SetTimer, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_APP, WM_KEYDOWN, WM_SYSKEYDOWN, WM_TIMER,
};
use windows::core::{PWSTR, w};

use crate::config::{self, Action, Mouse};
use crate::{hints, uia};
use crate::panel;
use crate::window;
use crate::keys::{Key, parse_shortcut};
use crate::machine::{Effect, Machine, VK_CAPITAL, VK_SPACE};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CAPS: [Key; 1] = [Key { vk: VK_CAPITAL, extended: false }];

/// Marks our own SendInput events so the hook skips exactly those (and not other tools' injected input).
const TAG: usize = 0x4348_4F52;

static EFFECTS: OnceLock<SyncSender<Effect>> = OnceLock::new();
static HOOK: AtomicIsize = AtomicIsize::new(0);
static HOOK_THREAD: AtomicU32 = AtomicU32::new(0);
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

/// Closes the console window when started with `--background` (e.g. from Start with Windows).
pub fn detach_console() {
    unsafe {
        let _ = FreeConsole();
    }
}

/// One chord-mode session: the window it started in and the chords usable there, read once.
struct Session {
    hwnd: HWND,
    app: String,
    title: String,
    files: Vec<config::ChordFile>,
    chords: Vec<(String, String)>,
}

impl Session {
    fn start(dir: &Path) -> Self {
        let hwnd = unsafe { GetForegroundWindow() };
        let (exe, title) = foreground();
        let (files, _) = config::load_dir(dir);
        Self { hwnd, chords: config::available(&files, &exe, &title), files, app: exe, title }
    }

    fn view(&self, buffer: &str) -> panel::View {
        let matching = self.chords.iter().filter(|(chord, _)| match chord.split_once('(') {
            // Pattern chords: compare only the literal part before the first `(`.
            Some((prefix, _)) => prefix.starts_with(buffer) || buffer.starts_with(prefix),
            None => chord.starts_with(buffer),
        });
        panel::View { app: self.app.clone(), typed: buffer.to_string(), rows: matching.cloned().collect() }
    }

    /// The click kind when `buffer` is exactly a `hints` chord here.
    fn hints_kind(&self, buffer: &str) -> Option<Mouse> {
        match config::resolve(&self.files, &self.app, &self.title, buffer)?.chord.action {
            Action::Hints(kind) => Some(kind),
            _ => None,
        }
    }
}

/// Hint labels on screen: the letters typed after `key` pick a target.
struct Hints {
    key: String,
    kind: Mouse,
    area: RECT,
    targets: Vec<(hints::Rect, String)>,
}

impl Hints {
    fn scan(hwnd: HWND, key: &str, kind: Mouse) -> Option<Self> {
        let rects = match uia::clickable_rects(hwnd) {
            Ok(rects) => hints::targets(rects),
            Err(e) => {
                say!("  ! could not read this window's buttons: {e}");
                return None;
            }
        };
        let mut area = RECT::default();
        unsafe {
            let _ = GetWindowRect(hwnd, &mut area);
        }
        let labels = hints::labels(rects.len());
        let targets = rects.into_iter().zip(labels).collect::<Vec<_>>();
        Some(Self { key: key.to_string(), kind, area, targets })
    }

    fn show(&self, typed: &str) {
        // Labels sit on each element's top-left corner, so they don't hide its text.
        let visible = self
            .targets
            .iter()
            .filter(|(_, label)| label.starts_with(typed))
            .map(|(r, label)| (POINT { x: r.0, y: r.1 }, label.clone()))
            .collect();
        panel::show_hints(self.area, visible, typed);
    }

    fn click(&self, label: &str) {
        match self.targets.iter().find(|(_, l)| l == label) {
            Some((r, _)) => {
                unsafe {
                    let _ = SetCursorPos((r.0 + r.2) / 2, (r.1 + r.3) / 2);
                }
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
    for effect in rx {
        match effect {
            Effect::Enter => {
                buffer.clear();
                end_hints(&mut hints);
                let s = Session::start(&chords_dir);
                panel::show(s.view(""));
                session = Some(s);
                show("");
            }
            Effect::Show(chord) => {
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
                let kind = s.hints_kind(&buffer);
                if kind.is_some() {
                    panel::hide_now();
                }
                match kind.and_then(|kind| Hints::scan(s.hwnd, &buffer, kind)) {
                    Some(h) => {
                        h.show("");
                        hints = Some(h);
                    }
                    None => panel::show(s.view(&buffer)),
                }
            }
            Effect::FireKeep(chord) => {
                // A Caps tap ran (and cleared) the typed letters; Shift + letter keeps them.
                if chord.as_str() == buffer {
                    buffer.clear();
                }
                if !click_hint(&mut hints, chord.as_str()) {
                    fire(&chords_dir, chord.as_str());
                }
                if let Some(s) = &session {
                    panel::show(s.view(&buffer));
                }
                show(&buffer);
            }
            Effect::Cancel => {
                panel::hide();
                end_hints(&mut hints);
                say!("  (cancelled)");
            }
            Effect::Fire(chord) => {
                panel::hide();
                if !click_hint(&mut hints, chord.as_str()) {
                    fire(&chords_dir, chord.as_str());
                }
            }
            Effect::ToggleCaps => send_combo(&CAPS),
            // Toggling twice leaves the state unchanged, and Windows rewrites the light to match it each time.
            Effect::ResyncCapsLight => {
                send_combo(&CAPS);
                send_combo(&CAPS);
            }
            Effect::Quit => {
                say!("\nemergency exit");
                panel::remove_tray();
                std::process::exit(0);
            }
        }
    }
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
        Action::Mouse(action) => window::mouse(*action),
        // Hint chords are handled while typing (see `run`); reaching here means hints found nothing to show.
        Action::Hints(_) => say!("  -> no clickable elements found"),
    }
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
    if code == HC_ACTION as i32 {
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if event.dwExtraInfo != TAG {
            let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
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
