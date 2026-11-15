use std::process::Command;

use crate::{
    fixture::Observation,
    task::{Runner, Step, StepError},
};

pub(crate) struct Host;

fn command(step: Step) -> Command {
    let mut command = Command::new(step.program);
    command.args(step.arguments);
    command
}

impl Runner for Host {
    fn run(&mut self, step: Step) -> Result<(), StepError> {
        let status = command(step).status().map_err(|error| StepError::Spawn {
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

    fn observe(&mut self, step: Step) -> Result<Observation, StepError> {
        let output = command(step).output().map_err(|error| StepError::Spawn {
            step,
            kind: error.kind(),
        })?;
        Ok(Observation {
            success: output.status.success(),
            diagnostic: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
