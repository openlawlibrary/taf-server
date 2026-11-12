//! Utility enum representation for the status of a document or library.

/// Enum representation for the status of a document or library.
///
/// Note that the declaration order below is **not** the integer order used by
/// [`Status::to_int`]: the stored integers are `added = 0`, `effective = 1`,
/// `changed = 2`, `removed = 3`. Each variant's doc comment states its own
/// stored integer.
pub enum Status {
    /// 0 - Element added.
    ElementAdded,
    /// 2 - Element changed.
    ElementChanged,
    /// 3 - Element removed.
    ElementRemoved,
    /// 1 - Element effective.
    ElementEffective,
}

impl Status {
    /// Convert a string to a `Status` enum.
    /// # Errors
    /// Returns an error if the string is not a valid status value.
    pub fn from_string(status: &str) -> anyhow::Result<Self> {
        match status {
            "Element added" => Ok(Self::ElementAdded),
            "Element changed" => Ok(Self::ElementChanged),
            "Element removed" => Ok(Self::ElementRemoved),
            "Element effective" => Ok(Self::ElementEffective),
            _ => Err(anyhow::anyhow!("Invalid status value")),
        }
    }

    /// Convert a `Status` enum to an integer.
    #[must_use]
    pub const fn to_int(&self) -> i64 {
        match *self {
            Self::ElementAdded => 0,
            Self::ElementEffective => 1,
            Self::ElementChanged => 2,
            Self::ElementRemoved => 3,
        }
    }

    /// Convert the integer stored in `document_change.status` back into a `Status`.
    ///
    /// Returns `None` for values outside `0..=3`, which indicates corrupt data.
    /// Mirrors [`Status::to_int`] exactly — note that the mapping is *not* the
    /// enum's declaration order.
    #[must_use]
    pub const fn from_int(value: i64) -> Option<Self> {
        match value {
            0 => Some(Self::ElementAdded),
            1 => Some(Self::ElementEffective),
            2 => Some(Self::ElementChanged),
            3 => Some(Self::ElementRemoved),
            _ => None,
        }
    }

    /// Stable lowercase token used in JSON API responses.
    ///
    /// Deliberately not the RDF phrasing (`"Element added"`): the RDF wording is
    /// an ingest detail and the frontend renders its own copy.
    #[must_use]
    pub const fn as_api_str(&self) -> &'static str {
        match *self {
            Self::ElementAdded => "added",
            Self::ElementEffective => "effective",
            Self::ElementChanged => "changed",
            Self::ElementRemoved => "removed",
        }
    }
}

#[cfg(test)]
#[expect(clippy::inline_modules, reason = "tests")]
mod tests {
    use super::Status;

    #[test]
    fn from_int_round_trips_to_int_for_every_variant() {
        for value in 0_i64..4_i64 {
            let status = Status::from_int(value);
            assert!(status.is_some(), "from_int({value}) should be Some");
            assert_eq!(
                status.map(|found| found.to_int()),
                Some(value),
                "from_int({value}).to_int() must round-trip"
            );
        }
    }

    #[test]
    fn from_int_rejects_out_of_range_values() {
        assert!(Status::from_int(4).is_none());
        assert!(Status::from_int(-1).is_none());
        assert!(Status::from_int(7).is_none());
    }

    #[test]
    fn as_api_str_matches_the_integer_mapping_not_declaration_order() {
        assert_eq!(Status::ElementAdded.as_api_str(), "added");
        assert_eq!(Status::ElementEffective.as_api_str(), "effective");
        assert_eq!(Status::ElementChanged.as_api_str(), "changed");
        assert_eq!(Status::ElementRemoved.as_api_str(), "removed");
        assert_eq!(
            Status::from_int(1).map(|found| found.as_api_str()),
            Some("effective")
        );
        assert_eq!(
            Status::from_int(2).map(|found| found.as_api_str()),
            Some("changed")
        );
    }
}
