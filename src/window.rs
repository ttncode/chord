//! Window focus, window layouts and mouse actions.

use std::time::Duration;

use std::sync::Mutex;
use std::sync::atomic::{AtomicI32, Ordering};

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_HWHEEL, MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
    SendInput,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GW_OWNER, GetForegroundWindow, GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN,
    SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, SetCursorPos, GetWindow, GetWindowTextLengthW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, SW_RESTORE, SetForegroundWindow, ShowWindow, GetClassNameW, GetWindowRect, IsZoomed, SW_MAXIMIZE,
    SWP_NOACTIVATE, SWP_NOZORDER, SetWindowPos,
};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::{
    CLSIDFromProgID, COINIT_MULTITHREADED, CoInitializeEx, DISPATCH_FLAGS, DISPATCH_METHOD, DISPATCH_PROPERTYGET, DISPPARAMS, IDispatch,
};
use windows::Win32::System::Ole::GetActiveObject;
use windows::core::{GUID, Interface, PCWSTR, VARIANT, w};

use crate::config::Mouse;
use crate::layout::{self, Layout};

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
    if flag == MOUSEEVENTF_HWHEEL && foreground_class() == "XLMAIN" {
        // A held key sends parts of a notch; Excel scrolls whole columns, so carry the rest.
        static REST: AtomicI32 = AtomicI32::new(0);
        let total = REST.load(Ordering::Relaxed) + amount;
        REST.store(total % WHEEL_STEP, Ordering::Relaxed);
        if total >= WHEEL_STEP {
            excel_scroll(sign * total / WHEEL_STEP);
        }
        return true;
    }
    send_mouse(&[flag], sign * amount);
    true
}

fn foreground_class() -> String {
    let mut class = [0u16; 32];
    let len = unsafe { GetClassNameW(GetForegroundWindow(), &mut class) } as usize;
    String::from_utf16_lossy(&class[..len])
}

/// Scrolls the active Excel window `columns` columns right (negative: left) with its own SmallScroll:
/// Excel ignores the horizontal wheel, and its grid ignores scroll bar messages too.
fn excel_scroll(columns: i32) {
    let scroll = || -> windows::core::Result<()> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            let mut app = None;
            GetActiveObject(&CLSIDFromProgID(w!("Excel.Application"))?, None, &mut app)?;
            let app: IDispatch = app.ok_or(windows::core::Error::from(E_FAIL))?.cast()?;
            let active = dispatch(&app, w!("ActiveWindow"), DISPATCH_PROPERTYGET, &mut [])?;
            // windows-rs converts only VT_UNKNOWN; Excel hands back VT_DISPATCH.
            const VT_DISPATCH: u16 = 9;
            let raw = &active.as_raw().Anonymous.Anonymous;
            if raw.vt != VT_DISPATCH {
                return Err(windows::core::Error::from(E_FAIL));
            }
            let active = IDispatch::from_raw_borrowed(&raw.Anonymous.pdispVal).ok_or(windows::core::Error::from(E_FAIL))?.clone();
            // SmallScroll(Down, Up, ToRight, ToLeft); COM takes arguments last first.
            let mut args = [VARIANT::from((-columns).max(0)), VARIANT::from(columns.max(0)), VARIANT::from(0), VARIANT::from(0)];
            dispatch(&active, w!("SmallScroll"), DISPATCH_METHOD, &mut args)?;
            Ok(())
        }
    };
    if let Err(e) = scroll() {
        say!("  ! could not scroll Excel: {e}");
    }
}

/// Calls `name` on a COM automation object (property get or method) with positional `args`.
unsafe fn dispatch(target: &IDispatch, name: PCWSTR, flags: DISPATCH_FLAGS, args: &mut [VARIANT]) -> windows::core::Result<VARIANT> {
    const LOCALE_USER_DEFAULT: u32 = 0x0400;
    let mut id = 0;
    unsafe {
        target.GetIDsOfNames(&GUID::zeroed(), &name, 1, LOCALE_USER_DEFAULT, &mut id)?;
        let params = DISPPARAMS { rgvarg: args.as_mut_ptr(), cArgs: args.len() as u32, ..Default::default() };
        let mut result = VARIANT::default();
        target.Invoke(id, &GUID::zeroed(), LOCALE_USER_DEFAULT, flags, &params, Some(&mut result), None, None)?;
        Ok(result)
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

/// (window, layout, frame index, where the window ended up) of the last `snap`, so running the same
/// layout again on that window steps to its next frame.
static LAST_SNAP: Mutex<Option<(isize, Layout, usize, layout::Rect)>> = Mutex::new(None);

/// Moves the window in front to `layout` on the monitor it is on.
pub fn snap(layout: Layout) {
    let hwnd = unsafe { GetForegroundWindow() };
    // The desktop and the taskbar are windows too; moving them breaks the shell until sign-out.
    if hwnd.is_invalid() || matches!(foreground_class().as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd") {
        return;
    }
    unsafe {
        if layout == Layout::Maximize {
            let _ = ShowWindow(hwnd, if IsZoomed(hwnd).as_bool() { SW_RESTORE } else { SW_MAXIMIZE });
            return;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let w = info.rcWork;
        let work = layout::Rect { left: w.left, top: w.top, right: w.right, bottom: w.bottom };
        let mut last = LAST_SNAP.lock().unwrap_or_else(|e| e.into_inner());
        let previous = last.filter(|(h, ..)| *h == hwnd.0 as isize).map(|(_, l, i, at)| (l, i, at));
        let index = layout::next_frame(layout, previous, outer_rect(hwnd));
        let target = layout::place(layout.frames()[index], work, GetDpiForWindow(hwnd).max(96) as i32);
        move_visible(hwnd, target);
        *last = Some((hwnd.0 as isize, layout, index, outer_rect(hwnd)));
    }
}

fn outer_rect(hwnd: HWND) -> layout::Rect {
    let mut r = RECT::default();
    let _ = unsafe { GetWindowRect(hwnd, &mut r) };
    layout::Rect { left: r.left, top: r.top, right: r.right, bottom: r.bottom }
}

/// Windows 10/11 windows carry invisible resize borders (~7px left, right, bottom): placing the outer
/// rectangle leaves gaps between snapped windows, so this places the visible frame on `to` instead.
unsafe fn move_visible(hwnd: HWND, to: layout::Rect) {
    let flags = SWP_NOZORDER | SWP_NOACTIVATE;
    unsafe {
        if IsZoomed(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetWindowPos(hwnd, HWND::default(), to.left, to.top, to.right - to.left, to.bottom - to.top, flags);
        let mut visible = RECT::default();
        if DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut visible as *mut _ as *mut _, size_of::<RECT>() as u32).is_err() {
            return;
        }
        let outer = outer_rect(hwnd);
        let (l, t) = (visible.left - outer.left, visible.top - outer.top);
        let (r, b) = (outer.right - visible.right, outer.bottom - visible.bottom);
        if l != 0 || t != 0 || r != 0 || b != 0 {
            let (x, y) = (to.left - l, to.top - t);
            let _ = SetWindowPos(hwnd, HWND::default(), x, y, to.right + r - x, to.bottom + b - y, flags);
        }
    }
}
