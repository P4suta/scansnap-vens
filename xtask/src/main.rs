mod process;
mod task;

use std::{
    env, fmt,
    io::{self, Write},
    process::ExitCode,
};

use task::{StepError, Task, UsageError};

enum Failure {
    Usage(UsageError),
    Step(StepError),
}

impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(error) => error.fmt(formatter),
            Self::Step(error) => error.fmt(formatter),
        }
    }
}

fn run() -> Result<(), Failure> {
    let task = Task::parse(env::args().skip(1)).map_err(Failure::Usage)?;
    task::execute(task, &mut process::Host).map_err(Failure::Step)
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
