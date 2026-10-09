//! Measurements that run on demand (landing S6). For now each command hands its
//! arguments to the script it replaces.

use crate::dispatch::{Command, Run, Script};

pub const COMMANDS: &[Command] = &[Command {
    words: &["bench", "records-memory"],
    about: "Measure what a first look at a big Claude transcript costs, release-built",
    usage: "[--runs N] [--lines N] [--transcript FILE] [--repo DIR]",
    run: Run::Node(Script::at_root("tests/bench/records-memory.mjs")),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn records_memory_hands_its_options_to_the_script() {
        delegates(
            COMMANDS,
            "bench records-memory",
            "tests/bench/records-memory.mjs",
            "",
            &[],
        );
        delegates(
            COMMANDS,
            "bench records-memory --runs 3 --lines 125000",
            "tests/bench/records-memory.mjs",
            "",
            &["--runs", "3", "--lines", "125000"],
        );
    }
}
