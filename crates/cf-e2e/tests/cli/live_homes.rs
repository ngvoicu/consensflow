//! No live case makes the home of a real harness by hand (T-724, after T-696: a live run on
//! Pi refreshed the owner's `openai` login, because the rig clears the environment and no
//! case ever set a throwaway Pi folder). The kit does it for every harness
//! (`live::isolation`, `live::login`) and the rig sets its variables last, so a case that
//! names a variable by which a harness finds its login, or makes a home to hold a copy of
//! one, is a second way to do it that the kit's checks do not see: the next harness added
//! would be reached by hand again, or not at all.
//!
//! A case that must give its windows another is in [`BY_HAND`] with the reason, and a case
//! whose window runs on the home of its own says so ([`OWN_HOME`]).

use cf_e2e::checkout;

use crate::tool_folders::code;
use crate::Outcome;

/// The variables a harness finds its login or its config by, and the home it keeps them in.
const HOMES: [&str; 8] = [
    "HOME",
    "USERPROFILE",
    "CODEX_HOME",
    "CLAUDE_CONFIG_DIR",
    "PI_CODING_AGENT_DIR",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "APPDATA",
];

/// What else makes a home by hand: a type of a test's that copied a login, and the flag that
/// hands the rig a Claude folder of the caller's.
const COPIES: [&str; 3] = [
    "CodexHome",
    "Throwaway::holding",
    "option(\"claude-config\")",
];

/// The folders of the sources that run real harnesses.
const LOOKED_IN: [&str; 3] = [
    "crates/cf-e2e/tests",
    "crates/cf-e2e/src/live",
    "crates/cf-evals/src",
];

/// The kit itself, which sets these by design.
const KIT: [&str; 4] = [
    "crates/cf-e2e/src/live/machine.rs",
    "crates/cf-e2e/src/live/isolation.rs",
    "crates/cf-e2e/src/live/login.rs",
    "crates/cf-e2e/src/live/members.rs",
];

/// The sources that name a variable of [`HOMES`] (or a copy of [`COPIES`]) outside the
/// kit, each with the reason: a source with no reason is a home made by hand.
const BY_HAND: [(&str, &str, &str); 11] = [
    (
        "crates/cf-e2e/tests/live_pi_parity.rs",
        "HOME",
        "its windows run on a HOME of the case's own, with a Pi folder made for it (`on_own_home`)",
    ),
    (
        "crates/cf-e2e/tests/live_pi_parity.rs",
        "USERPROFILE",
        "the same home, as Windows names it",
    ),
    (
        "crates/cf-e2e/tests/live_parity/claude.rs",
        "CLAUDE_CONFIG_DIR",
        "it copies the owner's whole Claude setup (skills, plugins, agents) into a throwaway on purpose, \
         to compare a member's window with a bare Claude's; it writes nothing of the owner's",
    ),
    (
        "crates/cf-e2e/tests/live_finish/compacted.rs",
        "HOME",
        "it reads the machine's Devin config where a launch reads it (`ask::devin_config` is given \
         the machine's environment) and writes it, with a compaction threshold, into the kit's home \
         (`with_home_file`): no window is given the variable",
    ),
    (
        "crates/cf-e2e/tests/live_finish/compacted.rs",
        "XDG_CONFIG_HOME",
        "the same read of the machine's Devin config, as macOS and Linux name its folder",
    ),
    (
        "crates/cf-e2e/tests/live_finish/compacted.rs",
        "APPDATA",
        "the same read of the machine's Devin config, as Windows names its folder",
    ),
    (
        "crates/cf-e2e/tests/live_muse_background.rs",
        "HOME",
        "it runs Muse alone, in no window of the kit's: the owner's home for the keychain, which \
         holds the login, and nothing else of it (Muse finds its keychain by HOME, T-705)",
    ),
    (
        "crates/cf-e2e/tests/live_muse_background.rs",
        "XDG_CONFIG_HOME",
        "the same run's config folder, a throwaway of its own: nothing of the owner's is read there",
    ),
    (
        "crates/cf-e2e/tests/live_muse_background.rs",
        "XDG_DATA_HOME",
        "the same run's data folder, a throwaway of its own, where Muse writes its sessions",
    ),
    (
        "crates/cf-e2e/tests/live_muse_reads.rs",
        "XDG_CONFIG_HOME",
        "its reads run `muse` alone on a throwaway copy of `auth.json` (T-841), and prove the owner's \
         folder untouched",
    ),
    (
        "crates/cf-e2e/tests/live_muse_reads.rs",
        "XDG_DATA_HOME",
        "the same reads' data folder, a throwaway holding a copy of the model catalog alone",
    ),
];

/// The sources that call `on_own_home`, each with the reason.
const OWN_HOME: [(&str, &str); 1] = [(
    "crates/cf-e2e/tests/live_pi_parity.rs",
    "its HOME holds the Pi folder the case made, and none of the owner's is read or written",
)];

/// The sources the evals still run real harnesses from on the owner's logins: their own
/// rig (`run/machine.rs`) names the owner's home and Codex's. The follow-up is to give the
/// evals' installs the kit's homes, with their findings kept for the report as Claude's
/// are (`run/login.rs`), not to fail a run of hours at its end.
const EVALS: [(&str, &str); 3] = [
    ("crates/cf-evals/src/run/machine.rs", "CODEX_HOME"),
    ("crates/cf-evals/src/run/machine.rs", "HOME"),
    ("crates/cf-evals/src/run/machine.rs", "USERPROFILE"),
];

/// Whether `text` sets the variable `name`: an entry of an environment, `("NAME", …)`,
/// `("NAME".to_owned(), …)` or `.var("NAME", …)`.
fn sets(text: &str, name: &str) -> bool {
    let code = code(text);
    [format!("(\"{name}\","), format!("(\"{name}\".to_owned(),")]
        .iter()
        .any(|entry| code.contains(entry.as_str()))
}

/// Whether `text` holds the copy `word` in its code.
fn copies(text: &str, word: &str) -> bool {
    code(text).contains(&word.split_whitespace().collect::<String>())
}

/// Whether `file` (as the repository names it) is code that runs a real harness: a live
/// suite (`tests/live_*`, with its parts in `tests/live_*/`), the kit's own, or the evals'.
/// Not the stand-in suites (`tests/rig`, `tests/cli`, `tests/smoke`, `tests/regressions`),
/// whose homes are the rig's own, nor a source's unit tests (`tests.rs`).
fn runs_real_harnesses(file: &str) -> bool {
    if file.ends_with("/tests.rs") {
        return false;
    }
    match file.strip_prefix("crates/cf-e2e/tests/") {
        Some(below) => below.starts_with("live_"),
        None => true,
    }
}

/// Every Rust source of [`LOOKED_IN`] that runs a real harness, as the repository names it.
fn sources() -> Result<Vec<String>, cf_e2e::Error> {
    let mut found = Vec::new();
    for folder in LOOKED_IN {
        for file in
            checkout::files_below(&checkout::path(folder), &[".git", "node_modules", "target"])?
        {
            if file.extension().is_some_and(|extension| extension == "rs")
                && checkout::is_plain_file(&file)
                && runs_real_harnesses(&checkout::relative(&file))
            {
                found.push(checkout::relative(&file));
            }
        }
    }
    Ok(found)
}

fn text(file: &str) -> String {
    std::fs::read(checkout::path(file)).map_or_else(
        |_| String::new(),
        |bytes| String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[test]
fn a_home_is_set_by_an_entry_of_code_and_a_name_in_a_comment_or_a_test_is_none() {
    for form in [
        ".var(\"CODEX_HOME\", home)",
        "config.var(\"PI_CODING_AGENT_DIR\", dir)",
        "(\"XDG_DATA_HOME\".to_owned(), data)",
        "plan.with_var(\"CODEX_HOME\", home)",
    ] {
        let name = form.split('"').nth(1).unwrap();
        assert!(sets(form, name), "{form}");
    }
    for name in [
        "//! the home is CODEX_HOME, which the kit sets",
        "assert_eq!(rig.var(\"CODEX_HOME\"), x);",
        "#[cfg(test)]\nmod tests {\n    let env = [(\"CODEX_HOME\", \"/x\")];\n}",
    ] {
        assert!(!sets(name, "CODEX_HOME"), "{name}");
    }
    assert!(copies(
        "let home = CodexHome::copying_login_of(&codex)?;",
        "CodexHome"
    ));
    assert!(!copies("// a CodexHome of the run's own", "CodexHome"));
}

#[test]
fn no_live_source_makes_the_home_of_a_real_harness_by_hand() -> Outcome {
    let sources = sources()?;
    let mut by_hand = Vec::new();
    for file in &sources {
        if KIT.contains(&file.as_str()) {
            continue;
        }
        let own = text(file);
        let named = HOMES
            .iter()
            .filter(|name| sets(&own, name))
            .map(|name| (*name).to_owned())
            .chain(
                COPIES
                    .iter()
                    .filter(|word| copies(&own, word))
                    .map(|word| (*word).to_owned()),
            );
        for name in named {
            let allowed = BY_HAND
                .iter()
                .any(|(source, held, _)| source == file && *held == name)
                || EVALS
                    .iter()
                    .any(|(source, held)| source == file && *held == name);
            if !allowed {
                by_hand.push(format!("{file}: {name}"));
            }
        }
    }
    assert!(
        by_hand.is_empty(),
        "these name a home of a real harness's own, or copy a login by hand: the kit's \
         (`live::isolation`, `live::login`) is given every window already, and its checks do \
         not see this one: {by_hand:?}"
    );
    // What the table excuses is still there to excuse.
    for (file, name, _) in BY_HAND {
        assert!(
            sources.iter().any(|source| source == file)
                && (sets(&text(file), name) || copies(&text(file), name)),
            "{file} no longer names {name} by hand: take it out of the table"
        );
    }
    for (file, name) in EVALS {
        assert!(
            sets(&text(file), name),
            "{file} no longer sets {name}: take it out of the table"
        );
    }
    Ok(())
}

#[test]
fn only_the_cases_that_are_their_own_home_say_so() -> Outcome {
    let sources = sources()?;
    let found: Vec<&String> = sources
        .iter()
        .filter(|file| {
            !file.ends_with("live/machine.rs") && code(&text(file)).contains("on_own_home")
        })
        .collect();
    let allowed: Vec<&str> = OWN_HOME.iter().map(|(file, _)| *file).collect();
    assert_eq!(
        found.iter().map(|file| file.as_str()).collect::<Vec<_>>(),
        allowed,
        "a case that opts out of the kit's home needs its reason in OWN_HOME"
    );
    Ok(())
}
