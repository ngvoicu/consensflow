//! A live run on a test machine renews the logins of Claude Code and Pi before it copies
//! them (T-1067, the owner's decision of 2026-11-01).
//!
//! **The problem.** On zeewin2 the owner rarely uses Claude Code or Pi himself. Their access
//! tokens end between his visits, and the kit refused a live run on a login that "has expired"
//! even where the refresh token behind it still worked ([`super::login`], [`super::isolation`]:
//! a run's copy that refreshes would use the refresh token up and log the owner out). He had to
//! open the harnesses again and again.
//!
//! **The rule.** On a test machine, when the owner's login of Claude Code or of Pi (the entry of
//! the provider the run's model needs) lasts less than the run does and its margin, the kit runs
//! that harness once, as the owner, with a tiny prompt, so that it renews its own token as it does
//! when the owner opens it. Then it reads the login again and goes on as it always did: the copy is
//! made, or the run is refused as before. This is the one exception to the kit never writing the
//! owner's harness files, and only the harness's own renewal of its own login is allowed it:
//!
//! - **Never in a say-only run.** A run that only says ([`crate::door::Door`], `CF_LIVE_AUDIT=1`)
//!   starts no harness: a login that would need the renewal ends the run with one line that says so,
//!   which the audit file holds too ([`Renewal::renew`]); the rig that needed it does not start, and
//!   what was done before stays done (the builds, the harnesses' version questions, the first folders).
//! - **Never on the owner's Mac.** A run knows it is on a test machine by one variable,
//!   [`TEST_MACHINE`], that the Windows runner (`cargo xtask windows`) gives the command it runs
//!   with the machine's name, and that nothing else sets. No variable, no renewal.
//! - **Never `/login`, `/logout`, `/model` or `/effort`, never usage credits.** The harness is
//!   started in its print mode on the model the run uses (Claude Code's `-p`, Pi's `-p`), with
//!   no tools, no MCP servers, no extensions and no session saved, in a folder of its own that is
//!   named as a folder of a run, so that the kit's own watches see anything it writes about it.
//! - **Bounded in time** ([`LIMIT`]), and **changing nothing of the owner's but its login**:
//!   what the kit watches of the owner's setup ([`super::login`], [`super::isolation`]) is
//!   compared before and after, and a difference is a refusal that names the file.
//! - **Nothing a login holds is ever said.** The run says which file the renewal changed, by name
//!   and by the time its login ends, before and after ([`Renewed`]).
//!
//! **The copy cannot renew.** What the run is given is the access token and no refresh token
//! ([`super::login`], [`super::isolation`]): a copy that cannot renew cannot use up the owner's refresh
//! token, whatever the run's length. So the margin a login must last beyond the run
//! ([`super::isolation::MARGIN`]) is no longer the owner's safety (it was 30 minutes of it) but the
//! harness's own look at the end of the access token, five minutes in both, and a few minutes for
//! the end of the run. A run that outlasts its token fails loudly where the token ends.
//!
//! **What a harness renews.** Claude Code refreshes a token when five minutes of it are left, and
//! Pi once the entry has ended: a token that lasts an hour and is wanted for two is not renewed by
//! using it. Then the renewal ran and failed, and the run is refused, with those words and the
//! times, as it is without a renewal. What the one-shot achieves is the case the rule is for: a
//! login that ended while nobody used the harness.

use std::path::Path;
use std::time::{Duration, Instant};

use super::login::span;
use super::machine::Harness;
use crate::door::Door;
use crate::process::{own_var, Run};
use crate::{Error, Result};

/// The variable the Windows runner gives the command it runs, with the name of the machine
/// (`zeewin2`): the one signal that a run is on a machine that is there to test on. The runner
/// spells it in `tools/xtask/src/windows/script.rs`.
pub const TEST_MACHINE: &str = "CONSENSFLOW_TEST_MACHINE";

/// How long the harness may take to renew its login, once: a print-mode call on one word. A
/// harness that is slower is ended, and the renewal has failed.
pub const LIMIT: Duration = Duration::from_secs(180);

/// How much of an access token a harness leaves before it renews it: Claude Code renews one with
/// five minutes left (and Pi an entry that has ended or has that much), so running either on a login
/// that has more is a call of the model for nothing, and the record of it in the owner's setup
/// besides (zeewin2, T-1136: 29 minutes left, the kit ran Claude Code, it moved nothing). A login
/// that has more is refused with the time it ends, and the advice to run again once it has.
pub const RENEWS_WITHIN: Duration = Duration::from_secs(5 * 60);

/// What the harness is asked: nothing it would need a tool for, and one word to answer.
const PROMPT: &str = "Reply with the single word ok.";

/// What the name of the folder the harness is started in begins with: the name of a folder of a
/// run, as the kit's watches of the owner's setup know them, so that a project, a prompt or a
/// conversation the harness kept of it in the owner's files is found.
const FOLDER: &str = "consensflow-integration-renew-";

/// How much of what the harness said when it failed is shown.
const SAID: usize = 300;

/// A machine that the runner says is there to test on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestMachine(String);

impl TestMachine {
    /// The machine this run is on, if the runner started it on one: none on the owner's Mac, which
    /// has no runner to give the variable.
    pub fn detect() -> Option<Self> {
        Self::signalled(own_var(TEST_MACHINE).as_deref())
    }

    /// The machine a signal names; none where there is none, or it names nothing. (The tests of
    /// the logins make a test machine with it.)
    pub(super) fn signalled(signal: Option<&str>) -> Option<Self> {
        let name = signal?.trim();
        (!name.is_empty()).then(|| Self(name.to_owned()))
    }

    /// The machine's name, as the runner calls it.
    pub fn name(&self) -> &str {
        &self.0
    }
}

/// A login on the machine that a renewal is for.
pub struct Kept<'a> {
    /// The file it is in, as a run says it (`~/.claude/.credentials.json`).
    pub file: String,
    /// Whose it is, as a run says it (`Claude Code's login`, `Pi's `openai` login`).
    pub login: String,
    /// Reads what the machine keeps of it now.
    pub read: &'a dyn Fn() -> Result<String>,
    /// When the login in a text ends (milliseconds since the epoch); none where it holds none.
    pub expiry: &'a dyn Fn(&str) -> Option<i64>,
}

/// What a renewal that ran came to: the file and the time its login ended before and after.
/// Never the login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renewed {
    pub file: String,
    pub login: String,
    pub before: Option<i64>,
    pub after: Option<i64>,
    /// How long the harness took.
    pub took: Duration,
    /// How the harness ended where it ended badly all the same (its model refused the owner's
    /// session, as Sonnet 5.5's safeguards did on zeewin2 with Claude Code in the owner's config):
    /// the renewal is judged by the login, which moved.
    pub ended: Option<String>,
}

impl Renewed {
    /// Whether the login ends later than it did.
    pub fn moved(&self) -> bool {
        matches!((self.before, self.after), (Some(before), Some(after)) if after > before)
    }

    /// What it came to, in words for the run's report: the file, the login, and its end before and
    /// after.
    pub fn said(&self, machine: &TestMachine) -> String {
        format!(
            "the kit ran the harness once as the owner on {} to renew {}: {} ended {}, and ends {} ({} s)",
            machine.name(),
            self.login,
            self.file,
            time(self.before),
            if self.moved() {
                time(self.after)
            } else {
                format!("{}: it did not move", time(self.after))
            },
            self.took.as_secs()
        ) + &self
            .ended
            .as_ref()
            .map_or_else(String::new, |how| format!("; the harness ended badly after renewing it: {how}"))
    }
}

/// `ms`, milliseconds since the epoch, as a time of day in UTC; `none` where there is none.
pub fn time(ms: Option<i64>) -> String {
    ms.and_then(|ms| jiff::Timestamp::from_millisecond(ms).ok())
        .map_or_else(
            || "at no time that can be read".to_owned(),
            |at| at.to_string(),
        )
}

/// A harness's renewal, on the test machine it is for.
pub struct Renewal {
    machine: TestMachine,
    /// The harness, as a run says it (`Claude Code`).
    harness: &'static str,
    /// When the harness renews a token, in words: what a login that is refused after a renewal
    /// that ran is told.
    only: &'static str,
    /// Runs the harness once, as the owner: none where it ended well, how it ended where it ran
    /// and ended badly (a status, its limit), and an error where it could not be run, or changed
    /// what it must not.
    shot: Box<dyn Fn() -> Result<Option<String>> + Send + Sync>,
    /// The run's door: a run that only says renews nothing ([`Renewal::renew`]).
    door: Door,
}

impl Renewal {
    pub fn new(
        machine: TestMachine,
        harness: &'static str,
        only: &'static str,
        shot: impl Fn() -> Result<Option<String>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            machine,
            harness,
            only,
            shot: Box::new(shot),
            door: Door::of_run().clone(),
        }
    }

    /// The same renewal under `door`, which is the run's own (the tests of a say-only run give
    /// theirs).
    #[must_use]
    pub fn under(mut self, door: Door) -> Self {
        self.door = door;
        self
    }

    pub fn machine(&self) -> &TestMachine {
        &self.machine
    }

    /// Runs the harness once and reads the login again: what the machine keeps of it now, and
    /// what the renewal came to. The words that say why it did not, if the harness could not renew.
    pub fn renew(&self, kept: &Kept, text: &str) -> std::result::Result<(String, Renewed), String> {
        // The harness would run as the owner on the owner's own login, which no run's folder holds:
        // a run that only says starts none, ends here (the rig that needed it does not start) and
        // says so in the audit file as well.
        if self.door.says_only() {
            let line = self.not_here();
            self.door.note(&line);
            return Err(line);
        }
        let before = (kept.expiry)(text);
        let started = Instant::now();
        let ended = (self.shot)().map_err(|failed| self.failed(&failed.to_string()))?;
        let took = started.elapsed();
        let fresh = (kept.read)().map_err(|failed| {
            self.failed(&format!("{} could not be read again ({failed})", kept.file))
        })?;
        let after = (kept.expiry)(&fresh);
        let renewed = Renewed {
            file: kept.file.clone(),
            login: kept.login.clone(),
            before,
            after,
            took,
            ended,
        };
        // The renewal is the login's, not the model call's: a harness that ended badly after it
        // renewed its login (an API error of the owner's own session) has renewed it.
        if let (Some(how), false) = (&renewed.ended, renewed.moved()) {
            return Err(self.failed(how));
        }
        Ok((fresh, renewed))
    }

    /// What a run that only says says of a login it would renew.
    pub fn not_here(&self) -> String {
        format!(
            "say only: the login of {} would be renewed here; a say-only run renews nothing",
            self.harness
        )
    }

    /// What follows the refusal of a login the renewal did not mend, at `now` (milliseconds since
    /// the epoch): the renewal ran and failed, and how. A login that moved but is still short says
    /// when it ends now; one that did not move says how long it has left and that the harness
    /// renews it itself once it has ended, when the kit asks it to; one that has ended and was not
    /// renewed has a refresh token that no longer works.
    pub fn ended(&self, renewed: &Renewed, now: i64) -> String {
        let how =
            match (renewed.moved(), renewed.after) {
                (true, _) => format!(
                    "the login now ends {}, which is still short of what the run needs",
                    time(renewed.after)
                ),
                (false, Some(after)) if after > now => format!(
                "the login still ends {} (in {}): {} renews a token {}, so one that has time left \
                 is not renewed by using it; run again once it has ended, when the kit runs {} and \
                 it renews its login itself",
                time(renewed.after),
                span(Duration::from_millis(u64::try_from(after - now).unwrap_or(0))),
                self.harness,
                self.only,
                self.harness
            ),
                _ => format!(
                "the login has still ended ({}): {} could not renew it, so its refresh token may \
                 be used up or revoked, and the owner is to log in again",
                time(renewed.after),
                self.harness
            ),
            };
        self.failed(&how)
    }

    /// The renewal ran and failed, because `why`.
    pub fn failed(&self, why: &str) -> String {
        format!(
            "On {} the kit ran {} once as the owner to renew it (T-1067), and the renewal ran and failed: {why}",
            self.machine.name(),
            self.harness
        )
    }
}

/// Runs `program` once with `words`, as the owner (the environment this run has), in a folder of
/// its own that is named as a folder of a run ([`FOLDER`]), with nothing to read, ended after
/// [`LIMIT`]. None where it ended well; how it ended where it did not, in what it said and never
/// what it printed (the renewal is judged by the login it left, [`Renewal::renew`]); an error where
/// it could not be run at all.
pub fn one_shot(harness: Harness, program: &Path, words: &[String]) -> Result<Option<String>> {
    one_shot_within(harness, program, words, LIMIT, &[])
}

/// [`one_shot`], whose own folder is also taken out of the owner's `records` when it ended: a
/// harness that keeps something of the folder it was started in there (Claude Code's
/// `projects/<the folder's path>/memory`, even with no session saved) leaves nothing of the
/// renewal's behind, so that the owner's setup is compared with what it was ([`FOLDER`] names it).
pub fn one_shot_in(
    harness: Harness,
    program: &Path,
    words: &[String],
    records: &[&Path],
) -> Result<Option<String>> {
    one_shot_within(harness, program, words, LIMIT, records)
}

/// [`one_shot`], ended after `limit`, and its record in each of `records` taken away. The harness
/// is told what every window of the kit's is ([`super::launcher::updates_off_of`]): it runs on the
/// owner's own folders, where an update or an install would land in what is theirs.
fn one_shot_within(
    harness: Harness,
    program: &Path,
    words: &[String],
    limit: Duration,
    records: &[&Path],
) -> Result<Option<String>> {
    let folder = crate::door::made(FOLDER).map_err(|source| Error::File {
        action: "make a folder in",
        path: std::env::temp_dir(),
        source,
    })?;
    let ran = Run::new(program)
        .args(words)
        .inheriting_env()
        .vars(super::launcher::updates_off_of(harness).iter().copied())
        .finding_programs()
        .cwd(folder.path())
        .closed_input()
        .limit(limit)
        .run()?;
    forget(records, folder.path());
    if ran.timed_out {
        return Ok(Some(format!("it was ended after {} s", limit.as_secs())));
    }
    if ran.code == Some(0) {
        return Ok(None);
    }
    let said: String = ran.stderr.split_whitespace().collect::<Vec<_>>().join(" ");
    let said: String = said.chars().take(SAID).collect();
    Ok(Some(format!(
        "it ended {} ({})",
        ran.code
            .map_or_else(|| "by a signal".to_owned(), |code| code.to_string()),
        if said.is_empty() {
            "saying nothing"
        } else {
            &said
        }
    )))
}

/// Takes away of each folder in `records` the one entry the harness made there for the folder
/// `own`, by its whole name ([`crate::door::Record`]: the real path of `own` spelt as Claude Code
/// spells a folder into a name), through the door. Nothing else of the owner's is touched: an
/// entry of theirs whose name ends the same is theirs (T-1557, finding 5). One that cannot be
/// taken away is left (the kit's watch then names it).
fn forget(records: &[&Path], own: &Path) {
    let door = crate::door::Door::of_run();
    for folder in records {
        if let Some(record) = crate::door::Record::of(folder, own) {
            let _ = door.remove_record(&record);
        }
    }
}

/// The words Claude Code is started with: the print mode on the model of the run, and nothing
/// else it would reach out for: no session saved, no MCP servers of the owner's, no browser, no
/// commands or skills, no tools.
pub fn claude_words(model: &str) -> Vec<String> {
    [
        "-p",
        PROMPT,
        "--model",
        model,
        "--no-session-persistence",
        "--strict-mcp-config",
        "--no-chrome",
        "--disable-slash-commands",
        "--tools",
        "",
    ]
    .map(str::to_owned)
    .into()
}

/// The words Pi is started with: the print mode on the model of the run, with no session saved
/// and no tools, extensions, MCP servers, skills, context files or templates.
pub fn pi_words(model: &str) -> Vec<String> {
    [
        "-p",
        PROMPT,
        "--model",
        model,
        "--thinking",
        "off",
        "--no-session",
        "--no-tools",
        "--no-extensions",
        "--no-mcp",
        "--no-skills",
        "--no-context-files",
        "--no-prompt-templates",
    ]
    .map(str::to_owned)
    .into()
}

#[cfg(test)]
pub(super) mod tests;
