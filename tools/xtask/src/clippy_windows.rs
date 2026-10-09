//! Clippy for Windows from a machine that is not one (landing S6): what only
//! Windows compiles is found in seconds, though nothing is run. For now the
//! command hands its arguments to the script it replaces, which lets cc-rs run
//! it again as the archiver of the C it does not compile.

use crate::dispatch::{Command, Run, Script};

pub const COMMANDS: &[Command] = &[Command {
    words: &["clippy-windows"],
    about: "Run clippy for x86_64-pc-windows-msvc, tests included and warnings denied",
    usage: "[CRATE ...]  (all of the workspace but the app by default; `app` names the app)",
    run: Run::Node(Script::at_root("tests/windows-clippy.mjs")),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn clippy_windows_hands_the_crates_it_is_given_to_the_script() {
        delegates(
            COMMANDS,
            "clippy-windows",
            "tests/windows-clippy.mjs",
            "",
            &[],
        );
        delegates(
            COMMANDS,
            "clippy-windows cf-daemon cf-process",
            "tests/windows-clippy.mjs",
            "",
            &["cf-daemon", "cf-process"],
        );
    }
}
