//! A `cargo test` step read binary by binary, from its job's log: how long the
//! build took, and how long each test binary ran (the `finished in` of its
//! summary), so that a step that took longer than it did shows which binary
//! grew. Cargo runs the binaries one after another, so they and the build add
//! up to the step.

/// The test binaries of a step and the build before them.
#[derive(Debug, PartialEq)]
pub(super) struct Timing {
    /// The seconds cargo says it built the tests in, where it built any.
    pub build: Option<f64>,
    /// Each binary and the seconds its tests ran, in the order cargo ran them.
    pub binaries: Vec<(String, f64)>,
}

/// What a log line says, without the time GitHub puts before it
/// (`2026-10-21T21:25:40.1358729Z `), or the job and step a downloaded log
/// puts before that.
fn said(line: &str) -> &str {
    line.match_indices("Z ")
        .find(|(at, _)| line[..*at].ends_with(|digit: char| digit.is_ascii_digit()))
        .map_or(line, |(at, _)| &line[at + 2..])
}

/// What cargo says a build took (`3m 33s`, `59.20s`), in seconds.
fn seconds_of(text: &str) -> Option<f64> {
    let mut total = 0.0;
    for part in text.split_whitespace() {
        let (number, unit) = part.split_at(part.find(|c: char| c.is_ascii_alphabetic())?);
        total += number.parse::<f64>().ok()?
            * match unit {
                "s" => 1.0,
                "m" => 60.0,
                "h" => 3600.0,
                _ => return None,
            };
    }
    Some(total)
}

/// The crate a library's test binary is of, from its file's name
/// (`cf_ledger-0f4511c365e3e775.exe`): the binaries of its other tests follow it,
/// in the order cargo runs a crate's in.
fn crate_of(exe: &str) -> String {
    let file = exe.rsplit(['/', '\\']).next().unwrap_or(exe);
    let stem = file.strip_suffix(".exe").unwrap_or(file);
    match stem.rsplit_once('-') {
        Some((name, hash)) if hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()) => {
            name.to_owned()
        }
        _ => stem.to_owned(),
    }
}

/// The binaries of the step `step` (its name as GitHub shows it, `Run cargo test …`)
/// in `log`, a job's whole log. None when the log has no such step.
pub(super) fn timing(log: &str, step: &str) -> Option<Timing> {
    let group = format!("##[group]{step}");
    let mut lines = log.lines().map(said);
    lines.find(|line| *line == group)?;
    let mut found = Timing {
        build: None,
        binaries: Vec::new(),
    };
    let (mut crate_name, mut running) = (String::new(), None::<usize>);
    for line in lines {
        if line.starts_with("##[group]Run") {
            break;
        }
        let line = line.trim();
        if let Some(finished) = line
            .strip_prefix("Finished ")
            .and_then(|rest| rest.rsplit_once(" in "))
        {
            // The build of the tests ends before the first runs; a test that builds
            // something says the same words after.
            if found.binaries.is_empty() {
                found.build = seconds_of(finished.1);
            }
        } else if let Some(running_line) = line.strip_prefix("Running ") {
            let (target, exe) = running_line
                .split_once(" (")
                .map_or((running_line, ""), |(target, exe)| {
                    (target, exe.trim_end_matches(')'))
                });
            // A library's unit tests start a crate; its other binaries follow them.
            if target.ends_with("lib.rs") {
                crate_name = crate_of(exe);
            }
            let shown = if let Some(unit) = target.strip_prefix("unittests ") {
                format!("{crate_name} {unit}")
            } else {
                format!("{crate_name} {target}")
            };
            found.binaries.push((shown.replace('\\', "/"), 0.0));
            running = Some(found.binaries.len() - 1);
        } else if let Some(crate_docs) = line.strip_prefix("Doc-tests ") {
            found
                .binaries
                .push((format!("{crate_docs} doc-tests"), 0.0));
            running = Some(found.binaries.len() - 1);
        } else if let (Some(at), true) = (running, line.starts_with("test result:")) {
            // `... 0 filtered out; finished in 61.02s`
            let took = line.rsplit_once("finished in ").map(|(_, took)| took);
            found.binaries[at].1 = took
                .and_then(|took| took.trim_end_matches('s').parse().ok())
                .unwrap_or(0.0);
            running = None;
        }
    }
    Some(found)
}
