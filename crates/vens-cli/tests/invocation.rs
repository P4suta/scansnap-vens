//! The parser turns the arguments into one typed invocation or a usage error that names the problem.

use std::{net::Ipv4Addr, path::PathBuf};

use vens_cli::invocation::{Invocation, UsageError, parse};

fn arguments(values: &[&str]) -> impl Iterator<Item = String> {
    values.iter().map(|value| (*value).to_owned())
}

#[test]
fn discovery_and_status_search_by_broadcast_unless_an_address_is_given() {
    assert_eq!(
        (
            parse(arguments(&["discover"])),
            parse(arguments(&["status", "--address", "192.0.2.7"])),
        ),
        (
            Ok(Invocation::Discover {
                target: Ipv4Addr::BROADCAST
            }),
            Ok(Invocation::Status {
                target: Ipv4Addr::new(192, 0, 2, 7)
            }),
        )
    );
}

#[test]
fn scan_takes_a_directory_and_optional_address_and_trace() {
    assert_eq!(
        (
            parse(arguments(&["scan", "out"])),
            parse(arguments(&[
                "scan",
                "--trace",
                "raw.trace",
                "out",
                "--address",
                "192.0.2.7"
            ])),
        ),
        (
            Ok(Invocation::Scan {
                directory: PathBuf::from("out"),
                target: Ipv4Addr::BROADCAST,
                trace: None,
            }),
            Ok(Invocation::Scan {
                directory: PathBuf::from("out"),
                target: Ipv4Addr::new(192, 0, 2, 7),
                trace: Some(PathBuf::from("raw.trace")),
            }),
        )
    );
}

#[test]
fn help_is_an_invocation_of_its_own() {
    for help in ["help", "--help", "-h"] {
        assert_eq!(parse(arguments(&[help])), Ok(Invocation::Help), "{help}");
    }
}

#[test]
fn a_missing_or_unknown_subcommand_is_refused() {
    assert_eq!(
        (parse(arguments(&[])), parse(arguments(&["print"]))),
        (
            Err(UsageError::MissingSubcommand),
            Err(UsageError::UnknownSubcommand("print".to_owned()))
        )
    );
}

#[test]
fn scan_without_a_directory_is_refused() {
    assert_eq!(
        parse(arguments(&["scan", "--address", "192.0.2.7"])),
        Err(UsageError::MissingDirectory)
    );
}

#[test]
fn an_option_without_its_value_is_refused_by_name() {
    assert_eq!(
        (
            parse(arguments(&["discover", "--address"])),
            parse(arguments(&["scan", "out", "--trace"])),
        ),
        (
            Err(UsageError::MissingValue("--address")),
            Err(UsageError::MissingValue("--trace"))
        )
    );
}

#[test]
fn an_address_that_is_not_ipv4_is_refused_with_its_text() {
    assert_eq!(
        parse(arguments(&["status", "--address", "scanner.local"])),
        Err(UsageError::InvalidAddress("scanner.local".to_owned()))
    );
}

#[test]
fn an_unknown_option_or_an_extra_argument_is_refused() {
    assert_eq!(
        (
            parse(arguments(&["discover", "--verbose"])),
            parse(arguments(&["discover", "extra"])),
            parse(arguments(&["scan", "out", "more"])),
            parse(arguments(&["status", "--trace", "raw.trace"])),
        ),
        (
            Err(UsageError::UnknownOption("--verbose".to_owned())),
            Err(UsageError::Unexpected("extra".to_owned())),
            Err(UsageError::Unexpected("more".to_owned())),
            Err(UsageError::UnknownOption("--trace".to_owned())),
        )
    );
}
