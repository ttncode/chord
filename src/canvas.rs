//! A 32-bit premultiplied-alpha bitmap for layered windows: antialiased rounded shapes and soft shadows,
//! which plain GDI can't draw, plus GDI text with the alpha channel GDI destroys put back.

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateBitmap,
    CreateCompatibleDC, CreateDIBSection, CreateFontW, DIB_RGB_COLORS, DT_CALCRECT, DT_NOPREFIX,
    DT_SINGLELINE, DeleteDC, DeleteObject, DRAW_TEXT_FORMAT, DrawTextW, GetTextFaceW, HBITMAP, HDC, HFONT,
    HGDIOBJ, SelectObject, SetBkMode, SetTextColor, TRANSPARENT,
};
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO, ULW_ALPHA, UpdateLayeredWindow};
use windows::core::HSTRING;

use crate::style::{self, Rgb};

const CLEARTYPE_QUALITY: u32 = 5;
const ANTIALIASED_QUALITY: u32 = 4;

pub struct Canvas {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u32,
    pub width: i32,
    pub height: i32,
}

impl Canvas {
    /// A fully transparent canvas.
    pub fn new(width: i32, height: i32) -> Option<Self> {
        let (width, height) = (width.max(1), height.max(1));
        unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // top-down rows
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            // DIB section memory starts zeroed: transparent black.
            let Ok(bitmap) = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0) else {
                let _ = DeleteDC(dc);
                return None;
            };
            let old = SelectObject(dc, bitmap);
            Some(Self { dc, bitmap, old, bits: bits.cast(), width, height })
        }
    }

    fn pixels(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height) as usize) }
    }

    /// Paints `color` at `alpha` (0–1) over each pixel the closure covers.
    fn blend(&mut self, area: RECT, color: Rgb, cover: impl Fn(i32, i32) -> f32) {
        let (w, h) = (self.width, self.height);
        let pixels = self.pixels();
        for y in area.top.max(0)..area.bottom.min(h) {
            for x in area.left.max(0)..area.right.min(w) {
                let a = cover(x, y);
                if a <= 0.0 {
                    continue;
                }
                let p = &mut pixels[(y * w + x) as usize];
                let mix = |src: u8, shift: u32| -> u32 {
                    let dst = ((*p >> shift) & 0xFF) as f32;
                    ((src as f32 * a + dst * (1.0 - a)).round() as u32) << shift
                };
                *p = mix(255, 24) | mix(color.0, 16) | mix(color.1, 8) | mix(color.2, 0);
            }
        }
    }

    pub fn round_rect(&mut self, r: RECT, radius: i32, color: Rgb) {
        self.blend(r, color, |x, y| {
            style::coverage(style::distance(x, y, r.left, r.top, r.right, r.bottom, radius))
        });
    }

    /// A soft black shadow under the rounded rectangle `r`, dropped by `drop` pixels.
    pub fn shadow(&mut self, r: RECT, radius: i32, blur: i32, drop: i32, strength: f32) {
        let area = RECT { left: r.left - blur, top: r.top - blur + drop, right: r.right + blur, bottom: r.bottom + blur + drop };
        self.blend(area, Rgb(0, 0, 0), |x, y| {
            let d = style::distance(x, y, r.left, r.top + drop, r.right, r.bottom + drop, radius);
            style::shadow(d, blur as f32, strength)
        });
    }

    /// Text over an already opaque area of the canvas.
    pub fn text(&mut self, r: RECT, text: &str, font: HFONT, color: Rgb, flags: DRAW_TEXT_FORMAT) {
        // GDI zeroes the alpha of every pixel it writes; save it and put it back.
        let saved: Vec<u32> = self.region(r).map(|i| self.pixels()[i] & 0xFF00_0000).collect();
        self.draw_text(r, text, font, color, flags);
        let indexes: Vec<usize> = self.region(r).collect();
        let pixels = self.pixels();
        for (i, alpha) in indexes.into_iter().zip(saved) {
            pixels[i] = (pixels[i] & 0x00FF_FFFF) | alpha;
        }
    }

    /// Text alone on a transparent area (an icon glyph): its greyscale edges become alpha.
    /// `font` must be made with `antialias_only`, since ClearType's coloured edges would tint it.
    pub fn glyph(&mut self, r: RECT, text: &str, font: HFONT, color: Rgb, flags: DRAW_TEXT_FORMAT) {
        self.draw_text(r, text, font, Rgb(255, 255, 255), flags);
        let indexes: Vec<usize> = self.region(r).collect();
        let pixels = self.pixels();
        for i in indexes {
            let a = (pixels[i] >> 8 & 0xFF) as u32;
            let pre = |c: u8| c as u32 * a / 255;
            pixels[i] = a << 24 | pre(color.0) << 16 | pre(color.1) << 8 | pre(color.2);
        }
    }

    fn draw_text(&mut self, r: RECT, text: &str, font: HFONT, color: Rgb, flags: DRAW_TEXT_FORMAT) {
        unsafe {
            let previous = SelectObject(self.dc, font);
            SetBkMode(self.dc, TRANSPARENT);
            SetTextColor(self.dc, COLORREF(color.0 as u32 | (color.1 as u32) << 8 | (color.2 as u32) << 16));
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let mut rect = r;
            DrawTextW(self.dc, &mut wide, &mut rect, flags | DT_SINGLELINE | DT_NOPREFIX);
            SelectObject(self.dc, previous);
        }
    }

    /// Width in pixels of `text` in `font`.
    pub fn measure(&self, text: &str, font: HFONT) -> i32 {
        unsafe {
            let previous = SelectObject(self.dc, font);
            let mut wide: Vec<u16> = text.encode_utf16().collect();
            let mut rect = RECT::default();
            DrawTextW(self.dc, &mut wide, &mut rect, DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX);
            SelectObject(self.dc, previous);
            rect.right - rect.left
        }
    }

    fn region(&self, r: RECT) -> impl Iterator<Item = usize> + use<> {
        let (w, h) = (self.width, self.height);
        (r.top.max(0)..r.bottom.min(h)).flat_map(move |y| (r.left.max(0)..r.right.min(w)).map(move |x| (y * w + x) as usize))
    }

    /// Puts the canvas on screen as `hwnd`'s whole content, top-left at `at`, faded to `alpha`.
    pub fn present(&self, hwnd: HWND, at: POINT, alpha: u8) {
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: alpha,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let size = SIZE { cx: self.width, cy: self.height };
        unsafe {
            let _ = UpdateLayeredWindow(
                hwnd,
                None,
                Some(&at),
                Some(&size),
                self.dc,
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
        }
    }

    /// Turns the canvas into an icon (icons want straight, not premultiplied, alpha).
    pub fn into_icon(mut self) -> Option<HICON> {
        for p in self.pixels() {
            let a = *p >> 24;
            if a > 0 {
                let straight = |shift: u32| ((*p >> shift & 0xFF) * 255 / a).min(255) << shift;
                *p = a << 24 | straight(16) | straight(8) | straight(0);
            }
        }
        unsafe {
            SelectObject(self.dc, self.old);
            let mask = CreateBitmap(self.width, self.height, 1, 1, None);
            let info = ICONINFO { fIcon: true.into(), hbmMask: mask, hbmColor: self.bitmap, ..Default::default() };
            let icon = CreateIconIndirect(&info).ok();
            let _ = DeleteObject(mask);
            self.old = SelectObject(self.dc, self.bitmap);
            icon
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.dc);
        }
    }
}

/// The first installed face of `faces` (GDI otherwise silently substitutes an unrelated one).
pub fn font(faces: &[&str], size: i32, weight: i32, antialias_only: bool) -> HFONT {
    let quality = if antialias_only { ANTIALIASED_QUALITY } else { CLEARTYPE_QUALITY };
    let make = |face: &str| unsafe {
        CreateFontW(-size, 0, 0, 0, weight, 0, 0, 0, 0, 0, 0, quality, 0, &HSTRING::from(face))
    };
    for face in faces {
        let font = make(face);
        if has_face(font, face) {
            return font;
        }
        unsafe {
            let _ = DeleteObject(font);
        }
    }
    make(faces.last().copied().unwrap_or("Segoe UI"))
}

fn has_face(font: HFONT, face: &str) -> bool {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let previous = SelectObject(dc, font);
        let mut name = [0u16; 64];
        GetTextFaceW(dc, Some(&mut name));
        SelectObject(dc, previous);
        let _ = DeleteDC(dc);
        let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
        String::from_utf16_lossy(&name[..len]).eq_ignore_ascii_case(face)
    }
}
