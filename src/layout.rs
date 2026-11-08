//! Window layouts for `window = "…"` chords: where each one puts the window in front (pure).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Left,
    Right,
    Top,
    Bottom,
    /// Maximizes, or restores a window that is already maximized.
    Maximize,
    Center,
    Padded,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    LeftThird,
    MiddleThird,
    RightThird,
    NarrowLeft,
    WideCenter,
    NarrowRight,
    LeftMaster,
    RightMaster,
}

/// One place for the window, inside the monitor's work area (the screen minus the taskbar).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Frame {
    /// x, y, width, height as fractions of the work area.
    Part(f64, f64, f64, f64),
    /// Gaps from the work area's edges in pixels at 96 DPI: top, right, bottom, left.
    Gaps(i32, i32, i32, i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

const H: f64 = 1.0 / 2.0;
const T: f64 = 1.0 / 3.0;
const TT: f64 = 2.0 / 3.0;
const Q: f64 = 1.0 / 4.0;

pub const NAMES: &str = "left, right, top, bottom, maximize, center, padded, top-left, top-right, bottom-left, \
    bottom-right, left-third, middle-third, right-third, narrow-left, wide-center, narrow-right, left-master or right-master";

impl Layout {
    pub fn parse(name: &str) -> Option<Self> {
        use Layout::*;
        Some(match name {
            "left" => Left,
            "right" => Right,
            "top" => Top,
            "bottom" => Bottom,
            "maximize" => Maximize,
            "center" => Center,
            "padded" => Padded,
            "top-left" => TopLeft,
            "top-right" => TopRight,
            "bottom-left" => BottomLeft,
            "bottom-right" => BottomRight,
            "left-third" => LeftThird,
            "middle-third" => MiddleThird,
            "right-third" => RightThird,
            "narrow-left" => NarrowLeft,
            "wide-center" => WideCenter,
            "narrow-right" => NarrowRight,
            "left-master" => LeftMaster,
            "right-master" => RightMaster,
            _ => return None,
        })
    }

    /// The places the layout steps through when it runs again on the same window. Empty for `Maximize`.
    pub fn frames(self) -> &'static [Frame] {
        use Frame::*;
        use Layout::*;
        match self {
            Left => &[Part(0.0, 0.0, H, 1.0), Part(0.0, 0.0, TT, 1.0), Part(0.0, 0.0, T, 1.0)],
            Right => &[Part(H, 0.0, H, 1.0), Part(T, 0.0, TT, 1.0), Part(TT, 0.0, T, 1.0)],
            Top => &[Part(0.0, 0.0, 1.0, H), Part(0.0, 0.0, 1.0, TT), Part(0.0, 0.0, 1.0, T)],
            Bottom => &[Part(0.0, H, 1.0, H), Part(0.0, T, 1.0, TT), Part(0.0, TT, 1.0, T)],
            Maximize => &[],
            Center => &[Part(0.15, 0.10, 0.70, 0.80)],
            Padded => &[Gaps(50, 30, 10, 30)],
            TopLeft => &[Part(0.0, 0.0, H, H)],
            TopRight => &[Part(H, 0.0, H, H)],
            BottomLeft => &[Part(0.0, H, H, H)],
            BottomRight => &[Part(H, H, H, H)],
            LeftThird => &[Part(0.0, 0.0, T, 1.0)],
            MiddleThird => &[Part(T, 0.0, T, 1.0)],
            RightThird => &[Part(TT, 0.0, T, 1.0)],
            NarrowLeft => &[Part(0.0, 0.0, Q, 1.0)],
            WideCenter => &[Part(Q, 0.0, H, 1.0)],
            NarrowRight => &[Part(3.0 * Q, 0.0, Q, 1.0)],
            LeftMaster => &[Part(0.0, 0.0, H, 1.0), Part(H, 0.0, Q, 1.0), Part(3.0 * Q, 0.0, Q, 1.0)],
            RightMaster => &[Part(H, 0.0, H, 1.0), Part(Q, 0.0, Q, 1.0), Part(0.0, 0.0, Q, 1.0)],
        }
    }
}

/// Where `frame` lands in `work` on a monitor at `dpi`. Edges are rounded, not sizes, so windows
/// snapped next to each other share an edge with no 1px seam.
pub fn place(frame: Frame, work: Rect, dpi: i32) -> Rect {
    let (w, h) = ((work.right - work.left) as f64, (work.bottom - work.top) as f64);
    match frame {
        Frame::Part(x, y, fw, fh) => Rect {
            left: work.left + (w * x).round() as i32,
            top: work.top + (h * y).round() as i32,
            right: work.left + (w * (x + fw)).round() as i32,
            bottom: work.top + (h * (y + fh)).round() as i32,
        },
        Frame::Gaps(top, right, bottom, left) => {
            let px = |v: i32| v * dpi / 96;
            Rect { left: work.left + px(left), top: work.top + px(top), right: work.right - px(right), bottom: work.bottom - px(bottom) }
        }
    }
}

/// The frame to use next. `last` is (layout, frame index, where the window ended up) of the previous
/// run on this window: running the same layout again steps on only while the window is still where
/// that run left it, so a window moved by hand starts over at the first frame.
pub fn next_frame(layout: Layout, last: Option<(Layout, usize, Rect)>, now: Rect) -> usize {
    match last {
        Some((l, i, at)) if l == layout && at == now => (i + 1) % layout.frames().len().max(1),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { left: 0, top: 0, right: 1920, bottom: 1032 };

    #[test]
    fn every_name_parses_and_has_a_place() {
        for name in NAMES.replace(" or ", ", ").split(", ") {
            let layout = Layout::parse(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(layout.frames().is_empty(), layout == Layout::Maximize, "{name}");
        }
        assert_eq!(Layout::parse("middle"), None);
    }

    #[test]
    fn thirds_share_edges_without_seams() {
        let odd = Rect { left: -1366, top: 0, right: 0, bottom: 728 };
        let [a, b, c] = [Layout::LeftThird, Layout::MiddleThird, Layout::RightThird].map(|l| place(l.frames()[0], odd, 96));
        assert_eq!((a.left, a.right, b.left, b.right, c.left, c.right), (-1366, -911, -911, -455, -455, 0));
    }

    #[test]
    fn padded_gaps_scale_with_dpi() {
        let at = |dpi| place(Layout::Padded.frames()[0], WORK, dpi);
        assert_eq!(at(96), Rect { left: 30, top: 50, right: 1890, bottom: 1022 });
        assert_eq!(at(144), Rect { left: 45, top: 75, right: 1875, bottom: 1017 });
    }

    #[test]
    fn running_again_steps_through_frames_only_while_the_window_stays_put() {
        let half = place(Layout::Left.frames()[0], WORK, 96);
        assert_eq!(next_frame(Layout::Left, None, half), 0);
        assert_eq!(next_frame(Layout::Left, Some((Layout::Left, 0, half)), half), 1);
        assert_eq!(next_frame(Layout::Left, Some((Layout::Left, 2, half)), half), 0, "wraps around");
        assert_eq!(next_frame(Layout::Left, Some((Layout::Left, 0, half)), WORK), 0, "moved by hand");
        assert_eq!(next_frame(Layout::Right, Some((Layout::Left, 0, half)), half), 0, "another layout");
        assert_eq!(next_frame(Layout::Center, Some((Layout::Center, 0, half)), half), 0, "one frame");
    }
}
