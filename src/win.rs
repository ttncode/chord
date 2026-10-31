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
use std::sync::mpsc::{SyncSender, sync_channel};
use std::time::Duration;

use windows::Win32::Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowTextW, GetWindowThreadProcessId,
    HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW, WH_KEYBOARD_LL, WM_KEYDOWN,
    WM_SYSKEYDOWN,
};
use windows::core::PWSTR;

use crate::config::{self, Action};
use crate::keys::{Key, parse_shortcut};
use crate::machine::{Effect, Machine, VK_CAPITAL};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CAPS: [Key; 1] = [Key { vk: VK_CAPITAL, extended: false }];

/// Marks our own SendInput events so the hook skips exactly those (and not other tools' injected input).
const TAG: usize = 0x4348_4F52;

static EFFECTS: OnceLock<SyncSender<Effect>> = OnceLock::new();

thread_local! {
    static MACHINE: RefCell<Machine> = const { RefCell::new(Machine::new()) };
}

pub fn run(chords_dir: PathBuf) {
    let (tx, rx) = sync_channel(64);
    EFFECTS.set(tx).expect("run() called twice");
    std::thread::spawn(hook_thread);

    println!("chord running. Chord files: {}", chords_dir.display());
    let (files, errors) = config::load_dir(&chords_dir);
    for f in &files {
        println!("  loaded {} ({} chords)", f.file, f.chords.len());
    }
    print_errors(&errors);
    println!("  Hold Caps Lock, then hold Space, type a chord, release Space.");
    println!("  Emergency exit: Ctrl + Space + Esc (or Ctrl+C here).");

    for effect in rx {
        match effect {
            Effect::Enter => show(""),
            Effect::Show(chord) => show(chord.as_str()),
            Effect::Cancel => println!("  (cancelled)"),
            Effect::Fire(chord) => fire(&chords_dir, chord.as_str()),
            Effect::ToggleCaps => send_combo(&CAPS),
            // Toggling twice leaves the state unchanged, and Windows rewrites the light to match it each time.
            Effect::ResyncCapsLight => {
                send_combo(&CAPS);
                send_combo(&CAPS);
            }
            Effect::Quit => {
                println!("\nemergency exit");
                std::process::exit(0);
            }
        }
    }
}

fn show(buffer: &str) {
    print!("\r\x1b[2K[chord] {buffer}");
    let _ = std::io::stdout().flush();
}

fn print_errors(errors: &[String]) {
    for e in errors {
        println!("  ! {e}");
    }
}

// shortcut: files are re-read on every fire; switch to a cached, watched config when the Phase 2 panel needs the list while typing.
fn fire(dir: &Path, chord: &str) {
    let (exe, title) = foreground();
    let (files, errors) = config::load_dir(dir);
    let Some(found) = config::resolve(&files, &exe, &title, chord) else {
        println!("  -> no chord '{chord}' for {exe} \"{title}\"");
        print_errors(&errors);
        return;
    };
    println!("  -> {} ({exe})", found.chord.name);
    print_errors(&errors);
    if !found.shadowed.is_empty() {
        println!("  ! '{chord}' is also in {}; {} wins", found.shadowed.join(", "), found.file.file);
    }
    run_action(&found.chord.action);
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
            std::thread::sleep(Duration::from_millis(150));
            type_text(text);
            std::thread::sleep(Duration::from_millis(50));
            send_combo(&shortcut("enter"));
        }
        Action::Shell(command) => {
            let spawned = std::process::Command::new("cmd")
                .raw_arg(format!("/C {command}"))
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
            if let Err(e) = spawned {
                println!("  ! could not run `{command}`: {e}");
            }
        }
    }
}

fn hook_thread() {
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);
        let module = GetModuleHandleW(None).expect("GetModuleHandleW");
        if let Err(e) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), HINSTANCE(module.0), 0) {
            eprintln!("could not install keyboard hook: {e}");
            std::process::exit(1);
        }
        // The hook only runs while this thread pumps messages.
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {}
    }
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        if event.dwExtraInfo != TAG {
            let down = matches!(wparam.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
            // A panic must not unwind into Windows; on panic the key just passes through.
            let decision = catch_unwind(AssertUnwindSafe(|| {
                MACHINE.with_borrow_mut(|m| m.key(event.vkCode as u16, down))
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
