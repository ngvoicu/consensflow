//! No test's `cf` finds the machine's own programs (the reviews of 2026-10-16). A
//! `cf` looks for a harness's CLI in the system's folders of programs last: the ones
//! `CONSENSFLOW_TOOL_FOLDERS` names, else Homebrew's on a Mac. A test that starts the
//! `cf` under test in an environment of its own and leaves the variable out finds the
//! machine's Homebrew `claude` or `codex`, and asks it things; a daemon of the page's
//! tests did. So every source that starts it sets the variable on what it starts, or
//! gives it the environment of a kit that does ([`THROUGH`]), or starts it where it
//! looks for no harness: it only names its path, or runs a window's Codex supervisor
//! on the Codex it is handed, which passes every `CONSENSFLOW_` variable on to that
//! Codex.
//!
//! Set means assigned: an entry of the environment given to a `Command` or a `Run`, or
//! of a list or an object of variables, in the source's own code, and not its
//! comments or its tests, where a name is no assignment (the second reviews of
//! 2026-10-16 took a kit's variable out and left this green on a comment).

use std::fs;
use std::path::Path;

use cf_e2e::checkout;

use crate::Outcome;

/// The words a source starts the `cf` under test by: Rust's two, the CLI goldens'
/// name for it, and the page's tests'.
const STARTS: [&str; 4] = [
    "env!(\"CARGO_BIN_EXE_cf\")",
    "cf::binary()",
    "names.cf",
    "NATIVE_CF",
];

/// The variable each sets.
const VARIABLE: &str = "CONSENSFLOW_TOOL_FOLDERS";

/// The folders of the sources that start programs: the crates, the tools, and the
/// scripts' and the page's tests.
const LOOKED_IN: [&str; 4] = ["crates", "tools", "tests", "app/tests"];

/// What is built, installed or fetched there, not looked in.
const UNSOURCED: [&str; 3] = [".git", "node_modules", "target"];

/// The kit of the suites' homes, whose environment sets the variable.
const SCRATCH: &str = "crates/cf-e2e/src/scratch_home.rs";

/// The rig's, likewise.
const RIG: &str = "crates/cf-e2e/src/rig/install.rs";

/// The sources that name the `cf` under test and set no variable themselves: the
/// kit whose environment their `cf` is given, or none where it looks for no harness.
const THROUGH: [(&str, Option<&str>); 14] = [
    ("app/tests/agents-server.mjs", Some("tests/helpers.mjs")),
    ("crates/cf-e2e/src/daemon.rs", Some(SCRATCH)),
    // It names the cf of a live window's hooks, which run in the rig's environment.
    ("crates/cf-e2e/src/live/window/open.rs", Some(RIG)),
    ("crates/cf-e2e/src/rig/launch.rs", Some(RIG)),
    ("crates/cf-e2e/tests/cli/commands.rs", Some(SCRATCH)),
    ("crates/cf-e2e/tests/cli/roster.rs", Some(SCRATCH)),
    ("crates/cf-e2e/tests/cli/tool_folders.rs", None),
    ("crates/cf-e2e/tests/daemon/core_daemon.rs", None),
    // It names the cf a rig is copied from, and starts it in the rig's environment.
    (
        "crates/cf-e2e/tests/regressions/a_cf_a_build_took_away_left_a_window_none.rs",
        Some(RIG),
    ),
    ("crates/cf-e2e/tests/rig/cf_native.rs", Some(SCRATCH)),
    // It compares the path of the cf a launch's hooks name.
    ("crates/cf-e2e/tests/rig/live_member.rs", None),
    ("crates/cf-e2e/tests/rig/codex_session.rs", None),
    ("crates/cf/tests/cli_goldens/names.rs", None),
    ("tests/choice.mjs", None),
];

/// The text of a source, as the repository names it.
fn text(file: &str) -> String {
    fs::read(checkout::path(file)).map_or_else(
        |_| String::new(),
        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
    )
}

/// The code of a source, where a variable is set: up to its own tests (its first
/// `#[cfg(test)]`), without its comments, and without the whitespace rustfmt and
/// Prettier lay an entry out across lines with.
fn code(text: &str) -> String {
    let own = text.split("#[cfg(test)]").next().unwrap_or_default();
    own.lines()
        .filter(|line| {
            let line = line.trim_start();
            !(line.starts_with("//") || line.starts_with('*') || line.starts_with("/*"))
        })
        .flat_map(|line| line.chars().filter(|character| !character.is_whitespace()))
        .collect()
}

/// Whether `text` sets the variable: an entry of an environment, `("NAME", …)`
/// or `("NAME".to_owned(), …)`, or of an object, `NAME: …`.
fn sets(text: &str) -> bool {
    let code = code(text);
    [
        format!("(\"{VARIABLE}\","),
        format!("(\"{VARIABLE}\".to_owned(),"),
        format!("{VARIABLE}:"),
    ]
    .iter()
    .any(|entry| code.contains(entry.as_str()))
}

/// Every source of [`LOOKED_IN`] that names the `cf` under test, as the repository
/// names it: a link is no source, and is not followed.
fn starters() -> Result<Vec<String>, cf_e2e::Error> {
    let mut found = Vec::new();
    for folder in LOOKED_IN {
        for file in checkout::files_below(&checkout::path(folder), &UNSOURCED)? {
            if !is_source(&file) {
                continue;
            }
            let named = checkout::relative(&file);
            if STARTS.iter().any(|word| text(&named).contains(word)) {
                found.push(named);
            }
        }
    }
    Ok(found)
}

/// Whether `file` is a source of Rust's or JavaScript's, and a file, not a link.
fn is_source(file: &Path) -> bool {
    let named = ["rs", "mjs", "js"]
        .iter()
        .any(|extension| file.extension().is_some_and(|its| its == *extension));
    named && checkout::is_plain_file(file)
}

#[test]
fn an_assignment_is_an_entry_of_code_and_a_name_in_a_comment_or_a_test_is_none() {
    let forms = [
        "command.env(\"CONSENSFLOW_TOOL_FOLDERS\", \"\");",
        ".var(\"CONSENSFLOW_TOOL_FOLDERS\", \"\")",
        "(\n    \"CONSENSFLOW_TOOL_FOLDERS\".to_owned(),\n    text(&root),\n),",
        "    CONSENSFLOW_TOOL_FOLDERS: join(root, 'system-bin'),",
    ];
    for form in forms {
        assert!(sets(form), "{form}");
    }
    let names = [
        "//! the variable `CONSENSFLOW_TOOL_FOLDERS` is set by the kit",
        " * that takes CONSENSFLOW_TOOL_FOLDERS: a folder of its own",
        "assert_eq!(names, [\"PATH\", \"CONSENSFLOW_TOOL_FOLDERS\"]);",
        "#[cfg(test)]\nmod tests {\n    let env = [(\"CONSENSFLOW_TOOL_FOLDERS\", \"\")];\n}",
    ];
    for name in names {
        assert!(!sets(name), "{name}");
    }
}

#[test]
fn every_source_that_starts_the_cf_under_test_names_a_folder_for_the_systems_programs() -> Outcome {
    let starters = starters()?;
    let unset: Vec<&String> = starters
        .iter()
        .filter(|file| !sets(&text(file)))
        .filter(|file| !THROUGH.iter().any(|(through, _)| through == file))
        .collect();
    assert!(
        unset.is_empty(),
        "these start the cf under test and set no {VARIABLE}, so it finds the machine's Homebrew programs: {unset:?}"
    );
    for (file, kit) in THROUGH {
        assert!(
            starters.iter().any(|starter| starter == file),
            "{file} names the cf under test no longer: take it out of the table"
        );
        if let Some(kit) = kit {
            assert!(
                sets(&text(kit)),
                "{file}'s cf is given {kit}'s environment, which sets no {VARIABLE}"
            );
        }
    }
    Ok(())
}
