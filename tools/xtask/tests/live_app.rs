//! No test, smoke, plant or tool reaches the live app. On 2026-10-28 an updater smoke's
//! copy of the app, of the live app's identity, came to the front over the live app, and
//! the Quit a person gave the app in front reached the live app next: the board closed.
//! The app a test starts is never of the live app's identity (`cf_base::bundle`, which
//! the programs xtask and the end-to-end rig start are held to, both ways they start
//! one). What these rules keep is the rest of that: nothing of the tests and the tools
//! starts, quits or signals an app by its name or its identity, which are the live app's
//! as much as any copy's, and nothing starts the program of an app bundle by a way of its
//! own, around the two that are held to it.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

/// The checkout's root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// Whether a line of code reaches an app by its name or its identity: `open -a` and
/// `open -b`, AppleScript's `tell application` (and JavaScript's `Application(…)`),
/// AppKit's running applications found by identity, Launch Services' opening, and the
/// programs and the signal that end processes by name or every one.
fn reaches_by_name(code: &str) -> bool {
    let lower = code.to_lowercase();
    let delimited = |at: usize, len: usize| {
        let before = code[..at].chars().next_back();
        let after = code[at + len..].chars().next();
        let edge = |it: Option<char>| it.is_none_or(|it| !it.is_alphanumeric() && it != '_');
        edge(before) && edge(after)
    };
    let words = |word: &str| {
        code.match_indices(word)
            .any(|(at, _)| delimited(at, word.len()))
    };
    let open_by_name = code.match_indices("open").any(|(at, _)| {
        delimited(at, 4) && {
            let near: String = code[at + 4..].chars().take(40).collect();
            ["-a", "-b"].iter().any(|flag| {
                near.match_indices(flag).any(|(found, _)| {
                    let before = near[..found].chars().next_back();
                    let after = near[found + 2..].chars().next();
                    let quote = |it: Option<char>| {
                        it.is_some_and(|it| it == '"' || it == '\'' || it == ' ')
                    };
                    quote(before) && quote(after)
                })
            })
        }
    });
    let every_process = [
        "kill(-1",
        "kill (-1",
        "kill -1",
        "kill -9 -1",
        "kill -KILL -1",
        "kill -s KILL -1",
    ]
    .iter()
    .any(|it| code.contains(it));
    open_by_name
        || lower.contains("tell application")
        || code.contains("Application(\"")
        || code.contains("Application('")
        || code.contains("WithBundleIdentifier")
        || code.contains("launchApplication")
        || code
            .match_indices("LSOpen")
            .any(|(at, _)| code[at + 6..].starts_with(|it: char| it.is_ascii_uppercase()))
        || words("killall")
        || words("pkill")
        || every_process
}

/// The files whose words are rules of the kind, or plants of them: they name what they refuse.
const RULES: [&str; 4] = [
    "tools/xtask/tests/live_app.rs",
    "tests/plants/release/live-app.toml",
    "crates/cf-e2e/src/live/members/descent/tests.rs",
    "crates/cf-evals/tests/long_run/signals.rs",
];

/// The files of the tools and of the rig that start a program a caller names (std's
/// `Command::new` of a value, not of a name written there), and what holds them to no app
/// of the live app's identity: the gate each calls (`cf_base::bundle`), or what they start.
const STARTERS: [(&str, &str); 3] = [
    (
        "tools/cf-publish/src/process.rs",
        "curl, gh and tar alone, which its callers name; it is built on serde_json and sha2 alone",
    ),
    (
        "tools/cf-publish/src/testing/mod.rs",
        "the cf-publish the test built",
    ),
    (
        "crates/cf-e2e/src/process/tests.rs",
        "the test's own program, started again",
    ),
];

/// What a file calls to hold what it starts to no app of the live app's identity.
const GATE: &str = "program_startable_by_a_test";

/// Whether `relative` is of the tests and the tools: everything of `tools`, `tests`,
/// `bin` and the app's scripts, the end-to-end and the evals crates, and the tests and
/// test support of every other crate and of the app.
fn of_tests_and_tools(relative: &str) -> bool {
    let tooling = [
        "tools/",
        "tests/",
        "bin/",
        "app/scripts/",
        "crates/cf-e2e/",
        "crates/cf-evals/",
    ]
    .iter()
    .any(|folder| relative.starts_with(folder));
    let testing = (relative.starts_with("crates/") || relative.starts_with("app/src-tauri/"))
        && [
            "/tests/",
            "/tests.rs",
            "/testing/",
            "/testing.rs",
            "/test_support",
            "_tests.rs",
        ]
        .iter()
        .any(|part| relative.contains(part));
    tooling || testing
}

/// The files of the tests and the tools a rule reads: their programs, scripts and plants.
fn files() -> Vec<(String, String)> {
    fn walk(root: &Path, folder: &Path, found: &mut Vec<(String, String)>) {
        // A folder a checkout has not (one a build makes), or a link to none, holds nothing.
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if ![
                    "target",
                    "node_modules",
                    ".git",
                    "golden",
                    "goldens",
                    "fixtures",
                ]
                .contains(&name)
                {
                    walk(root, &path, found);
                }
                continue;
            }
            let kind = path
                .extension()
                .and_then(|kind| kind.to_str())
                .unwrap_or("");
            if !["rs", "mjs", "js", "cjs", "sh", "toml", "ps1"].contains(&kind) {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if of_tests_and_tools(&relative) {
                if let Ok(text) = fs::read_to_string(&path) {
                    found.push((relative, text));
                }
            }
        }
    }
    let root = root();
    let mut found = Vec::new();
    for folder in ["tools", "tests", "bin", "app", "crates"] {
        walk(&root, &root.join(folder), &mut found);
    }
    found.sort();
    found
}

/// The lines of `text` that reach an app by its name or its identity, each with its number.
fn reaching_by_name(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = line.trim_start();
            !code.starts_with("//") && !code.starts_with('#') && reaches_by_name(code)
        })
        .map(|(at, line)| (at + 1, line.trim().to_owned()))
        .collect()
}

/// Whether `text` starts a program a caller names: std's `Command::new` of a value, not of
/// a name written there (`Command::new("/bin/sh")`).
fn starts_what_it_is_given(text: &str) -> bool {
    text.lines().any(|line| {
        let code = line.trim_start();
        !code.starts_with("//")
            && code.match_indices("Command::new(").any(|(at, start)| {
                let own = code[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|it| !it.is_alphanumeric() && it != '_');
                let given = code[at + start.len()..].trim_start();
                own && !(given.starts_with('"')
                    || given.starts_with("r\"")
                    || given.starts_with("r#"))
            })
    })
}

#[test]
fn what_reaches_an_app_by_its_name_or_its_identity_is_found() {
    for line in [
        "/usr/bin/open -a ConsensFlow",
        r#"Invocation::new("/usr/bin/open", root).args(["-b", "dev.ngvoicu.consensflow"])"#,
        r#"osascript -e 'tell application id "dev.ngvoicu.consensflow" to quit'"#,
        r#"Run::new("/usr/bin/osascript").args(["-e", "tell application \"ConsensFlow\" to quit"])"#,
        r#"Application("ConsensFlow").quit()"#,
        "$.NSRunningApplication.runningApplicationsWithBundleIdentifier('dev.ngvoicu.consensflow')",
        "NSWorkspace.sharedWorkspace.launchApplication('ConsensFlow')",
        "LSOpenCFURLRef(url, null)",
        "killall ConsensFlow",
        r#"Command::new("pkill").arg("-f").arg("app")"#,
        "kill -9 -1",
        "libc::kill(-1, libc::SIGKILL)",
    ] {
        assert_eq!(reaching_by_name(line).len(), 1, "{line}");
    }
    for line in [
        "$.NSRunningApplication.runningApplicationWithProcessIdentifier(42).hide",
        "the window is open -- and so is the app",
        "/usr/bin/open https://github.com/ngvoicu/consensflow/releases",
        "// osascript -e 'tell application id \"x\" to quit' is what is refused",
        "kill(-leader, signal)",
        "Developer ID Application: Name (TEAM)",
    ] {
        assert_eq!(reaching_by_name(line), [], "{line}");
    }
    assert!(starts_what_it_is_given(
        "let mut command = Command::new(program);"
    ));
    assert!(starts_what_it_is_given(
        "std::process::Command::new(&self.program)"
    ));
    assert!(!starts_what_it_is_given("Command::new(\"/bin/echo\")"));
    assert!(!starts_what_it_is_given(
        "Command::new(r\"C:\\Windows\\ping.exe\")"
    ));
    assert!(!starts_what_it_is_given(
        "// Command::new(program) is what is held"
    ));
    assert!(!starts_what_it_is_given("LongCommand::new(mark)"));
}

#[test]
fn no_test_or_tool_reaches_an_app_by_its_name_or_its_identity() {
    let files = files();
    for read in [
        "tools/xtask/src/updater_smoke/app.rs",
        "crates/cf-e2e/tests/smoke/hidden.rs",
        "tests/plants/release/updater-product.toml",
        "app/scripts/windows-smoke.mjs",
        "crates/cf-process/src/started.rs",
    ] {
        let tested = files.iter().any(|(path, _)| path == read);
        // The product's own sources are not the tests' and the tools'.
        assert_eq!(tested, !read.ends_with("started.rs"), "{read}");
    }
    let found: Vec<String> = files
        .iter()
        .filter(|(path, _)| !RULES.contains(&path.as_str()))
        .flat_map(|(path, text)| {
            reaching_by_name(text)
                .into_iter()
                .map(move |(at, line)| format!("{path}:{at}: {line}"))
        })
        .collect();
    assert!(
        found.is_empty(),
        "these reach an app by its name or its identity, which are the live app's too: reach \
         one by the process it is (its pid, as the smoke's hidden case does), and end one as \
         cf_process::Started:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_start_of_the_tools_and_the_rig_of_a_program_it_is_given_refuses_the_live_apps_identity() {
    let files = files();
    let starters: Vec<&str> = files
        .iter()
        .filter(|(path, text)| {
            (path.starts_with("tools/") || path.starts_with("crates/cf-e2e/"))
                && !RULES.contains(&path.as_str())
                && starts_what_it_is_given(text)
        })
        .map(|(path, _)| path.as_str())
        .collect();
    for held in [
        "tools/xtask/src/process.rs",
        "crates/cf-e2e/src/process.rs",
        "tools/cf-release/src/process.rs",
    ] {
        assert!(starters.contains(&held), "{held} is not read as a start");
    }
    let unheld: Vec<&str> = starters
        .iter()
        .copied()
        .filter(|path| {
            let text = &files.iter().find(|(it, _)| it == path).unwrap().1;
            !text.contains(GATE) && !STARTERS.iter().any(|(listed, _)| listed == path)
        })
        .collect();
    assert!(
        unheld.is_empty(),
        "these start a program they are given and are held to nothing: call \
         cf_base::bundle::{GATE} before it starts, as xtask's process and the rig's Run do:\n{}",
        unheld.join("\n")
    );
}
