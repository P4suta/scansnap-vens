use std::{ffi::OsString, fmt, net::Ipv4Addr};

use vens_escl::uuid::Uuid;

pub const DEFAULT_PORT: u16 = 8090;
/// Identifies a bridge started without `--uuid`; a second bridge on the same network needs its own.
pub const DEFAULT_UUID: Uuid = Uuid::from_bytes([
    0x9f, 0xf2, 0x39, 0x66, 0x94, 0xee, 0x48, 0xd8, 0x92, 0x84, 0x1e, 0x1e, 0x09, 0xff, 0x72, 0x71,
]);
const AVAHI_SERVICE: &str = "avahi-service";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Invocation {
    Serve {
        port: u16,
        target: Ipv4Addr,
        uuid: Uuid,
    },
    AvahiService {
        port: u16,
        uuid: Uuid,
    },
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    NotUnicode,
    UnknownSubcommand(String),
    UnknownOption(String),
    MissingValue(&'static str),
    InvalidPort(String),
    InvalidAddress(String),
    InvalidUuid(String),
    ForeignOption {
        option: &'static str,
        subcommand: &'static str,
    },
}

impl fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotUnicode => formatter.write_str("an argument isn't valid Unicode"),
            Self::UnknownSubcommand(word) => write!(formatter, "{word} isn't a subcommand"),
            Self::UnknownOption(word) => write!(formatter, "{word} isn't an option"),
            Self::MissingValue(option) => write!(formatter, "{option} needs a value"),
            Self::InvalidPort(word) => {
                write!(formatter, "{word} isn't a port number from 1 to 65535")
            }
            Self::InvalidAddress(word) => write!(formatter, "{word} isn't an IPv4 address"),
            Self::InvalidUuid(word) => write!(formatter, "{word} isn't a hyphenated UUID"),
            Self::ForeignOption { option, subcommand } => {
                write!(formatter, "{option} doesn't apply to {subcommand}")
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Subcommand {
    Serve,
    AvahiService,
}

fn value(
    words: &mut impl Iterator<Item = Result<String, UsageError>>,
    option: &'static str,
) -> Result<String, UsageError> {
    words
        .next()
        .unwrap_or(Err(UsageError::MissingValue(option)))
}

/// Reads the arguments after the program name.
///
/// # Errors
///
/// Names the first argument that the command can't use.
pub fn parse(arguments: impl IntoIterator<Item = OsString>) -> Result<Invocation, UsageError> {
    let mut words = arguments
        .into_iter()
        .map(|word| word.into_string().map_err(|_| UsageError::NotUnicode));
    let mut subcommand = Subcommand::Serve;
    let mut port = DEFAULT_PORT;
    let mut target = Ipv4Addr::BROADCAST;
    let mut uuid = DEFAULT_UUID;
    let mut first = true;
    while let Some(word) = words.next() {
        let word = word?;
        match word.as_str() {
            "--help" => return Ok(Invocation::Help),
            "--port" => {
                let text = value(&mut words, "--port")?;
                port = text
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port != 0)
                    .ok_or(UsageError::InvalidPort(text))?;
            }
            "--address" if subcommand == Subcommand::AvahiService => {
                return Err(UsageError::ForeignOption {
                    option: "--address",
                    subcommand: AVAHI_SERVICE,
                });
            }
            "--address" => {
                let text = value(&mut words, "--address")?;
                target = text.parse().map_err(|_| UsageError::InvalidAddress(text))?;
            }
            "--uuid" => {
                let text = value(&mut words, "--uuid")?;
                uuid = Uuid::parse(&text).ok_or(UsageError::InvalidUuid(text))?;
            }
            AVAHI_SERVICE if first => subcommand = Subcommand::AvahiService,
            option if option.starts_with('-') => {
                return Err(UsageError::UnknownOption(word));
            }
            _ => return Err(UsageError::UnknownSubcommand(word)),
        }
        first = false;
    }
    Ok(match subcommand {
        Subcommand::Serve => Invocation::Serve { port, target, uuid },
        Subcommand::AvahiService => Invocation::AvahiService { port, uuid },
    })
}
