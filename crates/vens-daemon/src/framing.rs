/// How a request delimits its body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framing {
    Length(u64),
    Chunked,
}

/// Why a request's body has no single delimitation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FramingError {
    /// Both a length and a transfer coding delimit the body.
    Conflicting,
    InvalidLength,
    /// The transfer codings don't end in exactly one chunked coding.
    Unframed,
    /// A coding other than chunked applies to the body.
    UnsupportedCoding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coding {
    Chunked,
    Other,
}

#[derive(Debug, Clone, Copy, Default)]
struct Codings {
    last: Option<Coding>,
    chunked_before: bool,
    other_before: bool,
}

impl Codings {
    fn then(self, coding: Coding) -> Self {
        Self {
            last: Some(coding),
            chunked_before: self.chunked_before || self.last == Some(Coding::Chunked),
            other_before: self.other_before || self.last == Some(Coding::Other),
        }
    }
}

fn elements(value: &[u8]) -> impl Iterator<Item = &[u8]> {
    value.split(|byte| *byte == b',').map(<[u8]>::trim_ascii)
}

fn decimal(digits: &[u8]) -> Option<u64> {
    if digits.is_empty() {
        return None;
    }
    digits.iter().try_fold(0_u64, |total, digit| {
        let value = char::from(*digit).to_digit(10)?;
        total.checked_mul(10)?.checked_add(u64::from(value))
    })
}

/// Decides how the request's headers delimit its body.
///
/// # Errors
///
/// Refuses a malformed or ambiguous length, a body framed two ways, and transfer codings other than one final chunked coding.
pub fn framing<'a>(
    headers: impl IntoIterator<Item = (&'a str, &'a [u8])>,
) -> Result<Framing, FramingError> {
    let mut length = None;
    let mut codings = Codings::default();
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("content-length") {
            for element in elements(value) {
                let found = decimal(element).ok_or(FramingError::InvalidLength)?;
                if length.is_some_and(|known| known != found) {
                    return Err(FramingError::InvalidLength);
                }
                length = Some(found);
            }
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            for element in elements(value).filter(|element| !element.is_empty()) {
                let coding = if element.eq_ignore_ascii_case(b"chunked") {
                    Coding::Chunked
                } else {
                    Coding::Other
                };
                codings = codings.then(coding);
            }
        }
    }
    match (length, codings.last) {
        (Some(_), Some(_)) => Err(FramingError::Conflicting),
        (Some(length), None) => Ok(Framing::Length(length)),
        (None, None) => Ok(Framing::Length(0)),
        (None, Some(Coding::Other)) => Err(FramingError::Unframed),
        (None, Some(Coding::Chunked)) if codings.chunked_before => Err(FramingError::Unframed),
        (None, Some(Coding::Chunked)) if codings.other_before => {
            Err(FramingError::UnsupportedCoding)
        }
        (None, Some(Coding::Chunked)) => Ok(Framing::Chunked),
    }
}
