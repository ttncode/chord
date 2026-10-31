//! Look and motion modelled on macOS: Apple HIG system colours, Spotlight-style panel, soft shadow,
//! short ease-out animations. Sizes are at 96 DPI and get scaled per monitor.

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub struct Palette {
    pub background: Rgb,
    pub border: Rgb,
    pub text: Rgb,
    pub secondary: Rgb,
    pub accent: Rgb,
    pub keycap: Rgb,
    pub shadow: f32,
}

/// Apple HIG dark: label #F5F5F7, secondaryLabel, systemBlue #0A84FF, systemGray5 #3A3A3C keycaps.
pub const DARK: Palette = Palette {
    background: Rgb(0x26, 0x26, 0x28),
    border: Rgb(0x3D, 0x3D, 0x40),
    text: Rgb(0xF5, 0xF5, 0xF7),
    secondary: Rgb(0x98, 0x98, 0x9D),
    accent: Rgb(0x0A, 0x84, 0xFF),
    keycap: Rgb(0x3A, 0x3A, 0x3C),
    shadow: 0.45,
};

/// Apple HIG light: label #1D1D1F, secondaryLabel #6E6E73, systemBlue #007AFF, systemGray5 #E5E5EA keycaps.
pub const LIGHT: Palette = Palette {
    background: Rgb(0xF6, 0xF6, 0xF6),
    border: Rgb(0xD1, 0xD1, 0xD6),
    text: Rgb(0x1D, 0x1D, 0x1F),
    secondary: Rgb(0x6E, 0x6E, 0x73),
    accent: Rgb(0x00, 0x7A, 0xFF),
    keycap: Rgb(0xE5, 0xE5, 0xEA),
    shadow: 0.22,
};

pub const WHITE: Rgb = Rgb(0xFF, 0xFF, 0xFF);

/// SF Pro may only be used on Apple platforms, so the closest Windows faces stand in, best first.
pub const UI_FONTS: &[&str] = &["Segoe UI Variable Text", "Segoe UI"];
pub const MONO_FONTS: &[&str] = &["Cascadia Mono", "Consolas"];

pub const PANEL_WIDTH: i32 = 560;
pub const PANEL_RADIUS: i32 = 12;
pub const PANEL_PADDING: i32 = 14;
pub const HEADER_HEIGHT: i32 = 44;
pub const ROW_HEIGHT: i32 = 30;
pub const KEYCAP_RADIUS: i32 = 5;
pub const SHADOW_BLUR: i32 = 22;
pub const SHADOW_DROP: i32 = 8;
/// Vimium's marker size (11px bold, 1×3px padding, 3px corners), so a label hides as little as it does.
pub const HINT_HEIGHT: i32 = 15;
pub const HINT_PADDING: i32 = 3;
pub const HINT_RADIUS: i32 = 3;

pub const APPEAR_MS: u32 = 160;
pub const DISAPPEAR_MS: u32 = 120;
pub const HINTS_APPEAR_MS: u32 = 90;
/// The panel drops into place by this much while it fades in, like Spotlight.
pub const APPEAR_DROP: i32 = 8;

/// Signed distance (pixels) from a pixel centre to a rounded rectangle: negative inside.
pub fn distance(x: i32, y: i32, left: i32, top: i32, right: i32, bottom: i32, radius: i32) -> f32 {
    let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
    let r = radius as f32;
    let (hw, hh) = ((right - left) as f32 / 2.0, (bottom - top) as f32 / 2.0);
    let (mx, my) = (left as f32 + hw, top as f32 + hh);
    let qx = (cx - mx).abs() - (hw - r);
    let qy = (cy - my).abs() - (hh - r);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - r
}

/// How much of a pixel at `distance` the shape covers: antialiased over one pixel.
pub fn coverage(distance: f32) -> f32 {
    (0.5 - distance).clamp(0.0, 1.0)
}

/// Shadow opacity at `distance` outside the shape, fading to nothing at `blur`.
pub fn shadow(distance: f32, blur: f32, strength: f32) -> f32 {
    if distance <= 0.0 {
        strength
    } else {
        strength * (1.0 - (distance / blur).min(1.0)).powi(2)
    }
}

/// Cubic ease-out: fast start, gentle stop.
pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t.clamp(0.0, 1.0)).powi(3)
}

/// Opacity (0–255) and vertical offset for a fade-in `elapsed` ms into `duration`.
pub fn appear(elapsed: u32, duration: u32, drop: i32) -> (u8, i32) {
    let t = ease_out(elapsed as f32 / duration.max(1) as f32);
    ((t * 255.0).round() as u8, ((1.0 - t) * -(drop as f32)).round() as i32)
}

/// Opacity (0–255) for a fade-out `elapsed` ms into `duration`.
pub fn disappear(elapsed: u32, duration: u32) -> u8 {
    ((1.0 - ease_out(elapsed as f32 / duration.max(1) as f32)) * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance_is_negative_inside_and_positive_outside() {
        assert!(distance(50, 20, 0, 0, 100, 40, 10) < 0.0);
        assert!(distance(150, 20, 0, 0, 100, 40, 10) > 0.0);
        // The square corner pixel is cut off by the rounding.
        assert!(distance(0, 0, 0, 0, 100, 40, 10) > 0.0);
        assert!(distance(1, 20, 0, 0, 100, 40, 10) < 0.0);
    }

    #[test]
    fn coverage_is_full_inside_empty_outside_partial_on_the_edge() {
        assert_eq!(coverage(-3.0), 1.0);
        assert_eq!(coverage(3.0), 0.0);
        assert!(coverage(0.0) > 0.0 && coverage(0.0) < 1.0);
    }

    #[test]
    fn shadow_fades_out_to_zero() {
        assert_eq!(shadow(-1.0, 20.0, 0.4), 0.4);
        assert!(shadow(10.0, 20.0, 0.4) < 0.4);
        assert_eq!(shadow(25.0, 20.0, 0.4), 0.0);
    }

    #[test]
    fn appear_runs_from_hidden_and_raised_to_opaque_in_place() {
        assert_eq!(appear(0, 160, 8), (0, -8));
        assert_eq!(appear(160, 160, 8), (255, 0));
        assert_eq!(appear(999, 160, 8), (255, 0));
        let (alpha, offset) = appear(80, 160, 8);
        assert!(alpha > 128 && offset > -8 && offset < 0);
    }

    #[test]
    fn disappear_runs_from_opaque_to_hidden() {
        assert_eq!(disappear(0, 120), 255);
        assert_eq!(disappear(120, 120), 0);
        assert!(disappear(60, 120) < 128);
    }
}
