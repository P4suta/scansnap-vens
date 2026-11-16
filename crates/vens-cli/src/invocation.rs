use std::{ffi::OsString, net::Ipv4Addr, path::PathBuf};

const ADDRESS: &str = "--address";
const TRACE: &str = "--trace";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    Discover {
        target: Ipv4Addr,
    },
    Status {
        target: Ipv4Addr,
    },
    Scan {
        directory: PathBuf,
        target: Ipv4Addr,
        trace: Option<PathBuf>,
    },
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    MissingSubcommand,
    UnknownSubcommand(String),
    MissingDirectory,
    MissingValue(&'static str),
    InvalidAddress(String),
    UnknownOption(String),
    /// Names an option that another subcommand takes.
    ForeignOption {
        option: &'static str,
        subcommand: &'static str,
    },
    Unexpected(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subcommand {
    Discover,
    Status,
    Scan,
}

impl Subcommand {
    const fn name(self) -> &'static str {
        match self {
            Self::Discover => "discover",
            Self::Status => "status",
            Self::Scan => "scan",
        }
    }
}

fn lossy(text: &OsString) -> String {
    text.to_string_lossy().into_owned()
}

/// Reads one invocation from the arguments that follow the program name.
///
/// A search goes to the broadcast address unless `--address` names the scanner.
/// A folder or a trace path may hold bytes that aren't text.
///
/// # Errors
///
/// Refuses a missing or unknown subcommand, an option without its value, an address that isn't IPv4, an option that the subcommand lacks, and an extra argument.
pub fn parse(mut arguments: impl Iterator<Item = OsString>) -> Result<Invocation, UsageError> {
    let name = arguments.next().ok_or(UsageError::MissingSubcommand)?;
    let subcommand = match name.to_str() {
        Some("discover") => Subcommand::Discover,
        Some("status") => Subcommand::Status,
        Some("scan") => Subcommand::Scan,
        Some("help" | "--help" | "-h") => return Ok(Invocation::Help),
        Some(_) | None => return Err(UsageError::UnknownSubcommand(lossy(&name))),
    };
    let mut target = Ipv4Addr::BROADCAST;
    let mut trace = None;
    let mut directory = None;
    while let Some(argument) = arguments.next() {
        let text = argument.to_str().map(str::to_owned);
        match text.as_deref() {
            Some(ADDRESS) => {
                let value = arguments.next().ok_or(UsageError::MissingValue(ADDRESS))?;
                let address = value
                    .to_str()
                    .ok_or_else(|| UsageError::InvalidAddress(lossy(&value)))?;
                target = address
                    .parse()
                    .map_err(|_| UsageError::InvalidAddress(address.to_owned()))?;
            }
            Some(TRACE) if subcommand == Subcommand::Scan => {
                let path = arguments.next().ok_or(UsageError::MissingValue(TRACE))?;
                trace = Some(PathBuf::from(path));
            }
            Some(TRACE) => {
                return Err(UsageError::ForeignOption {
                    option: TRACE,
                    subcommand: subcommand.name(),
                });
            }
            Some("--help" | "-h") => return Ok(Invocation::Help),
            Some(option) if option.starts_with('-') => {
                return Err(UsageError::UnknownOption(option.to_owned()));
            }
            Some(_) | None if subcommand == Subcommand::Scan && directory.is_none() => {
                directory = Some(PathBuf::from(argument));
            }
            Some(_) | None => return Err(UsageError::Unexpected(lossy(&argument))),
        }
    }
    match subcommand {
        Subcommand::Discover => Ok(Invocation::Discover { target }),
        Subcommand::Status => Ok(Invocation::Status { target }),
        Subcommand::Scan => Ok(Invocation::Scan {
            directory: directory.ok_or(UsageError::MissingDirectory)?,
            target,
            trace,
        }),
    }
}
