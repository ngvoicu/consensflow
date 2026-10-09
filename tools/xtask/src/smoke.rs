//! The packaged smoke and the updater smoke (landing S6): the app as it is
//! built, run as a user would, and the update path proven end to end between
//! apps. For now each command hands its arguments to the script it replaces.

use crate::dispatch::{Command, Run, Script};

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
        usage: "[--from RELEASES] [--only WORDS] [--reuse] [--build-only] [--keep] \
                [--from-app APP --from-release NAME --to-app APP] [--timeout MS] ...  \
                (every option is in tests/smoke-updater.mjs)",
        run: Run::Node(Script::at_root("tests/smoke-updater.mjs")),
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
    fn smoke_updater_hands_every_option_to_the_script() {
        delegates(
            COMMANDS,
            "smoke-updater",
            "tests/smoke-updater.mjs",
            "",
            &[],
        );
        delegates(
            COMMANDS,
            "smoke-updater --from flip --only refused --keep",
            "tests/smoke-updater.mjs",
            "",
            &["--from", "flip", "--only", "refused", "--keep"],
        );
    }
}
