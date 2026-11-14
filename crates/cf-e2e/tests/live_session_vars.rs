//! Live: what each harness's shell tool hands a command of its own, held to the one list of a
//! parent session's variables (`cf_proto::agents::parent_session_variables`). It is opt-in
//! (`cargo xtask live session-vars`): it spends the machine's quota on one short turn of a
//! window of each harness named, and needs each installed and logged in, so it is ignored
//! unless asked for and is no part of `check` or of CI.
//!
//! The list is what each harness marks its own session with, read from its binaries at one
//! version (T-971); a harness that updates may mark it with more, and an app started from
//! inside such a session would hand the new ones to every window. So each window here, opened
//! as the app opens one, in the kit's isolation (a run's own `CLAUDE_CONFIG_DIR`,
//! `CODEX_HOME`, `PI_CODING_AGENT_DIR` and Devin's and Muse's folders), is given one first
//! message: run a command that writes the names of its environment's variables, one a line,
//! to a file in its folder. Names only: the command reads no value, and nothing of a login is
//! written or said. The names are then held to the list:
//!
//! - a name on the list is the harness's own mark, seen (Devin's had no live sight before);
//! - a name that looks like a session's (a harness's or a terminal's word first, [`PREFIXES`],
//!   or a mark a harness sets by its whole name, [`NAMES`])
//!   and is neither on the list nor a setting the check knows ([`SETTINGS`], each with why) is
//!   a finding, which fails the run: decide it from the harness's binary, and put a marker on
//!   the list and a setting in [`SETTINGS`] (the decision of 2026-10-25, members have every
//!   feature of their harness, so a setting is never taken away);
//! - the rig's own and the board's (`CF_TEST_*`, `CONSENSFLOW_*`) are no harness's.
//!
//! ```text
//! cargo xtask live session-vars                          every harness the kit knows
//! cargo xtask live session-vars claude codex pi devin muse
//! ```

use std::fs;
use std::path::Path;
use std::time::Duration;

use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::Open;
use cf_e2e::live::{self, ask, Harness, Installed, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows: no
/// window of this test runs it (each is opened on the real harness's own path), but a rig
/// has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the check is called in the report.
const WHAT: &str = "every session variable its shell has is on the list";

/// How long a window has to write the file, and how often it is looked for.
const WRITTEN: Duration = Duration::from_secs(300);
const LOOK: Duration = Duration::from_secs(1);

/// What a variable that marks a session begins with: a harness's own word (Devin's store's
/// `CHISEL_` among them), a terminal's (cmux's); and the marks harnesses set for their children
/// by whole name.
const PREFIXES: [&str; 8] = [
    "CLAUDE",
    "CODEX_",
    "PI_",
    "MUSE_",
    "DEVIN_",
    "WINDSURF_",
    "CHISEL_",
    "CMUX_",
];
const NAMES: [&str; 3] = ["AI_AGENT", "TRACEPARENT", "TRACESTATE"];

/// Variables with a session's word in their name that are a harness's settings, not marks
/// of a session: a member keeps every feature of its harness, so they stay, each for why.
const SETTINGS: [Setting; 9] = [
    Setting {
        name: "CODEX_APPLY_PATCH_PRESERVE_LINE_ENDINGS",
        why: "how Codex's patch tool writes line ends, a setting Codex hands it",
    },
    Setting {
        name: "CLAUDE_CONFIG_DIR",
        why: "Claude Code's config folder, which the kit sets",
    },
    Setting {
        name: "CODEX_HOME",
        why: "Codex's home, which the kit sets",
    },
    Setting {
        name: "PI_CODING_AGENT_DIR",
        why: "Pi's folder, which the kit sets",
    },
    Setting {
        name: "MUSE_NO_AUTO_UPDATE",
        why: "Muse's switch against updating itself, which the launch sets",
    },
    Setting {
        name: "CLAUDE_CODE_SUBAGENT_MODEL",
        why: "the model of Claude Code's sub-agents, a setting of the owner's",
    },
    Setting {
        name: "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS",
        why: "Claude Code's agent teams, a setting of the owner's",
    },
    Setting {
        name: "CLAUDE_CODE_USE_BEDROCK",
        why: "Claude Code's provider, a setting of the owner's",
    },
    Setting {
        name: "CLAUDE_CODE_EFFORT_LEVEL",
        why: "Claude Code's effort, a setting (CLAUDE_EFFORT is the mark)",
    },
];

/// A variable with a session's word in its name that a harness reads as its setting: data
/// the check reads, never a variable it sets.
struct Setting {
    name: &'static str,
    /// Why it is a setting, and no mark of a session.
    why: &'static str,
}

/// What a window's names came to.
#[derive(Debug, Default, PartialEq, Eq)]
struct Findings {
    /// The names on the list it had: its harness's marks, seen.
    listed: Vec<String>,
    /// The names that look like a session's and are on neither the list nor [`SETTINGS`].
    lacking: Vec<String>,
}

/// Holds `names`, a window's variables, to the list and to [`SETTINGS`].
fn findings(names: &[String]) -> Findings {
    let mut found = Findings::default();
    for name in names {
        if name.starts_with("CF_TEST_") || name.starts_with("CONSENSFLOW_") {
            continue;
        }
        if cf_proto::agents::is_parent_session_variable(name) {
            found.listed.push(name.clone());
        } else if (PREFIXES.iter().any(|prefix| name.starts_with(prefix))
            || NAMES.contains(&name.as_str()))
            && !SETTINGS.iter().any(|setting| setting.name == name)
        {
            found.lacking.push(name.clone());
        }
    }
    found
}

/// The harnesses the command line names, or every one the kit knows.
fn named() -> Result<Vec<Harness>> {
    let words = live::list("harness");
    if words.is_empty() {
        return Ok(Harness::ALL.to_vec());
    }
    words
        .iter()
        .map(|word| {
            Harness::from_name(word)
                .ok_or_else(|| Error::Daemon(format!("no harness is called {word}")))
        })
        .collect()
}

/// The command that writes the names of its environment's variables, and no value, to
/// `file` in the folder it runs in: awk's `ENVIRON` on Unix, PowerShell's `Env:` drive on
/// Windows.
fn command(file: &str) -> String {
    if cfg!(windows) {
        format!(
            "powershell -NoProfile -Command \"(Get-ChildItem Env:).Name | Sort-Object | Set-Content -Path {file}\""
        )
    } else {
        format!("awk 'BEGIN {{ for (name in ENVIRON) print name }}' | sort > {file}")
    }
}

/// The first message of each window: the command, and a word to end on.
fn message(file: &str) -> String {
    format!(
        "Run this one command with your shell tool, exactly as written, in this folder, and \
         nothing else: {}\nThen reply with only the word done.",
        command(file)
    )
}

/// The names the file at `path` holds once it is written whole: none until it has been
/// there, the same, for two looks.
fn names_once_written(path: &Path) -> Option<Vec<String>> {
    let mut last: Option<String> = None;
    live::until(WRITTEN, LOOK, || {
        let now = fs::read_to_string(path)
            .ok()
            .filter(|text| !text.is_empty());
        let done = now.is_some() && now == last;
        last = now;
        done.then(|| last.clone().unwrap_or_default())
    })
    .map(|text| {
        text.lines()
            .map(|line| line.trim().to_owned())
            .filter(|line| !line.is_empty())
            .collect()
    })
}

/// Opens a window of `install`, has it write its names, and holds them to the list.
fn check_of(rig: &Rig, install: &Installed, log: &Log) -> Result<Check> {
    let name = install.harness().name();
    let file = cf_e2e::door::own_name("session-vars", "txt");
    let open = Open::new(
        rig,
        install,
        &format!("session-vars-{name}"),
        &format!("session-vars-{name}"),
    );
    // Devin takes no first message on its command line here: it is pasted once it is up, as
    // the app pastes one. Every other harness has it as it opens.
    let pasted = install.harness() == Harness::Devin;
    let window = if pasted {
        open.open_telling(log, name)?
    } else {
        open.seed(&message(&file)).open_telling(log, name)?
    };
    if pasted {
        if let Err(screen) = window.ready() {
            window.kill();
            return Ok(Check::failed(name, WHAT, format!("did not open: {screen}")));
        }
        window.paste(&window.given(&message(&file))?)?;
    }
    let path = window.workspace().join(&file);
    let names = names_once_written(&path);
    window.kill();
    let Some(names) = names else {
        return Ok(Check::failed(
            name,
            WHAT,
            format!("wrote no names to {} in {WRITTEN:?}", path.display()),
        ));
    };
    // The file is this run's own, named by its process, in a folder this run made: it goes once
    // read.
    let _ = cf_e2e::door::Door::of_run().remove_in_made(&path);
    log.block(
        &format!("{name}: the names its shell had"),
        &names.join("\n"),
    );
    let found = findings(&names);
    let detail = format!(
        "{} names; on the list: {}; not on it and a session's word in them: {}",
        names.len(),
        listed(&found.listed),
        listed(&found.lacking)
    );
    Ok(Check::new(name, WHAT, found.lacking.is_empty(), detail))
}

/// `names` as a report line says them.
fn listed(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_owned()
    } else {
        names.join(", ")
    }
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short turn of a window of each harness named; cargo xtask live session-vars"]
fn each_harness_s_shell_has_no_session_variable_the_one_list_lacks() -> Result {
    let log = Log::begin("live session-vars")?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let installs = named()?
        .into_iter()
        .map(|harness| log.found(harness))
        .collect::<Result<Vec<_>>>()?;
    let rig = Rig::start(live::rig(&installs, FAKE_AGENT))?;
    let mut checks = Vec::new();
    for install in &installs {
        checks.push(check_of(&rig, install, &log)?);
    }
    rig.close()?;
    report::conclude(&log, "what each harness's shell had", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(words: &[&str]) -> Vec<String> {
        words.iter().map(|&word| word.to_owned()).collect()
    }

    #[test]
    fn a_window_s_names_are_its_harness_s_marks_its_settings_the_rig_s_and_what_the_list_lacks() {
        let found = findings(&names(&[
            "CLAUDECODE",
            "CODEX_THREAD_ID",
            "CLAUDE_CONFIG_DIR",
            "CODEX_HOME",
            "CF_TEST_HARNESS",
            "CONSENSFLOW_URL",
            "CODEX_SOMETHING_NEW",
            "CHISEL_SOMETHING_NEW",
            "CMUX_SURFACE_ID",
            "CMUX_SOMETHING_NEW",
            "AGENT",
            "HOME",
            "PATH",
            "SSH_AUTH_SOCK",
            "SSH_AGENT_PID",
            "RUST_BACKTRACE",
            "CODEX_APPLY_PATCH_PRESERVE_LINE_ENDINGS",
        ]));
        assert_eq!(
            found.listed,
            names(&["CLAUDECODE", "CODEX_THREAD_ID", "CMUX_SURFACE_ID"])
        );
        assert_eq!(
            found.lacking,
            names(&[
                "CODEX_SOMETHING_NEW",
                "CHISEL_SOMETHING_NEW",
                "CMUX_SOMETHING_NEW"
            ])
        );
    }

    #[test]
    fn no_setting_the_check_knows_is_on_the_list() {
        for Setting { name: setting, why } in SETTINGS {
            assert!(!why.is_empty(), "{setting}");
            assert!(
                !cf_proto::agents::is_parent_session_variable(setting),
                "{setting}"
            );
        }
    }

    #[test]
    fn the_command_reads_names_and_no_value() {
        let unix = "awk 'BEGIN { for (name in ENVIRON) print name }' | sort > f.txt";
        if !cfg!(windows) {
            assert_eq!(command("f.txt"), unix);
        }
        assert!(message("f.txt").contains(&command("f.txt")));
    }
}
