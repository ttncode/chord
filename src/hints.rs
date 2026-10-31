//! Hint labels for clickable elements (Vimium style): short, home-row first, and prefix-free,
//! so typing a full label never also completes a shorter one.

pub const ALPHABET: &str = "sadfjklewcmpgh";

/// Screen rectangle: left, top, right, bottom.
pub type Rect = (i32, i32, i32, i32);

/// Element rectangles closer than this (pixels) on every edge are the same target.
const SAME: i32 = 6;

/// The rectangles worth a label, top-left first: an element holding another clickable element is a
/// container (list, pane, toolbar), and only its contents get labels; near-duplicates get one.
pub fn targets(rects: Vec<Rect>) -> Vec<Rect> {
    let same = |a: Rect, b: Rect| {
        (a.0 - b.0).abs() <= SAME && (a.1 - b.1).abs() <= SAME && (a.2 - b.2).abs() <= SAME && (a.3 - b.3).abs() <= SAME
    };
    let contains = |a: Rect, b: Rect| a.0 <= b.0 && a.1 <= b.1 && a.2 >= b.2 && a.3 >= b.3 && !same(a, b);
    let rects: Vec<Rect> = rects.into_iter().filter(|r| r.2 > r.0 && r.3 > r.1).collect();
    let mut kept: Vec<Rect> = Vec::new();
    for &r in &rects {
        if !rects.iter().any(|&inner| contains(r, inner)) && !kept.iter().any(|&k| same(k, r)) {
            kept.push(r);
        }
    }
    kept.sort_by_key(|r| (r.1, r.0));
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
    fn containers_are_dropped_and_their_contents_kept() {
        let list = (0, 0, 500, 400);
        let item = (10, 10, 200, 30);
        let other = (10, 40, 200, 60);
        assert_eq!(targets(vec![list, item, other]), vec![item, other]);
    }

    #[test]
    fn near_duplicates_get_one_target() {
        let button = (10, 10, 50, 30);
        let inner = (12, 11, 49, 29);
        assert_eq!(targets(vec![button, inner]).len(), 1);
    }

    #[test]
    fn targets_are_sorted_top_left_first() {
        let (a, b, c) = ((300, 10, 340, 30), (10, 10, 50, 30), (10, 50, 50, 70));
        assert_eq!(targets(vec![c, a, b]), vec![b, a, c]);
    }

    #[test]
    fn empty_rectangles_are_dropped() {
        assert!(targets(vec![(10, 10, 10, 30)]).is_empty());
    }

    #[test]
    fn zero_is_empty() {
        assert!(labels(0).is_empty());
    }
}
