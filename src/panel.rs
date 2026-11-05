//! The chord panel (a Spotlight-style window that never takes focus), the hint labels and the tray icon.
//! All live on one GUI thread; other threads talk to it only through the `pub fn`s here.
//! Both windows are layered and drawn on a `Canvas`, so they get antialiased corners, shadows and fades.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    DT_CENTER, DT_END_ELLIPSIS, DT_LEFT, DT_RIGHT, DT_VCENTER, DeleteObject, GetMonitorInfoW, HFONT, HMONITOR,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromRect, MonitorFromWindow,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::System::RemoteDesktop::{NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DispatchMessageW,
    GetCursorPos, GetForegroundWindow, GetMessageW, GetSystemMetrics, HICON, HMENU, HWND_TOPMOST, IDI_APPLICATION,
    IsWindowVisible, KillTimer, LoadIconW, MF_CHECKED, MF_SEPARATOR, MF_STRING, MSG, PBT_APMRESUMEAUTOMATIC,
    PostMessageW, RegisterClassW, RegisterWindowMessageW, SM_CXSMICON, SPI_GETCLIENTAREAANIMATION,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW, SW_HIDE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SendMessageW, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, SystemParametersInfoW, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_LBUTTONUP, WM_POWERBROADCAST, WM_RBUTTONUP,
    WM_SETTINGCHANGE, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP, WTS_SESSION_UNLOCK,
};
use windows::core::{PCSTR, PCWSTR, w};

use crate::autostart;
use crate::canvas::{self, Canvas};
use crate::style::{self, Palette, Rgb};

const WM_PANEL: u32 = WM_APP + 1;
const WM_TRAY: u32 = WM_APP + 2;
const WM_HINTS: u32 = WM_APP + 3;
const ID_OPEN: usize = 1;
const ID_AUTOSTART: usize = 2;
const ID_QUIT: usize = 3;
const ID_UPDATE: usize = 4;
const PANEL_HIDE: usize = 0;
const PANEL_SHOW: usize = 1;
const PANEL_HIDE_NOW: usize = 2;
const TIMER: usize = 1;
const FRAME_MS: u32 = 15;

/// What the panel shows: the app, the letters typed so far, and the chords that still match.
#[derive(Clone)]
pub struct View {
    pub app: String,
    pub typed: String,
    pub rows: Vec<(String, String)>,
    /// Chord groups (`"/w"`, `"Window"`) whose prefix starts with what was typed, for the reminder line.
    pub groups: Vec<(String, String)>,
    /// `typed` is search text (Tab), not chord keys.
    pub search: bool,
    /// Row highlighted in search, counted in `rows`.
    pub selected: Option<usize>,
}

static PANEL: AtomicIsize = AtomicIsize::new(0);
static OVERLAY: AtomicIsize = AtomicIsize::new(0);
static TRAY_ICON: AtomicIsize = AtomicIsize::new(0);
static HINTS: Mutex<(RECT, Vec<(POINT, String)>, String)> =
    Mutex::new((RECT { left: 0, top: 0, right: 0, bottom: 0 }, Vec::new(), String::new()));
static VIEW: Mutex<View> = Mutex::new(View { app: String::new(), typed: String::new(), rows: Vec::new(), groups: Vec::new(), search: false, selected: None });
static CHORDS_DIR: OnceLock<PathBuf> = OnceLock::new();
/// Explorer broadcasts this after it restarts; tray icons must be added again.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

thread_local! {
    /// What each layered window currently shows, keyed by its handle; GUI thread only.
    static LAYERS: RefCell<HashMap<isize, Layer>> = RefCell::new(HashMap::new());
}

struct Layer {
    canvas: Canvas,
    at: POINT,
    anim: Anim,
}

#[derive(Clone, Copy)]
enum Anim {
    Still,
    In { start: Instant, duration: u32, drop: i32 },
    Out { start: Instant, duration: u32 },
}

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
        RegisterClassW(&WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: module.into(),
            lpszClassName: class,
            ..Default::default()
        });
        // Click-through as well, so a mouse chord at the pointer never lands on the panel.
        let layered = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE;
        let Ok(hwnd) =
            CreateWindowExW(layered, class, w!("chord"), WS_POPUP, 0, 0, 1, 1, HWND::default(), HMENU::default(), module, None)
        else {
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
            layered,
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

/// Shows the panel with this view. Safe from any thread.
pub fn show(view: View) {
    *VIEW.lock().unwrap_or_else(|e| e.into_inner()) = view;
    post(WM_PANEL, PANEL_SHOW);
}

/// Hides the panel at once and waits until it's gone (before a hint scan, so it doesn't cover targets).
pub fn hide_now() {
    if let Some(hwnd) = panel() {
        unsafe {
            SendMessageW(hwnd, WM_PANEL, WPARAM(PANEL_HIDE_NOW), LPARAM(0));
        }
    }
}

/// Fades the panel out.
pub fn hide() {
    post(WM_PANEL, PANEL_HIDE);
}

/// Shows hint labels with their top-left corner at these screen points over `area` (the target window);
/// `typed` is the part of every label already typed. Safe from any thread.
pub fn show_hints(area: RECT, labels: Vec<(POINT, String)>, typed: &str) {
    *HINTS.lock().unwrap_or_else(|e| e.into_inner()) = (area, labels, typed.to_string());
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

/// Removes the tray icon; call before exiting so no dead icon stays behind.
pub fn remove_tray() {
    if let Some(hwnd) = panel() {
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &tray_data(hwnd));
        }
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let icon = match TRAY_ICON.load(Ordering::Acquire) {
        0 => unsafe { LoadIconW(None, IDI_APPLICATION).unwrap_or_default() },
        h => HICON(h as *mut _),
    };
    let mut data = NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY,
        hIcon: icon,
        ..Default::default()
    };
    for (dst, src) in data.szTip.iter_mut().zip("chord".encode_utf16()) {
        *dst = src;
    }
    data
}

fn add_tray(hwnd: HWND) {
    refresh_tray_icon();
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &tray_data(hwnd));
    }
}

/// A ⌘ glyph in the taskbar's text colour, like a macOS menu-bar template icon.
fn refresh_tray_icon() {
    let size = unsafe { GetSystemMetrics(SM_CXSMICON) }.max(16);
    let color = if light_theme(w!("SystemUsesLightTheme")) { Rgb(0, 0, 0) } else { style::WHITE };
    let Some(mut canvas) = Canvas::new(size, size) else { return };
    let font = canvas::font(&["Segoe UI Symbol"], size, 400, true);
    canvas.glyph(RECT { left: 0, top: 0, right: size, bottom: size }, "\u{2318}", font, color, DT_CENTER | DT_VCENTER);
    unsafe {
        let _ = DeleteObject(font);
    }
    if let Some(icon) = canvas.into_icon() {
        let old = TRAY_ICON.swap(icon.0 as isize, Ordering::AcqRel);
        if old != 0 {
            unsafe {
                let _ = DestroyIcon(HICON(old as *mut _));
            }
        }
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Acquire) {
        add_tray(hwnd);
        return LRESULT(0);
    }
    match msg {
        WM_PANEL => {
            match wparam.0 {
                PANEL_SHOW => {
                    if let Some((canvas, at)) = render_panel() {
                        layer_show(hwnd, canvas, at, style::APPEAR_MS, style::APPEAR_DROP);
                    }
                }
                PANEL_HIDE => layer_hide(hwnd, style::DISAPPEAR_MS),
                _ => layer_hide_now(hwnd),
            }
            LRESULT(0)
        }
        WM_TIMER => {
            layer_tick(hwnd);
            LRESULT(0)
        }
        WM_SETTINGCHANGE => {
            // Light/dark switches arrive here; the panel reads the theme each time it is shown.
            refresh_tray_icon();
            unsafe {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &tray_data(hwnd));
            }
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
        WM_HINTS => {
            if let Some(overlay) = overlay() {
                if wparam.0 == 1 {
                    if let Some((canvas, at)) = render_hints() {
                        layer_show(overlay, canvas, at, style::HINTS_APPEAR_MS, 0);
                    }
                } else {
                    layer_hide_now(overlay);
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

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_TIMER {
        layer_tick(hwnd);
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Puts `canvas` on screen in `hwnd`, fading in unless it is already showing.
fn layer_show(hwnd: HWND, canvas: Canvas, at: POINT, appear_ms: u32, drop: i32) {
    LAYERS.with_borrow_mut(|layers| {
        let visible = unsafe { IsWindowVisible(hwnd) }.as_bool();
        let anim = match layers.get(&(hwnd.0 as isize)).map(|l| l.anim) {
            // Already up (or coming up): just swap the content, so typing never flickers.
            Some(anim @ (Anim::Still | Anim::In { .. })) if visible => anim,
            _ if motion() => Anim::In { start: Instant::now(), duration: appear_ms, drop },
            _ => Anim::Still,
        };
        let layer = Layer { canvas, at, anim };
        present(hwnd, &layer);
        unsafe {
            if !visible {
                let flags = SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW;
                let _ = SetWindowPos(hwnd, HWND_TOPMOST, 0, 0, 0, 0, flags);
            }
            if !matches!(anim, Anim::Still) {
                SetTimer(hwnd, TIMER, FRAME_MS, None);
            }
        }
        layers.insert(hwnd.0 as isize, layer);
    });
}

fn layer_hide(hwnd: HWND, fade_ms: u32) {
    let fading = motion()
        && unsafe { IsWindowVisible(hwnd) }.as_bool()
        && LAYERS.with_borrow_mut(|layers| match layers.get_mut(&(hwnd.0 as isize)) {
            Some(layer) => {
                if !matches!(layer.anim, Anim::Out { .. }) {
                    layer.anim = Anim::Out { start: Instant::now(), duration: fade_ms };
                }
                true
            }
            None => false,
        });
    if fading {
        unsafe { SetTimer(hwnd, TIMER, FRAME_MS, None) };
    } else {
        layer_hide_now(hwnd);
    }
}

fn layer_hide_now(hwnd: HWND) {
    unsafe {
        let _ = KillTimer(hwnd, TIMER);
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
    // Drops the canvas: a hint overlay's bitmap is as big as the window it covers.
    LAYERS.with_borrow_mut(|layers| layers.remove(&(hwnd.0 as isize)));
}

fn layer_tick(hwnd: HWND) {
    let finished = LAYERS.with_borrow_mut(|layers| {
        let layer = layers.get_mut(&(hwnd.0 as isize))?;
        let done = present(hwnd, layer);
        if done && matches!(layer.anim, Anim::In { .. }) {
            layer.anim = Anim::Still;
        }
        Some((done, layer.anim))
    });
    match finished {
        Some((true, Anim::Out { .. })) => layer_hide_now(hwnd),
        Some((false, _)) => {}
        _ => unsafe {
            let _ = KillTimer(hwnd, TIMER);
        },
    }
}

/// Draws the layer's current animation frame; true once the animation has ended.
fn present(hwnd: HWND, layer: &Layer) -> bool {
    let since = |start: Instant| start.elapsed().as_millis().min(u32::MAX as u128) as u32;
    let (alpha, offset, done) = match layer.anim {
        Anim::Still => (255, 0, true),
        Anim::In { start, duration, drop } => {
            let (alpha, offset) = style::appear(since(start), duration, drop);
            (alpha, offset, since(start) >= duration)
        }
        Anim::Out { start, duration } => (style::disappear(since(start), duration), 0, since(start) >= duration),
    };
    layer.canvas.present(hwnd, POINT { x: layer.at.x, y: layer.at.y + offset }, alpha);
    done
}

/// Windows' "Animation effects" setting (off when the user asked for reduced motion).
fn motion() -> bool {
    let mut on = BOOL(1);
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut on as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    on.as_bool()
}

/// Reads a Personalize value; true (light) when it's missing, as on older Windows.
fn light_theme(value: PCWSTR) -> bool {
    let mut data = 1u32;
    let mut size = size_of::<u32>() as u32;
    unsafe {
        let _ = RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut _ as *mut _),
            Some(&mut size),
        );
    }
    data != 0
}

fn palette() -> &'static Palette {
    if light_theme(w!("AppsUseLightTheme")) { &style::LIGHT } else { &style::DARK }
}

fn scale(monitor: HMONITOR) -> impl Fn(i32) -> i32 {
    let (mut x, mut y) = (96u32, 96u32);
    unsafe {
        let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut x, &mut y);
    }
    let dpi = x.max(96) as i32;
    move |value| value * dpi / 96
}

/// Deletes the fonts it made when dropped.
struct Fonts(Vec<HFONT>);

impl Fonts {
    fn make(&mut self, faces: &[&str], size: i32, weight: i32) -> HFONT {
        let font = canvas::font(faces, size, weight, false);
        self.0.push(font);
        font
    }
}

impl Drop for Fonts {
    fn drop(&mut self) {
        for font in &self.0 {
            unsafe {
                let _ = DeleteObject(*font);
            }
        }
    }
}

/// Spotlight layout, centred near the top of the monitor the user is working on: the typed letters
/// large in a header, then one row per chord with its keys in a keycap and the typed part in accent.
fn render_panel() -> Option<(Canvas, POINT)> {
    let view = VIEW.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let colors = palette();
    let monitor = unsafe { MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe {
        let _ = GetMonitorInfoW(monitor, &mut info);
    }
    let work = info.rcWork;
    let s = scale(monitor);
    let (pad, header, row, gap) = (s(style::PANEL_PADDING), s(style::HEADER_HEIGHT), s(style::ROW_HEIGHT), s(6));
    let width = s(style::PANEL_WIDTH).min(work.right - work.left - 2 * pad);
    let margin = s(style::SHADOW_BLUR) + s(style::SHADOW_DROP);
    let top = work.top + (work.bottom - work.top) / 5;

    let room = ((work.bottom - top - margin - header - 2 * gap) / row).max(1) as usize;
    let mut rows: Vec<(String, String)> = view.rows.clone();
    // The last line holds "N more" / "No chord …" on the left and the groups on the right.
    let keep = if !view.groups.is_empty() || rows.len() > room { room - 1 } else { room };
    let first = crate::machine::first_shown(view.selected, keep);
    let more = rows.len().saturating_sub(keep);
    rows.drain(..first.min(rows.len()));
    rows.truncate(keep);
    let footer = more > 0 || rows.is_empty() || !view.groups.is_empty();
    let lines = (rows.len() + usize::from(footer)) as i32;
    let height = header + 1 + 2 * gap + lines * row;

    let mut canvas = Canvas::new(width + 2 * margin, height + 2 * margin)?;
    let mut fonts = Fonts(Vec::new());
    let query = fonts.make(style::UI_FONTS, s(20), 400);
    let small = fonts.make(style::UI_FONTS, s(12), 400);
    let body = fonts.make(style::UI_FONTS, s(14), 400);
    let mono = fonts.make(style::MONO_FONTS, s(13), 600);
    let symbol = fonts.make(&["Segoe UI Symbol"], s(18), 400);

    let panel = RECT { left: margin, top: margin, right: margin + width, bottom: margin + height };
    let radius = s(style::PANEL_RADIUS);
    canvas.shadow(panel, radius, s(style::SHADOW_BLUR), s(style::SHADOW_DROP), colors.shadow);
    canvas.round_rect(panel, radius, colors.border);
    let inner = RECT { left: panel.left + 1, top: panel.top + 1, right: panel.right - 1, bottom: panel.bottom - 1 };
    canvas.round_rect(inner, radius - 1, colors.background);

    let head = RECT { left: panel.left + pad, top: panel.top, right: panel.right - pad, bottom: panel.top + header };
    let icon_width = s(26);
    canvas.text(RECT { right: head.left + icon_width, ..head }, "\u{2318}", symbol, colors.secondary, DT_LEFT | DT_VCENTER);
    let query_area = RECT { left: head.left + icon_width, right: head.left + (head.right - head.left) * 2 / 3, ..head };
    if view.typed.is_empty() {
        let hint = if view.search { "Search chords" } else { "Type a chord" };
        canvas.text(query_area, hint, query, colors.secondary, DT_LEFT | DT_VCENTER);
    } else {
        canvas.text(query_area, &view.typed, query, colors.text, DT_LEFT | DT_VCENTER | DT_END_ELLIPSIS);
    }
    let app_area = RECT { left: query_area.right + pad, ..head };
    canvas.text(app_area, &view.app, small, colors.secondary, DT_RIGHT | DT_VCENTER | DT_END_ELLIPSIS);
    let line = panel.top + header;
    canvas.round_rect(RECT { left: inner.left, top: line, right: inner.right, bottom: line + 1 }, 0, colors.border);

    let key_pad = s(8);
    let key_width = rows.iter().map(|(chord, _)| canvas.measure(chord, mono)).max().unwrap_or(0) + 2 * key_pad;
    let mut y = line + 1 + gap;
    for (shown, (chord, name)) in rows.iter().enumerate() {
        let cell = RECT { left: head.left, top: y, right: head.right, bottom: y + row };
        if view.selected == Some(first + shown) {
            let band = RECT { left: inner.left + s(6), right: inner.right - s(6), ..cell };
            canvas.round_rect(band, s(style::KEYCAP_RADIUS), mix(colors.background, colors.accent, 0.25));
        }
        let typed = if view.search { 0 } else { common_prefix(chord, &view.typed) };
        let chord_width = canvas.measure(chord, mono) + 2 * key_pad;
        let cap = RECT { left: cell.left, top: cell.top + s(4), right: cell.left + chord_width, bottom: cell.bottom - s(4) };
        canvas.round_rect(cap, s(style::KEYCAP_RADIUS), colors.keycap);
        let typed_width = canvas.measure(&chord[..typed], mono);
        let keys = RECT { left: cap.left + key_pad, ..cap };
        canvas.text(keys, &chord[..typed], mono, colors.accent, DT_LEFT | DT_VCENTER);
        canvas.text(RECT { left: keys.left + typed_width, ..keys }, &chord[typed..], mono, colors.text, DT_LEFT | DT_VCENTER);
        let label = RECT { left: cell.left + key_width + s(12), ..cell };
        canvas.text(label, name, body, colors.text, DT_LEFT | DT_VCENTER | DT_END_ELLIPSIS);
        y += row;
    }
    let cell = RECT { left: head.left, top: y, right: head.right, bottom: y + row };
    let note = if more > 0 {
        Some((format!("{more} more"), small))
    } else if rows.is_empty() {
        let text = if view.search { "No chord matches" } else { "No chord starts with these keys" };
        Some((text.to_string(), body))
    } else {
        None
    };
    let mut groups_left = cell.left;
    if let Some((text, font)) = &note {
        canvas.text(cell, text, *font, colors.secondary, DT_LEFT | DT_VCENTER);
        groups_left += canvas.measure(text, *font) + s(24);
    }
    if !view.groups.is_empty() {
        let text = view.groups.iter().map(|(prefix, name)| format!("{prefix} {name}")).collect::<Vec<_>>().join("   ·   ");
        let area = RECT { left: groups_left, ..cell };
        canvas.text(area, &text, small, colors.secondary, DT_RIGHT | DT_VCENTER | DT_END_ELLIPSIS);
    }

    let at = POINT { x: work.left + (work.right - work.left - width) / 2 - margin, y: top - margin };
    Some((canvas, at))
}

/// Byte length of the longest common prefix of `a` and `b` (chords are ASCII, but stay safe).
fn common_prefix(a: &str, b: &str) -> usize {
    a.char_indices().zip(b.chars()).take_while(|((_, x), y)| x == y).last().map_or(0, |((i, c), _)| i + c.len_utf8())
}

/// Small accent-blue tags with white letters (the already-typed letters dimmed) at each target's corner.
/// The size each label in `labels` is drawn at over `area`, as `render_hints` draws it.
pub fn hint_sizes(area: RECT, labels: &[String]) -> Vec<(i32, i32)> {
    let s = scale(unsafe { MonitorFromRect(&area, MONITOR_DEFAULTTONEAREST) });
    let Some(canvas) = Canvas::new(1, 1) else { return vec![(0, 0); labels.len()] };
    let mut fonts = Fonts(Vec::new());
    let font = fonts.make(style::UI_FONTS, s(11), 700);
    let (height, side) = (s(style::HINT_HEIGHT), s(style::HINT_PADDING));
    labels.iter().map(|l| (canvas.measure(&l.to_uppercase(), font) + 2 * side, height)).collect()
}

fn render_hints() -> Option<(Canvas, POINT)> {
    let (area, labels, typed) = HINTS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let s = scale(unsafe { MonitorFromRect(&area, MONITOR_DEFAULTTONEAREST) });
    let colors = palette();
    let mut canvas = Canvas::new(area.right - area.left, area.bottom - area.top)?;
    let mut fonts = Fonts(Vec::new());
    let font = fonts.make(style::UI_FONTS, s(11), 700);
    let (height, side, radius) = (s(style::HINT_HEIGHT), s(style::HINT_PADDING), s(style::HINT_RADIUS));
    let dim = mix(colors.accent, style::WHITE, 0.55);
    let typed_upper = typed.to_uppercase();
    for (point, label) in &labels {
        // Every label shown starts with what was typed (the caller filters them).
        let label = label.to_uppercase();
        let rest = &label[typed_upper.len().min(label.len())..];
        let (done_width, text_width) = (canvas.measure(&typed_upper, font), canvas.measure(&label, font));
        let width = text_width + 2 * side;
        let (x, y) = ((point.x - area.left).max(0), (point.y - area.top).max(0));
        let pill = RECT { left: x, top: y, right: x + width, bottom: y + height };
        canvas.shadow(pill, radius, s(5), s(2), 0.3);
        canvas.round_rect(pill, radius, colors.accent);
        let text = RECT { left: x + (width - text_width) / 2, ..pill };
        canvas.text(text, &typed_upper, font, dim, DT_LEFT | DT_VCENTER);
        canvas.text(RECT { left: text.left + done_width, ..text }, rest, font, style::WHITE, DT_LEFT | DT_VCENTER);
    }
    Some((canvas, POINT { x: area.left, y: area.top }))
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Rgb(m(a.0, b.0), m(a.1, b.1), m(a.2, b.2))
}

/// Makes the tray menu follow Windows' light/dark setting. The only way is uxtheme's unnamed
/// SetPreferredAppMode (ordinal 135, Windows 10 1903+); where it is missing the menu stays light.
fn theme_menus() {
    unsafe {
        let Ok(uxtheme) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(set_mode) = GetProcAddress(uxtheme, PCSTR(135 as *const u8)) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            const FORCE_DARK: i32 = 2;
            const FORCE_LIGHT: i32 = 3;
            set_mode(if light_theme(w!("AppsUseLightTheme")) { FORCE_LIGHT } else { FORCE_DARK });
        }
        if let Some(flush) = GetProcAddress(uxtheme, PCSTR(136 as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

fn tray_menu(hwnd: HWND) {
    theme_menus();
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let autostart_flags = if autostart::enabled() { MF_STRING | MF_CHECKED } else { MF_STRING };
        let _ = AppendMenuW(menu, MF_STRING, ID_OPEN, w!("Open chords folder"));
        let _ = AppendMenuW(menu, autostart_flags, ID_AUTOSTART, w!("Start with Windows"));
        let _ = AppendMenuW(menu, MF_STRING, ID_UPDATE, w!("Check for updates"));
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
            ID_UPDATE => crate::update::check_in_background(),
            ID_QUIT => {
                remove_tray();
                std::process::exit(0);
            }
            _ => {}
        }
    }
}
