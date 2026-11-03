//! Live: a member's shell tool finds its window's own `cf`, not another `cf` its profile puts
//! first on the `PATH` (T-972; the issue pages `devin-shell-finds-installed-cf` and
//! `devin-shell-on-windows-does-not-find-cf`). It is opt-in (`cargo xtask live own-cf`): it
//! spends the machine's quota on one short task of each harness named, and needs each one
//! installed and logged in, so it is ignored unless asked for and is no part of `check` or of
//! CI.
//!
//! The daemon puts the bundle's `bin` first on every window's `PATH`, so the `cf` a window
//! names is this install's. A harness's shell may build its `PATH` again from the owner's
//! profile: Devin takes one snapshot of a login `/bin/zsh` at its start and runs every command
//! with it, and the owner's `.zprofile` puts `~/.local/bin` first, where a stale launcher ran
//! the live app's older `cf` (2026-10-01: `cf: unknown command "history"`). On Windows, a Devin
//! member's Git Bash answered `cf: command not found` (2026-10-16), and the live suite of the
//! day took the call for one that ran.
//!
//! Each harness named is the one worker of a project whose chief is the stand-in, on the native
//! daemon of a rig, one after another ([`cf_e2e::live::members`]). Its windows run on a home of
//! the run's own ([`home`]): a decoy `cf` in `~/.local/bin` and in the package manager's folder
//! (Homebrew's on a Mac), and every profile a shell reads putting both first on the `PATH`, as
//! the owner's do. The logins stay the kit's homes of the run's own
//! ([`cf_e2e::live::isolation`]), which no `HOME` reaches. The worker is asked to run the
//! `whoami` of its window's `cf` as its instructions start every `cf` command (never the word
//! `cf`, which a shell that reads the profile can find another program for: the task says no
//! command that starts with it), then the command that says which program the word `cf` is in
//! its shell, then to print its `PATH`, and to say what each printed. It holds where
//!
//! - no decoy wrote down a call (each writes every call down beside itself: the proof a shell
//!   found it first, whatever the model then says), and
//! - the result holds what only the window's own `cf` can answer to `cf whoami`: the worker's
//!   handle and role, from the daemon the window belongs to ([`verdict`]).
//!
//! What the worker reported its shell finds, and its `PATH`, are said for each.
//!
//! **Muse Code** is started with the person's own `HOME` by the kit's launcher (its login is in
//! the keychain, which it finds by `HOME`; a throwaway home opens its onboarding, T-705), so a
//! bash it runs reads the person's profile; its windows are given `ZDOTDIR` of the run's home
//! besides, which a zsh reads its profiles from whatever its `HOME`. Its line says so.
//!
//! ```text
//! cargo xtask live own-cf                          Claude Code, Codex, Pi, Devin and Muse
//! cargo xtask live own-cf devin
//! cargo xtask live --set windows                  on Windows: own-cf claude codex pi among the set
//! ```
//!
//! Devin and Muse run on the owner's own folders on Windows (the kit has no home of the run's
//! own for them there): name them on Windows only on purpose. The run fails when any worker's
//! shell ran a decoy, or its `cf` could not be shown to be the window's own.
//!
//! **What it found (T-972, 2026-10-31):** Devin 3000.11.228 on the Mac runs its commands with
//! one snapshot of a login zsh (`.zprofile`, then `.zshrc`), whose `PATH` puts the decoy in
//! `~/.local/bin` first, and Claude Code 2.1.312 on Windows runs them in a login Git Bash that
//! puts `~/bin` and the profile's folders first. The role texts used to say the word `cf`, so
//! both ran the decoy: the suite was red on them. Since T-1049 every command a window is
//! taught (its role text, its hooks and the texts ConsensFlow types into it) starts with the
//! window's own `cf` by its whole path, and the task says no bare `cf`: a worker that ran the
//! decoy typed the word it was never given. The line of `command -v cf` is still reported:
//! where it names a decoy, the shell would have found it first, and the path made that
//! harmless. (A first version of the run's home wrote a `.zshenv`, which every zsh reads, a
//! plain `zsh -c` too: Codex ran the decoy then, through that file alone.)

// The parts are in `live_own_cf/`: cargo looks for the modules of a test's root file beside
// it, where each would be a suite of its own.
#[path = "live_own_cf/home.rs"]
mod home;
#[path = "live_own_cf/verdict.rs"]
mod verdict;

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::members::{self, delivered_result, find, is_over, named, Members, Plan};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::{self, agents, between, cut, last_lines, text, Harness, Log, Who};
use cf_e2e::Result;

use home::Home;
use verdict::{found_line, judge, Seen};

/// The stand-in for the chief's window.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// The decoy: the stand-in `cf`, which writes down each call beside itself.
const FAKE_CF: &str = env!("CARGO_BIN_EXE_fake-cf");

/// The harnesses run where none is named: every one of [`Harness::ALL`], so a harness added
/// later is run too.
fn every_harness() -> Vec<Harness> {
    Harness::ALL.to_vec()
}

/// What the worker is asked. It names no command that starts with the word `cf`: the way its
/// instructions start every `cf` command is what the run tries (a task that said `cf whoami`
/// would have a shell that finds another `cf` first run it, whatever the instructions say).
pub const TASK: &str = "Run these three commands with your shell tool, one after another, \
and then reply with what each one printed, verbatim, each under its number. Run nothing else, \
and change no file.\n\
1. The `whoami` command of your ConsensFlow cf, started the way your instructions start \
every cf command.\n\
2. The command that says which program the word cf is in your shell: `command -v cf` in bash \
or zsh, `(Get-Command cf).Source` in PowerShell, `where cf` in cmd.\n\
3. Your shell's PATH: `echo \"$PATH\"` in bash or zsh, `$env:PATH` in PowerShell, \
`echo %PATH%` in cmd.";

/// How long the worker has to do its task, its result to reach the chief, and how often both
/// are looked at.
const WORK: Duration = Duration::from_secs(300);
const DELIVERS: Duration = Duration::from_secs(60);
const LOOK: Duration = Duration::from_secs(2);

/// The project of `harness`'s run: the stand-in chief and the worker, whose windows run on the
/// run's home `home` (and its zsh profiles on it, whatever its `HOME`).
fn plan(harness: Harness, home: &Home) -> Plan {
    let root = home.root.to_string_lossy().into_owned();
    let plan = Plan::new(
        &format!("own-cf-{}", harness.name()),
        members::stand_in_chief(),
    )
    .with(members::member(harness))
    .with_var("HOME", root.clone())
    .with_var("ZDOTDIR", root.clone());
    if cfg!(windows) {
        plan.with_var("USERPROFILE", root)
    } else {
        plan
    }
}

/// What one worker's run came to.
struct Ran {
    seen: Seen,
    /// What it reported its shell finds, and the line that says it.
    found: Option<String>,
}

/// Runs `harness` as the worker, on the run's home: what was seen of it.
fn run(harness: Harness, log: &Log) -> Result<Ran> {
    let home = home::make(Path::new(FAKE_CF))?;
    log.say(format!(
        "the windows' home is {}; the decoys: {}",
        home.root.display(),
        home.decoys
            .iter()
            .map(|decoy| decoy.folder.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ));
    let mut members = Members::open(&plan(harness, &home), Path::new(FAKE_AGENT), log)?;
    let observed = observe(&mut members, harness, &home, log);
    let closed = members.close();
    let ran = observed?;
    closed?;
    Ok(ran)
}

/// The worker given its task and waited for, and what it left looked at.
fn observe(members: &mut Members, harness: Harness, home: &Home, log: &Log) -> Result<Ran> {
    let agent = agents::id(harness);
    log.say(format!(
        "the window's own cf is {}",
        members.rig().cf().display()
    ));
    members.add(&agent, &[])?;
    let number = members.give(&agent, TASK)?;
    log.say(format!("T-{number} is given: {}", cut(TASK, 100)));
    // The worker's handle, as the board names its participant: what `cf whoami` says.
    let mut handle = String::new();
    let thread = find(log, "the worker's task to end", WORK, LOOK, || {
        if handle.is_empty() {
            handle = members.lane(&agent)?["participant"]["handle"]
                .as_str()
                .unwrap_or_default()
                .to_owned();
        }
        let thread = members.task(number)?;
        Ok(is_over(&thread).then_some(thread))
    })?;
    let state = thread
        .as_ref()
        .map_or_else(|| "not over".to_owned(), |thread| text(&thread["state"]));
    let result = if state == "done" {
        find(log, "the result to reach the chief", DELIVERS, LOOK, || {
            Ok(delivered_result(&members.inbox(Some(members.chief()))?))
        })?
    } else {
        None
    };
    if state != "done" {
        // Why the task is not done: what the board says of the worker, and what its window
        // last drew.
        let lane = members.lane(&agent)?;
        let screen = members
            .window_of(&agent)
            .ok()
            .flatten()
            .map(|window| last_lines(&members.rig().screen(window.id())))
            .unwrap_or_default();
        log.block(
            &format!("T-{number} is {state}"),
            &format!(
                "the worker's lane: {}\nits task: {}\nwhat its window last drew: {screen}",
                live::pretty(Ok(lane)),
                live::pretty(members.task(number)),
            ),
        );
    }
    let found = result.as_deref().and_then(found_line).map(str::to_owned);
    Ok(Ran {
        seen: Seen {
            state,
            result,
            decoys: home::called(home),
            handle,
        },
        found,
    })
}

/// What `harness`'s line of the table adds: where its shell's `HOME` is not the run's.
fn caveat(harness: Harness) -> &'static str {
    if harness == Harness::Muse {
        " (Muse runs on the person's HOME, for its keychain: only a zsh it runs reads the run's profiles, by ZDOTDIR)"
    } else {
        ""
    }
}

#[test]
#[ignore = "opt-in: spends the machine's quota on one short task of each harness named; cargo xtask live own-cf"]
fn a_members_shell_finds_its_windows_own_cf_ahead_of_a_decoy_its_profile_puts_first() -> Result {
    let log = Log::begin("live own-cf")?;
    let harnesses = named(
        &live::list("harness"),
        &Harness::ALL,
        Harness::name,
        "harness",
        &every_harness(),
    )?;
    let mut checks = Vec::new();
    for harness in harnesses {
        let log = log.scoped(harness.name());
        let who = Who::Harness(harness);
        let before = who.version();
        let ran = run(harness, &log);
        let version = between(before, who.version());
        let (held, detail) = match &ran {
            Ok(ran) => match judge(&ran.seen) {
                Ok(why) => (true, why),
                Err(problems) => (false, problems.join("; ")),
            },
            Err(failed) => (false, failed.to_string()),
        };
        if let Ok(ran) = &ran {
            log.block(
                &format!(
                    "== {} {version}: {}",
                    harness.name(),
                    if held { "ok" } else { "FAILED" }
                ),
                &format!(
                    "its shell finds: {}\nits result:\n{}",
                    ran.found.as_deref().unwrap_or("(it named no cf)"),
                    ran.seen.result.as_deref().unwrap_or("(none)")
                ),
            );
        }
        checks.push(Check::new(
            harness.name(),
            "its shell finds the window's own cf",
            held,
            format!("{version}: {detail}{}", caveat(harness)),
        ));
    }
    report::conclude(&log, "which cf each worker's shell found", &checks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_harness_named_every_one_is_run_and_a_name_asks_for_that_one() {
        let all = named(
            &[],
            &Harness::ALL,
            Harness::name,
            "harness",
            &every_harness(),
        )
        .unwrap();
        assert_eq!(all, Harness::ALL);
        let devin = named(
            &["devin".to_owned()],
            &Harness::ALL,
            Harness::name,
            "harness",
            &every_harness(),
        )
        .unwrap();
        assert_eq!(devin, [Harness::Devin]);
    }

    #[test]
    fn the_windows_run_on_the_runs_home_and_their_zsh_reads_its_profiles() {
        let home = Home {
            root: Path::new("/h/own").to_path_buf(),
            decoys: Vec::new(),
        };
        let plan = plan(Harness::Devin, &home);
        assert_eq!(plan.folder, "own-cf-devin");
        let var = |name: &str| {
            plan.vars
                .iter()
                .rev()
                .find(|(held, _)| held == name)
                .map(|(_, value)| value.clone())
        };
        assert_eq!(var("HOME").as_deref(), Some("/h/own"));
        assert_eq!(var("ZDOTDIR").as_deref(), Some("/h/own"));
        assert_eq!(var("USERPROFILE").is_some(), cfg!(windows));
        let ids: Vec<&str> = plan
            .roster
            .iter()
            .filter_map(|row| row["id"].as_str())
            .collect();
        assert_eq!(ids, ["chief", "live-devin"]);
    }

    #[test]
    fn the_task_asks_for_whoami_the_cf_found_and_the_path_in_every_shell() {
        for asked in [
            "`whoami` command of your ConsensFlow cf",
            "`command -v cf`",
            "(Get-Command cf).Source",
            "`where cf`",
            "$env:PATH",
        ] {
            assert!(TASK.contains(asked), "{asked}");
        }
    }

    /// A task that says `cf whoami` has a shell that finds another `cf` first run the decoy
    /// whatever the role text says: the run would be red for the task's own words, and green
    /// on a harness only by the model's luck. No command of the task starts with the word.
    #[test]
    fn the_task_teaches_no_command_that_starts_with_the_word_cf() {
        for line in TASK.lines() {
            for word in ["`cf ", " cf whoami", "`cf`"] {
                assert!(!line.contains(word), "{word:?} in {line:?}");
            }
        }
        assert!(!TASK.contains("cf whoami"));
    }

    #[test]
    fn only_muses_line_says_its_home_is_the_persons() {
        assert!(caveat(Harness::Muse).contains("ZDOTDIR"));
        for harness in every_harness()
            .into_iter()
            .filter(|harness| *harness != Harness::Muse)
        {
            assert_eq!(caveat(harness), "");
        }
    }
}
