//! Hint labels for clickable elements (Vimium style): short, home-row first, and prefix-free,
//! so typing a full label never also completes a shorter one.

pub const ALPHABET: &str = "sadfjklewcmpgh";

/// Screen rectangle: left, top, right, bottom.
pub type Rect = (i32, i32, i32, i32);

/// More labels than this are unreadable; mousemaster caps similarly. Counted after filtering, since
/// big web apps report thousands of covered or scrolled-away elements first.
const MAX_LABELS: usize = 300;

/// Element rectangles closer than this (pixels) on every edge are the same target.
const SAME: i32 = 6;

/// An item row is cut to this many times its height from its left edge before its middle is clicked.
const ITEM_WIDTH_PER_HEIGHT: i32 = 4;

/// Smallest scroll area side worth a label, in pixels.
const MIN_AREA: i32 = 80;

/// Areas showing at least this percentage of their content barely scroll.
const MAX_SHOWN: f64 = 95.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Clickable, but no known control type (custom, text, image).
    Generic,
    /// Link, file row, tab…
    Control,
    /// A button: one inside another control (a tab's close button) still gets a label.
    Button,
    /// A scroll bar: neither it nor its arrows get a label, as the scroll chords do its job.
    ScrollBar,
    /// Tree or list item: an open one's box also covers the items nested in it.
    Item,
}

/// The rectangles worth a label, top-left first. Containers (lists, panes) are already left out by
/// type. Anything inside a real control is a part of it (a file row's cells, a button's text) and
/// gets no label, except buttons (a tab's close button) and items nested in an open item (a folder's files), which keep theirs while
/// the open item is cut down to its own row above them. A generic box holding other clickable
/// things gives way to them (Vimium's rule for web pages). Near-duplicates and mostly overlapping
/// ones get one. Scroll bars and their parts get none.
pub fn targets(elements: Vec<(Rect, Kind)>) -> Vec<Rect> {
    let same = |a: Rect, b: Rect| {
        (a.0 - b.0).abs() <= SAME && (a.1 - b.1).abs() <= SAME && (a.2 - b.2).abs() <= SAME && (a.3 - b.3).abs() <= SAME
    };
    let contains = |a: Rect, b: Rect| a.0 <= b.0 && a.1 <= b.1 && a.2 >= b.2 && a.3 >= b.3 && !same(a, b);
    let elements: Vec<(Rect, Kind)> = elements.into_iter().filter(|(r, _)| r.2 > r.0 && r.3 > r.1).collect();
    let in_scroll_bar = |r: Rect| elements.iter().any(|&(bar, k)| k == Kind::ScrollBar && (same(bar, r) || contains(bar, r)));
    let elements: Vec<(Rect, Kind)> = elements.iter().copied().filter(|&(r, _)| !in_scroll_bar(r)).collect();
    let mut kept: Vec<Rect> = Vec::new();
    for &(mut r, kind) in &elements {
        let part_of_a_control = kind != Kind::Button
            && elements
                .iter()
                .any(|&(outer, k)| k != Kind::Generic && !(k == Kind::Item && kind == Kind::Item) && contains(outer, r));
        let box_around_others = kind == Kind::Generic && elements.iter().any(|&(inner, _)| contains(r, inner));
        if kind == Kind::Item {
            let nested_top = elements.iter().filter(|&&(inner, k)| k == Kind::Item && contains(r, inner) && inner.1 > r.1).map(|(inner, _)| inner.1).min();
            if let Some(top) = nested_top {
                r.3 = top;
            }
            // The click goes to the middle: keep it on the name, as a row can run past the visible
            // columns (Explorer's details view) or far beyond its text.
            r.2 = r.2.min(r.0 + ITEM_WIDTH_PER_HEIGHT * (r.3 - r.1));
        }
        if !part_of_a_control && !box_around_others && !kept.iter().any(|&k| same(k, r) || (mostly_overlap(k, r) && !contains(k, r) && !contains(r, k))) {
            kept.push(r);
        }
    }
    kept.sort_by_key(|r| (r.1, r.0));
    kept.truncate(MAX_LABELS);
    kept
}

/// `rects` (in label order) without those whose label, of the size in `sizes`, would cover an
/// earlier kept one's while the two elements overlap too: parts of one thing reported twice.
/// Labels that touch on neighbouring buttons stay.
pub fn unstack(rects: Vec<Rect>, sizes: &[(i32, i32)]) -> Vec<Rect> {
    let cross = |a: Rect, b: Rect| a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3;
    let pill = |r: Rect, (w, h): (i32, i32)| (r.0, r.1, r.0 + w, r.1 + h);
    let mut kept: Vec<(Rect, Rect)> = Vec::new();
    for (&r, &size) in rects.iter().zip(sizes) {
        let label = pill(r, size);
        if !kept.iter().any(|&(k, k_label)| cross(k, r) && cross(k_label, label)) {
            kept.push((r, label));
        }
    }
    kept.into_iter().map(|(r, _)| r).collect()
}

/// Half or more of the smaller one lies inside the other: an icon spilling out of its button, a badge
/// on an avatar. Their labels would sit on top of each other, and clicking either hits the same thing.
fn mostly_overlap(a: Rect, b: Rect) -> bool {
    let area = |r: Rect| (r.2 - r.0) as i64 * (r.3 - r.1) as i64;
    let (w, h) = (a.2.min(b.2) - a.0.max(b.0), a.3.min(b.3) - a.1.max(b.1));
    w > 0 && h > 0 && 2 * w as i64 * h as i64 >= area(a).min(area(b))
}

/// Scroll areas worth a label, top-left first, from rectangles with the percentage of their content
/// on view. Chromium calls anything that overflows by a pixel scrollable (a prompt box, a table, a
/// clipped line), so only areas big enough to aim at and hiding a real part of their content count.
/// Unlike `targets`, nested ones all stay: a page and the panel scrolling inside it are both choices.
/// Near-duplicates get one.
// shortcut: the pointer goes to an area's centre, so a page whose centre sits on an inner panel scrolls that panel; aim at an uncovered spot if it bites.
pub fn areas(rects: Vec<(Rect, f64)>) -> Vec<Rect> {
    let same = |a: Rect, b: Rect| {
        (a.0 - b.0).abs() <= SAME && (a.1 - b.1).abs() <= SAME && (a.2 - b.2).abs() <= SAME && (a.3 - b.3).abs() <= SAME
    };
    let mut kept: Vec<Rect> = Vec::new();
    let worth = |&(r, shown): &(Rect, f64)| r.2 - r.0 >= MIN_AREA && r.3 - r.1 >= MIN_AREA && shown < MAX_SHOWN;
    for (r, _) in rects.into_iter().filter(worth) {
        if !kept.iter().any(|&k| same(k, r)) {
            kept.push(r);
        }
    }
    kept.sort_by_key(|r| (r.1, r.0));
    kept.truncate(MAX_LABELS);
    kept
}

/// Where to put the pointer so the wheel scrolls something: None when it already rests on a scroll
/// area, else the centre of the biggest one. `rects` as `uia::scrollable_rects` gives them; navigation
/// landmarks (0) don't count, as they may not scroll at all.
pub fn aim(rects: Vec<(Rect, f64)>, pointer: (i32, i32)) -> Option<(i32, i32)> {
    let areas = areas(rects.into_iter().filter(|&(_, shown)| shown > 0.0).collect());
    let inside = |r: &Rect| (r.0..r.2).contains(&pointer.0) && (r.1..r.3).contains(&pointer.1);
    if areas.iter().any(inside) {
        return None;
    }
    let r = areas.iter().max_by_key(|r| (r.2 - r.0) * (r.3 - r.1))?;
    Some(((r.0 + r.2) / 2, (r.1 + r.3) / 2))
}

pub fn labels(count: usize) -> Vec<String> {
    if count == 0 {
        return vec![];
    }
    // Vimium's hintStrings: breadth-first, consuming a label each time it is extended, so no label
    // stays that is a prefix of another; reversing spreads shared first letters across the screen.
    let mut hints = vec![String::new()];
    let mut offset = 0;
    while hints.len() - offset < count || hints.len() == 1 {
        let hint = hints[offset].clone();
        offset += 1;
        hints.extend(ALPHABET.chars().map(|c| format!("{c}{hint}")));
    }
    let mut chosen = hints[offset..offset + count].to_vec();
    chosen.sort();
    chosen.into_iter().map(|l| l.chars().rev().collect()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn control(r: Rect) -> (Rect, Kind) {
        (r, Kind::Control)
    }
    fn generic(r: Rect) -> (Rect, Kind) {
        (r, Kind::Generic)
    }
    fn item(r: Rect) -> (Rect, Kind) {
        (r, Kind::Item)
    }
    fn button(r: Rect) -> (Rect, Kind) {
        (r, Kind::Button)
    }
    fn scroll_bar(r: Rect) -> (Rect, Kind) {
        (r, Kind::ScrollBar)
    }

    #[test]
    fn a_button_inside_a_tab_or_row_keeps_its_own_label() {
        // Windows Terminal: each tab holds its close button.
        let (tab, close) = ((649, 9, 889, 41), (852, 13, 884, 37));
        assert_eq!(targets(vec![control(tab), button(close)]), vec![tab, close]);
    }

    #[test]
    fn scroll_bars_and_their_parts_get_no_label_and_do_not_hide_the_pane() {
        // Windows Terminal: the text area holds the scroll bar's arrows and track.
        let pane = (641, 41, 1919, 1039);
        let bar = (1903, 41, 1919, 1039);
        let (up, track, down) = ((1903, 41, 1919, 57), (1903, 57, 1919, 1023), (1903, 1023, 1919, 1039));
        let got = targets(vec![generic(pane), scroll_bar(bar), button(up), button(track), button(down)]);
        assert_eq!(got, vec![pane]);
    }


    #[test]
    fn the_wheel_aims_at_the_biggest_scroll_area_unless_the_pointer_is_on_one() {
        let rects = vec![((0, 0, 200, 900), 0.0), ((200, 100, 1000, 900), 40.0), ((1000, 100, 1300, 900), 60.0)];
        assert_eq!(aim(rects.clone(), (1100, 500)), None);
        assert_eq!(aim(rects.clone(), (500, 50)), Some((600, 500)));
        assert_eq!(aim(rects.clone(), (100, 500)), Some((600, 500)));
        assert_eq!(aim(vec![], (100, 500)), None);
    }
    #[test]
    fn few_labels_are_single_letters() {
        assert_eq!(labels(3), vec!["a", "d", "s"]);
    }

    #[test]
    fn labels_are_unique_and_prefix_free() {
        for count in [1, 14, 15, 100, 300] {
            let all = labels(count);
            assert_eq!(all.len(), count);
            for (i, a) in all.iter().enumerate() {
                for (j, b) in all.iter().enumerate() {
                    if i != j {
                        assert!(!b.starts_with(a.as_str()), "{a} is a prefix of {b} (count {count})");
                    }
                }
            }
        }
    }

    #[test]
    fn labels_use_only_the_alphabet() {
        assert!(labels(200).iter().all(|l| l.chars().all(|c| ALPHABET.contains(c))));
    }

    #[test]
    fn nested_targets_keep_only_the_outer_one() {
        // A file row and its cells: one label for the row, not one per cell.
        let row = (0, 0, 500, 20);
        let (name, date) = ((20, 0, 200, 20), (200, 0, 350, 20));
        let other_row = (0, 20, 500, 40);
        assert_eq!(targets(vec![generic(name), control(row), generic(date), control(other_row)]), vec![row, other_row]);
    }

    #[test]
    fn a_generic_box_around_buttons_gives_way_to_them() {
        // Web apps wrap buttons in clickable boxes: label New and Import, not the box.
        let toolbar = (0, 0, 200, 30);
        let (new, import) = ((10, 5, 60, 25), (70, 5, 130, 25));
        assert_eq!(targets(vec![generic(toolbar), control(new), control(import)]), vec![new, import]);
    }

    #[test]
    fn overlapping_targets_that_are_not_nested_get_one_label() {
        // Mattermost: an avatar button and its status badge poking out of it; labels stacked unreadably.
        let (avatar, badge) = ((1300, 90, 1332, 122), (1316, 106, 1336, 126));
        assert_eq!(targets(vec![control(avatar), control(badge)]), vec![avatar]);
        // Rows that only touch keep both.
        let (a, b) = ((0, 0, 300, 20), (0, 20, 300, 40));
        assert_eq!(targets(vec![control(a), control(b)]), vec![a, b]);
    }

    #[test]
    fn a_label_that_would_cover_another_on_the_same_thing_is_dropped() {
        // Excel's title bar: the maximize button and a part of it 17 px off its corner.
        let (maximize, part) = ((1770, 5, 1816, 35), (1787, 10, 1810, 30));
        assert_eq!(unstack(vec![maximize, part], &[(22, 15), (22, 15)]), vec![maximize]);
        // Bold and Italic sit 20 px apart: the labels touch, but they're different buttons.
        let (bold, italic) = ((160, 100, 180, 120), (180, 100, 200, 120));
        assert_eq!(unstack(vec![bold, italic], &[(22, 15), (22, 15)]), vec![bold, italic]);
    }

    #[test]
    fn a_generic_box_with_nothing_inside_keeps_its_label() {
        let card = (0, 0, 200, 30);
        assert_eq!(targets(vec![generic(card)]), vec![card]);
    }

    #[test]
    fn text_inside_a_button_gets_no_label_of_its_own() {
        let (button, text) = ((0, 0, 100, 30), (20, 8, 80, 22));
        assert_eq!(targets(vec![control(button), generic(text)]), vec![button]);
    }

    #[test]
    fn items_nested_in_an_open_tree_item_get_labels_and_the_parent_keeps_its_own_row() {
        // GitHub's file tree: an open folder's box covers its children.
        let folder = (0, 0, 300, 90);
        let (child, grandchild_folder, grandchild) = ((20, 30, 300, 60), (20, 60, 300, 90), (40, 60, 300, 90));
        let got = targets(vec![item(folder), item(child), item(grandchild_folder), generic(grandchild)]);
        assert_eq!(got, vec![(0, 0, 120, 30), (20, 30, 140, 60), (20, 60, 140, 90)]);
    }

    #[test]
    fn a_wide_item_row_is_clicked_near_its_name_not_its_middle() {
        // Explorer's details rows run past the visible columns: their middle can be off screen.
        let row = (313, 241, 3000, 262);
        assert_eq!(targets(vec![item(row)]), vec![(313, 241, 313 + 4 * 21, 262)]);
    }

    #[test]
    fn near_duplicates_get_one_target() {
        let button = (10, 10, 50, 30);
        let inner = (12, 11, 49, 29);
        assert_eq!(targets(vec![control(button), control(inner)]).len(), 1);
    }

    #[test]
    fn targets_are_sorted_top_left_first() {
        let (a, b, c) = ((300, 10, 340, 30), (10, 10, 50, 30), (10, 50, 50, 70));
        assert_eq!(targets(vec![control(c), control(a), control(b)]), vec![b, a, c]);
    }

    #[test]
    fn the_cap_applies_after_filtering() {
        // 400 nested pairs: 800 rectangles in, 400 targets, 300 labels.
        let rects =
            (0..400).flat_map(|i| [control((0, i * 30, 100, i * 30 + 20)), generic((10, i * 30 + 2, 50, i * 30 + 18))]).collect();
        assert_eq!(targets(rects).len(), MAX_LABELS);
    }

    #[test]
    fn empty_rectangles_are_dropped() {
        assert!(targets(vec![control((10, 10, 10, 30))]).is_empty());
    }

    #[test]
    fn scroll_areas_keep_nested_ones_and_drop_duplicates_and_empties() {
        let page = (0, 0, 800, 600);
        let panel = (500, 100, 780, 580);
        let got = areas(vec![(panel, 40.0), (page, 20.0), ((2, 1, 801, 600), 20.0), ((10, 10, 10, 50), 20.0)]);
        assert_eq!(got, vec![page, panel]);
    }

    #[test]
    fn small_areas_and_areas_with_almost_nothing_hidden_get_no_label() {
        let prompt_box = (700, 900, 1500, 960);
        let table = (700, 800, 1500, 1000);
        let chat = (340, 130, 1900, 990);
        assert_eq!(areas(vec![(prompt_box, 50.0), (table, 98.0), (chat, 10.0)]), vec![chat]);
    }

    #[test]
    fn zero_is_empty() {
        assert!(labels(0).is_empty());
    }
}
