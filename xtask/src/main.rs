mod capture;
mod command;
mod dependencies;
mod fixture;
mod process;
mod task;

use std::{
    env, fmt,
    io::{self, Write},
    process::ExitCode,
};

use capture::CaptureError;
use command::Command;
use task::{TaskError, UsageError};

enum Failure {
    Usage(UsageError),
    Task(TaskError),
    Capture(CaptureError),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) => error.fmt(formatter),
            Self::Task(error) => error.fmt(formatter),
            Self::Capture(error) => error.fmt(formatter),
        }
    }
}

fn run() -> Result<(), Failure> {
    match Command::parse(env::args().skip(1)).map_err(Failure::Usage)? {
        Command::Gate(task) => task::execute(task, &mut process::Host).map_err(Failure::Task),
        Command::RedactCapture { raw, fixture } => {
            capture::redact_capture(&raw, &fixture).map_err(Failure::Capture)
        }
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            let _ = writeln!(io::stderr().lock(), "{failure}");
            ExitCode::FAILURE
        }
    }
}
