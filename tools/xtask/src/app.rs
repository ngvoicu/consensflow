//! The app crate's tests and lint where the app cannot be built as it ships
//! (landing S6): a worktree has no staged resources, and the build script refuses
//! without them, so these leave the resources out of the Tauri configuration. For
//! now each command hands its arguments to the script it replaces.

use crate::dispatch::{Command, Run, Script};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["app", "test"],
        about: "Run the app crate's tests, with its bundled resources left out",
        usage: "[test binary arguments: a filter such as portable::, --nocapture]",
        run: Run::Node(Script::at_root("tests/app-tests.mjs")),
    },
    Command {
        words: &["app", "clippy"],
        about: "Lint the app crate, tests included and warnings denied, with its bundled resources left out",
        usage: "[more clippy arguments]",
        run: Run::Node(Script::at_root("tests/app-clippy.mjs")),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn app_test_hands_the_filter_and_the_options_to_the_script() {
        delegates(COMMANDS, "app test", "tests/app-tests.mjs", "", &[]);
        delegates(
            COMMANDS,
            "app test portable::",
            "tests/app-tests.mjs",
            "",
            &["portable::"],
        );
        delegates(
            COMMANDS,
            "app test -- --nocapture",
            "tests/app-tests.mjs",
            "",
            &["--", "--nocapture"],
        );
    }

    #[test]
    fn app_clippy_hands_its_arguments_to_the_script() {
        delegates(COMMANDS, "app clippy", "tests/app-clippy.mjs", "", &[]);
        delegates(
            COMMANDS,
            "app clippy -W clippy::all",
            "tests/app-clippy.mjs",
            "",
            &["-W", "clippy::all"],
        );
    }
}
