//! Who could have written what changed in the owner's setup during a run, said without opening
//! the owner's files (B15, B23). A watch of an owner's file knows only that it differs across the
//! run; the owner's own tool may refresh it while a run is going. What the kit can tell, from
//! the machine's processes alone, is whether a process of the same harness that is **no part of
//! this run** was alive at the start or the end of it: where one was, the change is not proved
//! the run's; where none was, the run is the likely writer and is still not proved to be.
//! A change is a failure either way; this says how sure the failure is.

use crate::live::members::{descending_from, Identity, Machine, Running};

/// The processes of `harness`'s program among `all` that are none of this run's: not the run's
/// root `root` (the test's own process) and nothing that descends from it.
pub fn independent(all: &[Running], root: Identity, harness: &str) -> Vec<Identity> {
    let own = descending_from(all, root);
    all.iter()
        .filter(|process| process.identity != root)
        .filter(|process| !own.iter().any(|mine| mine.identity == process.identity))
        .filter(|process| is_program(process, harness))
        .map(|process| process.identity)
        .collect()
}

/// Whether the process runs the program `harness` (the name its command line starts with, or
/// the script `node` is started on).
fn is_program(process: &Running, harness: &str) -> bool {
    let stem = |word: &str| {
        std::path::Path::new(word)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::to_lowercase)
    };
    let mut words = process.command.split_whitespace();
    let first = words.next().and_then(stem);
    let program = match first.as_deref() {
        Some("node" | "bun") => words.find(|word| !word.starts_with('-')).and_then(stem),
        _ => first,
    };
    let exe = process.exe.as_deref().and_then(stem);
    program.as_deref() == Some(harness) || exe.as_deref() == Some(harness)
}

/// What can be said of who wrote the paths in `changed`, given the independent processes of the
/// harness alive when the run began (`at_start`) and when it ended (`at_end`).
pub fn say(harness: &str, at_start: &[Identity], at_end: &[Identity]) -> String {
    match (at_start.len(), at_end.len()) {
        (0, 0) => format!(
            "attribution: no {harness} process outside this run was alive at its start or its end; this run is the likely writer, which is not proved (the kit watches no write)"
        ),
        (start, end) => format!(
            "attribution: not established; {start} {harness} process(es) outside this run were alive at its start and {end} at its end, and the owner's own {harness} may have written it"
        ),
    }
}

/// The processes of `harness` outside the run now, on `machine`, as this run's root is.
pub fn now(machine: &dyn Machine, harness: &str) -> Vec<Identity> {
    let Ok(all) = machine.processes() else {
        return Vec::new();
    };
    let Ok(Some(root)) = machine.identity(std::process::id()) else {
        return Vec::new();
    };
    independent(&all, root, harness)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, ppid: u32, command: &str) -> Running {
        Running {
            identity: Identity {
                pid,
                started: u64::from(pid) * 10,
            },
            ppid,
            command: command.to_owned(),
            exe: None,
        }
    }

    /// The control pair: a Codex the run started (its descendant) is no independent writer, and
    /// one the owner started is, by its number and its start, however it is spelled.
    #[test]
    fn an_owners_own_harness_is_independent_and_the_runs_own_is_not() {
        let all = [
            process(1, 0, "/sbin/launchd"),
            process(100, 1, "cargo test live"),
            process(101, 100, "/rig/bin/codex app-server"),
            process(102, 101, "codex helper"),
            process(500, 1, "/Users/o/.local/bin/codex"),
            process(501, 1, "node /Users/o/.pi/bin/pi.js --offline"),
            process(502, 1, "/usr/bin/vim notes"),
        ];
        let root = all[1].identity;
        let codex = independent(&all, root, "codex");
        assert_eq!(codex, [all[4].identity], "the run's own Codex was counted");
        assert_eq!(independent(&all, root, "pi"), [all[5].identity]);
        assert!(independent(&all, root, "devin").is_empty());
    }

    #[test]
    fn what_is_said_of_a_change_names_how_sure_it_is_and_never_reads_a_file() {
        let seen = [Identity { pid: 5, started: 50 }];
        let alone = say("codex", &[], &[]);
        assert!(alone.contains("likely writer") && alone.contains("not proved"), "{alone}");
        let shared = say("codex", &seen, &[]);
        assert!(
            shared.contains("not established") && shared.contains("1 codex process(es)"),
            "{shared}"
        );
        assert!(say("pi", &[], &seen).contains("1 at its end"));
    }
}
