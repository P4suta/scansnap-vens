use std::{fmt, io};

use crate::fixture::{Expectation, Fixture, FixtureError, Observation, judge};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Task {
    Check,
    StaticTypes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Run(&'static [Step]),
    Judge(&'static [Fixture]),
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

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TaskError {
    Step(StepError),
    Fixture(FixtureError),
}

pub(crate) trait Runner {
    fn run(&mut self, step: Step) -> Result<(), StepError>;
    fn observe(&mut self, step: Step) -> Result<Observation, StepError>;
}

const NATIVE: &[Step] = &[
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
            "--exclude",
            LINT_FIXTURE_PACKAGE,
            "--all-targets",
            "--",
            "-D",
            "warnings",
        ],
    },
    Step {
        program: "cargo",
        arguments: &[
            "test",
            "--locked",
            "--workspace",
            "--exclude",
            LINT_FIXTURE_PACKAGE,
        ],
    },
];

const LINT_FIXTURE_PACKAGE: &str = "lint-fixtures";

macro_rules! lint_fixture {
    ($name:literal, $expectation:expr) => {
        Fixture {
            name: $name,
            expectation: $expectation,
            step: Step {
                program: "cargo",
                arguments: &[
                    "clippy",
                    "--locked",
                    "--package",
                    LINT_FIXTURE_PACKAGE,
                    "--bin",
                    $name,
                    "--",
                    "-D",
                    "warnings",
                ],
            },
        }
    };
}

pub(crate) const LINT_FIXTURES: &[Fixture] = &[
    lint_fixture!("accepted", Expectation::Accepted),
    lint_fixture!("unwrap", rejected("index.html#unwrap_used")),
    lint_fixture!("expect", rejected("index.html#expect_used")),
    lint_fixture!("explicit_panic", rejected("index.html#panic")),
    lint_fixture!("indexing", rejected("index.html#indexing_slicing")),
    lint_fixture!("lossy_cast", rejected("index.html#as_conversions")),
    lint_fixture!(
        "unchecked_arithmetic",
        rejected("index.html#arithmetic_side_effects")
    ),
    lint_fixture!(
        "wildcard_match",
        rejected("index.html#wildcard_enum_match_arm")
    ),
    lint_fixture!("allow_attribute", rejected("index.html#allow_attributes")),
    lint_fixture!("boxed", rejected("index.html#disallowed_types")),
    lint_fixture!("unsafe_block", rejected("usage of an `unsafe` block")),
];

const STATIC_TYPES_MARKER: &str = "error: static_types:";
const FORBIDDEN_EXCEPTION_MARKER: &str = "error[E0453]";

macro_rules! static_type_fixture {
    ($name:literal, $expectation:expr) => {
        Fixture {
            name: $name,
            expectation: $expectation,
            step: Step {
                program: "cargo",
                arguments: &[
                    "dylint",
                    "--path",
                    "verification/static-types",
                    "--all",
                    "--fail-on-no-libraries",
                    "--",
                    "--manifest-path",
                    "verification/type-fixtures/Cargo.toml",
                    "--bin",
                    $name,
                ],
            },
        }
    };
}

const fn rejected(marker: &'static str) -> Expectation {
    Expectation::Rejected { marker }
}

pub(crate) const STATIC_TYPE_FIXTURES: &[Fixture] = &[
    static_type_fixture!("concrete", Expectation::Accepted),
    static_type_fixture!("direct", rejected(STATIC_TYPES_MARKER)),
    static_type_fixture!("alias", rejected(STATIC_TYPES_MARKER)),
    static_type_fixture!("inferred", rejected(STATIC_TYPES_MARKER)),
    static_type_fixture!("trait_object", rejected(STATIC_TYPES_MARKER)),
    static_type_fixture!("authored_macro", rejected(STATIC_TYPES_MARKER)),
    static_type_fixture!("shared_mutation", rejected(STATIC_TYPES_MARKER)),
    static_type_fixture!("unused_alias", rejected(STATIC_TYPES_MARKER)),
    static_type_fixture!("trait_declaration", rejected(STATIC_TYPES_MARKER)),
    static_type_fixture!("broadened_exception", rejected(FORBIDDEN_EXCEPTION_MARKER)),
    static_type_fixture!("stale_exception", rejected(FORBIDDEN_EXCEPTION_MARKER)),
];

const WORKSPACE_STATIC_TYPES: &[Step] = &[Step {
    program: "cargo",
    arguments: &[
        "dylint",
        "--path",
        "verification/static-types",
        "--all",
        "--fail-on-no-libraries",
        "--",
        "--workspace",
        "--exclude",
        LINT_FIXTURE_PACKAGE,
        "--all-targets",
    ],
}];

impl Task {
    pub(crate) fn parse(mut arguments: impl Iterator<Item = String>) -> Result<Self, UsageError> {
        let name = arguments.next().ok_or(UsageError::Missing)?;
        let task = match name.as_str() {
            "check" => Self::Check,
            "static-types" => Self::StaticTypes,
            _ => return Err(UsageError::Unknown(name)),
        };
        match arguments.next() {
            Some(extra) => Err(UsageError::Trailing(extra)),
            None => Ok(task),
        }
    }

    const fn stages(self) -> &'static [Stage] {
        match self {
            Self::Check => &[
                Stage::Run(NATIVE),
                Stage::Judge(LINT_FIXTURES),
                Stage::Judge(STATIC_TYPE_FIXTURES),
                Stage::Run(WORKSPACE_STATIC_TYPES),
            ],
            Self::StaticTypes => &[
                Stage::Judge(STATIC_TYPE_FIXTURES),
                Stage::Run(WORKSPACE_STATIC_TYPES),
            ],
        }
    }
}

pub(crate) fn execute(task: Task, runner: &mut impl Runner) -> Result<(), TaskError> {
    task.stages().iter().try_for_each(|stage| match *stage {
        Stage::Run(steps) => steps
            .iter()
            .try_for_each(|step| runner.run(*step).map_err(TaskError::Step)),
        Stage::Judge(fixtures) => fixtures.iter().try_for_each(|fixture| {
            let observation = runner.observe(fixture.step).map_err(TaskError::Step)?;
            judge(*fixture, observation).map_err(TaskError::Fixture)
        }),
    })
}

impl fmt::Display for TaskError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Step(error) => error.fmt(formatter),
            Self::Fixture(error) => error.fmt(formatter),
        }
    }
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
            Self::Missing => formatter.write_str("expected a task name: check or static-types"),
            Self::Unknown(name) => write!(
                formatter,
                "unknown task: {name}; expected check or static-types"
            ),
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
    use std::{fs, io, path::Path};

    use super::{
        LINT_FIXTURES, Runner, STATIC_TYPE_FIXTURES, Step, StepError, Task, TaskError, UsageError,
        execute,
    };
    use crate::fixture::Fixture;
    use crate::fixture::{Expectation, FixtureError, Observation};

    fn arguments(values: &[&str]) -> impl Iterator<Item = String> {
        values.iter().map(|value| (*value).to_owned())
    }

    #[derive(Clone, Copy)]
    enum Lint {
        Faithful,
        AcceptsEverything,
    }

    struct Recorder {
        seen: Vec<Step>,
        observed: Vec<Step>,
        fail_at: Option<usize>,
        lint: Lint,
    }

    impl Recorder {
        const fn new(lint: Lint, fail_at: Option<usize>) -> Self {
            Self {
                seen: Vec::new(),
                observed: Vec::new(),
                fail_at,
                lint,
            }
        }

        fn subcommands(&self) -> Vec<Option<&str>> {
            self.seen
                .iter()
                .map(|step| step.arguments.first().copied())
                .collect()
        }
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

        fn observe(&mut self, step: Step) -> Result<Observation, StepError> {
            self.observed.push(step);
            let expectation = STATIC_TYPE_FIXTURES
                .iter()
                .chain(LINT_FIXTURES)
                .find(|fixture| fixture.step == step)
                .map(|fixture| fixture.expectation);
            Ok(match (self.lint, expectation) {
                (Lint::Faithful, Some(Expectation::Rejected { marker })) => Observation {
                    success: false,
                    diagnostic: marker.to_owned(),
                },
                (Lint::Faithful, Some(Expectation::Accepted) | None)
                | (Lint::AcceptsEverything, _) => Observation {
                    success: true,
                    diagnostic: String::new(),
                },
            })
        }
    }

    fn fixture_sources(package: &str) -> io::Result<Vec<String>> {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../verification")
            .join(package)
            .join("src/bin");
        let mut names = Vec::new();
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                names.push(stem.to_owned());
            }
        }
        names.sort();
        Ok(names)
    }

    #[test]
    fn parses_the_check_task() {
        assert_eq!(Task::parse(arguments(&["check"])), Ok(Task::Check));
    }

    #[test]
    fn parses_the_static_types_task() {
        assert_eq!(
            Task::parse(arguments(&["static-types"])),
            Ok(Task::StaticTypes)
        );
    }

    fn stated(fixtures: &[Fixture]) -> Vec<String> {
        let mut names: Vec<String> = fixtures
            .iter()
            .map(|fixture| fixture.name.to_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn every_fixture_source_has_a_stated_expectation() {
        assert_eq!(
            fixture_sources("type-fixtures").ok(),
            Some(stated(STATIC_TYPE_FIXTURES))
        );
        assert_eq!(
            fixture_sources("lint-fixtures").ok(),
            Some(stated(LINT_FIXTURES))
        );
    }

    #[test]
    fn check_judges_the_lint_fixtures_after_the_native_steps() {
        let mut recorder = Recorder::new(Lint::AcceptsEverything, None);
        assert_eq!(
            execute(Task::Check, &mut recorder),
            Err(TaskError::Fixture(FixtureError::Accepted {
                name: "unwrap"
            }))
        );
        assert_eq!(
            recorder.subcommands(),
            [Some("fmt"), Some("clippy"), Some("test")]
        );
    }

    #[test]
    fn static_types_judges_every_fixture_before_linting_the_workspace() {
        let mut recorder = Recorder::new(Lint::Faithful, None);
        assert_eq!(execute(Task::StaticTypes, &mut recorder), Ok(()));
        assert_eq!(recorder.observed.len(), STATIC_TYPE_FIXTURES.len());
        assert_eq!(recorder.subcommands(), [Some("dylint")]);
    }

    #[test]
    fn a_lint_that_accepts_everything_fails_before_the_workspace_runs() {
        let mut recorder = Recorder::new(Lint::AcceptsEverything, None);
        assert_eq!(
            execute(Task::StaticTypes, &mut recorder),
            Err(TaskError::Fixture(FixtureError::Accepted {
                name: "direct"
            }))
        );
        assert_eq!(recorder.subcommands(), []);
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
    fn check_runs_the_native_steps_then_the_static_type_lint() {
        let mut recorder = Recorder::new(Lint::Faithful, None);
        assert_eq!(execute(Task::Check, &mut recorder), Ok(()));
        assert_eq!(
            recorder.subcommands(),
            [Some("fmt"), Some("clippy"), Some("test"), Some("dylint")]
        );
        assert_eq!(
            recorder.observed.len(),
            STATIC_TYPE_FIXTURES
                .len()
                .saturating_add(LINT_FIXTURES.len())
        );
    }

    #[test]
    fn a_failed_step_stops_the_task_and_keeps_its_status() {
        let mut recorder = Recorder::new(Lint::Faithful, Some(1));
        let result = execute(Task::Check, &mut recorder);
        assert_eq!(recorder.seen.len(), 2);
        assert_eq!(
            result,
            recorder
                .seen
                .last()
                .map_or(Ok(()), |step| Err(TaskError::Step(StepError::Failed {
                    step: *step,
                    code: Some(3),
                })))
        );
    }
}
