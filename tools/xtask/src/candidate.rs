//! ConsensFlow Candidate (landing S6): this checkout built under another
//! identity and installed beside the live app, which is proven untouched. For
//! now the command hands its arguments to the script it replaces.

use crate::dispatch::{Command, Run, Script};

pub const COMMANDS: &[Command] = &[Command {
    words: &["candidate"],
    about: "Build this checkout as ConsensFlow Candidate and install it beside the live app",
    usage: "",
    run: Run::Node(Script::at_root("app/scripts/candidate.mjs")),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn candidate_runs_the_script_from_the_root() {
        delegates(COMMANDS, "candidate", "app/scripts/candidate.mjs", "", &[]);
    }
}
