use std::{fmt, io};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Task {
    Check,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Step {
    pub(crate) program: &'static str,
    pub(crate) arguments: &'static [&'static str],
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UsageError {
    Missing,
    Unknown(String),
    Trailing(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StepError {
    Spawn { step: Step, kind: io::ErrorKind },
    Failed { step: Step, code: Option<i32> },
}

pub(crate) trait Runner {
    fn run(&mut self, step: Step) -> Result<(), StepError>;
}

const CHECK: &[Step] = &[
    Step {
        program: "cargo",
        arguments: &["fmt", "--all", "--", "--check"],
    },
    Step {
        program: "cargo",
        arguments: &[
            "clippy",
            "--locked",
            "--workspace",
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    },
    Step {
        program: "cargo",
        arguments: &["test", "--locked", "--workspace"],
    },
];

impl Task {
    pub(crate) fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Self, UsageError> {
        let name = arguments.next().ok_or(UsageError::Missing)?;
        let task = match name.as_str() {
            "check" => Self::Check,
            _ => return Err(UsageError::Unknown(name)),
        };
        match arguments.next() {
            Some(extra) => Err(UsageError::Trailing(extra)),
            None => Ok(task),
        }
    }

    const fn steps(self) -> &'static [Step] {
        match self {
            Self::Check => CHECK,
        }
    }
}

pub(crate) fn execute(task: Task, runner: &mut impl Runner) -> Result<(), StepError> {
    task.steps().iter().try_for_each(|step| runner.run(*step))
}

impl fmt::Display for Step {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.program)?;
        self.arguments
            .iter()
            .try_for_each(|argument| write!(formatter, " {argument}"))
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing => formatter.write_str("expected a task name: check"),
            Self::Unknown(name) => write!(formatter, "unknown task: {name}; expected check"),
            Self::Trailing(extra) => write!(formatter, "unexpected argument: {extra}"),
        }
    }
}

impl fmt::Display for StepError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { step, kind } => write!(formatter, "{step} did not start: {kind}"),
            Self::Failed {
                step,
                code: Some(code),
            } => write!(formatter, "{step} failed with exit code {code}"),
            Self::Failed { step, code: None } => {
                write!(formatter, "{step} ended without an exit code")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Runner, Step, StepError, Task, UsageError, execute};

    fn arguments(values: &[&str]) -> impl Iterator<Item = String> {
        values.iter().map(|value| (*value).to_owned())
    }

    struct Recorder {
        seen: Vec<Step>,
        fail_at: Option<usize>,
    }

    impl Runner for Recorder {
        fn run(&mut self, step: Step) -> Result<(), StepError> {
            let index = self.seen.len();
            self.seen.push(step);
            if self.fail_at == Some(index) {
                Err(StepError::Failed {
                    step,
                    code: Some(3),
                })
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn parses_the_check_task() {
        assert_eq!(Task::parse(arguments(&["check"])), Ok(Task::Check));
    }

    #[test]
    fn refuses_a_missing_task() {
        assert_eq!(Task::parse(arguments(&[])), Err(UsageError::Missing));
    }

    #[test]
    fn refuses_an_unknown_task() {
        assert_eq!(
            Task::parse(arguments(&["deploy"])),
            Err(UsageError::Unknown("deploy".to_owned()))
        );
    }

    #[test]
    fn refuses_a_trailing_argument() {
        assert_eq!(
            Task::parse(arguments(&["check", "extra"])),
            Err(UsageError::Trailing("extra".to_owned()))
        );
    }

    #[test]
    fn check_runs_format_then_lint_then_tests() {
        let mut recorder = Recorder {
            seen: Vec::new(),
            fail_at: None,
        };
        assert_eq!(execute(Task::Check, &mut recorder), Ok(()));
        let subcommands: Vec<Option<&str>> = recorder
            .seen
            .iter()
            .map(|step| step.arguments.first().copied())
            .collect();
        assert_eq!(subcommands, [Some("fmt"), Some("clippy"), Some("test")]);
    }

    #[test]
    fn a_failed_step_stops_the_task_and_keeps_its_status() {
        let mut recorder = Recorder {
            seen: Vec::new(),
            fail_at: Some(1),
        };
        let result = execute(Task::Check, &mut recorder);
        assert_eq!(recorder.seen.len(), 2);
        assert_eq!(
            result,
            recorder
                .seen
                .last()
                .map_or(Ok(()), |step| Err(StepError::Failed {
                    step: *step,
                    code: Some(3),
                }))
        );
    }
}
