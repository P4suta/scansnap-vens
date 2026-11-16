//! The `vensd` command serves by default, and its one subcommand prints the Avahi service file.

use std::{ffi::OsString, net::Ipv4Addr};

use vens_daemon::invocation::{DEFAULT_PORT, DEFAULT_UUID, Invocation, USAGE, UsageError, parse};
use vens_escl::uuid::Uuid;

const ID: &str = "6f1c2e8a-3b4d-4c5e-9f60-7a8b9c0d1e2f";

fn run(words: &[&str]) -> Result<Invocation, UsageError> {
    parse(words.iter().map(OsString::from))
}

#[test]
fn no_arguments_serve_with_the_defaults() {
    assert_eq!(
        run(&[]),
        Ok(Invocation::Serve {
            port: DEFAULT_PORT,
            target: Ipv4Addr::BROADCAST,
            uuid: DEFAULT_UUID,
        })
    );
}

#[test]
fn the_defaults_are_port_8090_and_a_fixed_identifier() {
    assert_eq!(
        (DEFAULT_PORT, DEFAULT_UUID.to_string()),
        (8090, "9ff23966-94ee-48d8-9284-1e1e09ff7271".to_owned())
    );
}

#[test]
fn each_option_sets_its_value() {
    assert_eq!(
        run(&["--port", "9000", "--address", "192.0.2.7", "--uuid", ID]).ok(),
        Uuid::parse(ID).map(|uuid| Invocation::Serve {
            port: 9000,
            target: Ipv4Addr::new(192, 0, 2, 7),
            uuid,
        })
    );
}

#[test]
fn the_avahi_subcommand_takes_the_port_and_the_identifier() {
    assert_eq!(
        run(&["avahi-service", "--port", "9000", "--uuid", ID]).ok(),
        Uuid::parse(ID).map(|uuid| Invocation::AvahiService { port: 9000, uuid })
    );
}

#[test]
fn help_shows_from_any_position() {
    assert_eq!(
        [run(&["--help"]), run(&["avahi-service", "--help"])],
        [Ok(Invocation::Help), Ok(Invocation::Help)]
    );
}

#[test]
fn each_usage_error_names_its_problem() {
    let errors = [
        run(&["--port"]),
        run(&["--port", "http"]),
        run(&["--port", "0"]),
        run(&["--address", "scanner"]),
        run(&["--uuid", "abc"]),
        run(&["--verbose"]),
        run(&["status"]),
        run(&["--port", "9000", "avahi-service"]),
        run(&["avahi-service", "--address", "192.0.2.7"]),
    ];
    assert_eq!(
        errors,
        [
            Err(UsageError::MissingValue("--port")),
            Err(UsageError::InvalidPort("http".to_owned())),
            Err(UsageError::InvalidPort("0".to_owned())),
            Err(UsageError::InvalidAddress("scanner".to_owned())),
            Err(UsageError::InvalidUuid("abc".to_owned())),
            Err(UsageError::UnknownOption("--verbose".to_owned())),
            Err(UsageError::UnknownSubcommand("status".to_owned())),
            Err(UsageError::UnknownSubcommand("avahi-service".to_owned())),
            Err(UsageError::ForeignOption {
                option: "--address",
                subcommand: "avahi-service",
            }),
        ]
    );
}

#[cfg(unix)]
#[test]
fn an_argument_that_isnt_unicode_is_refused() {
    use std::os::unix::ffi::OsStringExt;
    assert_eq!(
        parse([OsString::from_vec([0x66, 0x80].to_vec())]),
        Err(UsageError::NotUnicode)
    );
}

#[test]
fn the_usage_names_both_forms_and_the_defaults() {
    assert_eq!(
        [
            USAGE.starts_with("usage: vensd [--port <port>]"),
            USAGE.contains("vensd avahi-service"),
            USAGE.contains(&format!("default {DEFAULT_PORT}")),
            USAGE.contains(&format!("default {DEFAULT_UUID}")),
        ],
        [true; 4]
    );
}

#[test]
fn usage_errors_read_as_sentences_about_the_argument() {
    assert_eq!(
        [
            UsageError::MissingValue("--port").to_string(),
            UsageError::InvalidPort("http".to_owned()).to_string(),
            UsageError::ForeignOption {
                option: "--address",
                subcommand: "avahi-service",
            }
            .to_string(),
        ],
        [
            "--port needs a value",
            "http isn't a port number from 1 to 65535",
            "--address doesn't apply to avahi-service",
        ]
    );
}
