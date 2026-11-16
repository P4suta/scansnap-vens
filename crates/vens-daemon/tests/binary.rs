//! The `vensd` binary prints its usage and the Avahi service file, and exits with status 2 on a usage error.
//! These cases never listen, because a listening test binary would ask the firewall for permission.

use std::process::{Command, Output};

const VENSD: &str = env!("CARGO_BIN_EXE_vensd");

fn vensd(arguments: &[&str]) -> Option<Output> {
    Command::new(VENSD).args(arguments).output().ok()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn help_prints_the_usage() {
    assert_eq!(
        vensd(&["--help"]).map(|output| (
            output.status.code(),
            text(&output.stdout).starts_with("usage: vensd"),
            output.stderr
        )),
        Some((Some(0), true, Vec::new()))
    );
}

#[test]
fn the_avahi_subcommand_prints_the_service_file() {
    assert_eq!(
        vensd(&["avahi-service", "--port", "9000"]).map(|output| {
            let printed = text(&output.stdout);
            (
                output.status.code(),
                printed.contains("<type>_uscan._tcp</type>"),
                printed.contains("<port>9000</port>"),
            )
        }),
        Some((Some(0), true, true))
    );
}

#[test]
fn a_usage_error_exits_with_status_2_and_names_the_argument() {
    assert_eq!(
        vensd(&["--port", "http"]).map(|output| (
            output.status.code(),
            text(&output.stderr)
                .starts_with("vensd: http isn't a port number from 1 to 65535\nusage: vensd"),
            output.stdout
        )),
        Some((Some(2), true, Vec::new()))
    );
}
