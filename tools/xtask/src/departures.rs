//! The traces of the tests the Rust engine departs from Node's on purpose
//! (landing S6), recorded again. For now the command hands its arguments to the
//! script it replaces.

use crate::dispatch::{Command, Run, Script};

pub const COMMANDS: &[Command] = &[Command {
    words: &["departures"],
    about: "Record again the traces of the tests the engine departs from Node's on purpose",
    usage: "",
    run: Run::Node(Script::at_root("tests/departures.mjs")),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn departures_runs_the_script_from_the_root() {
        delegates(COMMANDS, "departures", "tests/departures.mjs", "", &[]);
    }
}
