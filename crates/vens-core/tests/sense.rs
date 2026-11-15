//! The conditions follow section 8.7 of the ix1300-wifi protocol document.
//! The byte positions are those of fixed-format sense data in the SCSI Primary Commands standard.

use vens_core::sense::{Condition, SenseError, decode};

const LENGTH: usize = 18;
const END_OF_MEDIUM: u8 = 0x40;

fn sense(flags_and_key: u8, code: u8, qualifier: u8) -> [u8; LENGTH] {
    let mut data = [0_u8; LENGTH];
    let fields = [(0, 0x70), (2, flags_and_key), (12, code), (13, qualifier)];
    for (offset, value) in fields {
        if let Some(slot) = data.get_mut(offset) {
            *slot = value;
        }
    }
    data
}

#[test]
fn the_documented_conditions_are_named() {
    let conditions = [
        (sense(END_OF_MEDIUM, 0, 0), Condition::EndOfSide),
        (sense(3, 0x80, 0x01), Condition::PaperJam),
        (sense(3, 0x80, 0x02), Condition::CoverOpen),
        (sense(3, 0x80, 0x03), Condition::NoPaper),
        (sense(3, 0x80, 0x07), Condition::DoubleFeed),
        (sense(3, 0x80, 0x13), Condition::NoDataYet),
        (sense(5, 0x24, 0x00), Condition::InvalidField),
    ];
    for (data, condition) in conditions {
        assert_eq!(decode(&data), Ok(condition));
    }
}

#[test]
fn an_undocumented_condition_keeps_its_codes() {
    assert_eq!(
        decode(&sense(4, 0x44, 0x00)),
        Ok(Condition::Unrecognized {
            key: 4,
            code: 0x44,
            qualifier: 0x00,
        })
    );
}

#[test]
fn a_clear_key_without_end_of_medium_is_not_the_end_of_a_side() {
    assert_eq!(
        decode(&sense(0, 0, 0)),
        Ok(Condition::Unrecognized {
            key: 0,
            code: 0,
            qualifier: 0,
        })
    );
}

#[test]
fn end_of_medium_with_an_error_key_reports_the_error() {
    assert_eq!(
        decode(&sense(END_OF_MEDIUM | 3, 0x80, 0x01)),
        Ok(Condition::PaperJam)
    );
}

#[test]
fn sense_data_shorter_than_the_qualifier_is_refused() {
    let data = sense(3, 0x80, 0x03);
    assert_eq!(
        decode(data.get(..13).unwrap_or_default()),
        Err(SenseError::Truncated {
            expected: 14,
            actual: 13,
        })
    );
}
