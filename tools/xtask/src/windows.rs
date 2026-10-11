//! `cargo xtask windows` and `cargo xtask windows-matrix` (landing S14.4): the
//! Windows machine the checkout is tried on (`zeewin`, over ssh), and the matrix
//! of the chief evals run there. They are still the JavaScript tools `npm run
//! windows` and `npm run eval:windows`
//! (`tests/live/windows.mjs`, `tests/live/windows-matrix.mjs`), which the landing
//! that ports them replaces in this module: the rows below say so until then,
//! and the words they will take are in their `usage`.

use std::ffi::OsString;

use crate::context::Context;
use crate::dispatch::{not_yet, Command, Console, Failure, Run};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["windows"],
        about: "Run a command on the Windows machine the checkout is tried on, over ssh",
        usage: "--host HOST [--build] [--dir DIR] [--in DIR] [--env NAME=VALUE ...] -- <command ...>",
        run: Run::Native(windows),
    },
    Command {
        words: &["windows-matrix"],
        about: "Run the chief evals on the Windows machine, a pair of chief and staff at a time",
        usage: "--host HOST [--build] [--scenario NAME ...] [--pair CHIEF:STAFF ...] [--claude-model M] [--pi-model M] [--env NAME=VALUE ...]",
        run: Run::Native(windows_matrix),
    },
];

fn windows(_: &Context, _: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    not_yet("windows", "run `npm run windows` for now")
}

fn windows_matrix(_: &Context, _: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    not_yet("windows-matrix", "run `npm run eval:windows` for now")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use cf_base::env::Env;

    fn run(command: &Command) -> Failure {
        let context = Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let Run::Native(run) = &command.run;
        let said = run(
            &context,
            &[OsString::from("--host"), OsString::from("zeewin")],
            &mut console,
        );
        assert!(
            out.is_empty() && err.is_empty(),
            "it starts nothing and says nothing itself"
        );
        said.unwrap_err()
    }

    #[test]
    fn until_the_landing_that_ports_them_the_commands_say_so_and_what_to_run() {
        let said: Vec<String> = COMMANDS
            .iter()
            .map(|command| run(command).to_string())
            .collect();
        assert_eq!(
            said,
            [
                "windows is not ported yet: run `npm run windows` for now",
                "windows-matrix is not ported yet: run `npm run eval:windows` for now"
            ]
        );
        for command in COMMANDS {
            assert!(matches!(run(command), Failure::NotYet { .. }));
        }
    }
}
