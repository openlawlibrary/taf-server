//! Pure materialized-path helpers for the menu endpoint.
//!
//! An mpath is a `|`-delimited, `|`-terminated string such as
//! `City of San Mateo, Cal., Code|10|10.04|`; its depth is the delimiter count.

/// Number of `|`-delimited segments in a materialized path.
#[must_use]
#[expect(
    clippy::module_name_repetitions,
    reason = "`mpath_depth` reads correctly at every call site; a bare `depth` would collide with the menu depth parameter"
)]
pub fn mpath_depth(mpath: &str) -> usize {
    mpath.matches('|').count()
}

/// Prefix of `mpath` holding exactly `depth` segments, saturating at `mpath`.
/// `truncate_to_depth("a|b|c|", 2) == "a|b|"`.
#[must_use]
pub fn truncate_to_depth(mpath: &str, depth: usize) -> String {
    mpath.split_inclusive('|').take(depth).collect()
}

/// Last `|`-delimited segment. `"a|b|c|" -> "c"`, `"a|" -> "a"`.
#[must_use]
pub fn last_segment(mpath: &str) -> &str {
    mpath
        .trim_end_matches('|')
        .rsplit('|')
        .next()
        .unwrap_or(mpath)
}

/// Exclusive upper bound of the strings prefixed by `base`, or `None` when
/// `base` does not end with `|`.
///
/// Replacing the trailing `|` (0x7C) with `}` (0x7D) is the exact successor of
/// the prefix set for arbitrary UTF-8.
#[must_use]
pub fn prefix_upper_bound(base: &str) -> Option<String> {
    base.strip_suffix('|').map(|stem| format!("{stem}}}"))
}

#[cfg(test)]
#[expect(clippy::inline_modules, reason = "tests")]
mod tests {
    use super::{last_segment, mpath_depth, prefix_upper_bound, truncate_to_depth};

    #[test]
    fn mpath_depth_counts_delimiters() {
        assert_eq!(mpath_depth("City of San Mateo, Cal., Code|"), 1);
        assert_eq!(mpath_depth("City of San Mateo, Cal., Code|10|"), 2);
        assert_eq!(mpath_depth("City of San Mateo, Cal., Code|10|10.04|"), 3);
        assert_eq!(
            mpath_depth("Code of Maryland Regulations|19A|04|attachments|A|appendix-a|"),
            6
        );
        assert_eq!(mpath_depth("1997 SMTCA 1|"), 1);
        assert_eq!(mpath_depth(""), 0);
    }

    #[test]
    fn truncate_to_depth_keeps_whole_segments() {
        let mpath = "City of San Mateo, Cal., Code|27|27.21|II|";
        assert_eq!(
            truncate_to_depth(mpath, 1),
            "City of San Mateo, Cal., Code|"
        );
        assert_eq!(
            truncate_to_depth(mpath, 2),
            "City of San Mateo, Cal., Code|27|"
        );
        assert_eq!(
            truncate_to_depth(mpath, 3),
            "City of San Mateo, Cal., Code|27|27.21|"
        );
        assert_eq!(truncate_to_depth(mpath, 4), mpath);
    }

    #[test]
    fn truncate_to_depth_saturates_and_handles_zero() {
        assert_eq!(truncate_to_depth("a|b|", 0), "");
        assert_eq!(truncate_to_depth("a|b|", 9), "a|b|");
    }

    #[test]
    fn truncate_to_depth_preserves_empty_segments() {
        // A leading-dot Maryland segment, and a segment that is itself empty,
        // must both survive truncation unchanged.
        assert_eq!(truncate_to_depth("Code|01|.11|", 3), "Code|01|.11|");
        assert_eq!(truncate_to_depth("Code||x|", 2), "Code||");
    }

    #[test]
    fn last_segment_returns_the_trailing_component() {
        assert_eq!(last_segment("a|b|c|"), "c");
        assert_eq!(last_segment("a|"), "a");
        assert_eq!(
            last_segment("City of San Mateo, Cal., Code|"),
            "City of San Mateo, Cal., Code"
        );
        assert_eq!(last_segment("Code of Maryland Regulations|01|.11|"), ".11");
        assert_eq!(last_segment("no-delimiter"), "no-delimiter");
        assert_eq!(last_segment(""), "");
    }

    #[test]
    fn prefix_upper_bound_is_the_exclusive_successor() {
        assert_eq!(prefix_upper_bound("a|b|"), Some("a|b}".to_owned()));
        assert_eq!(prefix_upper_bound("Code|"), Some("Code}".to_owned()));
        assert_eq!(prefix_upper_bound("not-an-mpath"), None);
    }

    #[test]
    fn prefix_upper_bound_brackets_every_descendant() {
        let base = "Code|27|";
        let upper = prefix_upper_bound(base).unwrap_or_default();
        for descendant in [
            "Code|27|",
            "Code|27|27.21|",
            "Code|27|27.21|II|27.21.100|",
            "Code|27|\u{10ffff}|",
        ] {
            assert!(descendant >= base, "{descendant} must be >= {base}");
            assert!(
                descendant < upper.as_str(),
                "{descendant} must be < {upper}"
            );
        }
        // A sibling that merely shares the parent must fall outside the range.
        assert!("Code|270|" >= upper.as_str() || "Code|270|" < base);
    }
}
