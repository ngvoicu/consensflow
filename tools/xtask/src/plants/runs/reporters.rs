//! What each reporter says of a run, read from its own lines of results alone: what
//! a test prints may say anything, a result line of another test or reporter too.
//!
//! - libtest (`cargo test`, and the `cargo xtask` suites that run it): the list each
//!   test binary ends with, `failures:`, each failed test indented by four, a blank
//!   line and `test result: FAILED.`. Its `test <name> ... FAILED` as each test ends
//!   is not read: with `--nocapture` it is among what the tests print, and what a
//!   failed test printed is shown before that list. (Its own structured report,
//!   `--format json`, wants `-Z unstable-options`, which a stable toolchain refuses.)
//! - `node --test`: its spec report's list of the failed tests at its end, after
//!   `✖ failing tests:`, each `test at <where>` and then `✖ <name> (<time>)`, which
//!   what a test printed never follows; its TAP report's result lines,
//!   `not ok <n> - <name>` at any depth, out of the `---` … `...` blocks that hold
//!   why each failed, what a test printed being a `#` comment there. How many tests
//!   it ran is its summary's last count (`ℹ tests <n>`, `# tests <n>`), where a test
//!   file that failed whole is counted as one.
//! - Playwright: the last list of its summary, under `<n> failed`.

/// Who reports a run's tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Reporter {
    Libtest,
    Node,
    Playwright,
}

/// A test a reporter says failed, and where it is written, where the reporter says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Failed {
    pub name: String,
    pub location: Option<String>,
}

impl Reporter {
    /// Who reports the tests of `command`: Playwright where its program is what
    /// `node` runs, Node where `node` runs anything else (`--test`), libtest else.
    pub(super) fn of(command: &[String]) -> Self {
        match command {
            [node, rest @ ..] if node == "node" || node.ends_with("node.exe") => {
                if rest.iter().any(|word| word.contains("playwright")) {
                    Self::Playwright
                } else {
                    Self::Node
                }
            }
            _ => Self::Libtest,
        }
    }

    /// The tests `output` says failed, each once, in the order it says them.
    pub(super) fn failed(self, output: &str) -> Vec<Failed> {
        let failed = match self {
            Self::Libtest => libtest(output),
            Self::Node if output.starts_with("TAP version") => tap(output),
            Self::Node => spec(output),
            Self::Playwright => playwright(output),
        };
        let mut once: Vec<Failed> = Vec::new();
        for test in failed {
            if !once.iter().any(|seen| seen.name == test.name) {
                once.push(test);
            }
        }
        once
    }
}

/// How many tests Node says it ran, by the last count of its summary.
pub(super) fn node_counted(output: &str) -> Option<usize> {
    output.lines().rev().find_map(|line| {
        line.strip_prefix("ℹ tests ")
            .or_else(|| line.strip_prefix("# tests "))
            .and_then(|count| count.trim().parse().ok())
    })
}

/// The tests of each list libtest ends a test binary's run with.
fn libtest(output: &str) -> Vec<Failed> {
    let lines: Vec<&str> = output.lines().collect();
    let mut failed = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        if !line.starts_with("test result: FAILED.") {
            continue;
        }
        let Some(blank) = at.checked_sub(1).filter(|&blank| lines[blank].is_empty()) else {
            continue;
        };
        let mut first = blank;
        while first > 0 && listed(lines[first - 1]).is_some() {
            first -= 1;
        }
        if first < blank && first > 0 && lines[first - 1] == "failures:" {
            failed.extend(lines[first..blank].iter().filter_map(|line| {
                listed(line).map(|name| Failed {
                    name: name.to_owned(),
                    location: None,
                })
            }));
        }
    }
    failed
}

/// `    <name>`, a test of libtest's list.
fn listed(line: &str) -> Option<&str> {
    line.strip_prefix("    ")
        .filter(|name| !name.is_empty() && !name.starts_with(' '))
}

/// The tests of the list Node's spec report ends with.
fn spec(output: &str) -> Vec<Failed> {
    let lines: Vec<&str> = output.lines().collect();
    let Some(heading) = lines.iter().rposition(|line| *line == "✖ failing tests:") else {
        return Vec::new();
    };
    let mut failed = Vec::new();
    let mut location = None;
    for line in &lines[heading + 1..] {
        if let Some(at) = line.strip_prefix("test at ") {
            location = Some(at.to_owned());
        } else if let Some(name) = location.as_ref().and_then(|_| timed(line)) {
            failed.push(Failed {
                name: name.to_owned(),
                location: location.take(),
            });
        }
    }
    failed
}

/// `✖ <name> (<n>ms)`, as Node's spec report says a test failed.
fn timed(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("✖ ")?.strip_suffix("ms)")?;
    let open = rest.rfind(" (")?;
    let time = &rest[open + 2..];
    let number = time.starts_with(|c: char| c.is_ascii_digit())
        && time.chars().all(|c| c.is_ascii_digit() || c == '.');
    (number && open > 0).then(|| &rest[..open])
}

/// The result lines of Node's TAP report that say a test failed, at any depth, with
/// where each is written as the block under it says.
fn tap(output: &str) -> Vec<Failed> {
    let mut failed: Vec<Failed> = Vec::new();
    // The depth of the `---` of a block open now, and whether it is a failure's.
    let mut block: Option<(usize, bool)> = None;
    let mut last_failed = false;
    for line in output.lines() {
        let depth = line.len() - line.trim_start_matches(' ').len();
        let text = &line[depth..];
        if let Some((open, of_failure)) = block {
            if depth == open && text == "..." {
                block = None;
            } else if let (true, true, Some(at)) =
                (of_failure, depth == open, text.strip_prefix("location: "))
            {
                if let Some(test) = failed.last_mut() {
                    test.location = Some(at.trim_matches('\'').to_owned());
                }
            }
            continue;
        }
        if text == "---" {
            block = Some((depth, last_failed));
            continue;
        }
        last_failed = false;
        if depth % 4 != 0 {
            continue;
        }
        if let Some(name) = not_ok(text) {
            failed.push(Failed {
                name: name.to_owned(),
                location: None,
            });
            last_failed = true;
        }
    }
    failed
}

/// `not ok <n> - <name>`, as the TAP report says a test failed.
fn not_ok(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("not ok ")?;
    let digits = rest.chars().take_while(char::is_ascii_digit).count();
    let name = rest[digits..].strip_prefix(" - ").filter(|_| digits > 0)?;
    (!name.is_empty()).then_some(name)
}

/// The tests of the last list of Playwright's summary.
fn playwright(output: &str) -> Vec<Failed> {
    let lines: Vec<&str> = output.lines().collect();
    let Some(heading) = lines.iter().rposition(|line| failed_heading(line)) else {
        return Vec::new();
    };
    lines[heading + 1..]
        .iter()
        .map_while(|line| playwright_name(line))
        .map(|name| Failed {
            name: name.to_owned(),
            location: None,
        })
        .collect()
}

/// `  <n> failed`, the heading of the tests Playwright's summary says failed.
fn failed_heading(line: &str) -> bool {
    line.trim()
        .strip_suffix(" failed")
        .is_some_and(|count| !count.is_empty() && count.chars().all(|c| c.is_ascii_digit()))
}

/// `    <file>:<line>:<column> › <describe> › <title>`, a test of Playwright's summary,
/// after any project before the file, and any rule it draws after the title (`───`):
/// named from the title of its first group on.
fn playwright_name(line: &str) -> Option<&str> {
    let mut rest = line
        .strip_prefix("    ")?
        .trim_end_matches(|c: char| c == '─' || c.is_whitespace());
    loop {
        let (segment, after) = rest.split_once(" › ")?;
        if is_location(segment) {
            return (!after.is_empty()).then_some(after);
        }
        rest = after;
    }
}

/// `<file>:<line>:<column>`, where a Playwright test is written.
fn is_location(segment: &str) -> bool {
    let mut parts = segment.rsplitn(3, ':');
    let (Some(column), Some(line), Some(file)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    !file.is_empty()
        && [column, line]
            .iter()
            .all(|number| !number.is_empty() && number.chars().all(|c| c.is_ascii_digit()))
}
