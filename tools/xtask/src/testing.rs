//! What the tests of the command modules share.

use std::ffi::OsString;
use std::path::PathBuf;

use cf_base::env::Env;

use crate::context::Context;
use crate::dispatch::{parse, Command, Parsed, Run};
use crate::process::Invocation;

/// Asserts that, among `commands`, the command line `line` is one that hands
/// `args` to the Node script `script`, run from the folder `from` (both from the
/// checkout's root): `node <script> <args>`, in the folder, and nothing else.
pub fn delegates(commands: &[Command], line: &str, script: &str, from: &str, args: &[&str]) {
    let context = Context {
        root: PathBuf::from("checkout"),
        env: Env::default(),
    };
    let words: Vec<OsString> = line.split_whitespace().map(OsString::from).collect();
    let table: Vec<&Command> = commands.iter().collect();
    let Parsed::Run {
        command,
        args: rest,
    } = parse(&table, &words, &context.root)
    else {
        panic!("{line:?} names no command of the table");
    };
    let Run::Node(found) = &command.run else {
        panic!("{line:?} runs in Rust, not by a script");
    };
    let expected = Invocation::new("node", context.path(from))
        .arg(context.path(script))
        .args(args.iter().copied());
    assert_eq!(found.invocation(&context, rest), expected, "{line}");
}
