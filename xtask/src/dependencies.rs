use std::fmt;

use crate::{
    fixture::Observation,
    task::{Runner, Step, TaskError},
};

pub(crate) const ORIGIN: &str = "https://github.com/RustSec/advisory-db";

macro_rules! cache {
    () => {
        ".local/advisories/advisory-db-3157b0e258782691"
    };
}

macro_rules! advisory_git {
    ($($argument:literal),+) => {
        Step {
            program: "git",
            arguments: &["-C", cache!(), $($argument),+],
        }
    };
}

macro_rules! policy {
    ($target:literal) => {
        Step {
            program: "cargo",
            arguments: &[
                "deny",
                "--offline",
                "--locked",
                "--target",
                $target,
                "--log-level",
                "error",
                "check",
            ],
        }
    };
}

pub(crate) const PRESENT: Step = advisory_git!("rev-parse", "--git-dir");
pub(crate) const CLONE: Step = Step {
    program: "git",
    arguments: &[
        "clone",
        "--filter=blob:none",
        "--single-branch",
        "--branch",
        "main",
        ORIGIN,
        cache!(),
    ],
};
pub(crate) const REMOTE: Step = advisory_git!("remote", "get-url", "origin");
pub(crate) const STATUS: Step = advisory_git!("status", "--porcelain", "--untracked-files=all");
pub(crate) const FETCH: Step = advisory_git!("fetch", "origin", "main");
const MERGE: Step = advisory_git!("merge", "--ff-only", "FETCH_HEAD");
const REVISION: Step = advisory_git!("rev-parse", "HEAD");
const INDEX: Step = Step {
    program: "cargo",
    arguments: &["deny", "fetch", "index", "std-replacement"],
};
const POLICY: &[Step] = &[
    policy!("aarch64-apple-darwin"),
    policy!("x86_64-unknown-linux-gnu"),
    policy!("x86_64-pc-windows-msvc"),
];
pub(crate) const UNUSED: Step = Step {
    program: "cargo-machete",
    arguments: &["."],
};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CacheError {
    Unrelated { origin: String },
    Dirty,
}

fn verdict(remote: &Observation, status: &Observation) -> Result<(), CacheError> {
    let origin = remote.output.trim();
    if !remote.success || origin != ORIGIN {
        return Err(CacheError::Unrelated {
            origin: origin.to_owned(),
        });
    }
    if !status.success || !status.output.trim().is_empty() {
        return Err(CacheError::Dirty);
    }
    Ok(())
}

fn verify(runner: &mut impl Runner) -> Result<(), TaskError> {
    let remote = runner.observe(REMOTE).map_err(TaskError::Step)?;
    let status = runner.observe(STATUS).map_err(TaskError::Step)?;
    verdict(&remote, &status).map_err(TaskError::Cache)
}

pub(crate) fn check(runner: &mut impl Runner) -> Result<(), TaskError> {
    let present = runner.observe(PRESENT).map_err(TaskError::Step)?.success;
    if !present {
        runner.run(CLONE).map_err(TaskError::Step)?;
    }
    verify(runner)?;
    [FETCH, MERGE]
        .into_iter()
        .try_for_each(|step| runner.run(step).map_err(TaskError::Step))?;
    verify(runner)?;
    [REVISION, INDEX]
        .iter()
        .chain(POLICY)
        .chain(&[UNUSED])
        .try_for_each(|step| runner.run(*step).map_err(TaskError::Step))
}

impl fmt::Display for CacheError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unrelated { origin } => write!(
                formatter,
                "advisory cache origin is `{origin}`, expected {ORIGIN}"
            ),
            Self::Dirty => formatter.write_str("advisory cache has local changes"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CLONE, CacheError, FETCH, ORIGIN, PRESENT, REMOTE, STATUS, UNUSED, check};
    use crate::{
        fixture::Observation,
        task::{Runner, Step, StepError, TaskError},
    };

    struct Checkout {
        present: bool,
        remote: &'static str,
        status: &'static str,
        ran: Vec<Step>,
    }

    impl Checkout {
        const fn new(present: bool, remote: &'static str, status: &'static str) -> Self {
            Self {
                present,
                remote,
                status,
                ran: Vec::new(),
            }
        }
    }

    impl Runner for Checkout {
        fn run(&mut self, step: Step) -> Result<(), StepError> {
            self.ran.push(step);
            Ok(())
        }

        fn observe(&mut self, step: Step) -> Result<Observation, StepError> {
            let (success, output) = if step == PRESENT {
                (self.present, "")
            } else if step == REMOTE {
                (true, self.remote)
            } else if step == STATUS {
                (true, self.status)
            } else {
                (false, "")
            };
            Ok(Observation {
                success,
                output: output.to_owned(),
                diagnostic: String::new(),
            })
        }
    }

    #[test]
    fn a_missing_advisory_cache_is_cloned_before_the_policy_runs() {
        let mut checkout = Checkout::new(false, ORIGIN, "");
        assert_eq!(check(&mut checkout), Ok(()));
        assert_eq!(checkout.ran.first(), Some(&CLONE));
        assert_eq!(checkout.ran.last(), Some(&UNUSED));
    }

    #[test]
    fn a_present_advisory_cache_is_updated_without_a_clone() {
        let mut checkout = Checkout::new(true, ORIGIN, "\n");
        assert_eq!(check(&mut checkout), Ok(()));
        assert_eq!(checkout.ran.first(), Some(&FETCH));
        assert!(!checkout.ran.contains(&CLONE));
    }

    #[test]
    fn an_unrelated_advisory_checkout_is_refused_before_any_update() {
        let mut checkout = Checkout::new(true, "https://example.invalid/other\n", "");
        assert_eq!(
            check(&mut checkout),
            Err(TaskError::Cache(CacheError::Unrelated {
                origin: "https://example.invalid/other".to_owned(),
            }))
        );
        assert_eq!(checkout.ran, []);
    }

    #[test]
    fn a_dirty_advisory_checkout_is_refused_before_any_update() {
        let mut checkout = Checkout::new(true, ORIGIN, "?? untracked\n");
        assert_eq!(
            check(&mut checkout),
            Err(TaskError::Cache(CacheError::Dirty))
        );
        assert_eq!(checkout.ran, []);
    }
}
