//! Who takes which task, and who is out of quota: each open tiered task goes to
//! the best free member, its requester told once when nobody is free; a
//! window's refusal takes its member out until the reset, its work held with
//! its window; held work goes on there, and work of a member whose agent is
//! gone goes back to the board. The ledger says what may happen to a task; this says
//! which, and to whom. It owns the record's quota part and the tasks whose
//! requesters were told they wait.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use cf_base::refusal::Refusal;
use cf_base::time;
use cf_harness::contract::Observed;
use cf_harness::records::{Level, Quota, Role};
use cf_ledger::{
    Candidate, HeldTask, LedgerError, NewNote, ParticipantView, ProjectView, TaskCard, TaskThread,
    RESUME_WORDS,
};
use serde_json::Value;

use crate::deliveries::Delivering;
use crate::delivery_text::header_id;
use crate::dispatcher::Dispatcher;
use crate::record::Record;
use crate::seams::{EngineError, SavedAgent};

/// What the note to the human says of an account refused again as soon as its
/// task went on.
const MAY_NEED_YOU: &str = "may need you";

/// How soon after the words that resumed a task a refusal answers them, and
/// is the same wait going on.
const AGAIN_MS: i64 = 5 * 60_000;

/// How long a quota lasts when its harness names no reset.
const UNKNOWN_RESET_MS: i64 = 60 * 60_000;

/// What a member is marked out until when its account is unavailable (MUS-23): no
/// reset exists, so nothing resets it, and its held tasks are due at no time of their
/// own. Only the human's Try again now, or a turn of its account that got through, ends it.
const NEVER: &str = "9999-12-31T23:59:59.999Z";

/// Whether `until`, a member's time out, is [`NEVER`]: its account is unavailable.
pub(crate) fn never(until: &str) -> bool {
    until.starts_with("9999-")
}

/// What a window of a member out says it is doing: out of quota until the reset, or,
/// where the account is unavailable (no reset), why (MUS-23).
pub(crate) fn out_words(until: &str, unavailable: Option<&str>) -> String {
    match unavailable {
        Some(because) => format!("unavailable: {because}"),
        None if never(until) => "unavailable".to_owned(),
        None => format!("out of quota until {until}"),
    }
}

/// How much later one reset must be than another to be a limit of its own: a
/// harness names a reset to the hour (Claude Code's "resets 3pm"), or none, and
/// one that names none is an hour from each refusal.
const SAME_LIMIT_MS: i64 = 60 * 60_000;

/// The record's quota part.
#[derive(Debug, Default)]
pub(crate) struct QuotaPart {
    /// What the window's harness last said of its quota.
    pub(crate) reported: Option<Arc<Quota>>,
    /// The refusal this window acted on: the same one is history after.
    pub(crate) handled: Option<Arc<Quota>>,
    /// On a member's own record: until when it is low, and gets no new work.
    pub(crate) low_until: Option<String>,
    /// The words that resumed the window's task whose going on its requester
    /// was told of (`tell_it_went_on`): the ledger tells it once anyway.
    pub(crate) told_going_on: Option<i64>,
    /// The time of the window's last answer that was looked at as news of its
    /// account's quota: each answer is news once.
    pub(crate) answered: Option<String>,
}

/// News that an account has quota again.
pub(crate) enum News<'a> {
    /// A turn of the window `record` got through (`observed`, its look).
    /// `returned` is the reset its member was out until, where that turn brought
    /// it back: none where the member was not out.
    Turn {
        record: &'a Record,
        observed: &'a Observed,
        returned: Option<String>,
    },
    /// The human's Try again now.
    Human,
}

/// What the scheduler keeps across records: the open tasks whose requester
/// heard that they wait for a free member, each with its project; and the
/// windows out of quota whose waiting messages the human heard of, each with
/// the time its participant was marked out.
#[derive(Debug, Default)]
pub(crate) struct SchedulerState {
    waiting_noted: RefCell<HashMap<i64, i64>>,
    out_noted: RefCell<HashSet<(i64, String)>>,
    /// The participants a window's turn brought back, or left out for a later
    /// limit, each with the reset it was out until: the same limit again is no
    /// news a teammate's turn can undo. In memory, so a restart may bring a chief
    /// on a weekly limit back once more (one refused turn, one note).
    tried: RefCell<HashMap<i64, String>>,
}

/// The same refusal: the one a record's reading gave, or one of the same time.
fn same_refusal(handled: Option<&Arc<Quota>>, quota: &Arc<Quota>) -> bool {
    handled.is_some_and(|handled| {
        Arc::ptr_eq(handled, quota) || quota.at().is_some_and(|at| handled.at() == Some(at))
    })
}

/// What a message is, as the human is told of it: the result of a task, its words.
fn what_it_is(kind: &str, task: Option<i64>) -> String {
    match (kind, task) {
        ("result", Some(number)) => format!("the result of T-{number}"),
        ("task", Some(number)) => format!("the words of T-{number}"),
        (kind, Some(number)) => format!("{} {kind} on T-{number}", article(kind)),
        (kind, None) => format!("{} {kind}", article(kind)),
    }
}

/// The article a kind of message takes: an answer, a note.
fn article(kind: &str) -> &'static str {
    if kind.starts_with(['a', 'e', 'i', 'o', 'u']) {
        "an"
    } else {
        "a"
    }
}

/// Whether the reset `until` is a later limit's than the reset `returned`: more
/// than [`SAME_LIMIT_MS`] after it.
fn later_limit(until: &str, returned: &str) -> bool {
    time::parse(until)
        .zip(time::parse(returned))
        .is_some_and(|(until, returned)| until - returned > SAME_LIMIT_MS)
}

/// Whether the resets `one` and `other` are one limit's: within [`SAME_LIMIT_MS`]
/// of each other, as a reset no refusal named is an hour from each refusal.
fn same_limit(one: &str, other: &str) -> bool {
    time::parse(one)
        .zip(time::parse(other))
        .is_some_and(|(one, other)| (one - other).abs() <= SAME_LIMIT_MS)
}

/// When a window's last turn that got through answered, as its record names the
/// time: none for a turn that failed or was refused.
fn last_answer(observed: &Observed) -> Option<String> {
    if observed.failed
        || observed
            .quota
            .as_deref()
            .is_some_and(|quota| quota.level() == Level::Exhausted)
    {
        return None;
    }
    observed
        .items()
        .iter()
        .rev()
        .find(|item| item.role == Role::Assistant)
        .and_then(|item| item.at.as_ref())
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Whether the time `later` names comes after the one `earlier` names; a
/// time that does not read is after nothing.
fn after(later: &str, earlier: &str) -> bool {
    time::parse(later)
        .zip(time::parse(earlier))
        .is_some_and(|(later, earlier)| later > earlier)
}

impl Dispatcher {
    /// Each open task goes to the best free member of its tier; the
    /// requester hears once when none is. A task that needs others waits
    /// for them to be accepted, in silence: its card says what it waits for.
    /// Says whether it gave any task out.
    pub(crate) fn assign_open_tasks(&self, project: &ProjectView) -> Result<bool, EngineError> {
        let mut assigned = false;
        let open = self.seams.ledger.borrow().open_tasks(project.id)?;
        for task in open {
            if !task.blocked_by.is_empty() {
                continue;
            }
            let candidates = self
                .seams
                .ledger
                .borrow()
                .candidates(project.id, task.number)?;
            let free: Vec<&Candidate> = candidates
                .iter()
                .filter(|member| self.why_not_free(member).is_none())
                .collect();
            if let Some(best) = rank(free, &candidates).first() {
                self.seams.ledger.borrow_mut().assign_task(
                    project.id,
                    task.number,
                    best.member.id,
                )?;
                self.scheduler.waiting_noted.borrow_mut().remove(&task.id);
                self.changed();
                assigned = true;
            } else if !self.scheduler.waiting_noted.borrow().contains_key(&task.id) {
                self.scheduler
                    .waiting_noted
                    .borrow_mut()
                    .insert(task.id, project.id);
                let why: Vec<String> = candidates
                    .iter()
                    .filter_map(|member| self.why_not_free(member))
                    .collect();
                let pool = if task.pool.as_deref() == Some("designer") {
                    "image designer".to_owned()
                } else {
                    format!(
                        "{} {}",
                        task.tier.as_deref().unwrap_or("null"),
                        task.pool.as_deref().unwrap_or("null")
                    )
                };
                self.seams.ledger.borrow_mut().note(
                    project.id,
                    &NewNote {
                        from: None,
                        to: task.requester.clone(),
                        task: Some(task.number),
                        body: format!(
                            "T-{} waits for a free {pool}: {}.",
                            task.number,
                            why.join("; ")
                        ),
                    },
                )?;
                self.changed();
            }
        }
        Ok(assigned)
    }

    /// Why a member is not free, the first reason that holds for it: on a
    /// harness whose windows open, its agent saved, not out of quota, not
    /// low on it; none when it is free.
    fn why_not_free(&self, candidate: &Candidate) -> Option<String> {
        let member = &candidate.member;
        let harness = member.harness.as_deref().unwrap_or("null");
        if self.seams.adapters.adapter(harness).is_none() {
            return Some(format!(
                "@{} runs on {harness}, whose windows ConsensFlow cannot open",
                member.handle
            ));
        }
        let agent = member.agent.as_deref();
        match agent.map_or(Some(None), |name| self.saved_agent(name)) {
            None => {
                return Some(format!(
                    "@{}'s agent cannot be read (your agents file needs fixing: see Agents)",
                    member.handle
                ));
            }
            Some(None) => {
                let agent = agent.unwrap_or("null");
                return Some(match self.seams.roster.retired(agent) {
                    Some(why) => format!(
                        "@{} has no agent any more ({agent} left the catalog: {why}; remove @{} from the staff and add another agent)",
                        member.handle, member.handle
                    ),
                    None => format!(
                        "@{} has no agent any more ({agent} is not among your agents: define it, or remove the member)",
                        member.handle
                    ),
                });
            }
            Some(Some(_)) => {}
        }
        if self.out(member.out_until.as_deref()) {
            let until = member.out_until.as_deref().unwrap_or_default();
            return Some(if never(until) {
                format!(
                    "@{}'s account is unavailable (its billing or login was refused)",
                    member.handle
                )
            } else {
                format!("@{} is out of quota until {until}", member.handle)
            });
        }
        if self.is_low(member.id) {
            return Some(format!("@{} is low on quota", member.handle));
        }
        None
    }

    /// The member a session belongs to; a member or the chief is its own.
    pub(crate) fn member_of(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
    ) -> ParticipantView {
        participant
            .member_id
            .and_then(|member| project.participants.iter().find(|p| p.id == member))
            .unwrap_or(participant)
            .clone()
    }

    /// Whether a member is out of quota now.
    pub(crate) fn is_out(&self, member: &ParticipantView) -> bool {
        self.out(member.out_until.as_deref())
    }

    fn out(&self, until: Option<&str>) -> bool {
        until
            .and_then(time::parse)
            .is_some_and(|until| until > self.now())
    }

    /// Whether a member is low on quota now, as its own record says.
    fn is_low(&self, member: i64) -> bool {
        let until = self
            .record(member)
            .and_then(|record| record.quota.borrow().low_until.clone());
        self.out(until.as_deref())
    }

    /// Whether the refusal a window shows is news for it: each window of a
    /// member runs into its quota on its own, the first marking the member
    /// out and every one holding or giving back its own work. A harness
    /// keeps its last record, so a refusal stays in view long after its
    /// reset: one whose reset has passed, one dated before the member was
    /// last marked out or back, and one this window acted on already are
    /// history.
    pub(crate) fn refused_here(&self, record: &Record, owner: &ParticipantView) -> bool {
        let (reported, handled) = {
            let quota = record.quota.borrow();
            (quota.reported.clone(), quota.handled.clone())
        };
        let Some(quota) = reported.filter(|quota| quota.level() == Level::Exhausted) else {
            return false;
        };
        if quota
            .resets_at()
            .and_then(time::parse)
            .is_some_and(|reset| reset <= self.now())
        {
            return false;
        }
        if same_refusal(handled.as_ref(), &quota) {
            return false;
        }
        match (
            owner.out_since.as_deref(),
            quota.at().filter(|at| !at.is_empty()),
        ) {
            (Some(since), Some(at)) => after(at, since),
            _ => true,
        }
    }

    /// Whether a window shows a refusal of its own it has not acted on, whose
    /// reset has not passed: in a window of a member already out, one its harness
    /// dated before the daemon marked the member out, which `refused_here` takes
    /// for history (two windows of a member refused in one pass; the review of the
    /// quota wait, T-88).
    pub(crate) fn refused_unhandled(&self, record: &Record) -> bool {
        let (reported, handled) = {
            let quota = record.quota.borrow();
            (quota.reported.clone(), quota.handled.clone())
        };
        let Some(quota) = reported.filter(|quota| quota.level() == Level::Exhausted) else {
            return false;
        };
        let passed = quota
            .resets_at()
            .and_then(time::parse)
            .is_some_and(|reset| reset <= self.now());
        !passed && !same_refusal(handled.as_ref(), &quota)
    }

    /// Marks the refusal a window shows as acted on, so the window acts on it once.
    pub(crate) fn handled(&self, record: &Record) {
        let mut quota = record.quota.borrow_mut();
        let reported = quota.reported.clone();
        quota.handled = reported;
    }

    /// Whether a window of a member out of quota answered after it was
    /// marked out: its harness got a turn through, so the quota is back
    /// before its reset. Only a record that names its time says so.
    pub(crate) fn answered_since(&self, owner: &ParticipantView, observed: &Observed) -> bool {
        let Some(since) = owner.out_since.as_deref() else {
            return false;
        };
        last_answer(observed).is_some_and(|at| after(&at, since))
    }

    /// Whether a window of a member whose account is unavailable shows the harness's own
    /// count of an allowance, not used up: the refusal that made it unavailable is over, with
    /// no human to say so (Muse's `usage`, MUS-23). A member out for a quota that resets
    /// waits for its reset, whatever a count in an old record says.
    pub(crate) fn shows_an_allowance(&self, owner: &ParticipantView, observed: &Observed) -> bool {
        owner.out_until.as_deref().is_some_and(never)
            && matches!(
                observed.quota.as_deref(),
                Some(Quota::Usage {
                    level: Level::Ok | Level::Low,
                    ..
                })
            )
    }

    /// Whether a window's last turn ended in a refusal its member is past
    /// now: the turn was cut short, and failed nothing.
    pub(crate) fn cut_short(&self, observed: &Observed) -> bool {
        observed.failed
            && observed
                .quota
                .as_deref()
                .is_some_and(|quota| quota.level() == Level::Exhausted)
    }

    /// What a window's harness says of its quota, as of this look. Low is
    /// soft: the current task continues, nothing new comes until the reset it
    /// names (an hour when it names none). It outlives the window, on the
    /// member's own record, so a low member is not asked again at once.
    pub(crate) fn record_quota(
        &self,
        record: &Record,
        owner: &ParticipantView,
        quota: Option<Arc<Quota>>,
    ) {
        record.quota.borrow_mut().reported = quota.clone();
        let Some(quota) = quota else {
            return;
        };
        let low_until = (quota.level() == Level::Low).then(|| self.reset_of(Some(&quota)));
        self.record_of(owner.id).quota.borrow_mut().low_until = low_until;
    }

    /// When a refusal happened: the time its record names, or now.
    pub(crate) fn refused_at(&self, quota: Option<&Quota>) -> i64 {
        quota
            .and_then(Quota::at)
            .and_then(time::parse)
            .unwrap_or_else(|| self.now())
    }

    /// When a quota resets: the time its harness names, or an hour from now.
    pub(crate) fn reset_of(&self, quota: Option<&Quota>) -> String {
        if quota.is_some_and(Quota::unavailable) {
            return NEVER.to_owned();
        }
        quota
            .and_then(Quota::resets_at)
            .map_or_else(|| time::iso(self.now() + UNKNOWN_RESET_MS), str::to_owned)
    }

    /// Why a member's account is unavailable, as a refusal said it: none for a quota that
    /// resets, and for a refusal that is not shown now.
    pub(crate) fn unavailable_because(quota: Option<&Quota>) -> Option<&str> {
        match quota {
            Some(Quota::Unavailable { because, .. }) => Some(because),
            _ => None,
        }
    }

    /// The human is told that an account is unavailable, once for each time its member
    /// is marked out, whatever waits for it (MUS-23): nothing is sent to it until their
    /// word, so they hear it and what to do.
    pub(crate) fn tell_unavailable(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        owner: &ParticipantView,
        because: &str,
    ) -> Result<(), EngineError> {
        let mark = (owner.id, owner.out_since.clone().unwrap_or_default());
        if !self.scheduler.out_noted.borrow_mut().insert(mark) {
            return Ok(());
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: format!(
                    "@{}'s account is unavailable: {because}. Nothing is sent to it until you say it works again: press Try again now on @{} once it does.",
                    participant.handle, owner.handle
                ),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// News that an account has quota again brings back the others out on it.
    /// Where every window of `owner`'s harness is logged into the one account
    /// (`Adapter::one_account`), a quota that came back is theirs too: each other
    /// member out on that harness, and the chief, is back. A chief at rest takes
    /// no turn of its own, so a `/login` that brought its members back left it out
    /// until its reset, with everything sent to it queued (the field report of
    /// 2026-10-22, T-254).
    ///
    /// The human's word brings back all of them. A window's turn that got through
    /// is news for those marked out before it, read from the ledger as it is now
    /// (one marked out earlier in this pass too), once for each answer. One
    /// account is not one quota: a plan may limit one model by the week, so a
    /// member back from a reset brings back none out until a later one, and the
    /// human hears it stays out; and a turn brings one back once for each limit
    /// it is out until: refused again with the same reset, or one within the hour
    /// of it, its limit is not the teammate's.
    pub(crate) fn back_on_its_account(
        &self,
        project: i64,
        owner: &ParticipantView,
        news: &News<'_>,
    ) -> Result<(), EngineError> {
        let Some(harness) = owner.harness.as_deref() else {
            return Ok(());
        };
        let one_account = self
            .seams
            .adapters
            .adapter(harness)
            .is_some_and(|adapter| adapter.one_account());
        if !one_account {
            return Ok(());
        }
        if let News::Turn {
            record, observed, ..
        } = news
        {
            let Some(at) = last_answer(observed) else {
                return Ok(());
            };
            let mut quota = record.quota.borrow_mut();
            if quota.answered.as_deref() == Some(at.as_str()) {
                return Ok(());
            }
            // One read of the project for each new answer of a one-account
            // window: a turn adds one for each call it makes, bounded by the looks.
            quota.answered = Some(at);
        }
        let because = format!("@{} has quota again on the same account", owner.handle);
        let mut brought = false;
        for other in self.known_project(project)?.participants {
            let marked = other.role != "human" && other.member_id.is_none();
            if !marked
                || other.id == owner.id
                || other.harness.as_deref() != Some(harness)
                || !self.is_out(&other)
            {
                continue;
            }
            let until = other.out_until.clone().unwrap_or_default();
            let back = match news {
                News::Human => true,
                News::Turn {
                    observed,
                    returned: Some(returned),
                    ..
                } => {
                    if later_limit(&until, returned) {
                        // A later reset is a limit of its own, which no teammate's turn
                        // undoes: the human hears it stays out, once for the limit.
                        if self.tried_anew(other.id, &until) {
                            self.tell_still_out(project, &other, owner, &until)?;
                        }
                        false
                    } else {
                        self.answered_since(&other, observed) && self.tried_anew(other.id, &until)
                    }
                }
                News::Turn {
                    observed,
                    returned: None,
                    ..
                } => self.answered_since(&other, observed) && self.tried_anew(other.id, &until),
            };
            if back {
                self.seams
                    .ledger
                    .borrow_mut()
                    .mark_back(other.id, &because)?;
                brought = true;
            }
        }
        if brought {
            self.changed();
        }
        Ok(())
    }

    /// Whether a window's turn is news for `participant`, out until `until`, and
    /// records it: none where it was tried already for the same limit.
    fn tried_anew(&self, participant: i64, until: &str) -> bool {
        let mut tried = self.scheduler.tried.borrow_mut();
        if tried
            .get(&participant)
            .is_some_and(|last| same_limit(until, last))
        {
            return false;
        }
        tried.insert(participant, until.to_owned());
        true
    }

    /// `other` stays out though `owner` came back on its account: its limit
    /// resets later, a limit of its own (a model's weekly one) or of the account
    /// the human left. The human hears it, as a `/login` in a member's window
    /// brings back that member alone.
    fn tell_still_out(
        &self,
        project: i64,
        other: &ParticipantView,
        owner: &ParticipantView,
        until: &str,
    ) -> Result<(), EngineError> {
        self.seams.ledger.borrow_mut().note(
            project,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: format!(
                    "@{0} is still out of quota until {until}, though @{1} is back on the same account: its limit resets later than @{1}'s. If its account has quota again (another account, a bigger plan), press Try again now on @{0}.",
                    other.handle, owner.handle
                ),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// A window out of quota with a message waiting for it, in its queue or carried
    /// by the launch that opened it while out: nothing goes into it
    /// before the reset unless its account has quota again, which ConsensFlow
    /// learns only from a window whose turn gets through or from the human's Try
    /// again now. So the human hears it, once for each time its participant is
    /// marked out, rather than finding what was sent to it queued for days.
    pub(crate) fn tell_what_waits(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        owner: &ParticipantView,
    ) -> Result<(), EngineError> {
        // What a window opened while out carries as its first message waits in it too.
        let carried = record
            .delivery
            .borrow()
            .delivering
            .as_ref()
            .map(|delivering| delivering.message);
        let waiting = match self.next_in_line(record)? {
            Some(next) => Some(next),
            None => carried.map_or(Ok(None), |id| self.seams.ledger.borrow().message(id))?,
        };
        let Some(waiting) = waiting else {
            return Ok(());
        };
        let until = owner.out_until.clone().unwrap_or_default();
        // An unavailable account is told once, whatever waits (`tell_unavailable`).
        if never(&until) {
            return Ok(());
        }
        let mark = (participant.id, owner.out_since.clone().unwrap_or_default());
        if !self.scheduler.out_noted.borrow_mut().insert(mark) {
            return Ok(());
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: "human".to_owned(),
                task: None,
                body: format!(
                    "@{} is out of quota until {until}, and m-{} ({}) waits for it, as does whatever is sent to it until then. If its account has quota again (another account, a bigger plan), press Try again now on @{}: what waits is typed in once it is back.",
                    participant.handle,
                    waiting.id,
                    what_it_is(&waiting.kind, waiting.task_number),
                    owner.handle
                ),
            },
        )?;
        self.changed();
        Ok(())
    }

    /// The work a window of a member out of quota until `until` holds, once
    /// its harness refused it: it waits with its window, which has its work,
    /// and goes on there by itself, whoever else of its tier is free; only the
    /// human or the chief gives it to another (dip-agentos, 2026-10-19). The
    /// chief keeps its own tasks. Its requester is told it waits, where a
    /// refusal at `refused_at` (ms) holds it (none: held with no note), unless
    /// the refusal answered the words that resumed it: that is the same wait
    /// going on, which a reset nobody named, or a limit that does not reset by
    /// itself, would tell again at every hour; the human is told once, then,
    /// that the account may need them (the review of the quota wait, T-88).
    pub(crate) fn hold_work(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        owner: &ParticipantView,
        until: &str,
        refused_at: Option<i64>,
        unavailable: Option<&str>,
    ) -> Result<(), EngineError> {
        if participant.role == "chief" {
            return Ok(());
        }
        for task in self.active_work(project, participant)? {
            let again = match refused_at {
                Some(at) if unavailable.is_none() => {
                    self.refused_its_resume(participant, &task, at)?
                }
                _ => false,
            };
            self.seams.ledger.borrow_mut().hold_task(
                project.id,
                task.number,
                until,
                if unavailable.is_some() {
                    "account unavailable"
                } else {
                    "out of quota"
                },
            )?;
            if refused_at.is_none() {
                continue;
            }
            if let Some(because) = unavailable {
                // The requester is told it waits for the human's word, whatever the
                // refusal answered: the human is told too, once (`tell_unavailable`).
                self.seams.ledger.borrow_mut().note_hold_unavailable(
                    project.id,
                    &task.requester,
                    task.number,
                    &participant.handle,
                    because,
                )?;
            } else if again {
                self.tell_the_human_it_may_need_them(project, participant, owner, &task)?;
            } else {
                self.seams.ledger.borrow_mut().note_hold(
                    project.id,
                    &task.requester,
                    task.number,
                    &participant.handle,
                    until,
                )?;
            }
        }
        Ok(())
    }

    /// Whether a refusal at `refused_at` (ms) answered the daemon's own words
    /// that resumed `task`: they are the last the window was given about it, and
    /// went in within `AGAIN_MS` of it. A task that worked for hours in the turn
    /// they began, and then runs out, waits anew (T-96).
    fn refused_its_resume(
        &self,
        participant: &ParticipantView,
        task: &TaskCard,
        refused_at: i64,
    ) -> Result<bool, EngineError> {
        let last = self
            .seams
            .ledger
            .borrow()
            .last_pasted(participant.id, task.id)?;
        Ok(last.is_some_and(|message| {
            message.sender.is_none()
                && message.body == format!("Resumed: {RESUME_WORDS}")
                && message
                    .delivered_at
                    .as_deref()
                    .and_then(time::parse)
                    .is_some_and(|delivered| refused_at - delivered <= AGAIN_MS)
        }))
    }

    /// A task the daemon resumed whose window got a turn through in it: the
    /// words that resumed it are the last the window was given about it, and
    /// its harness answered after them, with no failure and no refusal. Its
    /// requester, told it waits, is told once that it goes on: not when the
    /// words go in, which a limit that is still spent refuses at once (T-96).
    pub(crate) fn tell_it_went_on(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        record: &Record,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        // A refusal the window has not acted on yet is no turn through; one it
        // acted on before the words went in is the record's last word no more.
        if participant.role == "chief" || observed.failed || self.refused_unhandled(record) {
            return Ok(());
        }
        let task = self
            .seams
            .ledger
            .borrow()
            .active_task(participant.id, false)?;
        let Some(thread) = task else {
            return Ok(());
        };
        let task = &thread.task;
        let latest = self
            .seams
            .ledger
            .borrow()
            .last_pasted(participant.id, task.id)?;
        let Some(latest) = latest.filter(|message| {
            message.sender.is_none()
                && message
                    .body
                    .starts_with(&format!("Resumed: {RESUME_WORDS}"))
        }) else {
            return Ok(());
        };
        if record.quota.borrow().told_going_on == Some(latest.id) {
            return Ok(());
        }
        let items = observed.items();
        let Some(pasted) = self.delivered_at(participant.id, &thread, items)? else {
            return Ok(());
        };
        // Through, with no refusal since: its answer is complete, as a turn of the
        // window's own must be (`went_on_by_itself`), or it has worked past the
        // time in which a refusal is still the same wait (`AGAIN_MS`). A fragment a
        // second in is neither: a refusal a minute later would then be the same
        // wait, and the requester would have heard a false "goes on" (T-165).
        let Some(last) = items[pasted + 1..]
            .iter()
            .rev()
            .find(|item| item.role == Role::Assistant)
        else {
            return Ok(());
        };
        let working = latest
            .delivered_at
            .as_deref()
            .and_then(time::parse)
            .is_some_and(|delivered| self.now() - delivered > AGAIN_MS);
        if !last.complete && !working {
            return Ok(());
        }
        self.seams
            .ledger
            .borrow_mut()
            .tell_it_goes_on(project.id, task.number)?;
        record.quota.borrow_mut().told_going_on = Some(latest.id);
        self.changed();
        Ok(())
    }

    /// The human is told, once for a task, that the account of the member its
    /// window is refused on again as soon as it goes on may need them.
    fn tell_the_human_it_may_need_them(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        owner: &ParticipantView,
        task: &TaskCard,
    ) -> Result<(), EngineError> {
        let Some(human) = project.participants.iter().find(|p| p.role == "human") else {
            return Ok(());
        };
        let told = self
            .seams
            .ledger
            .borrow()
            .inbox(human.id, i64::MAX)?
            .iter()
            .any(|message| {
                message.kind == "note"
                    && message.task_number == Some(task.number)
                    && message.body.contains(MAY_NEED_YOU)
            });
        if told {
            return Ok(());
        }
        self.seams.ledger.borrow_mut().note(
            project.id,
            &NewNote {
                from: None,
                to: human.handle.clone(),
                task: Some(task.number),
                body: format!(
                    "T-{} was refused for quota again as soon as it went on: @{}'s account {MAY_NEED_YOU} (a plan that includes its model, a balance, or another account). It waits, and ConsensFlow tries it again at each reset; press Try again now on @{} once its account has quota.",
                    task.number, participant.handle, owner.handle
                ),
            },
        )?;
        Ok(())
    }

    /// A member window whose last turn its quota cut short, the member past
    /// it now: its task goes on in the window, as a held task does at its
    /// time, and is not taken for failed. A queued task is on its way to the
    /// window already: its delivery starts the next turn.
    pub(crate) fn go_on(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
    ) -> Result<(), EngineError> {
        let now = time::iso(self.now());
        for task in self.active_work(project, participant)? {
            if task.state == "queued" {
                continue;
            }
            self.seams.ledger.borrow_mut().hold_task(
                project.id,
                task.number,
                &now,
                "out of quota",
            )?;
        }
        Ok(())
    }

    /// A held task whose time has come goes on in its own window, unless its
    /// member is still out. One the ledger refuses to resume is its own
    /// trouble (`stay_paused`): the others, and the rest of the pass, go on.
    pub(crate) fn resume_held(&self) -> Result<(), EngineError> {
        let due = self
            .seams
            .ledger
            .borrow()
            .held_tasks_due(&time::iso(self.now()))?;
        for held in due {
            let project = self.seams.ledger.borrow().project(held.project_id)?;
            let Some(project) = project.filter(|project| project.state == "open") else {
                continue;
            };
            let owner = held
                .assignee_id
                .and_then(|id| project.participants.iter().find(|p| p.id == id))
                .map(|assignee| self.member_of(&project, assignee));
            if owner.is_some_and(|owner| self.is_out(&owner)) {
                continue;
            }
            let resumed = self.seams.ledger.borrow_mut().resume_task(
                held.project_id,
                held.number,
                None,
                RESUME_WORDS,
            );
            match resumed {
                Ok(_) => {}
                // A refusal is this task's; the ledger failing is the pass's.
                Err(LedgerError::Refused(refusal)) => self.stay_paused(&held, &refusal)?,
                Err(failed) => return Err(failed.into()),
            }
            self.changed();
        }
        Ok(())
    }

    /// A held task the ledger would not resume (its session was deleted while
    /// it was held, and a task given to a session is its own: only a
    /// follow-up brings the session back, and that is not the daemon's to
    /// do): it stays paused with its words, its hold cleared so it is not due
    /// again, and its requester is told once that it waits for a decision.
    fn stay_paused(&self, held: &HeldTask, refusal: &Refusal) -> Result<(), EngineError> {
        let thread = self
            .seams
            .ledger
            .borrow()
            .task(held.project_id, held.number)?;
        let Some(TaskThread { task, .. }) = thread else {
            return Ok(());
        };
        self.seams.ledger.borrow_mut().clear_hold(
            held.project_id,
            held.number,
            &refusal.message,
        )?;
        // A member given the task by name has no session to name: the refusal says it.
        let why = match task.session {
            Some(session) if refusal.code == "session-ended" => {
                format!("@{session}, the session it was given to, was deleted")
            }
            _ => format!(
                "it could not go on when its hold ended ({})",
                refusal.message
            ),
        };
        self.seams.ledger.borrow_mut().note(
            held.project_id,
            &NewNote {
                from: None,
                to: task.requester,
                task: Some(held.number),
                body: format!(
                    "T-{} stays paused: {why}. It waits for your decision: cancel it, or give the work again.",
                    held.number
                ),
            },
        )?;
        Ok(())
    }

    /// A member whose agent is gone from the human's agents runs on no
    /// default: its tiered work goes back to the board for another member,
    /// with whatever was on its way to it withdrawn, and a request given to
    /// it by name fails so the requester hears why.
    pub(crate) fn without_agent(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        delivering: Option<Delivering>,
    ) -> Result<(), EngineError> {
        let agent = participant.agent.as_deref().unwrap_or("null");
        let because = self.gone_because(agent);
        let tiered: Vec<TaskCard> = self
            .active_work(project, participant)?
            .into_iter()
            .filter(|task| task.pool.is_some())
            .collect();
        for task in &tiered {
            self.seams
                .ledger
                .borrow_mut()
                .release_task(project.id, task.number, &because)?;
        }
        match delivering {
            Some(delivering) => self.settle_failure(
                delivering,
                &format!(
                    "{because}: {}",
                    self.member_remedy(agent, &participant.handle)
                ),
                false,
            )?,
            None if !tiered.is_empty() => self.changed(),
            None => {}
        }
        Ok(())
    }

    /// A held task's window that went on with it by itself, in a turn of its own
    /// that got through: Claude Code continues once its limit resets, or the
    /// human typed into it (T-95). That turn begins with a user's words that are
    /// no paste of ConsensFlow's, after the latest words the window was given
    /// about the task, and an answer follows them; a failed turn, or one refused
    /// again, is none. The task goes on with no words, or with the daemon's bare
    /// words that resume it withdrawn (`go_on_in_window`, which tells its
    /// requester it goes on), and the turn is its turn: turn-end's `collect`
    /// takes its final answer, what the window wrote after the last user's words.
    pub(crate) fn went_on_by_itself(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
        observed: &Observed,
    ) -> Result<(), EngineError> {
        let refused = observed
            .quota
            .as_deref()
            .is_some_and(|quota| quota.level() == Level::Exhausted);
        if participant.role == "chief" || observed.failed || refused {
            return Ok(());
        }
        let Some(thread) = self.seams.ledger.borrow().task_in_hand(participant.id)? else {
            return Ok(());
        };
        let task = &thread.task;
        let held = task.state == "paused" && task.held_until.is_some();
        if !held && task.state != "queued" {
            return Ok(());
        }
        // Where the latest words the window was given about the task went in, as
        // turn-end finds them for the task's result (`delivered_at`).
        let items = observed.items();
        let Some(pasted) = self.delivered_at(participant.id, &thread, items)? else {
            return Ok(());
        };
        let after = &items[pasted + 1..];
        // Another paste of ConsensFlow's since; a compaction's summary that quotes a
        // header is none (T-532's P3-1).
        if after.iter().any(|item| {
            item.role == Role::User && !item.compaction && header_id(&item.text).is_some()
        }) {
            return Ok(());
        }
        // A compaction's summary, written in the user's place, is no turn of its own,
        // as it is none of the answer's (`answers_to_the_task`).
        let Some(own) = after
            .iter()
            .rposition(|item| item.role == Role::User && !item.compaction)
        else {
            return Ok(());
        };
        let answered = after[own + 1..]
            .iter()
            .any(|item| item.role == Role::Assistant && item.complete);
        if !answered {
            return Ok(());
        }
        match self
            .seams
            .ledger
            .borrow_mut()
            .go_on_in_window(project.id, task.number)
        {
            Ok(_) => {}
            // Words that carry more, or are on their way already, go in.
            Err(LedgerError::Refused(_)) => return Ok(()),
            Err(failed) => return Err(failed.into()),
        }
        self.changed();
        Ok(())
    }

    /// The work a participant holds and has not finished: queued, working or waiting.
    fn active_work(
        &self,
        project: &ProjectView,
        participant: &ParticipantView,
    ) -> Result<Vec<TaskCard>, EngineError> {
        let board = self.seams.ledger.borrow().board(project.id)?;
        Ok(board
            .lanes
            .into_iter()
            .find(|lane| lane.participant.id == participant.id)
            .map(|lane| lane.tasks)
            .unwrap_or_default()
            .into_iter()
            .filter(|task| matches!(task.state.as_str(), "queued" | "working" | "waiting"))
            .collect())
    }

    /// Why `agent` is gone, as a task taken back or a launch says it: a
    /// ready-made agent the catalog no longer carries says why it went; any
    /// other is no longer among the human's agents.
    pub(crate) fn gone_because(&self, agent: &str) -> String {
        match self.seams.roster.retired(agent) {
            Some(why) => format!("{agent} left the catalog: {why}"),
            None => format!("{agent} is no longer among your agents"),
        }
    }

    /// What the human does about a member whose agent is gone: picks another
    /// agent for one the catalog no longer carries, or adds theirs back.
    fn member_remedy(&self, agent: &str, handle: &str) -> String {
        if self.seams.roster.retired(agent).is_some() {
            format!("remove @{handle} from the staff and add another agent")
        } else {
            format!("add it back under Agents, or remove @{handle} from the staff")
        }
    }

    /// A participant whose saved agent the human has since deleted.
    pub(crate) fn agent_gone(&self, participant: &ParticipantView) -> bool {
        participant
            .agent
            .as_deref()
            .is_some_and(|name| matches!(self.saved_agent(name), Some(None)))
    }

    /// A saved agent as the roster has it now: its row, or none when it is
    /// gone. While the human's agents file cannot be read it is not known:
    /// nobody counts as free for new work, and nobody's agent as gone, so no
    /// work is taken back for a typo.
    fn saved_agent(&self, name: &str) -> Option<Option<SavedAgent>> {
        self.seams.roster.agent(name).ok()
    }

    /// A deleted project's open tasks wait for nobody now: what their
    /// requesters heard is forgotten.
    pub(crate) fn forget_project(&self, project: i64) {
        self.scheduler
            .waiting_noted
            .borrow_mut()
            .retain(|_, noted| *noted != project);
    }
}

/// Who of the free members takes a task: one it was taken from goes last;
/// then the harness whose members of this role and tier have taken the
/// fewest tasks, so a tier's work is shared across harnesses; then the
/// member with the fewest; then the earliest joined.
fn rank<'a>(mut free: Vec<&'a Candidate>, candidates: &[Candidate]) -> Vec<&'a Candidate> {
    let mut load: HashMap<Option<&str>, i64> = HashMap::new();
    for member in candidates {
        *load.entry(member.member.harness.as_deref()).or_default() += member.member.taken;
    }
    free.sort_by_key(|member| {
        (
            member.had_it,
            load.get(&member.member.harness.as_deref())
                .copied()
                .unwrap_or_default(),
            member.member.taken,
            member.member.id,
        )
    });
    free
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_waits_is_told_by_its_kind_and_task_with_the_article_it_takes() {
        assert_eq!(what_it_is("result", Some(47)), "the result of T-47");
        assert_eq!(what_it_is("task", Some(3)), "the words of T-3");
        assert_eq!(what_it_is("answer", Some(3)), "an answer on T-3");
        assert_eq!(what_it_is("question", Some(3)), "a question on T-3");
        assert_eq!(what_it_is("note", None), "a note");
    }
}
