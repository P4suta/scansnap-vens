use std::{net::Ipv4Addr, path::PathBuf};

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
    Unexpected(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subcommand {
    Discover,
    Status,
    Scan,
}

/// Reads one invocation from the arguments that follow the program name.
///
/// A search goes to the broadcast address unless `--address` names the scanner.
///
/// # Errors
///
/// Refuses a missing or unknown subcommand, an option without its value, an address that isn't IPv4, an option that the subcommand lacks, and an extra argument.
pub fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Invocation, UsageError> {
    let name = arguments.next().ok_or(UsageError::MissingSubcommand)?;
    let subcommand = match name.as_str() {
        "discover" => Subcommand::Discover,
        "status" => Subcommand::Status,
        "scan" => Subcommand::Scan,
        "help" | "--help" | "-h" => return Ok(Invocation::Help),
        _ => return Err(UsageError::UnknownSubcommand(name)),
    };
    let mut target = Ipv4Addr::BROADCAST;
    let mut trace = None;
    let mut directory = None;
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            ADDRESS => {
                let text = arguments.next().ok_or(UsageError::MissingValue(ADDRESS))?;
                target = text.parse().map_err(|_| UsageError::InvalidAddress(text))?;
            }
            TRACE if subcommand == Subcommand::Scan => {
                let path = arguments.next().ok_or(UsageError::MissingValue(TRACE))?;
                trace = Some(PathBuf::from(path));
            }
            option if option.starts_with('-') => return Err(UsageError::UnknownOption(argument)),
            _ if subcommand == Subcommand::Scan && directory.is_none() => {
                directory = Some(PathBuf::from(argument));
            }
            _ => return Err(UsageError::Unexpected(argument)),
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
