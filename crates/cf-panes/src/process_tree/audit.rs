//! The audit of every place that can send a signal: the sources of this crate are read, and
//! each line that sends one (a system call, the real system's own sender, the seam the bound
//! sends through) is held to a table of the places that may, each with its reason. A sender the
//! table does not name fails the audit, and so does a name with no sender left: that nothing
//! sends around the bound is told here, in the sources, and not found in a run of real
//! processes. What is not looked at is test code (its own children, which a test ends), by the
//! names of its files.

use std::fs;
use std::path::Path;

/// What names a signal sent to the operating system, apart from a call of `kill` itself
/// ([`calls_kill`]): the child's own end, and the Windows calls.
const SYSTEM_SENDERS: [&str; 7] = [
    ".kill()",
    "killpg(",
    "pthread_kill(",
    "libc::kill",
    "TerminateProcess(",
    "TerminateJobObject(",
    "GenerateConsoleCtrlEvent(",
];

/// What names a signal sent through the seam of the tree (`System`), which only the files of
/// the tree have.
const SEAM_SENDERS: [&str; 3] = [".send(", ".send_group(", "native::signal"];

/// Where a signal may be sent: the file (from `src/`), a piece of the line that sends it, and
/// why it may.
const SENDERS: [(&str, &str, &str); 13] = [
    (
        "process_tree/bound.rs",
        "native::signal(to, signal)",
        "the real system's sender, which the bound alone is given",
    ),
    (
        "process_tree/bound.rs",
        "native::signal_group(group, signal)",
        "the same, to a group",
    ),
    (
        "process_tree/bound.rs",
        "system.send(target, signal)?;",
        "the bound sends what it allows, to a process",
    ),
    (
        "process_tree/bound.rs",
        "return system.send_group(group, signal);",
        "the bound sends the pane's group whole, while its program leads it",
    ),
    (
        "process_tree/native.rs",
        "libc::kill(identity.pid, signal)",
        "the system's own sender: it signals the process this tree discovered, which it checks first, and a test build asks the guard before it",
    ),
    (
        "process_tree/native.rs",
        "libc::kill(-group, signal)",
        "the same, to a group, which a test build asks the guard of as well",
    ),
    (
        "pty/close.rs",
        "kill(-process_group_id, signal)",
        "Unix but macOS, where the pane has no tree: its group whole; compiled out on macOS",
    ),
    (
        "pty/close.rs",
        "kill(-process_group_id, 0)",
        "signal 0 delivers nothing: whether the group has a process",
    ),
    (
        "pty/conpty.rs",
        "TerminateProcess(process.as_raw_handle(), 1)",
        "Windows: the window's own program, by the handle it was started with (`Child::kill`)",
    ),
    (
        "pty.rs",
        "kill(pid, 0)",
        "signal 0 delivers nothing: whether a process is there (what tests ask)",
    ),
    (
        "pty/sentinel.rs",
        "kill(self.pid, signal)",
        "a test's own stranger, which it ends by its number and when it began",
    ),
    (
        "job_object.rs",
        "TerminateJobObject(self.handle, 1)",
        "Windows: the pane's own job, in which nothing else is",
    ),
    (
        "job_object.rs",
        "TerminateProcess(self.0, 1)",
        "Windows: a member of the pane's job, by the handle it was admitted by",
    ),
];

/// Whether `path` (from `src/`) is test code: the tests of a module, what they stand on
/// (`testing.rs`, `support.rs`, `programs.rs`, `keys.rs`), and this audit, which names the
/// spellings it looks for.
fn is_test_code(path: &str) -> bool {
    path.split('/').any(|part| {
        let name = part.strip_suffix(".rs").unwrap_or(part);
        name.ends_with("tests")
            || ["audit", "testing", "support", "programs", "keys"].contains(&name)
    })
}

/// What of `source` runs in a build that is no test's: up to its module of tests, and with
/// its comments (documents, the notes beside the code) taken out.
fn code_of(source: &str) -> String {
    let production = source
        .split("#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or(source);
    production
        .lines()
        .map(|line| line.split("//").next().unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Whether `line` calls a function named `kill`: not a method of something (a table closes
/// a window with `.kill(&key)`), not a longer name, and not its declaration.
fn calls_kill(line: &str) -> bool {
    line.match_indices("kill(").any(|(at, _)| {
        let before = &line[..at];
        !before.ends_with("fn ")
            && !before.ends_with('.')
            && !before.ends_with(|c: char| c.is_alphanumeric() || c == '_')
    })
}

/// The lines of `code` that send a signal; those through the seam of the tree too where
/// `seam` says the file has it.
fn sending(code: &str, seam: bool) -> Vec<String> {
    code.lines()
        .map(str::trim)
        .filter(|line| {
            calls_kill(line)
                || SYSTEM_SENDERS
                    .iter()
                    .any(|spelling| line.contains(spelling))
                || (seam && SEAM_SENDERS.iter().any(|spelling| line.contains(spelling)))
        })
        .map(str::to_owned)
        .collect()
}

/// The files under `dir`, by their path from `base` and their text.
fn sources(base: &Path, dir: &Path) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut entries: Vec<_> = fs::read_dir(dir)
        .expect("a folder of the sources")
        .map(|entry| entry.expect("an entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            found.extend(sources(base, &path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            let name = path
                .strip_prefix(base)
                .expect("under the sources")
                .to_string_lossy()
                .replace('\\', "/");
            found.push((name, fs::read_to_string(&path).expect("a source")));
        }
    }
    found
}

/// What the audit finds wrong: the senders `found` (file, line) that no entry of `allowed`
/// names, and the entries that name none.
fn wrong(found: &[(String, String)], allowed: &[(&str, &str, &str)]) -> Vec<String> {
    let mut wrong = Vec::new();
    for (file, line) in found {
        let named = allowed
            .iter()
            .filter(|(path, piece, _)| path == file && line.contains(piece))
            .count();
        if named != 1 {
            wrong.push(format!(
                "{file}: `{line}` is named by {named} of the senders"
            ));
        }
    }
    for (path, piece, why) in allowed {
        if !found
            .iter()
            .any(|(file, line)| file == path && line.contains(piece))
        {
            wrong.push(format!("{path}: `{piece}` ({why}) sends nothing any more"));
        }
    }
    wrong
}

#[test]
fn nothing_but_the_bound_sends_a_signal() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for (path, text) in sources(&base, &base) {
        if is_test_code(&path) {
            continue;
        }
        let seam = path.starts_with("process_tree");
        for line in sending(&code_of(&text), seam) {
            found.push((path.clone(), line));
        }
    }

    let wrong = wrong(&found, &SENDERS);

    assert!(
        wrong.is_empty(),
        "what sends a signal is the bound, and these exceptions, named: {wrong:#?}"
    );
}

#[test]
fn a_call_of_kill_is_told_from_the_close_of_a_window_and_from_a_declaration() {
    for sends in [
        "if unsafe { kill(-process_group_id, signal) } == 0 {",
        "let _ = unsafe { libc::kill(pid, libc::SIGKILL) };",
        "unsafe { kill(pid, 0) == 0 }",
    ] {
        assert!(calls_kill(sends), "{sends}");
    }
    for does_not in [
        "fn kill(pid: i32, signal: i32) -> i32;",
        "table.kill(&pane.key).expect(\"close the pane\");",
        "let _ = self.kill(key);",
        "pane_kill(x)",
    ] {
        assert!(!calls_kill(does_not), "{does_not}");
    }
}

#[test]
fn the_other_senders_are_told_by_their_spelling_and_the_seam_by_the_files_that_have_it() {
    let code = "let _ = child.kill();\nTerminateProcess(handle, 1);\nsystem.send(target, 9)?;\nsender.send(1);";

    assert_eq!(
        sending(code, false),
        ["let _ = child.kill();", "TerminateProcess(handle, 1);"]
    );
    assert_eq!(
        sending(code, true),
        [
            "let _ = child.kill();",
            "TerminateProcess(handle, 1);",
            "system.send(target, 9)?;",
            "sender.send(1);"
        ],
        "a file of the tree has no channel of its own to tell this from"
    );
}

#[test]
fn comments_and_the_module_of_tests_are_no_code_that_runs() {
    let source = "/// kill(1, 9) is how it ends\nfn go() {\n    // libc::kill(1, 9)\n    work(); // child.kill()\n}\n#[cfg(test)]\nmod tests {\n    fn t() { unsafe { kill(1, 9) }; }\n}\n";

    assert!(sending(&code_of(source), true).is_empty());
}

#[test]
fn a_sender_that_is_not_named_and_a_name_that_sends_nothing_are_both_found() {
    let allowed = [("a.rs", "kill(-group, signal)", "its reason")];

    let named = [("a.rs".to_owned(), "kill(-group, signal)".to_owned())];
    assert!(wrong(&named, &allowed).is_empty());

    let unnamed = [
        ("a.rs".to_owned(), "kill(-group, signal)".to_owned()),
        ("b.rs".to_owned(), "kill(pid, 9)".to_owned()),
    ];
    let found = wrong(&unnamed, &allowed);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with("b.rs:"), "{found:?}");

    let gone = wrong(&[], &allowed);
    assert_eq!(gone.len(), 1, "{gone:?}");
    assert!(gone[0].contains("sends nothing any more"), "{gone:?}");
}

#[test]
fn the_files_of_tests_are_told_from_the_rest() {
    for test_code in [
        "process_tree/tests.rs",
        "process_tree/bound/tests.rs",
        "pty/turn/ending_tests.rs",
        "pty/turn/ending_tests/programs.rs",
        "pty/turn/support.rs",
        "process_tree/testing.rs",
        "pty/keys.rs",
    ] {
        assert!(is_test_code(test_code), "{test_code}");
    }
    for code in [
        "pty.rs",
        "pty/close.rs",
        "pty/sentinel.rs",
        "process_tree/native.rs",
        "process_tree/guard.rs",
        "job_object.rs",
    ] {
        assert!(!is_test_code(code), "{code}");
    }
}
