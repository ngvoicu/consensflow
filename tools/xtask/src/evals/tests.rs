//! Which `cf-evals` command each word of xtask hands its words to, and how.

use std::path::PathBuf;

use super::*;

fn words(line: &str) -> Vec<OsString> {
    line.split_whitespace().map(OsString::from).collect()
}

fn said(invocation: &Invocation) -> String {
    invocation.display()
}

#[test]
fn each_command_is_a_verb_of_the_cf_evals_command_and_the_words_after_it_go_as_they_are() {
    let root = PathBuf::from("checkout");
    let run = invocation(
        &root,
        "run",
        &words("--scenario round-trip --chief claude --staff claude"),
    );
    assert_eq!(
        said(&run),
        "cargo run --locked --quiet --package cf-evals -- run --scenario round-trip --chief claude --staff claude"
    );
    assert_eq!(run.cwd, root);
    assert_eq!(
        said(&invocation(&root, "summary", &[])),
        "cargo run --locked --quiet --package cf-evals -- summary"
    );
    assert_eq!(
        said(&invocation(
            &root,
            "watch",
            &words("--project poker-lab --tokens")
        )),
        "cargo run --locked --quiet --package cf-evals -- watch --project poker-lab --tokens"
    );
    // A word with a space in it stays one word.
    let spaced = invocation(
        &root,
        "summary",
        &[
            OsString::from("--reports"),
            OsString::from("A Folder/reports"),
        ],
    );
    assert_eq!(
        spaced.args.last(),
        Some(&OsString::from("A Folder/reports"))
    );
}

#[test]
fn the_commands_are_the_ones_the_scripts_were_and_summary_is_found_before_eval() {
    let named: Vec<String> = COMMANDS
        .iter()
        .map(|command| command.words.join(" "))
        .collect();
    assert_eq!(named, ["eval summary", "eval", "watch"]);
    let summary = COMMANDS
        .iter()
        .position(|command| command.words == ["eval", "summary"]);
    let eval = COMMANDS
        .iter()
        .position(|command| command.words == ["eval"]);
    assert!(summary < eval, "{named:?}");
}

#[test]
fn what_each_command_takes_is_what_the_cf_evals_command_takes() {
    // The flags xtask says each takes are the ones the command's own usage has.
    let eval: &[&str] = &[
        "--scenario",
        "--chief",
        "--staff",
        "--arm",
        "--switch-to",
        "--gate",
        "--repeat",
        "--timeout-min",
        "--reports",
    ];
    let summary: &[&str] = &["--reports", "--results"];
    let watch: &[&str] = &["--project", "--minutes", "--tokens", "--home"];
    for (command, flags) in [("eval", eval), ("eval summary", summary), ("watch", watch)] {
        let row = COMMANDS
            .iter()
            .find(|row| row.words.join(" ") == command)
            .unwrap();
        for flag in flags {
            assert!(
                row.usage.contains(flag),
                "{command}: {flag} in {}",
                row.usage
            );
        }
    }
}
