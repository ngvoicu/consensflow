//! What the smoke reads to know that an app is up and what is under it: the
//! app's own process, its daemon's process and readiness, and the ledger's one
//! holder. Each is read from what the machine shows (the process table, the
//! logs the app and its daemon write, the ledger's own refusal), not from a file
//! nothing writes, and each reader is a function of what it is given, so that
//! its tests can hold every one to its words.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use super::bundle::under;
use super::processes::{process_table, Row, Waits};
use super::sandbox::Sandbox;
use super::{Error, Result};

/// A file of the app's or the daemon's in the home, or nothing where it is not there yet.
fn home_file(sandbox: &Sandbox, parts: &[&str]) -> String {
    fs::read_to_string(under(&sandbox.state, parts)).unwrap_or_default()
}

/// The daemon's log.
pub fn daemon_log(sandbox: &Sandbox) -> String {
    home_file(sandbox, &["daemon.log"])
}

/// The app's log.
pub fn app_log(sandbox: &Sandbox) -> String {
    home_file(sandbox, &["app", "app.log"])
}

/// Which daemon a process is: Node's, which the releases before the deletion ran, or the native one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Node,
    Native,
}

/// The word the daemon's command line says it is: Node's runs `cf.mjs`, the native one is `cf`.
pub fn kind_of_command(command: &str) -> Kind {
    if command.ends_with("/cf.mjs ui --json --no-open") {
        Kind::Node
    } else {
        Kind::Native
    }
}

/// What a daemon's command line ends with.
const DAEMON_ARGUMENTS: &str = "ui --json --no-open";

/// Whether the command is one that ends with the daemon's words, as words of their own.
fn runs_a_daemon(command: &str) -> bool {
    let Some(before) = command.strip_suffix(DAEMON_ARGUMENTS) else {
        return false;
    };
    // `\b` before the first word: what comes before it is no part of a word.
    !before
        .chars()
        .next_back()
        .is_some_and(|last| last.is_alphanumeric() || last == '_')
}

/// The processes under `bundle` (the root of one app) that are a daemon: `ui --json --no-open`.
pub fn daemon_rows<'a>(table: &'a [Row], bundle: &str) -> Vec<&'a Row> {
    let root = format!("{bundle}/Contents/");
    table
        .iter()
        .filter(|row| row.command.contains(&root) && runs_a_daemon(&row.command))
        .collect()
}

/// The app is running: the process of `pid` is this bundle's executable.
pub fn assert_app<'a>(table: &'a [Row], pid: u32, binary: &str) -> Result<&'a Row> {
    let running = table
        .iter()
        .find(|each| each.pid == pid)
        .filter(|row| !row.state.starts_with('Z'));
    let Some(row) = running else {
        return Err(Error::new(format!("the app (pid {pid}) is not running")));
    };
    ensure!(
        row.command == binary,
        "pid {pid} is not the app: {}",
        row.command
    );
    Ok(row)
}

/// The start line a daemon's log holds for a process: which daemon started, by the
/// line it wrote first of all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartLine {
    pub pid: u32,
    /// `node v26.7.0` or `rust 3.0.0-alpha.81`.
    pub runtime: String,
    pub kind: Kind,
    pub line: String,
}

/// The pid a daemon's start line says, `<time> info start pid <pid> ` at the head of a line.
fn started(line: &str) -> Option<(u32, &str)> {
    let (stamp, rest) = line.split_once(' ')?;
    if stamp.is_empty() || stamp.contains(char::is_whitespace) {
        return None;
    }
    let rest = rest.strip_prefix("info start pid ")?;
    let (pid, rest) = rest.split_once(' ')?;
    if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((pid.parse().ok()?, rest))
}

/// `line` as a start line: `<time> info start pid <pid> (node v|rust )<version> home <home>`
/// (crates/cf-daemon/src/start.rs; Node's daemon, which the apps of the releases
/// before the deletion ran, wrote `node v…`).
fn start_of(line: &str) -> Option<StartLine> {
    let (pid, rest) = started(line)?;
    let (prefix, kind) = if rest.starts_with("node v") {
        ("node v", Kind::Node)
    } else if rest.starts_with("rust ") {
        ("rust ", Kind::Native)
    } else {
        return None;
    };
    let version = &rest[prefix.len()..];
    let length = version.find(char::is_whitespace).unwrap_or(version.len());
    if length == 0 || !version[length..].starts_with(" home ") {
        return None;
    }
    Some(StartLine {
        pid,
        runtime: rest[..prefix.len() + length].to_string(),
        kind,
        line: line.to_string(),
    })
}

/// The start line a daemon's log holds for `pid` (the last one, the log being
/// written on to by each start; any pid when none is given), or none.
pub fn start_line(log: &str, pid: Option<u32>) -> Option<StartLine> {
    log.split('\n')
        .filter_map(start_of)
        .rfind(|start| pid.is_none_or(|pid| start.pid == pid))
}

/// The pid of every start the daemon's log holds, in order.
fn starts_of(log: &str) -> Vec<u32> {
    log.split('\n')
        .filter_map(started)
        .map(|(pid, _)| pid)
        .collect()
}

/// The lines of a daemon log that one process wrote, from its start line to the
/// start of the next: the log is written on to by every start on the home.
pub fn lines_of(log: &str, pid: u32) -> Vec<&str> {
    let lines: Vec<&str> = log.split('\n').collect();
    let Some(from) = lines
        .iter()
        .position(|line| started(line).is_some_and(|(started, _)| started == pid))
    else {
        return Vec::new();
    };
    let to = lines
        .iter()
        .enumerate()
        .position(|(at, line)| at > from && started(line).is_some())
        .unwrap_or(lines.len());
    lines[from..to]
        .iter()
        .copied()
        .filter(|line| !line.is_empty())
        .collect()
}

/// The lines among them that say an error: its level is the line's second word.
pub fn error_lines<'a>(lines: &[&'a str]) -> Vec<&'a str> {
    lines
        .iter()
        .copied()
        .filter(|line| {
            line.split_once(' ')
                .is_some_and(|(stamp, rest)| !stamp.is_empty() && rest.starts_with("error "))
        })
        .collect()
}

/// Whether a line is the last a daemon refused the ledger writes.
fn refused_the_ledger(line: &str) -> bool {
    line.ends_with(" info exit 1")
}

/// What the machine shows of the daemon of an app.
pub struct Seen<'a> {
    /// `daemon.log`.
    pub log: &'a str,
    pub table: &'a [Row],
    /// The app's pid.
    pub app: u32,
    /// The app's root.
    pub bundle: &'a str,
    /// The pids of the second daemons the smoke tried.
    pub probes: &'a [u32],
}

/// The daemon of a home, as found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Daemon {
    pub pid: u32,
    pub kind: Kind,
    pub runtime: String,
    pub command: String,
}

/// The daemon of the home, as the machine shows it: the one process under the
/// app's own that runs this bundle's `cf ui --json --no-open`, and the only
/// daemon of the bundle that runs. Its log holds the start line it wrote, which
/// says which daemon it is (Node's or the native one), as its command line does.
/// And no daemon the app has started on this home was refused or failed: none of
/// their starts, a replaced app's included, is followed by an error or by `exit 1`.
/// What a probe started (`probes`, the pids of the second daemons the smoke
/// tried) says of its refusal is another's.
pub fn daemon_evidence(seen: &Seen) -> Result<Daemon> {
    let rows = daemon_rows(seen.table, seen.bundle);
    let mine: Vec<_> = rows.iter().filter(|row| row.ppid == seen.app).collect();
    ensure!(
        mine.len() == 1,
        "the app (pid {}) has {} daemons, and these run: {}",
        seen.app,
        mine.len(),
        if rows.is_empty() {
            "none".to_string()
        } else {
            rows.iter()
                .map(|row| format!("{} (parent {}) {}", row.pid, row.ppid, row.command))
                .collect::<Vec<_>>()
                .join("; ")
        }
    );
    ensure!(
        rows.len() == 1,
        "one daemon serves the home, and these run: {}",
        rows.iter()
            .map(|row| row.command.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    );
    let row = mine[0];
    let log = seen.log;
    let Some(start) = start_line(log, Some(row.pid)) else {
        return Err(Error::new(format!(
            "the daemon (pid {}) logged no start line:\n{log}",
            row.pid
        )));
    };
    ensure!(
        kind_of_command(&row.command) == start.kind,
        "the log says {} and the process runs {}",
        start.runtime,
        row.command
    );
    for pid in starts_of(log)
        .into_iter()
        .filter(|each| !seen.probes.contains(each))
    {
        let written = lines_of(log, pid);
        ensure!(
            error_lines(&written).is_empty(),
            "a daemon logged errors:\n{}",
            written.join("\n")
        );
        ensure!(
            !written.iter().any(|line| refused_the_ledger(line)),
            "a daemon failed to start:\n{}",
            written.join("\n")
        );
    }
    Ok(Daemon {
        pid: row.pid,
        kind: start.kind,
        runtime: start.runtime,
        command: row.command.clone(),
    })
}

/// Once every app is gone: the ledger refused no daemon but the probes the smoke
/// tried. A daemon refused logs `exit 1` as its last line, so the lines of it in
/// the log are the refusals, and each probe is one: an app's daemon that was
/// refused (the old one not let go when the new one started, or two at once) is
/// one more than the probes.
pub fn assert_only_probes_refused(log: &str, probes: &BTreeSet<u32>) -> Result {
    let refused = log
        .split('\n')
        .filter(|line| refused_the_ledger(line))
        .count();
    ensure!(
        refused == probes.len(),
        "{refused} daemons were refused the ledger, and {} were probes the smoke tried:\n{log}",
        probes.len()
    );
    Ok(())
}

/// Waits for the daemon of `app` to be the one `daemon_evidence` takes, and returns it.
pub fn daemon_of(
    sandbox: &Sandbox,
    waits: &Waits,
    app: u32,
    bundle: &Path,
    probes: &BTreeSet<u32>,
) -> Result<Daemon> {
    let bundle = bundle.to_string_lossy();
    let probes: Vec<u32> = probes.iter().copied().collect();
    let mut last = None;
    let label = format!("the daemon of pid {app} is running and logged its start");
    waits
        .until(&label, || {
            let seen = process_table().and_then(|table| {
                daemon_evidence(&Seen {
                    log: &daemon_log(sandbox),
                    table: &table,
                    app,
                    bundle: &bundle,
                    probes: &probes,
                })
            });
            match seen {
                Ok(daemon) => Ok(Some(daemon)),
                Err(cause) => {
                    last = Some(cause);
                    Ok(None)
                }
            }
        })
        .map_err(|cause| {
            let seen = last
                .as_ref()
                .map_or_else(|| "nothing was seen".to_string(), ToString::to_string);
            Error::new(format!("{cause}: {seen}"))
        })
}

/// What the app's error log says of the daemon it starts: the bundle's `cf`
/// (daemon_command.rs), by its path. Only the app that ships no Node says it; the
/// log is written on by every app of the home, so a line of an earlier one's
/// (the flip's, which said which daemon it chose and why) is not this one's.
pub fn assert_started_daemon(app_log_text: &str, cf: &Path) -> Result {
    let said = format!("starting the daemon: {} {DAEMON_ARGUMENTS}", cf.display());
    ensure!(
        app_log_text.split('\n').any(|line| line.ends_with(&said)),
        "app.log does not say the app started the bundle's cf ({said}):\n{}",
        tail(app_log_text, 2000)
    );
    Ok(())
}

/// The last `count` characters of `text`.
pub fn tail(text: &str, count: usize) -> &str {
    let skip = text.chars().count().saturating_sub(count);
    text.char_indices()
        .nth(skip)
        .map_or("", |(at, _)| &text[at..])
}

mod holder;
pub use holder::ledger_held;

#[cfg(test)]
mod tests;
