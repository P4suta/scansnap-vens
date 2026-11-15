use std::process::Command;

use crate::task::{Runner, Step, StepError};

pub(crate) struct Host;

impl Runner for Host {
    fn run(&mut self, step: Step) -> Result<(), StepError> {
        let status = Command::new(step.program)
            .args(step.arguments)
            .status()
            .map_err(|error| StepError::Spawn {
                step,
                kind: error.kind(),
            })?;
        if status.success() {
            Ok(())
        } else {
            Err(StepError::Failed {
                step,
                code: status.code(),
            })
        }
    }
}
