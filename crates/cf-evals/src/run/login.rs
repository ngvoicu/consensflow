//! The Claude config folder an eval's windows run on: the kit's throwaway login
//! ([`cf_e2e::live::login::Login`]), never the owner's `~/.claude`.
//!
//! An eval starts the real Claude Code for hours. A Claude Code that starts in the
//! owner's config rewrites its `settings.json`, keeps every prompt it is typed in
//! `history.jsonl` and the workspace in `~/.claude.json`, so its windows run on a
//! folder of the run's own, with a copy of the Keychain login, removed when the run is
//! over (T-302 made it for the live suites; it was decided for every live Claude run
//! on 2026-10-22). `cf-evals` already depends on `cf-e2e`, and the rig is the kit's,
//! so the login is the kit's too: this adds what an eval needs of it.
//!
//! **How long.** The kit refuses a login with less than two hours left, for a live suite
//! that is shorter. An eval states its own length, `--timeout-min` times `--repeat` or
//! `--hours`, and the login must last that much more and [`MARGIN`]: Claude Code
//! refreshes a token when five minutes of it are left, and a refresh in the copy leaves the
//! owner's Keychain item with a refresh token that no longer works. A login that would end
//! before the run does is refused before anything is spent, saying how long the run needs.
//! Claude Code refreshes a token only when five minutes of it are left, so the way on is to run
//! again after the owner's own Claude Code has refreshed it (a fresh token lasts hours, and the
//! window in which a run of hours fits comes just after a refresh), or to make the run shorter:
//! `--hours`, `--timeout-min`, `--repeat`.
//! Nothing here refreshes it, by hand or otherwise, nor writes the Keychain.
//!
//! **A token that ends in the run anyway** (a run that went on past its budget, or a login
//! that cannot cover a run as long as the one asked for) is refreshed by Claude Code
//! itself, in the copy, and the run goes on. What the kit finds out when the run ends (the
//! copy changed, a Keychain item gained, or the owner's setup touched) must not cost the run
//! its report, which may be hours of evidence: [`Throwaway`] keeps it and says it in the
//! report, where it fails the run, instead of ending the rig in an error.
//!
//! **A run with no real Claude Code** (the dry runs, on the stand-in agent's models) has no
//! use of a login and starts none: its windows are as they were.
//!
//! **The other harnesses** (Codex, Pi, Devin, Muse Code) run on the kit's homes
//! ([`cf_e2e::live::isolation`]), each with a copy of its login and none of the owner's other
//! files, made for every harness with a real window ([`Seated`], [`super::machine`]): before
//! T-785 an eval's Pi, Codex and Devin windows ran for hours on the owner's own login folders.
//! Pi is given only logins that last the run's planned length and the kit's margin of 30
//! minutes, never an expired one. What those homes find at the end is kept for the report as
//! Claude's is
//! ([`cf_e2e::live::isolation::Isolation::keeping_what_it_finds`]), with the owner's hashes before and after.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use cf_e2e::live::agents::{STAND_IN_CHIEF_MODEL, STAND_IN_WORKER_MODEL};
use cf_e2e::live::login::Login;
use cf_e2e::live::Harness;
use cf_e2e::rig::ClaudeConfig;
use cf_e2e::Result as Support;

use super::floor::Floor;
use super::machine::Machine;
use crate::measure::Check;
use crate::plan::Staff;
use crate::Result;

/// What a login must last beyond the time a run is planned to take: the end of the run
/// (the last look, the app's close, the settle of a job's end) and a run that overran its
/// budget a while. Claude Code refreshes a token when five minutes of it are left.
pub const MARGIN: Duration = Duration::from_secs(30 * 60);

/// The windows of a run as the homes they run on need them: the harness and model of each, and
/// how long the run is planned to take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seated {
    /// Each window's harness and model: the chief's, then each member's.
    pub windows: Vec<(Harness, String)>,
    /// How long the run is planned to take: what a login it is given must last, and a margin.
    pub planned: Duration,
}

impl Seated {
    /// The windows of a run whose chief is the harness `chief` on `chief_model` and whose staff
    /// is `staff`, planned to take `planned`.
    pub fn of(chief: &str, chief_model: &str, staff: &Staff, planned: Duration) -> Self {
        let mut windows: Vec<(Harness, String)> = Harness::from_name(chief)
            .map(|harness| (harness, chief_model.to_owned()))
            .into_iter()
            .collect();
        windows.extend(staff.agents.iter().filter_map(|agent| {
            Harness::ALL
                .into_iter()
                .find(|harness| harness.kind() == agent.kind)
                .map(|harness| (harness, agent.model.clone()))
        }));
        Self { windows, planned }
    }

    /// Whether a window of `harness` is the real one: on a model that is none of the stand-in
    /// agent's (the dry runs' windows are the stand-in, which reads no login).
    pub fn real(&self, harness: Harness) -> bool {
        !self.models(harness).is_empty()
    }

    /// The models of the real windows of `harness`, each once.
    pub fn models(&self, harness: Harness) -> Vec<String> {
        let mut models: Vec<String> = Vec::new();
        for (held, model) in &self.windows {
            let stand_in = model == STAND_IN_CHIEF_MODEL || model == STAND_IN_WORKER_MODEL;
            if *held == harness && !stand_in && !models.contains(model) {
                models.push(model.clone());
            }
        }
        models
    }
}

/// How long the login must last for a run that is planned to take `planned`.
pub fn lasting(planned: Duration) -> Duration {
    planned + MARGIN
}

/// The login the Claude Code windows of the run `seated` run on, if one of them is real and the
/// machine has a login to copy.
pub fn throwaway(seated: &Seated) -> Option<Arc<Throwaway>> {
    if !seated.real(Harness::Claude) {
        return None;
    }
    Login::of_machine_lasting(lasting(seated.planned)).map(|login| Arc::new(Throwaway::over(login)))
}

/// What a report says of the homes of the run's own once its rig has ended: on the floor what the
/// owner's files of each harness were before the run and after it (their hashes), what the run did
/// to the owner's setups or to the copies of their logins, if it did anything, and the check that it
/// did not. None for a run with no such home, which had no such thing to leave.
pub fn left_as_it_was(machine: &Machine, floor: &mut Floor) -> Result<Option<Check>> {
    if !machine.has_homes() {
        return Ok(None);
    }
    for checked in machine.homes_checked() {
        floor.say(checked)?;
    }
    let found = machine.homes_findings();
    if let Some(found) = &found {
        floor.say(format!(
            "the run left the owner's setup or login of a harness changed: {found}"
        ))?;
    }
    Ok(Some(Check {
        name: "the owner's setups and logins of the run's harnesses were left as they were"
            .to_owned(),
        ok: found.is_none(),
    }))
}

/// A Claude config folder (the kit's throwaway, with a copy of the machine's login) whose rig
/// never ends in an error for what the run did to the owner's setup or to the login: it is kept
/// ([`Throwaway::findings`]) for the report.
#[derive(Debug)]
pub struct Throwaway {
    config: Box<dyn ClaudeConfig>,
    /// What the kit found out when a rig that used the folder ended, if it found anything.
    found: Mutex<Option<String>>,
}

impl Throwaway {
    /// `config`, whose findings at the end are kept instead of failing the rig.
    pub fn over(config: impl ClaudeConfig + 'static) -> Self {
        Self {
            config: Box::new(config),
            found: Mutex::new(None),
        }
    }

    /// What the run did that it was not to, in a line: the owner's setup touched, or the copy of
    /// the login refreshed (the owner's may be stale then). None for a run that did neither.
    pub fn findings(&self) -> Option<String> {
        self.found
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// What the kit's banner says, one finding to a line (`  - …`), as a line of words; the banner
/// itself where it has no such lines.
fn in_a_line(banner: &str) -> String {
    let lines: Vec<&str> = banner
        .lines()
        .filter_map(|line| line.strip_prefix("  - "))
        .map(str::trim)
        .collect();
    if lines.is_empty() {
        banner
            .trim_matches(|c: char| c == '!' || c.is_whitespace())
            .to_owned()
    } else {
        lines.join("; ")
    }
}

impl ClaudeConfig for Throwaway {
    fn folder(&self) -> Support<PathBuf> {
        self.config.folder()
    }

    fn described(&self) -> String {
        self.config.described()
    }

    fn keep_records(&self, into: &Path) -> Support {
        self.config.keep_records(into)
    }

    fn finish(&self) -> Support {
        if let Err(found) = self.config.finish() {
            *self.found.lock().unwrap_or_else(PoisonError::into_inner) =
                Some(in_a_line(&found.to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
