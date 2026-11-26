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
/// names a Claude folder of the caller's, which only the kit reads, for its login, and never
/// writes in (`Login::of_folder`: the run's windows run on a folder of the kit's own).
const COPIES: [&str; 3] = [
    "CodexHome",
    "Throwaway::holding",
    "option(\"claude-config\")",
];

/// The folders of the sources that run real harnesses: the matrix's among them (LMX-04), which
/// seats every harness through the kit.
const LOOKED_IN: [&str; 3] = [
    "crates/cf-e2e/tests",
    "crates/cf-e2e/src/live",
    "crates/cf-e2e/src/matrix",
];

/// The kit itself, which sets these by design.
const KIT: [&str; 5] = [
    "crates/cf-e2e/src/live/machine.rs",
    "crates/cf-e2e/src/live/isolation.rs",
    "crates/cf-e2e/src/live/isolation/windows.rs",
    "crates/cf-e2e/src/live/login.rs",
    "crates/cf-e2e/src/live/members.rs",
];

/// The sources that name a variable of [`HOMES`] (or a copy of [`COPIES`]) outside the
/// kit, each with the reason: a source with no reason is a home made by hand.
const BY_HAND: [(&str, &str, &str); 29] = [
    (
        "crates/cf-e2e/src/live/ask.rs",
        "HOME",
        "Muse's model and usage reads use a config folder validated as run-owned, and the kit \
         places their process behind the same boundary as a window",
    ),
    (
        "crates/cf-e2e/src/live/ask.rs",
        "USERPROFILE",
        "the same validated run-owned config folder, as Windows names the home",
    ),
    (
        "crates/cf-e2e/src/live/ask.rs",
        "APPDATA",
        "the roaming folder below that same run-owned config folder on Windows",
    ),
    (
        "crates/cf-e2e/tests/live_muse_reads.rs",
        "HOME",
        "its questions' HOME is a folder the case makes below its own scratch folder (a home of the \
         run's, as every start's is); the copy of Muse's login is in the scratch's config folder",
    ),
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
        "it reads the owner's Claude login alone (`STILL_ACTS`) and copies it, without its refresh token, into a \
         config folder of the run's own, to compare a member's window with a bare Claude's; it writes nothing of the owner's",
    ),
    (
        "crates/cf-e2e/tests/live_finish/compacted.rs",
        "HOME",
        "it reads the machine's Devin config through a source, into a folder of the run's, and asks \
         `ask::devin_config` there (HOME is that folder), then writes it, with a compaction threshold, into the kit's home \
         (`with_home_file`): no window is given the variable",
    ),
    (
        "crates/cf-e2e/tests/live_finish/compacted.rs",
        "XDG_CONFIG_HOME",
        "the same folder, as macOS and Linux name the config folder (the run's own, not the machine's)",
    ),
    (
        "crates/cf-e2e/tests/live_finish/compacted.rs",
        "APPDATA",
        "the same read of the machine's Devin config, as Windows names its folder",
    ),
    (
        "crates/cf-e2e/tests/live_compaction_routes.rs",
        "XDG_DATA_HOME",
        "the same turns' data folder, the throwaway's own, where the case reads their sessions",
    ),
    (
        "crates/cf-e2e/tests/live_muse/tui.rs",
        "HOME",
        "what `live_muse_background` and `live_muse_posture` share (T-790 moved it there): they run \
         Muse alone, in no window of the kit's, behind the test machine's marker: HOME is the run's own root \
         there, below a folder this run made, and Muse's file sandbox keeps it from the owner's (T-705)",
    ),
    (
        "crates/cf-e2e/tests/live_muse/tui.rs",
        "XDG_CONFIG_HOME",
        "the same runs' config folder, a throwaway of their own: nothing of the owner's is read there",
    ),
    (
        "crates/cf-e2e/tests/live_muse/tui.rs",
        "XDG_DATA_HOME",
        "the same runs' data folder, a throwaway of their own, where Muse writes its sessions",
    ),
    (
        "crates/cf-e2e/tests/live_muse_window.rs",
        "CODEX_HOME",
        "the question case gives its window a throwaway folder holding one AGENTS.md, the foreign \
         rules Muse reads as the person's where they have no ~/.claude/CLAUDE.md (T-790); no Codex \
         runs, and nothing of the owner's Codex is read or written",
    ),
    (
        "crates/cf-e2e/tests/live_own_cf.rs",
        "HOME",
        "its windows' shells read the profiles of a home of the run's own, which put decoy `cf`s \
         first on the PATH (T-972); every login stays the kit's home of the run's own, which no \
         HOME reaches",
    ),
    (
        "crates/cf-e2e/tests/live_own_cf.rs",
        "USERPROFILE",
        "the same home, as Windows names it",
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
    (
        "crates/cf-e2e/tests/live_history_items.rs",
        "CLAUDE_CONFIG_DIR",
        "it sets no home: with `--keep` it copies what the run's own window recorded, from the folder \
         the kit gave that window under this variable (`members.rig().var`)",
    ),
    (
        "crates/cf-e2e/tests/live_history_items.rs",
        "CODEX_HOME",
        "the same copy of the records, for another harness",
    ),
    (
        "crates/cf-e2e/tests/live_history_items.rs",
        "PI_CODING_AGENT_DIR",
        "the same copy of the records, for another harness",
    ),
    (
        "crates/cf-e2e/tests/live_history_items.rs",
        "XDG_DATA_HOME",
        "the same copy of the records, for another harness",
    ),
    (
        "crates/cf-e2e/tests/live_keychain.rs",
        "CLAUDE_CONFIG_DIR",
        "the control's start of Claude Code (`Control::start_claude`) is given a folder of the case's own, holding a made-up login: the folder is the thing it shows the Keychain item of; the door gives HOME itself",
    ),
    (
        "crates/cf-e2e/src/live/launcher.rs",
        "XDG_DATA_HOME",
        "the launcher guard READS the account's data folder (`Account::variable`) to find where Claude's and Devin's versions are listed before and after a run; it gives it to no program",
    ),
    (
        "crates/cf-e2e/tests/live_launcher.rs",
        "XDG_DATA_HOME",
        "it gives a window the incident's environment on purpose (T-1082): the owner's HOME and a \
         throwaway XDG_DATA_HOME, which a seat for Muse gives every window, so that Claude Code's \
         updater finds its versions apart from its launcher. The folder is the run's own and nothing \
         of the owner's is read there; `live::launcher` is what holds the owner's launcher",
    ),
    (
        "crates/cf-e2e/src/live/layout.rs",
        "HOME",
        "the layout of a person's home that the kit's own tests give a program they start \
         (`Layout::vars`), with other runs' folders in it to be held as they were: no harness \
         runs on it, and no login is in it",
    ),
    (
        "crates/cf-e2e/src/live/layout.rs",
        "USERPROFILE",
        "the same home, as Windows names it",
    ),
    (
        "crates/cf-e2e/tests/live_finish/stand_in.rs",
        "HOME",
        "the stand-in's window runs on a folder the test made through the door, to show that a \
         process whose home is outside the run's temp root is none of what the windows left: a \
         stand-in, no real harness",
    ),
];

/// The exact constructions of the doors' own that name a variable of [`HOMES`]: a store the table
/// lists to protect, the account's variables the renewal is given, the folder of a Pi renewal. Each is
/// the text of one construction, never a file: any other naming in the same file is still a home made
/// by hand.
const DOORS_OWN: [(&str, &str, &str); 6] = [
    (
        "crates/cf-e2e/src/live/isolation/pi.rs",
        "\"PI_CODING_AGENT_DIR\".to_owned(),",
        "the renewal of Pi's login runs on the OWNER's own Pi folder (`agent`), by name, through the renewal's door: safe only because the renewal needs a test machine's marker, which an owner's account never has",
    ),
    (
        "crates/cf-e2e/src/live/renew.rs",
        "(\"HOME\".to_owned(), text.clone())",
        "the renewal's account variables: the ACCOUNT's own home, by name, for the harness the renewal runs once: safe only behind the test machine's marker",
    ),
    (
        "crates/cf-e2e/src/live/renew.rs",
        "(\"USERPROFILE\".to_owned(), text.clone())",
        "the same account home, as Windows names it: safe only behind the test machine's marker",
    ),
    (
        "crates/cf-e2e/src/live/stores.rs",
        "under(\"XDG_CONFIG_HOME\",",
        "a row of the store table: a folder the doors protect, which no run sets",
    ),
    (
        "crates/cf-e2e/src/live/stores.rs",
        "under(\"XDG_DATA_HOME\",",
        "a row of the store table: a folder the doors protect, which no run sets",
    ),
    (
        "crates/cf-e2e/src/live/stores.rs",
        "under(\"APPDATA\",",
        "a row of the store table: a folder the doors protect, which no run sets",
    ),
];

/// `own` with the doors' own constructions of `file` taken out ([`DOORS_OWN`]).
fn without_the_doors_own(file: &str, own: &str) -> String {
    let mut left = own.to_owned();
    for (source, construction, _) in DOORS_OWN {
        if source == file {
            left = left.replace(construction, "");
        }
    }
    left
}

/// The sources that call `on_own_home`, each with the reason.
const OWN_HOME: [(&str, &str); 1] = [(
    "crates/cf-e2e/tests/live_pi_parity.rs",
    "its HOME holds the Pi folder the case made, and none of the owner's is read or written",
)];

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
/// suite (`tests/live_*`, with its parts in `tests/live_*/`) or the kit's own.
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

/// A harness's server a live case starts itself, outside a rig (`AppServer::start`, Codex's
/// app-server), is given the run's own home by the kit (`Installed::home_outside_a_rig`), as a
/// rig gives its windows theirs: on the case's own environment it ran on the person's `~/.codex`
/// (T-1039, `live_codex_interrupt`).
#[test]
fn a_harness_server_a_live_case_starts_itself_runs_on_the_runs_own_home() -> Outcome {
    let mut on_the_persons = Vec::new();
    for file in &sources()? {
        let own = code(&text(file));
        if own.contains("AppServer::start(") && !own.contains("home_outside_a_rig()") {
            on_the_persons.push(file.clone());
        }
    }
    assert!(
        on_the_persons.is_empty(),
        "these start a harness's server on the case's own environment, the person's home: give \
         it `install.home_outside_a_rig()?`: {on_the_persons:?}"
    );
    Ok(())
}

#[test]
fn no_live_source_makes_the_home_of_a_real_harness_by_hand() -> Outcome {
    let sources = sources()?;
    let mut by_hand = Vec::new();
    for file in &sources {
        if KIT.contains(&file.as_str()) {
            continue;
        }
        let own = without_the_doors_own(file, &text(file));
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
                .any(|(source, held, _)| source == file && *held == name);
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
    // What the tables excuse is still there to excuse.
    for (file, construction, _) in DOORS_OWN {
        assert!(
            text(file).contains(construction),
            "{file} no longer holds {construction}: take it out of DOORS_OWN"
        );
    }
    for (file, name, _) in BY_HAND {
        assert!(
            sources.iter().any(|source| source == file)
                && (sets(&text(file), name) || copies(&text(file), name)),
            "{file} no longer names {name} by hand: take it out of the table"
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
