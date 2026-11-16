use std::{fmt, fs, io, path::Path};

use vens_emulator::redact::{RedactError, redact};
use vens_net::trace::{self, TraceError};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CaptureError {
    Read(io::ErrorKind),
    Trace(TraceError),
    Redact(RedactError),
    Write(io::ErrorKind),
}

/// Parses a raw trace, redacts it, and renders the fixture text.
pub(crate) fn transform(raw: &str) -> Result<String, CaptureError> {
    let events = trace::parse(raw).map_err(CaptureError::Trace)?;
    let redacted = redact(&events).map_err(CaptureError::Redact)?;
    Ok(trace::render(&redacted))
}

/// Writes the redacted fixture of the raw trace at `raw` to `fixture`.
pub(crate) fn redact_capture(raw: &Path, fixture: &Path) -> Result<(), CaptureError> {
    let text = fs::read_to_string(raw).map_err(|error| CaptureError::Read(error.kind()))?;
    let redacted = transform(&text)?;
    fs::write(fixture, redacted).map_err(|error| CaptureError::Write(error.kind()))
}

impl fmt::Display for CaptureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(kind) => write!(formatter, "the raw trace can't be read: {kind}"),
            Self::Trace(error) => write!(
                formatter,
                "the raw trace is malformed at line {}: {:?}",
                error.line, error.fault
            ),
            Self::Redact(error) => write!(formatter, "the raw trace can't be redacted: {error:?}"),
            Self::Write(kind) => write!(formatter, "the fixture can't be written: {kind}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use vens_emulator::redact::RedactError;
    use vens_net::trace::{Fault, TraceError};

    use super::{CaptureError, transform};

    const GREETING: &str = "control < 0000001056454e530000000000000000\n";

    #[test]
    fn a_trace_without_identifiers_keeps_every_byte() {
        assert_eq!(transform(GREETING), Ok(GREETING.to_owned()));
    }

    #[test]
    fn a_malformed_trace_is_refused_with_its_line() {
        assert_eq!(
            transform("control < 00\ncontrol ? 00\n"),
            Err(CaptureError::Trace(TraceError {
                line: 2,
                fault: Fault::UnknownDirection
            }))
        );
    }

    #[test]
    fn a_trace_that_redaction_refuses_is_refused() {
        assert_eq!(
            transform("control < 00\n"),
            Err(CaptureError::Redact(RedactError::UnexpectedEvent {
                index: 0
            }))
        );
    }
}
