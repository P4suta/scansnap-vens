use std::{
    env, fs,
    io::{self, Write},
    net::Ipv4Addr,
    path::{Path, PathBuf},
    process::ExitCode,
    sync::mpsc,
    time::Duration,
};

use vens_cli::{
    invocation::{self, Invocation},
    operation, report,
};
use vens_net::{
    host::{DirectorySink, SystemNetwork},
    network::RecordingNetwork,
    trace::{self, Event},
};

const PROBE_INTERVAL: Duration = Duration::from_millis(500);
const DEADLINE: Duration = Duration::from_secs(30);
const USAGE_FAILURE: u8 = 2;

const fn network() -> SystemNetwork {
    SystemNetwork::new(PROBE_INTERVAL, DEADLINE)
}

fn save_trace(path: &Path, log: &mpsc::Receiver<Event>) -> Result<(), String> {
    let events: Vec<Event> = log.try_iter().collect();
    fs::write(path, trace::render(&events))
        .map_err(|error| format!("the trace couldn't be written: {error}"))
}

fn scan(directory: PathBuf, target: Ipv4Addr, trace: Option<&Path>) -> Result<String, String> {
    fs::create_dir_all(&directory)
        .map_err(|error| format!("the folder couldn't be created: {error}"))?;
    let mut sink = DirectorySink::new(directory);
    let scanned = match trace {
        None => operation::scan(&mut network(), target, &mut sink),
        Some(path) => {
            let (events, log) = mpsc::channel();
            let scanned = operation::scan(
                &mut RecordingNetwork::new(network(), events),
                target,
                &mut sink,
            );
            save_trace(path, &log)?;
            scanned
        }
    };
    scanned
        .map(|scanned| report::scanned(&scanned, sink.written()))
        .map_err(|failure| report::failure(&failure))
}

fn run(invocation: Invocation) -> Result<String, String> {
    match invocation {
        Invocation::Help => Ok(report::usage().to_owned()),
        Invocation::Discover { target } => operation::discover(&mut network(), target)
            .map(|reply| report::found(&reply))
            .map_err(|failure| report::failure(&failure)),
        Invocation::Status { target } => operation::status(&mut network(), target)
            .map(|state| report::state(&state))
            .map_err(|failure| report::failure(&failure)),
        Invocation::Scan {
            directory,
            target,
            trace,
        } => scan(directory, target, trace.as_deref()),
    }
}

fn main() -> ExitCode {
    let invocation = match invocation::parse(env::args_os().skip(1)) {
        Ok(invocation) => invocation,
        Err(error) => {
            let _ = write!(
                io::stderr().lock(),
                "vens: {}\n{}",
                report::usage_error(&error),
                report::usage()
            );
            return ExitCode::from(USAGE_FAILURE);
        }
    };
    match run(invocation) {
        Ok(text) => {
            let _ = write!(io::stdout().lock(), "{text}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "vens: {message}");
            ExitCode::FAILURE
        }
    }
}
