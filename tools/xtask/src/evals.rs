//! `cargo xtask eval`, `eval summary` and `watch` (landing S14.3): the chief
//! evals, their summary and the look at a project that is running. The code is
//! `crates/cf-evals` (a library and a binary), which these commands hand their
//! words to; until the landing that ports it they are still the JavaScript tools
//! `npm run eval`, `npm run eval:summary` and `npm run watch` (`evals/run.mjs`,
//! `evals/summarize.mjs`, `evals/watch.mjs`), and the rows below say so.
//!
//! `eval summary` comes before `eval` in the table: a command is found by the
//! first row whose words begin the command line.

use std::ffi::OsString;

use crate::context::Context;
use crate::dispatch::{not_yet, Command, Console, Failure, Run};

pub const COMMANDS: &[Command] = &[
    Command {
        words: &["eval", "summary"],
        about: "Write the summary of the eval reports kept in evals/reports (evals/RESULTS.md)",
        usage: "",
        run: Run::Native(summary),
    },
    Command {
        words: &["eval"],
        about: "Run a chief eval: a scenario, played by a chief and a staff of real harnesses on the native daemon",
        usage: "[--scenario NAME] [--chief HARNESS] [--staff HARNESS,...] [--model M] [--arm ARM] [--repeat N] [--gate] ...",
        run: Run::Native(eval),
    },
    Command {
        words: &["watch"],
        about: "Look at a project that is running: its tasks, its messages, and the tokens its windows used",
        usage: "[--project N] [--minutes N] [--tokens] [--home DIR]",
        run: Run::Native(watch),
    },
];

fn summary(_: &Context, _: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    not_yet("eval summary", "run `npm run eval:summary` for now")
}

fn eval(_: &Context, _: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    not_yet("eval", "run `npm run eval` for now")
}

fn watch(_: &Context, _: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    not_yet("watch", "run `npm run watch` for now")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use cf_base::env::Env;

    #[test]
    fn until_the_landing_that_ports_them_the_commands_say_so_and_what_to_run() {
        let context = Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        };
        let said: Vec<String> = COMMANDS
            .iter()
            .map(|command| {
                let (mut out, mut err) = (Vec::new(), Vec::new());
                let mut console = Console {
                    out: &mut out,
                    err: &mut err,
                };
                let Run::Native(run) = &command.run;
                let failed = run(&context, &[], &mut console).unwrap_err();
                assert!(out.is_empty() && err.is_empty());
                failed.to_string()
            })
            .collect();
        assert_eq!(
            said,
            [
                "eval summary is not ported yet: run `npm run eval:summary` for now",
                "eval is not ported yet: run `npm run eval` for now",
                "watch is not ported yet: run `npm run watch` for now",
            ]
        );
    }

    #[test]
    fn eval_summary_is_found_before_eval_which_would_take_its_words_for_its_own() {
        let words: Vec<_> = COMMANDS.iter().map(|command| command.words.len()).collect();
        let summary = COMMANDS
            .iter()
            .position(|command| command.words == ["eval", "summary"]);
        let eval = COMMANDS
            .iter()
            .position(|command| command.words == ["eval"]);
        assert!(summary < eval, "{words:?}");
    }
}
