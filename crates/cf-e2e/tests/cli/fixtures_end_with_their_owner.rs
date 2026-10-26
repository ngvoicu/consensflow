//! No test fixture outlives the test process that started it (T-686, T-693). A program a
//! test starts to be waited on, ended or left alone runs as long as its script says, and a
//! test that is ended on its way (a harness whose output is closed exits at once with its
//! tests in flight, a run is killed) never sends it the signal it waits for: a shell that
//! ignored `TERM` and looped went on for eight hours, two of them, and twenty-six that
//! ignored `TERM` and `HUP` for four days. So a fixture that does not end of itself ends when
//! its owner does: `while kill -0 $PPID 2>/dev/null; do …`, or the shapes of
//! `cf_base::fixture`, which are that.
//!
//! What the rule refuses, in a string of a Rust test (its comments are not read), unless the
//! string has `kill -0 $PPID` in it (the owner is looked at: `kill -0 {owner}` where it is not
//! the parent; `os.getppid()` in Python's) or counts
//! its tries out (`$((n + 1))` against a `-gt`):
//! - a loop that never ends of itself: `while :`, `while true`, `until false`, a Python
//!   `while True`;
//! - a loop that waits for a file or a process to appear, which never does if the test is
//!   gone: `while [ ! -s file ]`, `until [ -e file ]`, `until ps …`;
//! - a pipe held open for reading and writing (`exec 3<>fifo`), whose reader never sees an end;
//! - a program that ignores signals (`trap '' TERM`, Python's `signal.SIG_IGN`) and runs on
//!   for over a minute (`sleep 61`, `time.sleep(61)`) or for as long as a placeholder says
//!   (`sleep {SECONDS}`).
//!
//! A program that sleeps for a minute or less is ended by its own length, and one on a
//! terminal or a pipe of the test is ended by the hangup when the test is gone: neither is
//! asked for. The rule reads Rust: the JavaScript tests start no program that runs on. The
//! few strings it lets, and why, are in [`LET`].

use std::fs;

use cf_e2e::checkout;

use crate::Outcome;

/// The folders of the sources that start programs.
const LOOKED_IN: [&str; 3] = ["crates", "tools", "app/src-tauri"];

/// What is built, installed or fetched there, not looked in.
const UNSOURCED: [&str; 3] = [".git", "node_modules", "target"];

/// This file names the shapes it refuses, as data.
const THIS: &str = "crates/cf-e2e/tests/cli/fixtures_end_with_their_owner.rs";

/// The longest a program that ignores signals may sleep without its owner being looked at: a
/// minute is what the crates' tests keep to (`cf_process::testing`).
const LONGEST_DEAF_SLEEP: f64 = 60.0;

/// The strings that run on and are let: the file, a piece of the string, and why. Each is
/// looked for, and one that is no longer there is to be taken out.
const LET: [(&str, &str, &str); 5] = [
    (
        "crates/cf-e2e/tests/live_monitor_wait.rs",
        "cf-mark-noisy",
        "a prompt for a real model: the monitor it names ends at the timeout_ms the prompt gives it, 30 s",
    ),
    (
        "crates/cf-e2e/tests/live_stops/slow.rs",
        "sleep {IGNORE_S}",
        "the hook of a live run, which ignores the signals for 100 s so that the product's force is seen; a live run's windows are Windows ones too, where Git Bash's `kill -0` does not see the runner",
    ),
    (
        "crates/cf-e2e/tests/live_stops/slow.rs",
        "sleep 100;",
        "the text of that hook, as the case that pins it says it",
    ),
    (
        "crates/cf-e2e/tests/live_stops/stuck.rs",
        "sleep {STUCK_S}",
        "the command of a live run, which ignores the signals for 200 s; a Windows one too, as the hook",
    ),
    (
        "crates/cf-e2e/tests/live_stops/stuck.rs",
        "sleep 200",
        "the text of that command, as the case that pins it says it",
    ),
];

/// A string of a source, as the source spells it, and the line it begins on.
struct Literal {
    line: usize,
    text: String,
}

/// A string that runs on, what it does, and where.
struct Found {
    line: usize,
    what: &'static str,
    text: String,
}

/// The strings of Rust source `source`, comments left out: ordinary, byte and raw strings.
fn literals(source: &str) -> Vec<Literal> {
    let chars: Vec<char> = source.chars().collect();
    let (mut found, mut at, mut line) = (Vec::new(), 0, 1);
    let at_ident = |at: usize| {
        at.checked_sub(1)
            .and_then(|before| chars.get(before))
            .is_some_and(|c| c.is_alphanumeric() || *c == '_')
    };
    while at < chars.len() {
        match chars[at] {
            '\n' => {
                line += 1;
                at += 1;
            }
            '/' if chars.get(at + 1) == Some(&'/') => {
                while at < chars.len() && chars[at] != '\n' {
                    at += 1;
                }
            }
            '/' if chars.get(at + 1) == Some(&'*') => {
                let mut depth = 0;
                while at < chars.len() {
                    if chars[at] == '/' && chars.get(at + 1) == Some(&'*') {
                        depth += 1;
                        at += 2;
                    } else if chars[at] == '*' && chars.get(at + 1) == Some(&'/') {
                        depth -= 1;
                        at += 2;
                        if depth == 0 {
                            break;
                        }
                    } else {
                        if chars[at] == '\n' {
                            line += 1;
                        }
                        at += 1;
                    }
                }
            }
            'r' | 'b' if !at_ident(at) && raw_string_at(&chars, at).is_some() => {
                let (opens, hashes) = raw_string_at(&chars, at).unwrap_or((at, 0));
                let began = line;
                let body = opens + 1;
                let mut end = body;
                while end < chars.len()
                    && !(chars[end] == '"'
                        && chars.len() >= end + 1 + hashes
                        && chars[end + 1..].iter().take(hashes).all(|c| *c == '#'))
                {
                    if chars[end] == '\n' {
                        line += 1;
                    }
                    end += 1;
                }
                found.push(Literal {
                    line: began,
                    text: chars[body..end.min(chars.len())].iter().collect(),
                });
                at = (end + 1 + hashes).min(chars.len());
            }
            '"' => {
                let began = line;
                let body = at + 1;
                let mut end = body;
                while end < chars.len() && chars[end] != '"' {
                    if chars[end] == '\\' {
                        if chars.get(end + 1) == Some(&'\n') {
                            line += 1;
                        }
                        end += 1;
                    } else if chars[end] == '\n' {
                        line += 1;
                    }
                    end += 1;
                }
                found.push(Literal {
                    line: began,
                    text: chars[body..end.min(chars.len())].iter().collect(),
                });
                at = end + 1;
            }
            '\'' => {
                if chars.get(at + 1) == Some(&'\\') {
                    // A character with an escape: up to its closing quote.
                    at += 3;
                    while at < chars.len() && chars[at] != '\'' {
                        at += 1;
                    }
                    at += 1;
                } else if chars.get(at + 2) == Some(&'\'') {
                    at += 3;
                } else {
                    // A lifetime or a label.
                    at += 1;
                }
            }
            _ => at += 1,
        }
    }
    found
}

/// Where the raw string that begins at `at` opens (the place of its quote) and how many `#`
/// it is written with: none where a raw string does not begin there.
fn raw_string_at(chars: &[char], at: usize) -> Option<(usize, usize)> {
    let mut next = at;
    if chars.get(next) == Some(&'b') {
        next += 1;
    }
    if chars.get(next) != Some(&'r') {
        return None;
    }
    next += 1;
    let mut hashes = 0;
    while chars.get(next) == Some(&'#') {
        hashes += 1;
        next += 1;
    }
    (chars.get(next) == Some(&'"')).then_some((next, hashes))
}

/// The words of a string as a shell reads them: split at white space, at `;`, `&`, `|`, `(`
/// and `)`, the escapes a Rust string writes a script's lines and quotes with read as what
/// they are.
fn words(text: &str) -> Vec<String> {
    let mut flat = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            flat.push(c);
            continue;
        }
        match chars.next() {
            // A line continued: the next line's indent is not part of it.
            Some('\n') => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
                flat.push(' ');
            }
            Some('n' | 't' | 'r') => flat.push(' '),
            Some(other) => flat.push(other),
            None => {}
        }
    }
    flat.split(|c: char| c.is_whitespace() || ";&|()".contains(c))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// The ways a string runs on without an end of its own, named for the report: none for a
/// string that has its owner looked at (`kill -0`), counts its tries out, or runs on for no
/// more than a minute.
fn unbounded(text: &str) -> Vec<&'static str> {
    let words = words(text);
    let word = |at: usize| words.get(at).map_or("", String::as_str);
    // `kill -0 $PPID`, or the number of the owner the script was given (`kill -0 {owner}`,
    // `kill -0 4242`): not `kill -0 $job`, which looks at something else.
    let looks_at_its_owner = words.windows(3).any(|three| {
        three[0] == "kill"
            && three[1] == "-0"
            && (three[2] == "$PPID"
                || three[2].starts_with('{')
                || three[2].chars().all(|c| c.is_ascii_digit()))
    }) || words.iter().any(|word| word.contains("getppid"));
    // A loop that counts its tries out: `n=$((n + 1))` and a `-gt` against a number.
    let counts_out = text.contains("$((")
        && words
            .iter()
            .any(|word| ["-gt", "-lt", "-ge", "-le"].contains(&word.as_str()));
    if looks_at_its_owner || counts_out {
        return Vec::new();
    }
    let mut found = Vec::new();
    let (mut deaf, mut long_sleep) = (false, false);
    for at in 0..words.len() {
        match (word(at), word(at + 1)) {
            ("while", ":" | "true" | "True" | "True:") | ("until", "false" | "False") => {
                found.push("a loop that never ends");
            }
            ("while" | "until", "[") => {
                let test = if word(at + 2) == "!" {
                    word(at + 3)
                } else {
                    word(at + 2)
                };
                if ["-e", "-s", "-f", "-d", "-r", "-w", "-p", "-S"].contains(&test) {
                    found.push("a loop that waits for a file");
                }
            }
            ("until", "ps" | "pgrep") => found.push("a loop that waits for a process"),
            ("trap", "''" | "\"\"") => deaf = true,
            (sleep, seconds)
                if sleep == "sleep" || sleep.ends_with("/sleep") || sleep == "time.sleep" =>
            {
                match seconds.parse::<f64>() {
                    Ok(seconds) => long_sleep |= seconds > LONGEST_DEAF_SLEEP,
                    // As long as a placeholder or a variable says.
                    Err(_) => long_sleep |= seconds.starts_with(['{', '$']),
                }
            }
            _ => {}
        }
        // Python's way of ignoring a signal: `signal.signal(signal.SIGHUP, signal.SIG_IGN)`.
        if word(at) == "signal.SIG_IGN" {
            deaf = true;
        }
        // `3<>file`: a descriptor opened for reading and writing.
        if word(at).starts_with(|c: char| c.is_ascii_digit())
            && word(at).get(1..).is_some_and(|rest| rest.starts_with("<>"))
        {
            found.push("a pipe held open for reading and writing");
        }
    }
    if deaf && long_sleep {
        found.push("a program that ignores signals and sleeps for over a minute");
    }
    found.sort_unstable();
    found.dedup();
    found
}

/// Every string of Rust source `source` that runs on without an end of its own.
fn unowned(source: &str) -> Vec<Found> {
    literals(source)
        .into_iter()
        .flat_map(|literal| {
            unbounded(&literal.text).into_iter().map(move |what| Found {
                line: literal.line,
                what,
                text: literal.text.clone(),
            })
        })
        .collect()
}

#[test]
fn a_script_that_runs_on_for_ever_is_found_and_one_that_looks_at_its_owner_or_ends_is_not() {
    let runs_on = [
        "trap '' TERM HUP; printf ready; while :; do sleep 1; done",
        "trap '' TERM; while true; do sleep 0.05; done",
        "i=0; while :; do i=$((i+1)); sleep 1; done",
        "while [ ! -s '{file}' ]; do sleep 0.01; done; echo said",
        "until [ -e go ]; do sleep 1; done",
        "until ps -o comm= -p $! | grep -q python; do sleep 0.05; done",
        "exec 3<>\"$0\"; while read line <&3; do :; done",
        "trap '' TERM HUP; exec /bin/sleep 1000",
        "trap '' INT TERM HUP; sleep {IGNORE_S}",
        "import time\nwhile True:\n    time.sleep(1)",
        "import signal, time\nsignal.signal(signal.SIGHUP, signal.SIG_IGN)\ntime.sleep(1000)",
        "trap '' TERM; \\\n   while \\\n   :; do :; done",
        "while :; do kill -0 $job 2>/dev/null || break; sleep 1; done",
    ];
    for script in runs_on {
        assert!(!unbounded(script).is_empty(), "{script}");
    }
    let ends = [
        "trap '' TERM; while kill -0 $PPID 2>/dev/null; do sleep 0.05; done",
        "trap '' TERM; while kill -0 {owner} 2>/dev/null; do sleep 0.05; done",
        "trap '' TERM; while kill -0 4242 2>/dev/null; do sleep 0.05; done",
        "while [ ! -s '{file}' ] && kill -0 $PPID 2>/dev/null; do sleep 0.01; done",
        "until ps -o comm= -p $! | grep -q python || ! kill -0 $PPID 2>/dev/null; do sleep 0.05; done",
        "n=0; while [ ! -e \"$1\" ]; do n=$((n + 1)); if [ \"$n\" -gt 1200 ]; then exit 1; fi; sleep 0.05; done",
        "import os, time\nparent = os.getppid()\nwhile os.getppid() == parent:\n    time.sleep(1)",
        "trap '' TERM HUP; sleep 37 & echo $!; wait",
        "trap '' TERM; exec sleep 60",
        "sleep 1000 & wait",
        "i=0; while [ $i -lt 20000 ]; do printf x; i=$((i+1)); done",
        "while read -r line; do :; done",
        "trap 'echo x' TERM; sleep 600",
        "sleep 4711",
    ];
    for script in ends {
        assert!(unbounded(script).is_empty(), "{script}");
    }
}

#[test]
fn only_the_strings_of_code_are_read() {
    let source = "// while :; do :; done\n\
                  /// `trap '' TERM; while true; do :; done`\n\
                  /* while :; do :; done */\n\
                  let a = 'x'; let b = '\\''; let c: &'static str = \"fine\";\n\
                  let d = \"while :; do :; done\";\n\
                  let e = r#\"trap '' TERM; while true; do :; done\"#;\n";
    let found: Vec<(usize, &str)> = unowned(source)
        .iter()
        .map(|found| (found.line, found.what))
        .collect();
    assert_eq!(
        found,
        [(5, "a loop that never ends"), (6, "a loop that never ends")],
        "{found:?}"
    );
}

#[test]
fn no_string_of_a_test_in_the_workspace_runs_on_without_looking_at_its_owner() -> Outcome {
    let (mut found, mut let_stand) = (Vec::new(), [false; LET.len()]);
    for folder in LOOKED_IN {
        for file in checkout::files_below(&checkout::path(folder), &UNSOURCED)? {
            let named = checkout::relative(&file);
            let is_rust = file.extension().is_some_and(|extension| extension == "rs");
            if !is_rust || named == THIS || !checkout::is_plain_file(&file) {
                continue;
            }
            let source = String::from_utf8_lossy(&fs::read(&file)?).into_owned();
            for one in unowned(&source) {
                let let_at = LET
                    .iter()
                    .position(|(file, piece, _)| *file == named && one.text.contains(piece));
                match let_at {
                    Some(at) => let_stand[at] = true,
                    None => found.push(format!("{named}:{}: {}", one.line, one.what)),
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "these fixtures run on when the test that started them is gone; have them look at their owner (`kill -0 $PPID 2>/dev/null`, or cf_base::fixture):\n{}",
        found.join("\n")
    );
    let stale: Vec<String> = LET
        .iter()
        .zip(let_stand)
        .filter(|(_, stands)| !stands)
        .map(|((file, piece, _), _)| format!("{file}: {piece}"))
        .collect();
    assert!(
        stale.is_empty(),
        "these are let and run on no longer: take them out of LET: {stale:?}"
    );
    Ok(())
}
