//! Windows glue: low-level keyboard hook, SendInput, foreground app lookup.
//!
//! Threads: the hook thread only runs `Machine` and hands effects over with `try_send`;
//! it never blocks, because Windows silently drops a hook that takes longer than ~1 s.
//! The main thread does everything slow (printing, SendInput, process lookup).

use std::cell::RefCell;
use std::io::Write;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;
use std::sync::mpsc::{SyncSender, sync_channel};

use windows::Win32::Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentThread, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW, SetThreadPriority, THREAD_PRIORITY_TIME_CRITICAL,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, HC_ACTION, HHOOK,
    KBDLLHOOKSTRUCT, MSG, SetWindowsHookExW, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
};
use windows::core::PWSTR;

use crate::chords;
use crate::machine::{Effect, Machine, VK_CAPITAL};

/// Marks our own SendInput events so the hook skips exactly those (and not other tools' injected input).
const TAG: usize = 0x4348_4F52;

static EFFECTS: OnceLock<SyncSender<Effect>> = OnceLock::new();

thread_local! {
    static MACHINE: RefCell<Machine> = const { RefCell::new(Machine::new()) };
}

pub fn run() {
    let (tx, rx) = sync_channel(64);
    EFFECTS.set(tx).expect("run() called twice");
    std::thread::spawn(hook_thread);

    println!("chord phase 0 running.");
    println!("  Hold Caps Lock, then hold Space, type a chord, release Space.");
    println!("  Emergency exit: Ctrl + Space + Esc (or Ctrl+C here).");

    for effect in rx {
        match effect {
            Effect::Enter => show(""),
            Effect::Show(chord) => show(chord.as_str()),
            Effect::Cancel => println!("  (cancelled)"),
            Effect::Fire(chord) => fire(chord.as_str()),
            Effect::ToggleCaps => send(&[VK_CAPITAL]),
            // Toggling twice leaves the state unchanged, and Windows rewrites the light to match it each time.
            Effect::ResyncCapsLight => {
                send(&[VK_CAPITAL]);
                send(&[VK_CAPITAL]);
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

fn fire(chord: &str) {
    let exe = foreground_exe().unwrap_or_default();
    match chords::lookup(&exe, chord) {
        Some(keys) => {
            send(keys);
            println!("  -> fired in {exe}");
        }
        None => println!("  -> no chord '{chord}' for {exe}"),
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

/// Presses `keys` in order, then releases them in reverse, as one SendInput call.
fn send(keys: &[u16]) {
    let input = |vk: u16, flags: KEYBD_EVENT_FLAGS| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: TAG,
            },
        },
    };
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|&vk| input(vk, KEYBD_EVENT_FLAGS(0)))
        .chain(keys.iter().rev().map(|&vk| input(vk, KEYEVENTF_KEYUP)))
        .collect();
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}

/// Lowercase file name of the foreground process, e.g. "code.exe".
fn foreground_exe() -> Option<String> {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
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
