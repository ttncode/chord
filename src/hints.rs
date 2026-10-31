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

/// The rectangles worth a label, top-left first. Each comes with whether it is a real control
/// (button, link, list row…) rather than a generic clickable box. Containers (lists, panes) are
/// already left out by type. Anything inside a real control is a part of it (a file row's cells, a
/// button's text) and gets no label; a generic box holding other clickable things gives way to them
/// (Vimium's rule for web pages). Near-duplicates get one.
pub fn targets(elements: Vec<(Rect, bool)>) -> Vec<Rect> {
    let same = |a: Rect, b: Rect| {
        (a.0 - b.0).abs() <= SAME && (a.1 - b.1).abs() <= SAME && (a.2 - b.2).abs() <= SAME && (a.3 - b.3).abs() <= SAME
    };
    let contains = |a: Rect, b: Rect| a.0 <= b.0 && a.1 <= b.1 && a.2 >= b.2 && a.3 >= b.3 && !same(a, b);
    let elements: Vec<(Rect, bool)> = elements.into_iter().filter(|(r, _)| r.2 > r.0 && r.3 > r.1).collect();
    let mut kept: Vec<Rect> = Vec::new();
    for &(r, control) in &elements {
        let part_of_a_control = elements.iter().any(|&(outer, c)| c && contains(outer, r));
        let box_around_others = !control && elements.iter().any(|&(inner, _)| contains(r, inner));
        if !part_of_a_control && !box_around_others && !kept.iter().any(|&k| same(k, r)) {
            kept.push(r);
        }
    }
    kept.sort_by_key(|r| (r.1, r.0));
    kept.truncate(MAX_LABELS);
    kept
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

    fn control(r: Rect) -> (Rect, bool) {
        (r, true)
    }
    fn generic(r: Rect) -> (Rect, bool) {
        (r, false)
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
    fn zero_is_empty() {
        assert!(labels(0).is_empty());
    }
}
