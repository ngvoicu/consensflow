//! What a long run holds the whole time it runs: ten invariants, nine of them made
//! of the bugs the owner met in the field on 2026-10-15 (A1 to A5, B1, B2 and B5),
//! each a function of a [`Look`] and nothing else, so that a ledger and a record
//! written for it hold it as a running project does; and the tenth, that what a
//! disruption asks of ConsensFlow is done (`play`). A check never steers: it reads,
//! and what it finds is a [`Break`], with its evidence, which the [`Keeper`] records
//! once, at the look it was first found in, with what looks like a key or a token in
//! it hidden ([`super::mask`]).
//!
//! The invariants, by number:
//!
//! 1. No task is working whose window has neither run a turn nor held a background
//!    job for `--idle-min` minutes (A1). A task at work for long is no fault: there
//!    are no time limits on work, and a turn at work or a job held clears it. So do
//!    the background agents and workflows a Claude Code turn ended with, as its own
//!    last boundary counts them: 3.0.5 holds such a turn's task working until the
//!    turn their ends wake (Tyr's turn-end work, `wt/turn-end`,
//!    `crates/cf-engine/tests/dispatcher/pending_agents.rs`). The count is the last
//!    turn's alone: a turn after it that counts none, or says nothing of them, holds
//!    nothing.
//! 2. No task is done while a background job of its window still runs, unless it
//!    was finished (A2): a Claude Code job its harness will wake the worker for.
//! 3. An "ended" notice never comes after its result (A3). Which result is a
//!    notice's is the log's word alone (the moves of its task), never how near the two
//!    were written; one the log does not tie to a result is unresolved ([`Unjudged`]):
//!    said in the timeline, no break, and never taken to hold.
//! 4. A result is the last turn's final answer (A4): no progress line in it.
//! 5. No "turn was cut" note for a turn that was not cut (A5), as the window's own
//!    record says, an idle compaction being no turn.
//! 6. Every window the board shows open has its process, no window closes by itself
//!    mid-task, and no process of the run belongs to no window (B2, strangers). What
//!    the watch cannot hold to it breaks it too: a process whose identity was not
//!    taken while its window showed it, and a close of the app whose pane host's last
//!    word was not read to its end.
//! 7. No message is delivered twice, or joined to another (B5).
//! 8. No tool call comes back "User skipped this tool call", and no dialog waits
//!    unanswered for `--dialog-min` minutes (B1).
//! 9. The project reaches its end within the budget: every task in an end state and
//!    the chief's report to the owner, both before the budget is spent (`end`, at the
//!    end of the run).
//! 10. A disruption has its effect: the paused task pauses and goes on, the killed
//!     window's task pauses, the compaction is in the chief's record, the switched
//!     chief's window opens, the resumed project opens with its chief (`play`).
//!
//! The checks are in a module for each kind of thing they read: the work of the
//! tasks (`work`: 1, 2), the order and the words of what reaches the chief (`order`:
//! 3, 4), a turn's word against the record (`turns`: 5), the windows (`windows`: 6,
//! 8), the deliveries (`deliveries`: 7) and the end (`end`: 9).

mod deliveries;
mod end;
mod order;
mod turns;
mod windows;
mod work;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::look::Look;
use super::mask::{mask, screen, Names};

pub use end::end;
pub use order::{ENDED_NOTE, LET_GO_NOTE};

/// What each invariant is called in a report, and the field's shape it is made of.
pub const INVARIANTS: [(u8, &str, &str); 10] = [
    (1, "no task works on at rest", "A1"),
    (2, "no task done while its job runs", "A2"),
    (3, "an ended notice comes with or before its result", "A3"),
    (4, "a result is the last turn's final answer", "A4"),
    (5, "no false \"turn was cut\"", "A5"),
    (6, "every window has its process, and no stranger", "B2"),
    (7, "no message delivered twice or joined", "B5"),
    (8, "no tool call skipped, no dialog left waiting", "B1"),
    (9, "the project reaches its end within the budget", "end"),
    (10, "a disruption has its effect", "play"),
];

/// The number of the invariant a disruption's effect is held to.
pub const EFFECT: u8 = 10;

/// How long the watch lets things be before they count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// How long a window with a working task may rest, holding no job (1), in
    /// milliseconds.
    pub idle: i64,
    /// How long a dialog may wait in a window (8).
    pub dialog: i64,
    /// How long after a result the notification of its job's end may be written and
    /// still say the job had ended by then (2): a job that ends as the result is taken
    /// is told of a moment after.
    pub settle: i64,
    /// How old a result must be before it is judged against the record (2, 4): a
    /// record is copied a moment after it is written. As long as `settle` while the
    /// app runs; none once it has closed, and the records are all they will be.
    pub young: i64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            idle: 20 * 60_000,
            dialog: 5 * 60_000,
            settle: 60_000,
            young: 60_000,
        }
    }
}

/// What a break is shown with: the ledger's rows, a window's screen, and the
/// harness's record, each as lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Evidence {
    pub rows: Vec<String>,
    pub screen: Vec<String>,
    pub record: Vec<String>,
}

/// An invariant that did not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Break {
    pub invariant: u8,
    /// What makes it this break and no other, so that one that holds look after
    /// look is recorded once: `2:m-131`.
    pub key: String,
    /// What broke, in a sentence.
    pub said: String,
    pub evidence: Evidence,
    /// Whether it is recorded only once two looks in a row find it: what a moment
    /// of the windows shows (a window opening is not yet answered for).
    pub twice: bool,
    /// When it was first found, in milliseconds (the keeper's).
    pub at: i64,
    /// What kind of break of its invariant it is, where its check tells kinds apart:
    /// what a list of known breaks matches it by ([`super::known`]).
    pub shape: Option<&'static str>,
}

impl Break {
    /// A break of `invariant`, keyed `key`, that says `said`.
    pub fn new(invariant: u8, key: impl Into<String>, said: impl Into<String>) -> Self {
        Self {
            invariant,
            key: key.into(),
            said: said.into(),
            evidence: Evidence::default(),
            twice: false,
            at: 0,
            shape: None,
        }
    }

    /// The break, of the kind `shape`.
    #[must_use]
    pub fn with_shape(mut self, shape: &'static str) -> Self {
        self.shape = Some(shape);
        self
    }

    /// The break with `evidence`.
    #[must_use]
    pub fn with(mut self, evidence: Evidence) -> Self {
        self.evidence = evidence;
        self
    }

    /// The break, recorded only once two looks in a row find it.
    #[must_use]
    pub fn twice(mut self) -> Self {
        self.twice = true;
        self
    }

    /// What the invariant is called.
    pub fn name(&self) -> &'static str {
        INVARIANTS
            .iter()
            .find(|(number, ..)| *number == self.invariant)
            .map_or("?", |(_, name, _)| name)
    }

    /// The break as a report keeps it.
    pub fn to_value(&self) -> Value {
        json!({
            "invariant": self.invariant,
            "name": self.name(),
            "key": self.key,
            "shape": self.shape,
            "at": iso(self.at),
            "said": self.said,
            "evidence": {
                "rows": self.evidence.rows,
                "screen": self.evidence.screen,
                "record": self.evidence.record,
            },
        })
    }
}

/// What a check could not judge from what the run reads: no break, and never taken
/// for one that held. The [`Keeper`] says each once, in the timeline, and a run with
/// one does not pass: its verdict is `UNRESOLVED`, where nothing broke. One that waits
/// for evidence a later look may read (a gap) is judged again then ([`Keeper::fill`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unjudged {
    pub invariant: u8,
    /// What makes it this one, as a break's key does.
    pub key: String,
    pub said: String,
    /// Whether it counts only once two looks in a row find it: what a moment of the
    /// windows shows (a window closing as its tree is asked for).
    pub twice: bool,
    /// When it was first found, in milliseconds (the keeper's).
    pub at: i64,
    /// What a later look's evidence fills it with, where one may: a record read through
    /// the moment it needs.
    pub gap: Option<Gap>,
}

/// How a gap in what a check could not judge is judged again, on a later look.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gap {
    /// Every look judges the same moment again (a result's turn and the settle time after
    /// it, a cut note's): filled at the first look that no longer finds it unjudged, what
    /// that look's checks found standing as it is.
    Again,
    /// A task's rest, judged at a look's moment (the first): T-`number`, given to its
    /// window at `given`, judged again as of `at` ([`work::rest_again`]).
    Rest {
        number: i64,
        given: Option<i64>,
        at: i64,
    },
}

impl Unjudged {
    /// What the invariant `invariant` could not judge, keyed `key`, said `said`.
    pub fn new(invariant: u8, key: impl Into<String>, said: impl Into<String>) -> Self {
        Self {
            invariant,
            key: key.into(),
            said: said.into(),
            twice: false,
            at: 0,
            gap: None,
        }
    }

    /// What could not be judged, a gap that `gap` says how to fill.
    #[must_use]
    pub fn gap(mut self, gap: Gap) -> Self {
        self.gap = Some(gap);
        self
    }
}

/// A gap a later look filled: what could not be judged (as it was kept), when it was
/// filled, and whether it came to a break.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filled {
    pub open: Unjudged,
    pub at: i64,
    pub broke: bool,
}

/// Every invariant but the end's, against one look.
pub fn check(look: &Look, settings: &Settings) -> Vec<Break> {
    let mut found = work::idle(look, settings);
    found.extend(work::done_while_job(look, settings));
    found.extend(order::ended_after_result(look));
    found.extend(order::final_answer(look, settings));
    found.extend(turns::false_cut(look));
    found.extend(windows::processes(look));
    found.extend(deliveries::twice_or_joined(look));
    found.extend(windows::skipped_or_waiting(look, settings));
    found
}

/// What one look holds that the checks cannot judge, with `settings`: the tasks whose
/// Claude Code window the copy says rested too long, its own record not read through
/// the look (1); the results whose worker's own record was not read through their turn
/// (2); the ended notices the log ties to no result (3); the results whose turn the copy
/// of their worker's conversation does not hold (4); the cut notes to a Claude Code
/// window whose own record was not read through them (5); and what the watch could not
/// see of a window's tree, or of a task's state when its window closed by itself (6).
pub fn unjudged(look: &Look, settings: &Settings) -> Vec<Unjudged> {
    let mut found = work::idle_unread(look, settings);
    found.extend(work::unread_records(look, settings));
    found.extend(order::unbound_notices(look));
    found.extend(order::unanswered_results(look, settings));
    found.extend(turns::false_cut_unread(look));
    found.extend(windows::gaps(look));
    found.extend(windows::untold_exits(look));
    found
}

/// The breaks of a run, each recorded once, when it was first found; and what it
/// could not judge, each said once.
#[derive(Debug, Default)]
pub struct Keeper {
    recorded: BTreeSet<String>,
    /// What the last look found that waits for a second look.
    waiting: BTreeSet<String>,
    pub breaks: Vec<Break>,
    /// What the run could not judge, each when it was first found: what counts against
    /// it.
    pub unjudged: Vec<Unjudged>,
    /// The gaps a later look filled, kept for the report and no more counted.
    pub filled: Vec<Filled>,
    /// What the last look could not judge that waits for a second look.
    waiting_unjudged: BTreeSet<String>,
}

impl Keeper {
    /// Takes what a look found: the breaks it records now, which were not
    /// recorded before, each with the look's time and what looks like a key or a
    /// token in its words hidden (the look's own names shown), before it is kept or
    /// said.
    pub fn keep(&mut self, look: &Look, found: Vec<Break>) -> Vec<Break> {
        self.keep_at(look.at, found, &Names::of(look))
    }

    /// [`Keeper::keep`] of breaks found at `at`, the run's `names` shown.
    pub fn keep_at(&mut self, at: i64, found: Vec<Break>, names: &Names) -> Vec<Break> {
        let mut now = Vec::new();
        let mut still: BTreeMap<String, ()> = BTreeMap::new();
        for mut found in found {
            if self.recorded.contains(&found.key) {
                continue;
            }
            if found.twice && !self.waiting.contains(&found.key) {
                still.insert(found.key.clone(), ());
                continue;
            }
            found.at = at;
            found.said = mask(&found.said, names);
            for lines in [&mut found.evidence.rows, &mut found.evidence.record] {
                for line in lines.iter_mut() {
                    *line = mask(line, names);
                }
            }
            // A screen is one text, its wrapped rows joined.
            found.evidence.screen = screen(&found.evidence.screen, names);
            self.recorded.insert(found.key.clone());
            self.breaks.push(found.clone());
            now.push(found);
        }
        self.waiting = still.into_keys().collect();
        now
    }

    /// Fills each gap kept open that `look` fills (`found`, what it could not judge):
    /// one judged again ([`Gap`]) on what the look reads now. Filled, it no longer counts
    /// against the run, and is kept with when, and whether it came to a break, which is
    /// returned with it to be recorded. What waits for evidence no look reads stays open:
    /// a gap the run's end leaves open blocks it.
    pub fn fill(
        &mut self,
        look: &Look,
        settings: &Settings,
        found: &[Unjudged],
    ) -> Vec<(Filled, Option<Break>)> {
        let mut filled = Vec::new();
        let mut still = Vec::new();
        for open in std::mem::take(&mut self.unjudged) {
            let judged = match &open.gap {
                None => None,
                Some(Gap::Again) => {
                    (!found.iter().any(|other| other.key == open.key)).then_some(None)
                }
                Some(Gap::Rest { number, given, at }) => {
                    work::rest_again(look, settings, *number, *given, *at)
                }
            };
            match judged {
                Some(broke) => {
                    let one = Filled {
                        open,
                        at: look.at,
                        broke: broke.is_some(),
                    };
                    self.filled.push(one.clone());
                    filled.push((one, broke));
                }
                None => still.push(open),
            }
        }
        self.unjudged = still;
        filled
    }

    /// Whether a gap kept open waits for a record to be read: one more look may fill it.
    pub fn waits_for_a_reading(&self) -> bool {
        self.unjudged.iter().any(|open| open.gap.is_some())
    }

    /// Takes what a look at `at` could not judge: what was not said before, to be
    /// said now and kept, with the look's time and what looks like a key or a token in
    /// its words hidden (the run's `names` shown); what counts at two looks in a row
    /// waits for the second. Once kept it stays, but for a gap a later look fills
    /// ([`Keeper::fill`]).
    pub fn keep_unjudged(&mut self, at: i64, found: Vec<Unjudged>, names: &Names) -> Vec<Unjudged> {
        let mut now = Vec::new();
        let mut still = BTreeSet::new();
        for open in found {
            if self.unjudged.iter().any(|kept| kept.key == open.key) {
                continue;
            }
            if open.twice && !self.waiting_unjudged.contains(&open.key) {
                still.insert(open.key);
                continue;
            }
            let open = Unjudged {
                said: mask(&open.said, names),
                at,
                ..open
            };
            self.unjudged.push(open.clone());
            now.push(open);
        }
        self.waiting_unjudged = still;
        now
    }

    /// Whether the invariant `number` broke at all.
    pub fn broke(&self, number: u8) -> bool {
        self.breaks.iter().any(|found| found.invariant == number)
    }
}

/// `text` with its white space squeezed and its ends trimmed: what the checks
/// compare words by.
pub(super) fn plain(text: &str) -> String {
    crate::js::squeeze(text).trim().to_owned()
}

/// A time in milliseconds as the ledger writes one.
pub(super) fn iso(at: i64) -> String {
    jiff::Timestamp::from_millisecond(at).map_or_else(|_| at.to_string(), crate::clock::iso)
}
