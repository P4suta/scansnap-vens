use crate::wire::field;

const REQUIRED_LENGTH: usize = 14;
const FLAGS_AND_KEY: usize = 2;
const CODE: usize = 12;
const QUALIFIER: usize = 13;
const KEY_MASK: u8 = 0x0F;
const END_OF_MEDIUM: u8 = 0x40;

const NO_SENSE: u8 = 0;
const MEDIUM_ERROR: u8 = 3;
const ILLEGAL_REQUEST: u8 = 5;
const VENDOR_PAPER: u8 = 0x80;
const JAM: u8 = 0x01;
const COVER_OPEN: u8 = 0x02;
const NO_PAPER: u8 = 0x03;
const DOUBLE_FEED: u8 = 0x07;
const NO_DATA_YET: u8 = 0x13;
const INVALID_FIELD: u8 = 0x24;
const NO_QUALIFIER: u8 = 0x00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Condition {
    EndOfSide,
    PaperJam,
    CoverOpen,
    NoPaper,
    DoubleFeed,
    NoDataYet,
    InvalidField,
    Unrecognized { key: u8, code: u8, qualifier: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SenseError {
    Truncated { expected: usize, actual: usize },
}

/// # Errors
///
/// Refuses sense data that ends before the additional sense code qualifier.
pub fn decode(data: &[u8]) -> Result<Condition, SenseError> {
    let truncated = SenseError::Truncated {
        expected: REQUIRED_LENGTH,
        actual: data.len(),
    };
    let [flags_and_key] = field(data, FLAGS_AND_KEY).ok_or(truncated)?;
    let [code] = field(data, CODE).ok_or(truncated)?;
    let [qualifier] = field(data, QUALIFIER).ok_or(truncated)?;
    let key = flags_and_key & KEY_MASK;
    let end_of_medium = flags_and_key & END_OF_MEDIUM != 0;
    Ok(match (key, code, qualifier) {
        (NO_SENSE, _, _) if end_of_medium => Condition::EndOfSide,
        (MEDIUM_ERROR, VENDOR_PAPER, JAM) => Condition::PaperJam,
        (MEDIUM_ERROR, VENDOR_PAPER, COVER_OPEN) => Condition::CoverOpen,
        (MEDIUM_ERROR, VENDOR_PAPER, NO_PAPER) => Condition::NoPaper,
        (MEDIUM_ERROR, VENDOR_PAPER, DOUBLE_FEED) => Condition::DoubleFeed,
        (MEDIUM_ERROR, VENDOR_PAPER, NO_DATA_YET) => Condition::NoDataYet,
        (ILLEGAL_REQUEST, INVALID_FIELD, NO_QUALIFIER) => Condition::InvalidField,
        _ => Condition::Unrecognized {
            key,
            code,
            qualifier,
        },
    })
}
