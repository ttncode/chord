//! The chord panel (an always-on-top window that never takes focus) and the tray icon.
//! Both live on one GUI thread; other threads talk to it only through `show`/`hide`/`remove_tray`.

use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DrawTextW,
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
    WM_PAINT, WM_RBUTTONUP, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{PCWSTR, w};

use crate::autostart;

const WM_PANEL: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
const ID_OPEN: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_QUIT: usize = 3;

const LINE_HEIGHT: i32 = 22;
const WIDTH: i32 = 460;
const PADDING: i32 = 12;

static PANEL: AtomicIsize = AtomicIsize::new(0);
static LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CHORDS_DIR: OnceLock<PathBuf> = OnceLock::new();

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
        add_tray(hwnd);

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

pub fn hide() {
    if let Some(hwnd) = panel() {
        unsafe {
            let _ = PostMessageW(hwnd, WM_PANEL, WPARAM(0), LPARAM(0));
        }
    }
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
    match msg {
        WM_PANEL => {
            if wparam.0 == 1 { place_and_show(hwnd) } else { unsafe { let _ = ShowWindow(hwnd, SW_HIDE); } }
            LRESULT(0)
        }
        WM_PAINT => {
            paint(hwnd);
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
        let max_height = (work.bottom - work.top - 2 * PADDING).max(LINE_HEIGHT);
        let height = (count * LINE_HEIGHT + 2 * PADDING).min(max_height);
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            work.left + PADDING,
            work.top + PADDING,
            WIDTH,
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
        let font: HFONT = CreateFontW(18, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0, w!("Consolas"));
        SelectObject(hdc, font);
        SetBkMode(hdc, TRANSPARENT);
        for (i, line) in lines.iter().enumerate() {
            // Header in accent colour, chord lines in light grey.
            SetTextColor(hdc, COLORREF(if i == 0 { 0x0066_CCFF } else { 0x00E6_E6E6 }));
            let mut rect = RECT {
                left: PADDING,
                top: PADDING + i as i32 * LINE_HEIGHT,
                right: WIDTH - PADDING,
                bottom: PADDING + (i as i32 + 1) * LINE_HEIGHT,
            };
            let mut text: Vec<u16> = line.encode_utf16().collect();
            DrawTextW(hdc, &mut text, &mut rect, DT_LEFT | DT_SINGLELINE | DT_NOPREFIX);
        }
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(font);
        let _ = windows::Win32::Graphics::Gdi::DeleteObject(background);
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
