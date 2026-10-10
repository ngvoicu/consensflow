//! The packaged smoke and the updater smoke: the app as it is built, run as a
//! user would, and the update path proven end to end between apps. The updater
//! smoke runs in Rust (`updater_smoke`, landing S12); the packaged smoke, for now,
//! hands its arguments to the script it replaces.

use crate::dispatch::{Command, Run, Script};
use crate::updater_smoke;

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["smoke"],
        about: "Run the packaged smoke on the built app, or on the one --app names",
        usage: "[--app PATH]",
        run: Run::Node(Script::at_root("tests/smoke.mjs")),
    },
    Command {
        words: &["smoke-updater"],
        about:
            "Prove the update path on the packaged app, from each release it can be installed from",
        usage: updater_smoke::USAGE,
        run: Run::Native(updater_smoke::run),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn smoke_hands_the_app_it_is_given_to_the_script() {
        delegates(COMMANDS, "smoke", "tests/smoke.mjs", "", &[]);
        delegates(
            COMMANDS,
            "smoke --app dist/ConsensFlow.app",
            "tests/smoke.mjs",
            "",
            &["--app", "dist/ConsensFlow.app"],
        );
    }

    #[test]
    fn the_updater_smoke_runs_in_rust_and_is_no_script() {
        let command = COMMANDS
            .iter()
            .find(|command| command.words == ["smoke-updater"]);
        assert!(matches!(command.map(|c| &c.run), Some(Run::Native(_))));
    }
}
