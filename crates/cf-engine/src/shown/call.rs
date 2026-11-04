//! What a note may say of the last tool call a window's record shows: what the window was
//! doing when it closed under its task (kluris T-153: a `cargo test --workspace` the project's
//! own tests had killed), which no screen of it says.
//!
//! **Never the arguments of a command.** A command line carries a credential in more shapes
//! than any rule can list, and each version that tried failed: the first quoted what the masker
//! and the secret-name test let through (`curl -u alice:swordfish`), the second quoted "plain
//! words of a listed tool" and the review of `860010398` read a password out of
//! `npm exec -- docker login -p …` (a wrapper), `git -C /w config http.extraHeader …` (an option
//! with an argument before the subcommand) and `node login.js alice …` (a script with positional
//! credentials). So the note says the tool (`Bash`) and, for a shell call, the first word of the
//! command where it is a program the list below names, and nothing after it: `Bash: "cargo …"`.
//! A command whose first word is not on the list (a variable assignment, `sudo`, `curl`, a path
//! into someone's folder) is the tool's name alone. What `cargo test --workspace` was running
//! when the window closed is the cause, and `cargo …` points at it; the rest of the line is the
//! record's, which the human can read.
//!
//! A tool given text of its own (a script, a patch, a prompt, a URL, a query) never reaches
//! here: the reader keeps it as nothing (`cf-harness`' `calls`). A file a tool works on is
//! said, masked, unless its name names a secret or is token-shaped.

use std::sync::LazyLock;

use cf_harness::records::LastCall;
use regex::Regex;

use super::{cut_to, mask::mask, secret::names_a_secret};

/// The most characters the text of a call takes in a note, between its quotation marks.
const CALL_CHARS: usize = 200;

/// The most characters a tool's name takes.
const TOOL_CHARS: usize = 40;

/// The programs a note may name as the first word of a shell call: build, test,
/// version-control and file tools. The program only: what it was given is never said, so a
/// program that takes a credential on its line (`git`, `npm`, `cargo login`) is named safely,
/// and the list is of programs worth naming, not of programs that are safe to run.
const PROGRAMS: [&str; 42] = [
    "cargo", "rustc", "rustfmt", "npm", "npx", "yarn", "pnpm", "bun", "tsc", "vitest", "jest",
    "biome", "eslint", "prettier", "make", "cmake", "ninja", "git", "ls", "cat", "head", "tail",
    "wc", "diff", "mkdir", "rm", "cp", "mv", "tar", "python", "python3", "pytest", "node", "go",
    "mvn", "gradle", "dotnet", "java", "ruby", "kill", "pkill", "killall",
];

/// A URL with a user in it, with or without a password: `scheme://user:pass@host`.
#[allow(clippy::expect_used)]
static USER_IN_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z][A-Za-z0-9+.-]*://[^/\s]*@").expect("a pattern of call"));

/// What a note says of `call`, after "; ", or nothing where the call names nothing.
pub(super) fn words(call: &LastCall) -> String {
    let tool = label(&call.tool);
    if let Some(command) = &call.command {
        return match program(command) {
            Some((program, more)) => {
                let said = if more {
                    format!("{program} \u{2026}")
                } else {
                    program.to_owned()
                };
                let who = if tool.is_empty() {
                    "a command".to_owned()
                } else {
                    tool
                };
                format!("; its last tool call was {who}: \"{said}\"")
            }
            None if tool.is_empty() => "; its last tool call was a command".to_owned(),
            None => format!("; its last tool call was {tool}"),
        };
    }
    if let Some(target) = &call.target {
        if quotable_target(target) {
            let said = if tool.is_empty() {
                target.clone()
            } else {
                format!("{tool} {target}")
            };
            return format!(
                "; its last tool call was: \"{}\"",
                cut_to(&said, CALL_CHARS)
            );
        }
    }
    if tool.is_empty() {
        String::new()
    } else {
        format!("; its last tool call was {tool}")
    }
}

/// The program a shell command begins with, where the list names it, and whether the command
/// goes on after it. The first word as it is written: a path to it is its last part, and
/// nothing else is looked at.
fn program(command: &str) -> Option<(&'static str, bool)> {
    let mut words = command.split_whitespace();
    let first = words.next()?;
    let name = first.rsplit('/').next().unwrap_or(first);
    let listed = PROGRAMS.iter().find(|program| **program == name)?;
    let written = first
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c));
    written.then(|| (*listed, words.next().is_some()))
}

/// A tool's name as a note says it: masked, on one line, cut short.
fn label(tool: &str) -> String {
    let one_line = tool.split_whitespace().collect::<Vec<_>>().join(" ");
    cut_to(&mask(&one_line).text.replace('"', "'"), TOOL_CHARS)
}

/// Whether a file a tool works on may be said: its name names no secret and is no token.
fn quotable_target(target: &str) -> bool {
    !target.contains('\n')
        && !names_a_secret(target)
        && mask(target).count == 0
        && !USER_IN_URL.is_match(target)
}

#[cfg(test)]
mod tests;
