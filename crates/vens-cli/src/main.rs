use std::{
    env,
    io::{self, Write},
    process::ExitCode,
    time::Duration,
};

use vens_cli::{
    command::{self, Exit},
    invocation,
};
use vens_net::host::SystemNetwork;

const PROBE_INTERVAL: Duration = Duration::from_millis(500);
const DEADLINE: Duration = Duration::from_secs(30);
const USAGE_FAILURE: u8 = 2;

fn main() -> ExitCode {
    let output = match invocation::parse(env::args_os().skip(1)) {
        Ok(invocation) => command::run(SystemNetwork::new(PROBE_INTERVAL, DEADLINE), invocation),
        Err(error) => command::usage_failure(&error),
    };
    let mut stdout = io::stdout().lock();
    let printed = stdout
        .write_all(output.stdout.as_bytes())
        .and_then(|()| stdout.flush());
    let _ = io::stderr().lock().write_all(output.stderr.as_bytes());
    match (output.exit, printed) {
        (Exit::Usage, _) => ExitCode::from(USAGE_FAILURE),
        (Exit::Success, Ok(())) => ExitCode::SUCCESS,
        (Exit::Success, Err(_)) | (Exit::Failure, _) => ExitCode::FAILURE,
    }
}
