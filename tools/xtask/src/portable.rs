//! The portable Windows exe (landing S2): the built app with its runtime
//! appended and a footer that says where the runtime starts. For now the
//! command hands its arguments to the script it replaces.

use crate::dispatch::{Command, Run, Script};

pub const COMMANDS: &[Command] = &[Command {
    words: &["portable"],
    about: "Pack the portable Windows exe from the built app",
    usage: "[--release DIR] [--out DIR] [--version X]",
    run: Run::Node(Script::at_root("app/scripts/portable.mjs")),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn portable_hands_its_options_to_the_script() {
        delegates(COMMANDS, "portable", "app/scripts/portable.mjs", "", &[]);
        delegates(
            COMMANDS,
            "portable --out dist --version 3.0.0",
            "app/scripts/portable.mjs",
            "",
            &["--out", "dist", "--version", "3.0.0"],
        );
    }
}
