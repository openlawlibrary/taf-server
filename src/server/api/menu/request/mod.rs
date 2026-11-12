//! Request types for the menu endpoint.
use serde::Deserialize;

use super::{DEFAULT_MENU_DEPTH, MAX_MENU_DEPTH, MIN_MENU_DEPTH};

/// Path parameters for the menu endpoint. All optional, because one handler
/// serves routes carrying different subsets of them.
#[derive(Deserialize, Debug)]
pub struct Menu {
    /// Publication name, from the `_publication/{publication}` segment.
    pub publication: Option<String>,
    /// Version date, from the `_date/{date}` segment. `YYYY-MM-DD` or `current`.
    pub date: Option<String>,
    /// Path to the document/collection the menu is rooted at.
    pub path: Option<String>,
}

/// Query parameters for the menu endpoint.
///
/// `depth` is a query parameter because `{path:.*}` consumes the whole tail. A
/// non-numeric value fails extraction and actix answers 400; an out-of-range
/// one is clamped, and the response echoes both served and requested depth.
#[derive(Deserialize, Debug)]
pub struct Depth {
    /// Relative levels below the root the client asked for.
    pub depth: Option<u32>,
}

impl Depth {
    /// Levels actually served, clamped into `[MIN_MENU_DEPTH, MAX_MENU_DEPTH]`.
    #[must_use]
    pub fn resolved(&self) -> usize {
        self.depth.map_or(DEFAULT_MENU_DEPTH, |requested| {
            usize::try_from(requested)
                .unwrap_or(DEFAULT_MENU_DEPTH)
                .clamp(MIN_MENU_DEPTH, MAX_MENU_DEPTH)
        })
    }

    /// The raw depth the client asked for, echoed back unchanged.
    #[must_use]
    pub fn requested(&self) -> u32 {
        self.depth
            .unwrap_or_else(|| u32::try_from(DEFAULT_MENU_DEPTH).unwrap_or(2))
    }
}

#[cfg(test)]
#[expect(clippy::inline_modules, reason = "tests")]
mod tests {
    use super::Depth;

    /// Build a `Depth` carrying the given raw query value.
    const fn depth(value: Option<u32>) -> Depth {
        Depth { depth: value }
    }

    #[test]
    fn absent_depth_defaults_to_two() {
        assert_eq!(depth(None).resolved(), 2);
        assert_eq!(depth(None).requested(), 2);
    }

    #[test]
    fn in_range_depths_are_served_as_asked() {
        for value in 1_u32..7_u32 {
            let parsed = depth(Some(value));
            assert_eq!(parsed.resolved(), usize::try_from(value).unwrap_or(0));
            assert_eq!(parsed.requested(), value);
        }
    }

    #[test]
    fn zero_is_clamped_up_and_the_request_is_still_echoed() {
        let parsed = depth(Some(0));
        assert_eq!(parsed.resolved(), 1);
        assert_eq!(parsed.requested(), 0);
    }

    #[test]
    fn oversized_depths_are_clamped_down_and_the_request_is_still_echoed() {
        let parsed = depth(Some(99));
        assert_eq!(parsed.resolved(), 6);
        assert_eq!(parsed.requested(), 99);

        let huge = depth(Some(u32::MAX));
        assert_eq!(huge.resolved(), 6);
        assert_eq!(huge.requested(), u32::MAX);
    }
}
