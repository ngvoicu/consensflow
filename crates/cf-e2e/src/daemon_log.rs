//! A daemon's log, `daemon.log` in its home, as the suites read it: which daemon
//! started (the first line of a start, `start pid <pid> <runtime> <version>
//! home <home>`, says what runtime it is), the lines one process wrote in a log
//! that every start on the home writes to, and the errors among them.
//!
//! The native daemon says `rust <version>`; the Node daemon of the releases
//! before its deletion said `node v<version>`. A suite that asked for the
//! native one refuses what is not ([`assert_started`]).

use std::sync::OnceLock;

use regex::Regex;

use crate::{pattern, Error, Result};

/// Which runtime a daemon says it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The Node daemon, which `node v…` in its start line names.
    Node,
    /// The native daemon: `rust …`.
    Native,
}

/// The start line of a daemon: who it is and what it said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartLine {
    pub pid: u32,
    /// The runtime and its version as the line says them: `rust 3.0.0-alpha.83`.
    pub runtime: String,
    pub kind: Kind,
    /// The whole line.
    pub line: String,
}

/// A start line, in a log of many lines.
fn start_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    pattern::once(
        &PATTERN,
        r"(?m)^\S+ info start pid (\d+) ((node v|rust )\S+) home .*$",
    )
}

/// A line that starts a daemon, whichever one: its pid.
fn started_pid(line: &str) -> Option<&str> {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    pattern::once(&PATTERN, r"^\S+ info start pid (\d+) ")
        .captures(line)
        .and_then(|found| found.get(1))
        .map(|pid| pid.as_str())
}

/// The start line `log` holds for `pid` (the last one, the log being written
/// on to by each start; any pid when none is given): which daemon started, by
/// the line it wrote first of all, or none when there is none.
pub fn start_line(log: &str, pid: Option<u32>) -> Option<StartLine> {
    start_pattern()
        .captures_iter(log)
        .filter_map(|found| {
            let said: u32 = found[1].parse().ok()?;
            Some(StartLine {
                pid: said,
                runtime: found[2].to_owned(),
                kind: if &found[3] == "rust " {
                    Kind::Native
                } else {
                    Kind::Node
                },
                line: found[0].to_owned(),
            })
        })
        .filter(|start| pid.is_none_or(|wanted| start.pid == wanted))
        .last()
}

/// The lines of `log` that one process wrote, from its start line to the start
/// of the next: the log is written on to by every start on the home. None when
/// `pid` never started there.
pub fn lines_of(log: &str, pid: u32) -> Vec<String> {
    let lines: Vec<&str> = log.split('\n').collect();
    let wanted = pid.to_string();
    let Some(from) = lines
        .iter()
        .position(|line| started_pid(line) == Some(wanted.as_str()))
    else {
        return Vec::new();
    };
    let to = lines
        .iter()
        .enumerate()
        .skip(from + 1)
        .find(|(_, line)| started_pid(line).is_some())
        .map_or(lines.len(), |(at, _)| at);
    lines[from..to]
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| (*line).to_owned())
        .collect()
}

/// Whether the line says an error: its level is the line's second word.
fn is_error(line: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    pattern::once(&PATTERN, r"^\S+ error ").is_match(line)
}

/// Each error among `lines` with what the log says under it (the error's own
/// first line, indented): an error is logged as what was being done, and the
/// cause follows.
pub fn errors_with_cause(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, line)| is_error(line))
        .map(|(at, line)| match lines.get(at + 1) {
            Some(under) if under.starts_with("    ") => format!("{line} {}", under.trim()),
            _ => line.clone(),
        })
        .collect()
}

/// Says the daemon that started is the native one: `log` is the daemon's log
/// and `pid` its process. Returns its start line. A daemon that is not,
/// whatever was asked, is refused, which fails the suite by itself; a stand-in
/// that a case starts to see it refused is the `liar-daemon` binary.
pub fn assert_started(log: &str, pid: u32) -> Result<StartLine> {
    let Some(start) = start_line(log, Some(pid)) else {
        return Err(Error::Daemon(format!(
            "the native daemon was asked for, and its log holds no start line of pid {pid}"
        )));
    };
    if start.kind != Kind::Native {
        return Err(Error::Daemon(format!(
            "the native daemon was asked for, but the start line in its log says {}: {}",
            start.runtime, start.line
        )));
    }
    Ok(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "\
2026-10-06T10:00:00.000Z info start pid 11 node v26.8.1 home /h
2026-10-06T10:00:01.000Z error a pass failed
    at stack line that says error handling
2026-10-06T10:00:02.000Z info stop: stdin ended; rss 90 MB
2026-10-06T10:01:00.000Z info start pid 12 rust 3.0.0 home /h
2026-10-06T10:01:01.000Z warn a pass took 6000 ms
2026-10-06T10:01:02.000Z error the bridge failed
2026-10-06T10:02:00.000Z info start pid 13 node v26.8.1 home /h
";

    fn owned(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| (*line).to_owned()).collect()
    }

    #[test]
    fn a_start_line_names_the_runtime_that_wrote_it_and_the_last_start_of_a_pid_counts() {
        let node = start_line(LOG, Some(11)).unwrap();
        assert_eq!((node.pid, node.kind), (11, Kind::Node));
        assert_eq!(node.runtime, "node v26.8.1");
        assert_eq!(
            node.line,
            "2026-10-06T10:00:00.000Z info start pid 11 node v26.8.1 home /h"
        );
        let native = start_line(LOG, Some(12)).unwrap();
        assert_eq!((native.pid, native.kind), (12, Kind::Native));
        assert_eq!(native.runtime, "rust 3.0.0");
        // Any pid: the last start there is.
        assert_eq!(start_line(LOG, None).unwrap().pid, 13);
        assert_eq!(start_line(LOG, Some(99)), None);
        assert_eq!(start_line("", None), None);
        // A pid started twice on the home (it is reused): the later start is its.
        let twice = format!("{LOG}2026-10-06T10:03:00.000Z info start pid 11 rust 3.0.1 home /h\n");
        assert_eq!(start_line(&twice, Some(11)).unwrap().runtime, "rust 3.0.1");
    }

    #[test]
    fn only_a_line_that_is_a_start_is_one() {
        for line in [
            "info start pid 1 rust 3.0.0 home /h",
            "2026-10-06T10:00:00.000Z warn start pid 1 rust 3.0.0 home /h",
            "2026-10-06T10:00:00.000Z info start pid x rust 3.0.0 home /h",
            "2026-10-06T10:00:00.000Z info start pid 1 python 3 home /h",
            "2026-10-06T10:00:00.000Z info start pid 1 rust 3.0.0",
        ] {
            assert_eq!(start_line(line, None), None, "{line}");
        }
    }

    #[test]
    fn the_lines_of_one_start_run_to_the_next_start() {
        assert_eq!(lines_of(LOG, 11).len(), 4);
        assert_eq!(lines_of(LOG, 12).len(), 3);
        assert_eq!(
            lines_of(LOG, 13),
            owned(&["2026-10-06T10:02:00.000Z info start pid 13 node v26.8.1 home /h"])
        );
        assert_eq!(lines_of(LOG, 99), Vec::<String>::new());
    }

    #[test]
    fn an_error_is_a_line_whose_level_is_error() {
        assert!(is_error("2026-10-06T10:00:01.000Z error a pass failed"));
        assert!(is_error("2026-10-06T10:01:02.000Z error the bridge failed"));
        // The word anywhere else in a line is no error.
        for line in [
            "2026-10-06T10:00:00.000Z info an error was handled",
            "    error under a line",
            "2026-10-06T10:00:00.000Z warn error",
            "",
        ] {
            assert!(!is_error(line), "{line:?}");
        }
    }

    #[test]
    fn what_is_logged_under_an_error_goes_with_it_when_something_is() {
        assert_eq!(
            errors_with_cause(&lines_of(LOG, 11)),
            owned(&["2026-10-06T10:00:01.000Z error a pass failed at stack line that says error handling"])
        );
        assert_eq!(
            errors_with_cause(&lines_of(LOG, 12)),
            owned(&["2026-10-06T10:01:02.000Z error the bridge failed"])
        );
        assert_eq!(errors_with_cause(&lines_of(LOG, 13)), Vec::<String>::new());
    }

    #[test]
    fn the_native_daemon_is_accepted_and_any_other_is_refused_in_words() {
        let accepted = assert_started(LOG, 12).unwrap();
        assert_eq!(accepted.kind, Kind::Native);
        let node = assert_started(LOG, 11).unwrap_err().to_string();
        assert_eq!(
            node,
            "the native daemon was asked for, but the start line in its log says node v26.8.1: \
             2026-10-06T10:00:00.000Z info start pid 11 node v26.8.1 home /h"
        );
        let none = assert_started(LOG, 99).unwrap_err().to_string();
        assert_eq!(
            none,
            "the native daemon was asked for, and its log holds no start line of pid 99"
        );
    }
}
