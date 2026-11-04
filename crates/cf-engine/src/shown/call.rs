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
//! **Never a file's name either** (the re-check T-1224): a path carries a secret in more shapes
//! than a rule can list, and each was found: a token split by blanks, a key's body of twelve
//! characters, a URL with its credentials written in percent signs
//! (`/tmp/https%3A%2F%2Falice%3Aswordfish%40example.test.json`). A call that works on a file is
//! the tool's name alone in alpha.2. A tool given text of its own (a script, a patch, a prompt,
//! a URL, a query) never reaches here: the reader keeps it as nothing (`cf-harness`' `calls`).

use cf_harness::records::LastCall;

use super::{cut_to, mask::mask};

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

#[cfg(test)]
mod tests;
