mod dependencies;
mod fixture;
mod process;
mod task;

use std::{
    env, fmt,
    io::{self, Write},
    process::ExitCode,
};

use task::{Task, TaskError, UsageError};

enum Failure {
    Usage(UsageError),
    Task(TaskError),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) => error.fmt(formatter),
            Self::Task(error) => error.fmt(formatter),
        }
    }
}

fn run() -> Result<(), Failure> {
    let task = Task::parse(env::args().skip(1)).map_err(Failure::Usage)?;
    task::execute(task, &mut process::Host).map_err(Failure::Task)
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
