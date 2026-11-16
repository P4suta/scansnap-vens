use std::path::PathBuf;

use crate::task::{Task, UsageError};

const REDACT_CAPTURE: &str = "redact-capture";

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Command {
    Gate(Task),
    RedactCapture { raw: PathBuf, fixture: PathBuf },
}

impl Command {
    pub(crate) fn parse(arguments: impl Iterator<Item = String>) -> Result<Self, UsageError> {
        let mut arguments = arguments.peekable();
        if arguments.peek().map(String::as_str) != Some(REDACT_CAPTURE) {
            return Task::parse(arguments).map(Self::Gate);
        }
        arguments.next();
        let raw = arguments
            .next()
            .map(PathBuf::from)
            .ok_or(UsageError::MissingArgument("raw trace"))?;
        let fixture = arguments
            .next()
            .map(PathBuf::from)
            .ok_or(UsageError::MissingArgument("fixture"))?;
        match arguments.next() {
            Some(extra) => Err(UsageError::Trailing(extra)),
            None => Ok(Self::RedactCapture { raw, fixture }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::Command;
    use crate::task::{Task, UsageError};

    fn arguments(values: &[&str]) -> impl Iterator<Item = String> {
        values.iter().map(|value| (*value).to_owned())
    }

    #[test]
    fn a_gate_task_keeps_its_parser() {
        assert_eq!(
            Command::parse(arguments(&["check"])),
            Ok(Command::Gate(Task::Check))
        );
        assert_eq!(
            Command::parse(arguments(&["deploy"])),
            Err(UsageError::Unknown("deploy".to_owned()))
        );
    }

    #[test]
    fn redact_capture_takes_the_raw_trace_and_the_fixture() {
        assert_eq!(
            Command::parse(arguments(&["redact-capture", "raw.trace", "fixture.trace"])),
            Ok(Command::RedactCapture {
                raw: PathBuf::from("raw.trace"),
                fixture: PathBuf::from("fixture.trace"),
            })
        );
    }

    #[test]
    fn redact_capture_refuses_a_missing_path_by_name() {
        assert_eq!(
            Command::parse(arguments(&["redact-capture"])),
            Err(UsageError::MissingArgument("raw trace"))
        );
        assert_eq!(
            Command::parse(arguments(&["redact-capture", "raw.trace"])),
            Err(UsageError::MissingArgument("fixture"))
        );
    }

    #[test]
    fn redact_capture_refuses_a_trailing_argument() {
        assert_eq!(
            Command::parse(arguments(&["redact-capture", "a", "b", "c"])),
            Err(UsageError::Trailing("c".to_owned()))
        );
    }
}
