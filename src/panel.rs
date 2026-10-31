//! The chord panel (an always-on-top window that never takes focus) and the tray icon.
//! Both live on one GUI thread; other threads talk to it only through `show`/`hide`/`remove_tray`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DT_CENTER, DT_LEFT, DT_VCENTER, DT_NOPREFIX, DT_SINGLELINE, DrawTextW,
    EndPaint, FillRect, GetMonitorInfoW, HFONT, InvalidateRect, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromWindow, PAINTSTRUCT, SelectObject, SetBkMode, SetTextColor,
    TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetForegroundWindow, GetMessageW, HMENU, HWND_TOPMOST, IDI_APPLICATION,
    LoadIconW, MF_CHECKED, MF_SEPARATOR, MF_STRING, MSG, PostMessageW, RegisterClassW,
    SW_HIDE, SWP_NOACTIVATE, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowPos, ShowWindow,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_LBUTTONUP,
    PBT_APMRESUMEAUTOMATIC, WM_PAINT, WM_POWERBROADCAST, WM_RBUTTONUP, WM_WTSSESSION_CHANGE,
    WTS_SESSION_UNLOCK, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    LWA_COLORKEY, RegisterWindowMessageW, SendMessageW, SetLayeredWindowAttributes, WS_EX_LAYERED,
    WS_EX_TRANSPARENT,
};
use windows::core::{PCWSTR, w};

use windows::Win32::System::RemoteDesktop::{NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification};

use crate::autostart;

const WM_PANEL: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
const WM_HINTS: u32 = WM_APP + 3;
/// Overlay pixels of exactly this colour are see-through (and click-through).
const TRANSPARENT_KEY: COLORREF = COLORREF(0x00FF_00FF);
const ID_OPEN: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_QUIT: usize = 3;

const LINE_HEIGHT: i32 = 22;
const WIDTH: i32 = 460;
const PADDING: i32 = 12;

static PANEL: AtomicIsize = AtomicIsize::new(0);
static OVERLAY: AtomicIsize = AtomicIsize::new(0);
static HINTS: Mutex<(RECT, Vec<(POINT, String)>)> = Mutex::new((RECT { left: 0, top: 0, right: 0, bottom: 0 }, Vec::new()));
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CHORDS_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Explorer broadcasts this after it restarts; tray icons must be added again.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

fn panel() -> Option<HWND> {
    match PANEL.load(Ordering::Acquire) {
        0 => None,
        h => Some(HWND(h as *mut _)),
    }
}

/// Starts the GUI thread. `chords_dir` is what the tray's "Open chords folder" opens.
pub fn start(chords_dir: PathBuf) {
    let _ = CHORDS_DIR.set(chords_dir);
    std::thread::spawn(|| unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let class = w!("chord-panel");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            class,
            w!("chord"),
            WS_POPUP,
            0,
            0,
            WIDTH,
            LINE_HEIGHT,
            HWND::default(),
            HMENU::default(),
            module,
            None,
        ) else {
            return;
        };
        PANEL.store(hwnd.0 as isize, Ordering::Release);
        let overlay_class = w!("chord-hints");
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(overlay_proc),
            hInstance: module.into(),
            lpszClassName: overlay_class,
            ..Default::default()
        });
        if let Ok(overlay) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
            overlay_class,
            w!("chord hints"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            HWND::default(),
            HMENU::default(),
            module,
            None,
        ) {
            let _ = SetLayeredWindowAttributes(overlay, TRANSPARENT_KEY, 0, LWA_COLORKEY);
            OVERLAY.store(overlay.0 as isize, Ordering::Release);
        }
        add_tray(hwnd);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Release);
        // Unlock events arrive as WM_WTSSESSION_CHANGE; resume as WM_POWERBROADCAST (top-level windows get it).
        let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

/// Shows the panel with these lines (first line is the header). Safe from any thread.
pub fn show(lines: Vec<String>) {
    *LINES.lock().unwrap_or_else(|e| e.into_inner()) = lines;
    if let Some(hwnd) = panel() {
        unsafe {
            let _ = PostMessageW(hwnd, WM_PANEL, WPARAM(1), LPARAM(0));
        }
    }
}

/// Hides the panel and waits until it's gone (before a hint scan, so it doesn't cover targets).
pub fn hide_now() {
    if let Some(hwnd) = panel() {
        unsafe {
            SendMessageW(hwnd, WM_PANEL, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn hide() {
    if let Some(hwnd) = panel() {
        unsafe {
            let _ = PostMessageW(hwnd, WM_PANEL, WPARAM(0), LPARAM(0));
        }
    }
}

/// Shows hint labels at these screen points over `area` (the target window). Safe from any thread.
pub fn show_hints(area: RECT, labels: Vec<(POINT, String)>) {
    *HINTS.lock().unwrap_or_else(|e| e.into_inner()) = (area, labels);
    post(WM_HINTS, 1);
}

pub fn hide_hints() {
    post(WM_HINTS, 0);
}

fn post(msg: u32, wparam: usize) {
    if let Some(hwnd) = panel() {
        unsafe {
            let _ = PostMessageW(hwnd, msg, WPARAM(wparam), LPARAM(0));
        }
    }
}

fn overlay() -> Option<HWND> {
    match OVERLAY.load(Ordering::Acquire) {
        0 => None,
        h => Some(HWND(h as *mut _)),
    }
}

/// Scales a 96-DPI size to the window's monitor.
fn px(hwnd: HWND, value: i32) -> i32 {
    value * unsafe { GetDpiForWindow(hwnd) }.max(96) as i32 / 96
}

/// Removes the tray icon; call before exiting so no dead icon stays behind.
pub fn remove_tray() {
    if let Some(hwnd) = panel() {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
        }
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
        ..Default::default()
    };
    for (dst, src) in data.szTip.iter_mut().zip("chord".encode_utf16()) {
        *dst = src;
    }
    data
}

fn add_tray(hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &tray_data(hwnd));
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Acquire) {
        add_tray(hwnd);
        return LRESULT(0);
    }
    match msg {
        WM_PANEL => {
            if wparam.0 == 1 { place_and_show(hwnd) } else { unsafe { let _ = ShowWindow(hwnd, SW_HIDE); } }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE if wparam.0 as u32 == WTS_SESSION_UNLOCK => {
            crate::win::reset_hook();
            LRESULT(0)
        }
        WM_POWERBROADCAST if wparam.0 as u32 == PBT_APMRESUMEAUTOMATIC => {
            crate::win::reset_hook();
            LRESULT(1)
        }
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_HINTS => {
            if let Some(overlay) = overlay() {
                if wparam.0 == 1 {
                    let area = HINTS.lock().unwrap_or_else(|e| e.into_inner()).0;
                    unsafe {
                        let _ = SetWindowPos(
                            overlay,
                            HWND_TOPMOST,
                            area.left,
                            area.top,
                            area.right - area.left,
                            area.bottom - area.top,
                            SWP_NOACTIVATE | SWP_SHOWWINDOW,
                        );
                        let _ = InvalidateRect(overlay, None, true);
                    }
                } else {
                    unsafe {
                        let _ = ShowWindow(overlay, SW_HIDE);
                    }
                }
            }
            LRESULT(0)
        }
        WM_TRAY => {
            let event = (lparam.0 & 0xFFFF) as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                tray_menu(hwnd);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Left side of the monitor the user is working on, tall enough for the lines.
fn place_and_show(hwnd: HWND) {
    let count = LINES.lock().unwrap_or_else(|e| e.into_inner()).len() as i32;
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(monitor, &mut info);
        let work = info.rcWork;
        let (line, pad) = (px(hwnd, LINE_HEIGHT), px(hwnd, PADDING));
        let max_height = (work.bottom - work.top - 2 * pad).max(line);
        let height = (count * line + 2 * pad).min(max_height);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            work.left + pad,
            work.top + pad,
            px(hwnd, WIDTH),
            height,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        let _ = InvalidateRect(hwnd, None, true);
    }
}

fn paint(hwnd: HWND) {
    let lines = LINES.lock().unwrap_or_else(|e| e.into_inner()).clone();
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let background = CreateSolidBrush(COLORREF(0x0030_2A28));
        FillRect(hdc, &ps.rcPaint, background);
        let (line, pad, width) = (px(hwnd, LINE_HEIGHT), px(hwnd, PADDING), px(hwnd, WIDTH));
        let font: HFONT = CreateFontW(px(hwnd, 18), 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0, w!("Consolas"));
        let previous_font = SelectObject(hdc, font);
        SetBkMode(hdc, TRANSPARENT);
        for (i, text_line) in lines.iter().enumerate() {
            // Header in accent colour, chord lines in light grey.
            SetTextColor(hdc, COLORREF(if i == 0 { 0x0066_CCFF } else { 0x00E6_E6E6 }));
            let mut rect = RECT {
                left: pad,
                top: pad + i as i32 * line,
                right: width - pad,
                bottom: pad + (i as i32 + 1) * line,
            };
            let mut text: Vec<u16> = text_line.encode_utf16().collect();
            DrawTextW(hdc, &mut text, &mut rect, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);
        }
        // A font still selected into the DC can't be deleted; put the old one back first.
        SelectObject(hdc, previous_font);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(font);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(background);
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        paint_hints(hwnd);
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Yellow labels with black text, centred on each target; everything else is see-through.
fn paint_hints(hwnd: HWND) {
    let (area, labels) = HINTS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let clear = CreateSolidBrush(TRANSPARENT_KEY);
        FillRect(hdc, &ps.rcPaint, clear);
        let label_brush = CreateSolidBrush(COLORREF(0x0000_D7FF));
        let font: HFONT = CreateFontW(px(hwnd, 15), 0, 0, 0, 700, 0, 0, 0, 0, 0, 0, 0, 0, w!("Consolas"));
        let previous_font = SelectObject(hdc, font);
        SetBkMode(hdc, TRANSPARENT);
        SetTextColor(hdc, COLORREF(0x0000_0000));
        let (char_w, h) = (px(hwnd, 9), px(hwnd, 18));
        for (point, label) in &labels {
            let w = char_w * label.len() as i32 + px(hwnd, 6);
            let (x, y) = (point.x - area.left - w / 2, point.y - area.top - h / 2);
            let mut rect = RECT { left: x, top: y, right: x + w, bottom: y + h };
            FillRect(hdc, &rect, label_brush);
            let mut text: Vec<u16> = label.to_uppercase().encode_utf16().collect();
            DrawTextW(hdc, &mut text, &mut rect, DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX);
        }
        // A font still selected into the DC can't be deleted; put the old one back first.
        SelectObject(hdc, previous_font);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(font);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(label_brush);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(clear);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let autostart_flags = if autostart::enabled() { MF_STRING | MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(menu, MF_STRING, ID_OPEN, w!("Open chords folder"));
        let _ = AppendMenuW(menu, autostart_flags, ID_AUTOSTART, w!("Start with Windows"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_QUIT, w!("Quit"));
        let mut point = Default::default();
        let _ = GetCursorPos(&mut point);
        // Without this the menu doesn't close when clicking elsewhere (documented TrackPopupMenu quirk).
        let _ = SetForegroundWindow(hwnd);
        let choice = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON, point.x, point.y, 0, hwnd, None);
        let _ = DestroyMenu(menu);
        match choice.0 as usize {
            ID_OPEN => {
                if let Some(dir) = CHORDS_DIR.get() {
                    let _ = std::process::Command::new("explorer").arg(dir).spawn();
                }
            }
            ID_AUTOSTART => {
                if let Some(dir) = CHORDS_DIR.get() {
                    if let Err(e) = autostart::set(!autostart::enabled(), dir) {
                        say!("  ! could not change Start with Windows: {e}");
                    }
                }
            }
            ID_QUIT => {
                remove_tray();
                std::process::exit(0);
            }
            _ => {}
        }
    }
}
