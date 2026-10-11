//! `cargo xtask scrub-transcript <transcript.jsonl> <fixture.jsonl>`: a Claude
//! transcript with what identifies a person taken out, which is how the fixtures
//! of `crates/cf-harness/tests/fixtures/claude` are made. It is still the
//! JavaScript tool `tests/live/scrub-transcript.mjs` (and its test,
//! `tests/scrub-transcript.test.mjs`), which the landing that ports it replaces
//! in this module; the row below says so until then.

use std::ffi::OsString;

use crate::context::Context;
use crate::dispatch::{not_yet, Command, Console, Failure, Run};

pub const COMMANDS: &[Command] = &[Command {
    words: &["scrub-transcript"],
    about: "Take out of a Claude transcript what identifies a person, to make a fixture of it",
    usage: "<transcript.jsonl> <fixture.jsonl>",
    run: Run::Native(scrub_transcript),
}];

fn scrub_transcript(_: &Context, _: &[OsString], _: &mut Console) -> Result<i32, Failure> {
    not_yet(
        "scrub-transcript",
        "run `node tests/live/scrub-transcript.mjs <transcript.jsonl> <fixture.jsonl>` for now",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use cf_base::env::Env;

    #[test]
    fn until_the_landing_that_ports_it_the_command_says_so_and_what_to_run() {
        let context = Context {
            root: PathBuf::from("checkout"),
            env: Env::default(),
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut console = Console {
            out: &mut out,
            err: &mut err,
        };
        let Run::Native(run) = &COMMANDS[0].run;
        let said = run(&context, &[], &mut console).unwrap_err();
        assert_eq!(
            said.to_string(),
            "scrub-transcript is not ported yet: run `node tests/live/scrub-transcript.mjs <transcript.jsonl> <fixture.jsonl>` for now"
        );
        assert!(out.is_empty() && err.is_empty());
    }
}
