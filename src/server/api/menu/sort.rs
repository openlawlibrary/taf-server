//! Natural ("human") ordering for menu siblings, keyed on the last mpath
//! segment.
//!
//! Digit runs are compared by significant-digit count then digit-by-digit,
//! never parsed: that is numeric comparison for non-negative integers with no
//! overflow mode, which keeps the comparator infallible.

use std::cmp::Ordering;
use std::iter::Peekable;
use std::str::Chars;

use super::mpath::last_segment;

/// Compare two strings in natural order: maximal digit and non-digit runs
/// pairwise, digits before non-digits, then run count, then a plain byte
/// comparison that makes this a **total order**.
#[must_use]
pub fn natural_cmp(left: &str, right: &str) -> Ordering {
    let mut left_chars = left.chars().peekable();
    let mut right_chars = right.chars().peekable();
    let mut left_runs: usize = 0;
    let mut right_runs: usize = 0;

    while let (Some(&left_head), Some(&right_head)) = (left_chars.peek(), right_chars.peek()) {
        let ordering = match (left_head.is_ascii_digit(), right_head.is_ascii_digit()) {
            (true, true) => compare_digit_runs(&mut left_chars, &mut right_chars),
            (false, false) => compare_text_runs(&mut left_chars, &mut right_chars),
            // Digit runs sort before non-digit runs.
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
        left_runs += 1;
        right_runs += 1;
    }

    left_runs += count_remaining_runs(&mut left_chars);
    right_runs += count_remaining_runs(&mut right_chars);
    left_runs.cmp(&right_runs).then_with(|| left.cmp(right))
}

/// Order materialized paths by [`natural_cmp`] over their last segments.
#[must_use]
pub fn sorted_siblings(siblings: &[String]) -> Vec<String> {
    let mut ordered = siblings.to_vec();
    ordered.sort_by(|left, right| natural_cmp(last_segment(left), last_segment(right)));
    ordered
}

/// Consume one digit run from each iterator and order the two numbers. Leading
/// zeros are skipped first, so the remaining digit count is the magnitude.
fn compare_digit_runs(left: &mut Peekable<Chars<'_>>, right: &mut Peekable<Chars<'_>>) -> Ordering {
    skip_leading_zeros(left);
    skip_leading_zeros(right);
    let mut first_difference = Ordering::Equal;
    loop {
        let left_digit = left.next_if(char::is_ascii_digit);
        let right_digit = right.next_if(char::is_ascii_digit);
        match (left_digit, right_digit) {
            (Some(left_char), Some(right_char)) => {
                if first_difference == Ordering::Equal {
                    first_difference = left_char.cmp(&right_char);
                }
            }
            (Some(_), None) => {
                consume_run(left, true);
                return Ordering::Greater;
            }
            (None, Some(_)) => {
                consume_run(right, true);
                return Ordering::Less;
            }
            (None, None) => return first_difference,
        }
    }
}

/// Consume one non-digit run from each iterator and order them by code point.
fn compare_text_runs(left: &mut Peekable<Chars<'_>>, right: &mut Peekable<Chars<'_>>) -> Ordering {
    loop {
        let left_char = left.next_if(|found| !found.is_ascii_digit());
        let right_char = right.next_if(|found| !found.is_ascii_digit());
        match (left_char, right_char) {
            (Some(left_found), Some(right_found)) => {
                let ordering = left_found.cmp(&right_found);
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return Ordering::Equal,
        }
    }
}

/// Advance past every leading `'0'` of the digit run under the cursor.
fn skip_leading_zeros(chars: &mut Peekable<Chars<'_>>) {
    while chars.next_if(|found| *found == '0').is_some() {}
}

/// Advance past the remainder of the run under the cursor.
fn consume_run(chars: &mut Peekable<Chars<'_>>, digits: bool) {
    while chars
        .next_if(|found| found.is_ascii_digit() == digits)
        .is_some()
    {}
}

/// Count how many whole runs are left in an iterator, consuming it.
fn count_remaining_runs(chars: &mut Peekable<Chars<'_>>) -> usize {
    let mut runs: usize = 0;
    while let Some(&head) = chars.peek() {
        consume_run(chars, head.is_ascii_digit());
        runs += 1;
    }
    runs
}

#[cfg(test)]
#[expect(clippy::inline_modules, reason = "tests")]
mod tests {
    use super::{natural_cmp, sorted_siblings};
    use std::cmp::Ordering;

    /// Sort a slice of bare segments with the comparator under test.
    fn sorted(input: &[&str]) -> Vec<String> {
        let mut values: Vec<String> = input.iter().map(|found| (*found).to_owned()).collect();
        values.sort_by(|left, right| natural_cmp(left, right));
        values
    }

    #[test]
    fn san_mateo_titles_sort_numerically() {
        assert_eq!(sorted(&["27", "10", "2", "1"]), ["1", "2", "10", "27"]);
    }

    #[test]
    fn san_mateo_chapters_compare_fraction_runs_by_magnitude() {
        assert_eq!(natural_cmp("10.04", "10.100"), Ordering::Less);
        assert_eq!(
            sorted(&["10.100", "10.04", "10.4"]),
            ["10.04", "10.4", "10.100"]
        );
    }

    #[test]
    fn leading_zero_ties_fall_through_to_the_byte_tie_break() {
        assert_eq!(natural_cmp("1.01.010", "1.1.10"), Ordering::Less);
        assert_eq!(natural_cmp("01", "1"), Ordering::Less);
        assert_eq!(natural_cmp("1", "01"), Ordering::Greater);
        assert_eq!(natural_cmp("0", "00"), Ordering::Less);
    }

    #[test]
    fn maryland_segments_ignore_leading_zeros_for_magnitude() {
        assert_eq!(
            sorted(&["1972", "02", "1971", "01"]),
            ["01", "02", "1971", "1972"]
        );
    }

    #[test]
    fn maryland_leading_dot_segments_sort_after_bare_numerics() {
        assert_eq!(sorted(&[".11", ".2", ".10"]), [".2", ".10", ".11"]);
        // A non-digit run first loses to a digit run first.
        assert_eq!(natural_cmp("11", ".11"), Ordering::Less);
        assert_eq!(sorted(&[".11", "11"]), ["11", ".11"]);
    }

    #[test]
    fn roman_numerals_sort_after_numerics_and_are_not_interpreted() {
        assert_eq!(sorted(&["II", "10", "2", "1"]), ["1", "2", "10", "II"]);
        assert_eq!(natural_cmp("II", "III"), Ordering::Less);
        // Documented limitation: Roman numerals are plain text, not numbers.
        assert_eq!(natural_cmp("IX", "V"), Ordering::Less);
    }

    #[test]
    fn comparator_is_a_total_order_so_sorting_is_stable_under_input_order() {
        let forward = sorted(&["1", "2", "10", "27", "II", ".11", "01"]);
        let reverse = sorted(&["01", ".11", "II", "27", "10", "2", "1"]);
        assert_eq!(forward, reverse);
    }

    #[test]
    fn comparator_is_reflexive_antisymmetric_and_consistent() {
        let values = [
            "1", "01", "2", "10", "10.04", "10.100", "II", "III", ".2", ".11", "1971", "",
        ];
        for left in values {
            assert_eq!(natural_cmp(left, left), Ordering::Equal);
            for right in values {
                assert_eq!(
                    natural_cmp(left, right).reverse(),
                    natural_cmp(right, left),
                    "antisymmetry broken for {left:?} / {right:?}"
                );
                if left != right {
                    assert_ne!(
                        natural_cmp(left, right),
                        Ordering::Equal,
                        "distinct keys {left:?} / {right:?} must not tie"
                    );
                }
            }
        }
    }

    #[test]
    fn text_runs_of_differing_length_compare_lexicographically() {
        assert_eq!(natural_cmp("Chapter", "Chapters"), Ordering::Less);
        assert_eq!(natural_cmp("appendix-a", "appendix-b"), Ordering::Less);
        assert_eq!(natural_cmp("attachments", "04"), Ordering::Greater);
    }

    #[test]
    fn sorted_siblings_keys_on_the_last_mpath_segment() {
        let siblings = vec![
            "Code|27|".to_owned(),
            "Code|10|".to_owned(),
            "Code|2|".to_owned(),
        ];
        assert_eq!(
            sorted_siblings(&siblings),
            ["Code|2|", "Code|10|", "Code|27|"]
        );
    }

    #[test]
    fn sorted_siblings_ignores_the_shared_prefix_entirely() {
        // Keyed on the segment, not the whole path: a long document-id prefix
        // must not drag everything into byte order.
        let siblings = vec![
            "City of San Mateo, Cal., Code|10|10.100|".to_owned(),
            "City of San Mateo, Cal., Code|10|10.04|".to_owned(),
        ];
        assert_eq!(
            sorted_siblings(&siblings),
            [
                "City of San Mateo, Cal., Code|10|10.04|",
                "City of San Mateo, Cal., Code|10|10.100|"
            ]
        );
    }

    #[test]
    fn sorted_siblings_does_not_reorder_the_input() {
        let siblings = vec!["Code|27|".to_owned(), "Code|10|".to_owned()];
        let ordered = sorted_siblings(&siblings);
        assert_eq!(siblings, ["Code|27|", "Code|10|"]);
        assert_eq!(ordered, ["Code|10|", "Code|27|"]);
    }
}
