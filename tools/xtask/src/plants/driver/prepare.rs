//! The commands and files a plant requires before it is applied.

use super::super::kit::Plant;
use super::super::report;
use super::super::runs::{Execution, Runner};
use super::super::Error;
use crate::context::Context;

/// Whether what a plant needs first is done.
pub(super) enum Prepared {
    Done,
    /// A command failed.
    Failed(Error),
    Stopped,
}

/// Runs the commands a plant needs before it is planted, which must succeed.
pub(super) fn prepare(plant: &Plant, runner: &mut dyn Runner) -> Result<Prepared, Error> {
    for command in &plant.prepare {
        match runner.run(command)? {
            Execution::Stopped => return Ok(Prepared::Stopped),
            Execution::Ended(ran) if ran.code != Some(0) => {
                return Ok(Prepared::Failed(Error::Prepare {
                    plant: plant.name.clone(),
                    command: command.join(" "),
                    said: report::tail(&ran.output, 1500).to_owned(),
                }));
            }
            Execution::Ended(_) => {}
        }
    }
    Ok(Prepared::Done)
}

/// The files its runs take as given: one that is not there makes a run fail by
/// itself, which would be taken for a catch.
pub(super) fn required(plant: &Plant, context: &Context) -> Result<(), Error> {
    match plant
        .requires
        .iter()
        .find(|file| !context.path(file).exists())
    {
        Some(file) => Err(Error::NotThere {
            plant: plant.name.clone(),
            file: file.clone(),
        }),
        None => Ok(()),
    }
}
