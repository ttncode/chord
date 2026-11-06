//! Window focus and mouse actions.

use std::time::Duration;

use std::sync::atomic::{AtomicI32, Ordering};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, WPARAM};
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
    SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetForegroundWindow, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetCursorPos, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow, FindWindowExW, GetClassNameW, PostMessageW,
    SB_LINELEFT, SB_LINERIGHT, WM_HSCROLL,
};
use windows::core::w;

use crate::config::Mouse;

const WHEEL_STEP: i32 = 120;
const XBUTTON1: i32 = 1;
const XBUTTON2: i32 = 2;

/// Brings `exe`'s window to the front, or its next window when one is already in front.
/// Returns false when the app has no window.
pub fn focus_app(exe: &str, exe_of: impl Fn(HWND) -> Option<String>) -> bool {
    let windows: Vec<HWND> = app_windows().into_iter().filter(|&h| exe_of(h).as_deref() == Some(exe)).collect();
    let Some(&first) = windows.first() else { return false };
    let current = unsafe { GetForegroundWindow() };
    // Front to back: the bottom-most one has waited longest, so taking it each time visits every window.
    let target = if windows.contains(&current) { *windows.last().unwrap_or(&first) } else { first };
    bring_to_front(target);
    true
}

pub fn pid_of(hwnd: HWND) -> u32 {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

/// Top-level windows a user would switch to with Alt+Tab, front to back.
fn app_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
        let list = unsafe { &mut *(list.0 as *mut Vec<HWND>) };
        if is_switchable(hwnd) {
            list.push(hwnd);
        }
        true.into()
    }
    let mut list: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize));
    }
    list
}

fn is_switchable(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
            return false;
        }
        if GetWindow(hwnd, GW_OWNER).is_ok_and(|owner| !owner.is_invalid()) {
            return false;
        }
        // UWP and other-desktop windows stay "visible" while cloaked.
        let mut cloaked = 0u32;
        let _ = DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, size_of::<u32>() as u32);
        cloaked == 0
    }
}

/// SetForegroundWindow is refused unless we "own" the last input, so send an empty mouse event first
/// (komorebi and GlazeWM do the same), then verify and retry.
fn bring_to_front(hwnd: HWND) {
    unsafe {
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        for _ in 0..5 {
            send_mouse(&[MOUSEEVENTF_MOVE], 0);
            let _ = SetForegroundWindow(hwnd);
            if GetForegroundWindow() == hwnd {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

pub fn mouse(action: Mouse) {
    match action {
        Mouse::Click => send_mouse(&[MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP], 0),
        Mouse::RightClick => send_mouse(&[MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP], 0),
        Mouse::MiddleClick => send_mouse(&[MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP], 0),
        Mouse::DoubleClick => send_mouse(
            &[MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP],
            0,
        ),
        Mouse::ScrollUp | Mouse::ScrollDown | Mouse::ScrollLeft | Mouse::ScrollRight => {
            scroll(action, 3 * WHEEL_STEP);
        }
        Mouse::Back => send_mouse(&[MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP], XBUTTON1),
        Mouse::Forward => send_mouse(&[MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP], XBUTTON2),
        Mouse::Point => {}
    }
}

/// Scrolls by `amount` (120 = one wheel notch) in `action`'s direction; false if `action` isn't a scroll.
pub fn scroll(action: Mouse, amount: i32) -> bool {
    // Horizontal wheel: positive scrolls right.
    let (flag, sign) = match action {
        Mouse::ScrollUp => (MOUSEEVENTF_WHEEL, 1),
        Mouse::ScrollDown => (MOUSEEVENTF_WHEEL, -1),
        Mouse::ScrollLeft => (MOUSEEVENTF_HWHEEL, -1),
        Mouse::ScrollRight => (MOUSEEVENTF_HWHEEL, 1),
        _ => return false,
    };
    if flag == MOUSEEVENTF_HWHEEL {
        if let Some(grid) = excel_grid() {
            // A held key sends parts of a notch; Excel scrolls whole columns, so carry the rest.
            static REST: AtomicI32 = AtomicI32::new(0);
            let total = REST.load(Ordering::Relaxed) + amount;
            REST.store(total % WHEEL_STEP, Ordering::Relaxed);
            let line = if sign > 0 { SB_LINERIGHT } else { SB_LINELEFT };
            for _ in 0..total / WHEEL_STEP {
                unsafe {
                    let _ = PostMessageW(grid, WM_HSCROLL, WPARAM(line.0 as usize), LPARAM(0));
                }
            }
            return true;
        }
    }
    send_mouse(&[flag], sign * amount);
    true
}

/// The sheet grid of the Excel window in front. Excel ignores the horizontal wheel, but its grid
/// scrolls a column per WM_HSCROLL line, as its scroll bar sends.
fn excel_grid() -> Option<HWND> {
    unsafe {
        let main = GetForegroundWindow();
        let mut class = [0u16; 16];
        let len = GetClassNameW(main, &mut class) as usize;
        if String::from_utf16_lossy(&class[..len]) != "XLMAIN" {
            return None;
        }
        let desk = FindWindowExW(main, None, w!("XLDESK"), None).ok()?;
        FindWindowExW(desk, None, w!("EXCEL7"), None).ok()
    }
}

/// Puts the pointer at screen point (`x`, `y`). SetCursorPos alone isn't mouse input: a pointer hidden
/// while typing stays hidden, and hover menus don't notice it. So a real move follows; SetCursorPos
/// stays because SendInput lands later, and callers read the position back at once.
pub fn move_pointer(x: i32, y: i32) {
    unsafe {
        let _ = SetCursorPos(x, y);
    }
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(2),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(2),
        )
    };
    // Absolute moves count 0..=65535 across the whole virtual desktop.
    let input = |x: i32, y: i32| INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: (i64::from(x - left) * 65535 / i64::from(width - 1)) as i32,
                dy: (i64::from(y - top) * 65535 / i64::from(height - 1)) as i32,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    // A move onto the spot SetCursorPos already set is no movement, and Windows ignores it: step 1 px
    // aside first so the pointer is shown again.
    let aside = if x > left { x - 1 } else { x + 1 };
    unsafe { SendInput(&[input(aside, y), input(x, y)], size_of::<INPUT>() as i32) };
}

/// One SendInput call with these mouse events at the current pointer position.
pub fn send_mouse(events: &[MOUSE_EVENT_FLAGS], wheel: i32) {
    let inputs: Vec<INPUT> = events
        .iter()
        .map(|&flags| INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT { dx: 0, dy: 0, mouseData: wheel as u32, dwFlags: flags, time: 0, dwExtraInfo: 0 },
            },
        })
        .collect();
    unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
}
