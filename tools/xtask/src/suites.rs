//! The Node suites that run against the native `cf` (landing S6): they build
//! the `cf` (and, for the daemons, the pane host's bridge) and run the suites of
//! one set with `node --test`. For now each command hands its arguments to the
//! script it replaces.

use crate::dispatch::{Command, Run, Script};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["test", "daemons"],
        about: "Build cf and the bridge, and run the daemon suites and the rig's against the native daemon",
        usage: "[--offline]",
        run: Run::Node(Script::at_root("tests/daemons.mjs")),
    },
    Command {
        words: &["test", "clis"],
        about: "Build cf and run the CLI suites against it",
        usage: "[--offline]",
        run: Run::Node(Script::at_root("tests/clis.mjs")),
    },
    Command {
        words: &["test", "agents"],
        about: "Build cf and run the proof of the agents screens against the native daemon",
        usage: "[--offline]",
        run: Run::Node(Script::at_root("tests/agents-daemons.mjs")),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn each_suite_hands_its_offline_flag_to_its_script() {
        for (line, script) in [
            ("test daemons", "tests/daemons.mjs"),
            ("test clis", "tests/clis.mjs"),
            ("test agents", "tests/agents-daemons.mjs"),
        ] {
            delegates(COMMANDS, line, script, "", &[]);
            delegates(
                COMMANDS,
                &format!("{line} --offline"),
                script,
                "",
                &["--offline"],
            );
        }
    }
}
