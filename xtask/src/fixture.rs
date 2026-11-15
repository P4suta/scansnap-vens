use std::fmt;

use crate::task::Step;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Expectation {
    Accepted,
    Rejected { marker: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Fixture {
    pub(crate) name: &'static str,
    pub(crate) expectation: Expectation,
    pub(crate) step: Step,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Observation {
    pub(crate) success: bool,
    pub(crate) diagnostic: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FixtureError {
    Accepted {
        name: &'static str,
    },
    Rejected {
        name: &'static str,
        diagnostic: String,
    },
    WrongReason {
        name: &'static str,
        marker: &'static str,
        diagnostic: String,
    },
}

pub(crate) fn judge(fixture: Fixture, observation: Observation) -> Result<(), FixtureError> {
    let name = fixture.name;
    let Observation {
        success,
        diagnostic,
    } = observation;
    match (fixture.expectation, success) {
        (Expectation::Accepted, true) => Ok(()),
        (Expectation::Accepted, false) => Err(FixtureError::Rejected { name, diagnostic }),
        (Expectation::Rejected { .. }, true) => Err(FixtureError::Accepted { name }),
        (Expectation::Rejected { marker }, false) => {
            if diagnostic.contains(marker) {
                Ok(())
            } else {
                Err(FixtureError::WrongReason {
                    name,
                    marker,
                    diagnostic,
                })
            }
        }
    }
}

impl fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Accepted { name } => {
                write!(formatter, "fixture {name} passed but must be rejected")
            }
            Self::Rejected { name, diagnostic } => {
                write!(
                    formatter,
                    "fixture {name} must pass but failed:\n{diagnostic}"
                )
            }
            Self::WrongReason {
                name,
                marker,
                diagnostic,
            } => write!(
                formatter,
                "fixture {name} failed without `{marker}`:\n{diagnostic}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Expectation, Fixture, FixtureError, Observation, judge};
    use crate::task::Step;

    const STEP: Step = Step {
        program: "cargo",
        arguments: &["check"],
    };
    const ACCEPTED: Fixture = Fixture {
        name: "concrete",
        expectation: Expectation::Accepted,
        step: STEP,
    };
    const REJECTED: Fixture = Fixture {
        name: "direct",
        expectation: Expectation::Rejected {
            marker: "error: static_types:",
        },
        step: STEP,
    };

    fn observation(success: bool, diagnostic: &str) -> Observation {
        Observation {
            success,
            diagnostic: diagnostic.to_owned(),
        }
    }

    #[test]
    fn an_accepted_fixture_must_succeed() {
        assert_eq!(judge(ACCEPTED, observation(true, "")), Ok(()));
        assert_eq!(
            judge(ACCEPTED, observation(false, "error: broken")),
            Err(FixtureError::Rejected {
                name: "concrete",
                diagnostic: "error: broken".to_owned(),
            })
        );
    }

    #[test]
    fn a_rejected_fixture_must_fail_for_its_stated_reason() {
        assert_eq!(
            judge(
                REJECTED,
                observation(false, "error: static_types: dynamic dispatch")
            ),
            Ok(())
        );
        assert_eq!(
            judge(REJECTED, observation(true, "")),
            Err(FixtureError::Accepted { name: "direct" })
        );
        assert_eq!(
            judge(
                REJECTED,
                observation(false, "error[E0432]: unresolved import")
            ),
            Err(FixtureError::WrongReason {
                name: "direct",
                marker: "error: static_types:",
                diagnostic: "error[E0432]: unresolved import".to_owned(),
            })
        );
    }
}
