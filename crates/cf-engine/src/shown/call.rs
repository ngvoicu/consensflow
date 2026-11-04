//! What a note may say of the last tool call a window's record shows: what the window was
//! doing when it closed under its task (kluris T-153: a `cargo test --workspace` the project's
//! own tests had killed), which no screen of it says.
//!
//! **A command is quoted only when every word of it can be told safe.** A command line
//! carries a credential in more shapes than a list holds: `curl -u user:pass`, a URL's user,
//! `mysql -psecret`, `--with-token`, a token as the word after `cargo login`, a password
//! typed in a quoted string or a heredoc. The first version quoted what the masker and the
//! secret-name test let through, and `curl -u alice:swordfish https://example.test` reached
//! the ledger (the review of `a26e0848e`, P1). So the rule is the other way: a command is
//! said when it is made of plain words (no quotes, no parentheses or backticks, so no
//! substitution, and no heredoc), each of its
//! commands (split at `&&`, `||`, `;`, `|`, `&` and the end of a line) is a build, test,
//! version-control or file tool the list below names, none takes a credential by its
//! subcommand or an environment variable it was given, and neither the secret-name test, the
//! masker nor a URL's user finds anything in the whole of it. Anything else is left out, and
//! the note says the tool alone. The decision is on the whole command, before the note's bound
//! cuts it (a secret past the 200 characters is still a secret in the call).
//!
//! What a tool was given as text of its own (a script, a patch, a prompt, a URL, a query) never
//! reaches here: the reader keeps it as nothing (`cf-harness`' `calls`). A file a tool works on
//! is said unless its name names a secret or is token-shaped.

use std::sync::LazyLock;

use cf_harness::records::LastCall;
use regex::Regex;

use super::{cut_to, mask::mask, secret::names_a_secret};

/// The most characters the text of a call takes in a note, between its quotation marks.
const CALL_CHARS: usize = 200;

/// The most characters a tool's name takes.
const TOOL_CHARS: usize = 40;

/// The commands a note may quote: build, test, version-control and file tools, whose words
/// are flags, names and paths. A tool that takes a credential or runs the text it is given
/// (`curl`, `ssh`, `mysql`, `docker`, `gh`, `bash -c`, `echo`, `sed`, `grep`) is not here.
const SAFE_TOOLS: [&str; 62] = [
    "cargo", "rustc", "rustfmt", "npm", "npx", "yarn", "pnpm", "bun", "tsc", "vitest", "jest",
    "biome", "eslint", "prettier", "make", "cmake", "ninja", "git", "ls", "cd", "pwd", "cat",
    "head", "tail", "wc", "diff", "sort", "uniq", "mkdir", "rmdir", "rm", "cp", "mv", "touch",
    "chmod", "tar", "unzip", "gzip", "gunzip", "which", "whoami", "date", "sleep", "true", "false",
    "test", "python", "python3", "pytest", "node", "go", "mvn", "gradle", "dotnet", "java", "ruby",
    "tree", "kill", "pkill", "killall", "file", "stat",
];

/// The tools that run the text they are given as code: `-c`, `-e` and their kin say it.
const INTERPRETERS: [&str; 6] = ["python", "python3", "node", "ruby", "perl", "bun"];

/// The flags of an interpreter that take code from the line.
const CODE_FLAGS: [&str; 7] = ["-c", "-e", "-E", "--eval", "-p", "--print", "-"];

/// The environment variables a command may be given on its line: none that holds a secret by
/// its name, and the values of these are settings.
const SAFE_VARIABLES: [&str; 8] = [
    "RUST_LOG",
    "RUST_BACKTRACE",
    "CARGO_TARGET_DIR",
    "CARGO_BUILD_JOBS",
    "NODE_ENV",
    "CI",
    "FORCE_COLOR",
    "NO_COLOR",
];

/// The subcommands that take a credential on the line or store one, by tool.
const CREDENTIAL_SUBCOMMANDS: [(&str, &[&str]); 6] = [
    ("cargo", &["login", "logout", "owner", "yank"]),
    (
        "npm",
        &[
            "login", "logout", "adduser", "token", "config", "set", "auth",
        ],
    ),
    ("yarn", &["login", "logout", "npm", "config", "set", "auth"]),
    ("pnpm", &["login", "logout", "config", "set", "auth"]),
    ("npx", &["login", "adduser"]),
    ("git", &["credential", "config"]),
];

/// A URL with a user in it, with or without a password: `scheme://user:pass@host`.
#[allow(clippy::expect_used)]
static USER_IN_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[A-Za-z][A-Za-z0-9+.-]*://[^/\s]*@").expect("a pattern of call"));

/// A redirection, whose target is a path: `2>&1`, `&>`, `> file`, `>> file`, `< file`.
#[allow(clippy::expect_used)]
static REDIRECTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\d*>&\d+|&>|\d*>>?\s*[^\s|&;<>]+|<\s*[^\s|&;<>]+").expect("a pattern of call")
});

/// Where a command ends and the next begins.
#[allow(clippy::expect_used)]
static SEPARATOR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"&&|\|\||[;|&\n]").expect("a pattern of call"));

/// What a note says of `call`, after "; ", or nothing where the call names nothing.
pub(super) fn words(call: &LastCall) -> String {
    let tool = label(&call.tool);
    if let Some(command) = &call.command {
        return if quotable_command(command) {
            let one_line = command.split_whitespace().collect::<Vec<_>>().join(" ");
            format!(
                "; its last tool call was: \"{}\"",
                cut_to(&one_line, CALL_CHARS)
            )
        } else {
            left_out(&tool)
        };
    }
    if let Some(target) = &call.target {
        return if quotable_target(target) {
            let said = if tool.is_empty() {
                target.clone()
            } else {
                format!("{tool} {target}")
            };
            format!(
                "; its last tool call was: \"{}\"",
                cut_to(&said, CALL_CHARS)
            )
        } else {
            left_out(&tool)
        };
    }
    if tool.is_empty() {
        String::new()
    } else {
        format!("; its last tool call was {tool}")
    }
}

/// What a note says of a call whose command or file it cannot say.
fn left_out(tool: &str) -> String {
    if tool.is_empty() {
        "; its last tool call was a command that may name a secret, so it is left out".to_owned()
    } else {
        format!("; its last tool call was {tool}; what it was given may name a secret, so it is left out")
    }
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

/// Whether a shell command may be quoted: see the module's rule.
fn quotable_command(command: &str) -> bool {
    let plain = command
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || " \t\n_-./:=+,@%^~*?[]{}$|&;<>#!\\".contains(c));
    if !plain
        || command.contains("<<")
        || names_a_secret(command)
        || mask(command).count > 0
        || USER_IN_URL.is_match(command)
    {
        return false;
    }
    let joined = command.replace("\\\n", " ");
    let bare = REDIRECTION.replace_all(&joined, " ");
    let mut any = false;
    for segment in SEPARATOR.split(&bare) {
        let words: Vec<&str> = segment.split_whitespace().collect();
        if words.is_empty() {
            continue;
        }
        any = true;
        if !quotable_segment(&words) {
            return false;
        }
    }
    any
}

/// Whether one command of a line (`words`, its variables first) may be quoted.
fn quotable_segment(words: &[&str]) -> bool {
    let mut at = 0;
    while let Some((name, value)) = words.get(at).and_then(|word| word.split_once('=')) {
        let a_name = !name.is_empty()
            && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            && !name.starts_with(|c: char| c.is_ascii_digit());
        if !a_name {
            break;
        }
        let setting = value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:,=-".contains(c));
        if !SAFE_VARIABLES.contains(&name) || !setting {
            return false;
        }
        at += 1;
    }
    let Some(first) = words.get(at) else {
        return false;
    };
    let tool = first.rsplit('/').next().unwrap_or(first);
    if !SAFE_TOOLS.contains(&tool) {
        return false;
    }
    let rest = &words[at + 1..];
    if INTERPRETERS.contains(&tool) && rest.iter().any(|word| CODE_FLAGS.contains(word)) {
        return false;
    }
    if tool == "git" && rest.contains(&"-c") {
        return false;
    }
    let subcommand = rest.iter().find(|word| !word.starts_with('-'));
    CREDENTIAL_SUBCOMMANDS
        .iter()
        .filter(|(name, _)| *name == tool)
        .all(|(_, denied)| subcommand.is_none_or(|word| !denied.contains(word)))
}

#[cfg(test)]
mod tests;
