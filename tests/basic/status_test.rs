//! `Status` integer mapping. The enum's declaration order does NOT match
//! `to_int()`, and the per-variant doc comments disagree with both.

use taf_server::db::models::status::Status;

/// `from_int` is the exact inverse of `to_int`, and rejects anything else.
#[test]
fn test_status_from_int_round_trips_to_int() {
    for value in 0..4_i64 {
        let status =
            Status::from_int(value).unwrap_or_else(|| panic!("from_int({value}) should be Some"));
        assert_eq!(status.to_int(), value);
    }
    for value in [-1_i64, 4, 7, i64::MAX] {
        assert!(Status::from_int(value).is_none(), "from_int({value})");
    }
}
