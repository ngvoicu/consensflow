//! What the app's bundle carries (landing S1): the native `cf` in `bin/`, the
//! resources the bundle is staged with, and the console host Windows ships
//! beside it. For now each command hands its arguments to the script it
//! replaces; the landing that ports one replaces its row.

use crate::dispatch::{Command, Run, Script};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["build-cf"],
        about: "Build the native cf and put it in bin/ (signed ad hoc on macOS)",
        usage: "[--offline]",
        run: Run::Node(Script::at_root("app/scripts/build-cf.mjs")),
    },
    Command {
        words: &["stage"],
        about:
            "Build cf and stage it, and on Windows the console host, as the app bundle's resources",
        usage: "",
        run: Run::Node(Script {
            file: "app/scripts/prepare-sidecar.mjs",
            from: "app",
        }),
    },
    Command {
        words: &["conpty"],
        about: "Fetch Microsoft's console host, check its SHA-256, and put its two files in DIR",
        usage: "--into DIR",
        run: Run::Node(Script::at_root("app/scripts/conpty.mjs")),
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::delegates;

    #[test]
    fn build_cf_hands_its_arguments_to_the_script_from_the_root() {
        delegates(COMMANDS, "build-cf", "app/scripts/build-cf.mjs", "", &[]);
        delegates(
            COMMANDS,
            "build-cf --offline",
            "app/scripts/build-cf.mjs",
            "",
            &["--offline"],
        );
    }

    #[test]
    fn stage_runs_the_script_from_app_as_the_apps_package_ran_it() {
        delegates(
            COMMANDS,
            "stage",
            "app/scripts/prepare-sidecar.mjs",
            "app",
            &[],
        );
    }

    #[test]
    fn conpty_hands_its_destination_to_the_script() {
        delegates(
            COMMANDS,
            "conpty --into app/src-tauri/target/release",
            "app/scripts/conpty.mjs",
            "",
            &["--into", "app/src-tauri/target/release"],
        );
    }
}
