//! Hint labels for clickable elements (Vimium style): short, home-row first, and prefix-free,
//! so typing a full label never also completes a shorter one.

pub const ALPHABET: &str = "sadfjklewcmpgh";

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
    fn zero_is_empty() {
        assert!(labels(0).is_empty());
    }
}
