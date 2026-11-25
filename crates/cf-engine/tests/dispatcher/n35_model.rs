//! D3's N35, proved by a model of schedules (T-2288). The rule the model holds the dispatcher
//! and the ledger to, in its own words and with its own record:
//!
//! - `turn_retry_at` is the last moment anything happened to a numbered retry of the task: one
//!   was charged, delivered or withdrawn.
//! - A failure with fewer than two numbered retries charged since the count started holds the
//!   task and charges a numbered retry when main's Resume of the hold is queued; a failure with
//!   two charged fails the task.
//! - The count starts again at zero on exactly three things: a person's or the chief's action on
//!   the task, a result, and a failure 30 minutes or more after `turn_retry_at`. (A task given to
//!   another window starts at zero there too: that is a fresh run.) A clock that reads earlier
//!   than `turn_retry_at` never renews. Nothing else resets the count, and nothing else writes
//!   that time.
//!
//! Every test below draws a schedule of steps from a seed (the seed is printed), runs it through
//! the real dispatcher and ledger, and keeps its own record of the same rule: when a numbered
//! retry was last charged, delivered or withdrawn, and how many were charged since the count
//! last started again. After every step the invariant is checked against the ledger as it is:
//! no more than two numbered retries between two resets, never a task failed while the rule
//! still allows a retry, and the stored count and time the model's own.
//!
//! `CF_N35_SEEDS` is how many schedules the one model test runs (2,000 by default),
//! `CF_N35_FIRST_SEED` where its seeds begin.

// A model run is told its seeds by the environment, and tells its seed and its schedule on stderr.
#![allow(clippy::print_stderr, clippy::disallowed_methods)]

use super::fixtures::{given, Tiers};
use super::n35_hold::{fail, next_turn_opens, resumes, retrying, working};
use cf_engine::testing::{Context, Restarted};
use cf_harness::records::Quota;
use cf_harness::seams::Time;

const RENEWAL_MS: i64 = 30 * 60 * 1000;
const STEPS: usize = 18;

/// splitmix64: the model's own random numbers, from a seed anyone can replay.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Where the task is, as the model knows it.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Phase {
    /// A turn is at work in the task's window.
    Working,
    /// A failed turn's task is held for its retry, due at this wall-clock time.
    Held {
        due: i64,
    },
    /// main's numbered Resume is queued for the busy window.
    Queued,
    /// The member's quota or a busy model holds the task until this time.
    Quota {
        until: i64,
        busy: bool,
    },
    /// A numbered retry's delivery was given up for good: the task is paused, held by nothing.
    Paused,
    Failed,
    Done,
}

#[derive(Debug, Clone, Copy)]
enum Step {
    Fail,
    DueQueued,
    DueDelivered,
    Deliver,
    Withdraw,
    /// The window cannot take the queued Resume at all: its delivery is given up.
    GiveUp,
    ChiefWord,
    PersonResume,
    Result,
    Reopen,
    QuotaStart(bool),
    QuotaEnd,
    Move,
    Restart,
    Forward,
    Back,
    /// The clock moves on to the failure's side of the 30 minutes after the last numbered retry:
    /// a few milliseconds either way of the boundary.
    Boundary,
    /// The wall clock is put back to 31 minutes before the last numbered retry (where no word
    /// was pasted since): the failure that follows reads a time earlier than the retry's.
    Behind,
    /// The clock moves on by this many milliseconds (both clocks).
    Wait(i64),
    /// The wall clock is put back by this many milliseconds.
    Rewind(i64),
}

struct Model<'a> {
    c: &'a Context,
    t: Tiers<'a>,
    window: String,
    restarted: Option<Restarted<'a>>,
    phase: Phase,
    /// The model's own record: numbered retries charged since the count last started again.
    count: i64,
    /// The model's own record: when a numbered retry was last charged, delivered or withdrawn.
    last: Option<i64>,
    /// The ledger's numbered Resumes (the facts that name a message) when the count last
    /// started again: the invariant counts from it.
    baseline: i64,
    /// A background job was left running by a withdrawal: the window's next turn is its wake.
    woke: bool,
    /// The window's turn is the plain one a Resume, a brief or a move opened: it can answer.
    fresh: bool,
    /// The latest time a member was out of quota until: a clock put back below it would hold the
    /// member out again, which is no business of the retry count.
    floor: i64,
    /// The wall-clock time of the last words pasted into the window: the engine reads a failed end
    /// as later than the paste that opened its turn, so a clock is never put back below it.
    pasted: i64,
    /// How many Resume texts the window has been given, quota's own included.
    given: usize,
    word: usize,
    log: Vec<String>,
    charged: u64,
    resets: u64,
    renewals: u64,
}

fn ms(text: &str) -> i64 {
    cf_base::time::parse(text).expect("a time")
}

impl<'a> Model<'a> {
    fn new(c: &'a Context) -> Self {
        let (t, window) = working(c);
        Self {
            c,
            t,
            window,
            restarted: None,
            phase: Phase::Working,
            count: 0,
            last: None,
            baseline: 0,
            woke: false,
            fresh: true,
            floor: 0,
            pasted: c.time.wall_ms(),
            given: 0,
            word: 0,
            log: Vec::new(),
            charged: 0,
            resets: 0,
            renewals: 0,
        }
    }

    fn now(&self) -> i64 {
        self.c.time.wall_ms()
    }

    fn pass(&self) {
        match &self.restarted {
            Some(restarted) => restarted.pass().unwrap(),
            None => self.c.pass().unwrap(),
        }
    }

    fn passes(&self, n: usize) {
        for _ in 0..n {
            self.pass();
        }
    }

    fn state(&self) -> String {
        self.t.task(1).task.state
    }

    fn stored_count(&self) -> i64 {
        self.c
            .ledger
            .borrow()
            .sqlite()
            .query_row("SELECT turn_retries FROM task WHERE number = 1", [], |r| {
                r.get(0)
            })
            .unwrap()
    }

    fn stored_time(&self) -> Option<String> {
        self.c
            .ledger
            .borrow()
            .sqlite()
            .query_row("SELECT turn_retry_at FROM task WHERE number = 1", [], |r| {
                r.get(0)
            })
            .unwrap()
    }

    /// The numbered Resumes the ledger has ever carried: facts that name their message.
    fn facts_with_message(&self) -> i64 {
        self.c
            .ledger
            .borrow()
            .sqlite()
            .query_row(
                "SELECT count(*) FROM turn_retry WHERE message_id IS NOT NULL",
                [],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// The count starts again: a reset of one of the three kinds (or a fresh window).
    fn reset(&mut self, why: &str) {
        self.count = 0;
        self.baseline = self.facts_with_message();
        self.resets += 1;
        self.log.push(format!("reset: {why}"));
    }

    /// A numbered retry was charged, delivered or withdrawn now.
    fn touched(&mut self) {
        self.last = Some(self.now());
    }

    fn enabled(&self) -> Vec<Step> {
        use Step::*;
        let mut steps = vec![Forward];
        if self.last.is_some() {
            steps.extend([Boundary, Boundary]);
        }
        if matches!(self.phase, Phase::Working | Phase::Held { .. }) {
            steps.push(Back);
            if self
                .last
                .is_some_and(|last| last - 31 * 60_000 > self.floor)
            {
                steps.extend([Behind, Behind]);
            }
        }
        match self.phase {
            Phase::Working => {
                steps.extend([
                    Fail,
                    Fail,
                    Fail,
                    ChiefWord,
                    QuotaStart(false),
                    QuotaStart(true),
                    Move,
                ]);
                if self.fresh && !self.woke {
                    steps.push(Result);
                }
            }
            Phase::Held { due } => {
                steps.extend([DueQueued, DueDelivered, DueDelivered]);
                // A pass that finds the hold due queues its Resume: a word or a Resume that
                // runs passes waits until the model has taken that step itself.
                if self.now() < due {
                    steps.extend([ChiefWord, PersonResume, Restart]);
                }
            }
            Phase::Queued => {
                steps.extend([Deliver, Withdraw, Withdraw]);
                if self.numbered().is_some_and(|(_, state)| state == "queued") {
                    steps.push(GiveUp);
                }
            }
            Phase::Paused => {
                steps.extend([PersonResume, PersonResume]);
            }
            Phase::Quota { .. } => steps.extend([QuotaEnd]),
            Phase::Failed | Phase::Done => steps.extend([Reopen, Reopen]),
        }
        steps
    }

    fn run(&mut self, step: Step, rng: &mut Rng) {
        self.log
            .push(format!("{step:?} at {} ({:?})", self.now(), self.phase));
        if !matches!(
            step,
            Step::Forward
                | Step::Back
                | Step::Behind
                | Step::Boundary
                | Step::Wait(_)
                | Step::Rewind(_)
        ) {
            self.fresh = matches!(
                step,
                Step::DueDelivered | Step::Deliver | Step::Reopen | Step::QuotaEnd | Step::Move
            );
            if self.fresh {
                self.woke = false;
            }
        }
        let pastes = matches!(
            step,
            Step::DueDelivered
                | Step::Deliver
                | Step::ChiefWord
                | Step::PersonResume
                | Step::Reopen
                | Step::QuotaEnd
                | Step::Move
        );
        match step {
            Step::Forward => {
                let by = [
                    1_000, 31_000, 121_000, 600_000, 1_799_000, 1_800_000, 1_801_000, 3_600_000,
                ][rng.below(8) as usize];
                self.c.advance(by);
            }
            Step::Boundary => {
                // The failure that follows waits a second first: it lands this far from the
                // 30 minutes after the last moment anything happened to a numbered retry.
                let d = [-1_001, -1_000, -999, -1, 0, 1, 1_000][rng.below(7) as usize];
                let last = self.last.expect("a numbered retry happened");
                let at = last + RENEWAL_MS + d - 1_000;
                if at > self.now() {
                    self.c.advance(at - self.now());
                }
            }
            Step::Behind => {
                let last = self.last.expect("a numbered retry happened");
                let to = last - 31 * 60_000;
                self.c.step_wall(to - self.now());
            }
            Step::Wait(by) => self.c.advance(by),
            Step::Rewind(by) => self.c.step_wall(-by),
            Step::Back => {
                // From a millisecond to an hour, below the last paste too.
                let by = [
                    1, 999, 1_000, 1_001, 5_000, 60_000, 300_000, 1_800_000, 1_860_000, 2_700_000,
                    3_600_000,
                ][rng.below(11) as usize];
                if self.now() - by > self.floor {
                    self.c.step_wall(-by);
                }
            }
            Step::Restart => self.restart(),
            Step::Fail => self.fail(),
            Step::DueQueued => self.due(false),
            Step::DueDelivered => self.due(true),
            Step::Deliver => self.deliver(),
            Step::Withdraw => self.withdraw(),
            Step::GiveUp => self.give_up(),
            Step::ChiefWord => self.chief_word(),
            Step::PersonResume => self.person_resume(),
            Step::Result => self.result(),
            Step::Reopen => self.reopen(),
            Step::QuotaStart(busy) => self.quota_start(busy),
            Step::QuotaEnd => self.quota_end(),
            Step::Move => self.move_window(),
        }
        if pastes {
            self.pasted = self.pasted.max(self.now());
        }
    }

    fn restart(&mut self) {
        let c = self.c;
        c.edit_ledger(|_| {});
        c.ledger.borrow_mut().suspend_for_restart().unwrap();
        let restarted = c.make();
        restarted.settle_after_restart().unwrap();
        restarted.resume_project(self.t.project.id).unwrap();
        self.restarted = Some(restarted);
    }

    /// The turn at work fails. The rule decides what the ledger must do.
    fn fail(&mut self) {
        let c = self.c;
        let window = self.window.clone();
        // A failed end is later than the paste that opened its turn.
        c.advance(1_000);
        if self.woke {
            c.adapter.wake(
                &window,
                "<task-notification>The last background step finished.</task-notification>",
            );
            self.woke = false;
        }
        fail(c, &window);
        self.passes(3);
        let now = self.now();
        // The renewal by time: 30 minutes or more after the last moment anything happened to a
        // numbered retry. A clock reading earlier than that moment never renews.
        if self.count > 0 && self.last.is_some_and(|last| now - last >= RENEWAL_MS) {
            self.renewals += 1;
            self.reset("a failure 30 minutes after the last numbered retry");
        }
        let thread = self.t.task(1);
        if std::env::var("CF_N35_DEBUG").is_ok() {
            for m in &thread.messages {
                eprintln!(
                    "  m-{} {} {} from {:?}: {}",
                    m.id,
                    m.kind,
                    m.state,
                    m.sender,
                    m.body.chars().take(60).collect::<String>()
                );
            }
        }
        let task = thread.task;
        if self.count < 2 {
            assert_eq!(
                task.state, "paused",
                "a task is never failed while the rule allows a retry (count {})",
                self.count
            );
            let retry = task.retry.expect("held for its retry");
            assert_eq!(retry.which, self.count + 1, "the try the hold is for");
            let due = ms(task.held_until.as_deref().expect("a time"));
            self.phase = Phase::Held { due };
        } else {
            assert_eq!(
                task.state, "failed",
                "two numbered retries are charged: the next failure fails the task"
            );
            self.phase = Phase::Failed;
        }
    }

    fn due_time(&self) -> i64 {
        match self.phase {
            Phase::Held { due } => due,
            other => panic!("not held: {other:?}"),
        }
    }

    /// The numbered Resume the ledger has carried last, and where it is now.
    fn numbered(&self) -> Option<(i64, String)> {
        let id: Option<i64> = self
            .c
            .ledger
            .borrow()
            .sqlite()
            .query_row(
                "SELECT message_id FROM turn_retry WHERE message_id IS NOT NULL ORDER BY id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )
            .ok();
        id.map(|id| (id, self.c.message(id).state))
    }

    /// What became of the numbered Resume in the pass that just ran: delivered to the window,
    /// withdrawn, or still waiting. The model records the moment of what it sees.
    fn observe(&mut self) {
        let c = self.c;
        let window = self.window.clone();
        let (_, state) = self.numbered().expect("a numbered Resume");
        match state.as_str() {
            "queued" | "delivering" => self.phase = Phase::Queued,
            "cancelled" | "failed" => {
                self.touched();
                self.woke = true;
                self.phase = Phase::Working;
            }
            _ => {
                self.touched();
                self.pasted = self.pasted.max(self.now());
                next_turn_opens(c, &window);
                self.passes(2);
                self.phase = Phase::Working;
            }
        }
        self.given = resumes(c, "zeus");
    }

    /// The hold's time comes and main's Resume of it is queued: a numbered retry is charged.
    fn due(&mut self, deliver: bool) {
        let c = self.c;
        let window = self.window.clone();
        let now = self.now();
        let due = self.due_time();
        if due >= now {
            c.advance(due - now + 1);
        }
        if deliver {
            let before = resumes(c, "zeus");
            for _ in 0..8 {
                self.pass();
                if resumes(c, "zeus") > before {
                    break;
                }
            }
            next_turn_opens(c, &window);
            self.passes(2);
        } else {
            c.adapter.with(&window, |agent| agent.settled = false);
            self.passes(2);
        }
        self.count += 1;
        self.charged += 1;
        self.touched();
        self.observe();
    }

    /// The window takes the queued Resume.
    fn deliver(&mut self) {
        let c = self.c;
        let window = self.window.clone();
        let before = resumes(c, "zeus");
        c.adapter.with(&window, |agent| agent.settled = true);
        for _ in 0..8 {
            self.pass();
            if resumes(c, "zeus") > before {
                break;
            }
        }
        next_turn_opens(c, &window);
        self.passes(2);
        self.observe();
    }

    /// The window went on by itself with a job still running: the queued Resume is withdrawn,
    /// if the window is at the place where that happens.
    fn withdraw(&mut self) {
        let c = self.c;
        let window = self.window.clone();
        c.adapter.wake(
            &window,
            "<task-notification>One background step finished.</task-notification>",
        );
        c.adapter.with(&window, |agent| {
            agent.failed = false;
            agent.failure = None;
        });
        c.adapter
            .answer_leaving_a_job(&window, "The other background step is still running.");
        self.passes(3);
        self.observe();
    }

    /// The window cannot take the queued Resume: its delivery is begun and given up for good, the
    /// message fails and the task is paused, held by nothing.
    fn give_up(&mut self) {
        let c = self.c;
        let (id, _) = self.numbered().expect("a numbered Resume");
        c.advance(1_000);
        c.ledger.borrow_mut().begin_delivery(id).unwrap();
        let (message, decision) = c
            .ledger
            .borrow_mut()
            .give_up_delivery(id, "the stand-in could not accept the words")
            .unwrap();
        assert_eq!(message.state, "failed");
        assert_eq!(decision, cf_ledger::GivenUp::Paused);
        let window = self.window.clone();
        c.adapter.with(&window, |agent| agent.settled = true);
        self.touched();
        self.phase = Phase::Paused;
    }

    /// The chief's word to the task's window: an action on the task.
    fn chief_word(&mut self) {
        let c = self.c;
        self.word += 1;
        c.ledger
            .borrow_mut()
            .tell_queued(
                self.t.project.id,
                1,
                "chief",
                &format!("Use file number {}.", self.word),
            )
            .unwrap();
        self.reset("the chief's word");
        if matches!(self.phase, Phase::Held { .. }) {
            // A held task's window takes the word with the Resume, or when it is next looked at.
            return;
        }
        // The window takes the word before the schedule goes on.
        let said = format!("Use file number {}.", self.word);
        for _ in 0..8 {
            self.pass();
            if given(c, "zeus").iter().any(|text| text.contains(&said)) {
                break;
            }
        }
        let window = self.window.clone();
        next_turn_opens(c, &window);
        self.passes(2);
    }

    /// A person's (here the chief's) Resume of the held task.
    fn person_resume(&mut self) {
        let c = self.c;
        let window = self.window.clone();
        c.resume_task(self.t.project.id, 1, "go on with the plan");
        for _ in 0..8 {
            self.pass();
            if given(c, "zeus")
                .iter()
                .any(|text| text.contains("go on with the plan"))
            {
                break;
            }
        }
        next_turn_opens(c, &window);
        self.passes(2);
        self.reset("a person's Resume");
        self.phase = Phase::Working;
    }

    /// The window's turn ends with its answer: the task's result.
    fn result(&mut self) {
        let c = self.c;
        if self.woke {
            return;
        }
        c.advance(1_000);
        c.adapter
            .answer("zeus", "Receipt 42, the requested action is complete.");
        self.passes(4);
        assert_eq!(
            self.state(),
            "done",
            "the turn's answer is the task's result"
        );
        self.reset("a result");
        self.phase = Phase::Done;
    }

    /// A person reopens a failed or finished task.
    fn reopen(&mut self) {
        let c = self.c;
        let window = self.window.clone();
        c.ledger
            .borrow_mut()
            .reopen_task(self.t.project.id, 1, "chief", "Handle one more case.")
            .unwrap();
        for _ in 0..8 {
            self.pass();
            if given(c, "zeus")
                .iter()
                .any(|text| text.contains("Handle one more case."))
            {
                break;
            }
        }
        next_turn_opens(c, &window);
        self.passes(2);
        self.reset("a reopen");
        self.phase = Phase::Working;
    }

    /// The member's quota (or a busy model) holds the task for a short time.
    fn quota_start(&mut self, busy: bool) {
        if self.woke {
            return;
        }
        let c = self.c;
        c.advance(1_000);
        let at = Some(super::fixtures::now(c));
        let refusal = if busy {
            Quota::Busy {
                at,
                because: "Selected model is at capacity. Please try a different model.".into(),
            }
        } else {
            Quota::Exhausted {
                at,
                resets_at: Some(super::fixtures::after(c, 2 * 60_000)),
            }
        };
        c.adapter.refuse(&self.window, refusal);
        self.passes(3);
        let task = self.t.task(1).task;
        assert_eq!(task.state, "paused", "held to the reset");
        assert!(task.retry.is_none(), "a quota's hold is no retry");
        let until = ms(task.held_until.as_deref().expect("a time"));
        self.floor = self.floor.max(until);
        self.phase = Phase::Quota { until, busy };
    }

    /// The hold's end: main's own Resume, which is neither a numbered retry nor a reset.
    fn quota_end(&mut self) {
        let c = self.c;
        let window = self.window.clone();
        let Phase::Quota { until, .. } = self.phase else {
            return;
        };
        let now = self.now();
        if until >= now {
            c.advance(until - now + 1);
        }
        let before = resumes(c, "zeus");
        for _ in 0..8 {
            self.pass();
            if resumes(c, "zeus") > before {
                break;
            }
        }
        assert!(
            resumes(c, "zeus") > before,
            "the quota's own Resume reached the window"
        );
        c.adapter.with(&window, |agent| agent.quota = None);
        next_turn_opens(c, &window);
        self.passes(2);
        self.phase = Phase::Working;
        self.given = resumes(c, "zeus");
    }

    /// A person gives the task to another window: a fresh run with a count of its own.
    fn move_window(&mut self) {
        if self.woke {
            return;
        }
        let c = self.c;
        c.reassign_task(self.t.project.id, 1).expect("reassigned");
        self.passes(8);
        let next = self.t.assignee(1);
        assert_ne!(next, self.window, "another window");
        self.window = next;
        next_turn_opens(c, &self.window);
        self.passes(2);
        self.reset("the task moved to another window");
        self.phase = Phase::Working;
    }

    /// What must hold after every step.
    fn check(&self) {
        assert_eq!(
            self.stored_count(),
            self.count,
            "the stored count is the model's: {:#?}",
            self.log
        );
        let stored = self.stored_time();
        assert_eq!(
            stored.as_deref().map(ms),
            self.last,
            "the stored time is the last moment a numbered retry was charged, delivered or \
             withdrawn: {stored:?} {:#?}",
            self.log
        );
        // A task fails exactly where the model says its two numbered retries are charged and the
        // next failure came: never at any other step, whatever the step.
        let state = self.state();
        assert_eq!(
            state == "failed",
            self.phase == Phase::Failed,
            "the task's state is {state}, the model's phase {:?}: {:#?}",
            self.phase,
            self.log
        );
        let in_a_row = self.facts_with_message() - self.baseline;
        assert!(
            in_a_row <= 2,
            "{in_a_row} numbered retries charged between two resets: {:#?}",
            self.log
        );
    }
}

impl Drop for Model<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            eprintln!("the schedule so far:\n{}", self.log.join("\n"));
        }
    }
}

/// One schedule: the steps its seed draws.
fn schedule(seed: u64) -> (u64, u64, u64, u64) {
    let c = retrying();
    let mut model = Model::new(&c);
    let mut rng = Rng(seed);
    for _ in 0..STEPS {
        let steps = model.enabled();
        let step = steps[rng.below(steps.len() as u64) as usize];
        model.run(step, &mut rng);
        model.check();
    }
    (model.charged, model.resets, model.renewals, STEPS as u64)
}

fn seeds() -> (u64, u64) {
    let count = std::env::var("CF_N35_SEEDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(2_000);
    let first = std::env::var("CF_N35_FIRST_SEED")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(1);
    (first, count)
}

#[test]
fn no_schedule_of_steps_charges_a_third_numbered_retry_or_fails_a_task_early() {
    let (first, count) = seeds();
    eprintln!("n35 model: seeds {first}..{}", first + count);
    let (mut charged, mut resets, mut renewals, mut steps) = (0, 0, 0, 0);
    for seed in first..first + count {
        let outcome = std::panic::catch_unwind(|| schedule(seed));
        match outcome {
            Ok((a, b, r, s)) => {
                charged += a;
                resets += b;
                renewals += r;
                steps += s;
            }
            Err(cause) => {
                eprintln!("n35 model: SEED {seed} failed");
                std::panic::resume_unwind(cause);
            }
        }
    }
    eprintln!(
        "n35 model: {count} schedules, {steps} steps, {charged} numbered retries charged, {resets} resets ({renewals} by time)"
    );
}

// The five checks' schedules, each by its number, run through the model's own steps and the same
// check after every one.

use Step::*;

const MIN: i64 = 60_000;

fn script(steps: &[Step]) -> (Context, Phase, i64) {
    let c = retrying();
    let (phase, count) = {
        let mut model = Model::new(&c);
        let mut rng = Rng(0);
        for step in steps {
            model.run(*step, &mut rng);
            model.check();
        }
        (model.phase, model.count)
    };
    (c, phase, count)
}

fn held(phase: Phase) -> bool {
    matches!(phase, Phase::Held { .. })
}

#[test]
fn t_2186_three_failed_turns_in_a_row_charge_two_numbered_retries_and_fail_the_task() {
    // Its native-reader replays end in three failed turns after two Resumes; the model's steps
    // are the same schedule.
    let (_c, phase, count) = script(&[Fail, DueDelivered, Fail, DueDelivered, Fail]);
    assert_eq!((phase, count), (Phase::Failed, 2));
}

#[test]
fn t_2189_a_quota_or_busy_hold_after_two_numbered_retries_leaves_the_next_failure_to_fail_the_task()
{
    for busy in [false, true] {
        let (_c, phase, count) = script(&[
            Fail,
            DueDelivered,
            Fail,
            DueDelivered,
            QuotaStart(busy),
            QuotaEnd,
            Fail,
        ]);
        assert_eq!((phase, count), (Phase::Failed, 2), "busy {busy}");
    }
}

#[test]
fn t_2189_a_wall_clock_put_back_five_minutes_after_the_first_retry_gives_no_third() {
    let (_c, phase, count) = script(&[
        Fail,
        DueDelivered,
        Rewind(5 * MIN),
        Fail,
        DueDelivered,
        Fail,
    ]);
    assert_eq!((phase, count), (Phase::Failed, 2));
}

#[test]
fn t_2246_finding_4_a_withdrawn_second_retry_hides_nothing_and_renews_30_minutes_after_it() {
    // The 30 minutes run from the withdrawal, the last moment anything happened to a numbered
    // retry: a failure a millisecond short of them fails the task, at them it begins again.
    for (after, renewed) in [(30 * MIN - 1, false), (30 * MIN, true)] {
        let (_c, phase, count) = script(&[
            Fail,
            DueDelivered,
            Fail,
            DueQueued,
            Withdraw,
            Wait(after - 1_000),
            Fail,
        ]);
        // (the model's Fail waits a second first)
        assert_eq!(held(phase), renewed, "{after} ms after the withdrawal");
        assert_eq!(count, if renewed { 0 } else { 2 });
    }
}

#[test]
fn t_2246_finding_5_a_quota_between_two_failures_changes_nobody_s_count() {
    let (_c, phase, count) = script(&[Fail, DueDelivered, QuotaStart(false), QuotaEnd, Fail]);
    assert_eq!((held(phase), count), (true, 1), "the second of two");
}

#[test]
fn t_2246_finding_6_a_word_in_a_hold_begins_the_count_and_the_next_charge_is_the_first() {
    let (_c, phase, count) = script(&[Fail, DueDelivered, Fail, ChiefWord, DueDelivered, Fail]);
    assert_eq!(
        (held(phase), count),
        (true, 1),
        "the second try of a fresh pair"
    );
}

#[test]
fn t_2262_finding_8_one_delivery_renews_the_count_once() {
    // A failure 30 minutes after the delivery renews the count; the fresh pair's first retry was
    // queued and withdrawn; the next failures must find two numbered retries, no more. (The
    // withdrawal is the last moment anything happened to a numbered retry: a later failure is
    // judged from it, never from the delivery before.)
    let (_c, phase, count) = script(&[
        Fail,
        DueDelivered,
        Wait(30 * MIN - 1_000),
        Fail,
        DueQueued,
        Withdraw,
        Fail,
        DueDelivered,
        Fail,
    ]);
    assert_eq!(
        (phase, count),
        (Phase::Failed, 2),
        "two in the fresh pair, no third"
    );
}

#[test]
fn t_2280_finding_9_a_first_turn_failing_after_a_word_with_a_count_of_zero_stamps_its_charge() {
    // Two retries, the chief's word, a first turn that fails 31 minutes later at a count of
    // zero: its charge writes the time, so the withdrawn retry and the next failure within
    // seconds find a count of 1, never a renewed 0.
    let (_c, phase, count) = script(&[
        Fail,
        DueDelivered,
        Fail,
        DueDelivered,
        ChiefWord,
        Wait(31 * MIN),
        Fail,
        DueQueued,
        Withdraw,
        Fail,
        DueDelivered,
        Fail,
    ]);
    assert_eq!(
        (phase, count),
        (Phase::Failed, 2),
        "two charged since the word, no third"
    );
}

#[test]
fn a_clock_read_well_before_the_last_numbered_retry_never_renews_the_count() {
    // The failure's clock reads 31 minutes before the retry that was withdrawn: no renewal,
    // the count of 1 stands and the next hold is for the second retry.
    let (_c, phase, count) = script(&[
        Fail,
        Wait(40 * MIN),
        DueQueued,
        Withdraw,
        Rewind(31 * MIN),
        Fail,
    ]);
    assert_eq!((held(phase), count), (true, 1));
}

#[test]
fn t_2305_finding_11_a_failed_turn_is_seen_after_the_clock_steps_back_a_millisecond_past_a_retry_s_confirmation(
) {
    // A retry's delivery was confirmed at 31.001; after a restart the clock is corrected back
    // 1,001 ms and a person's Resume is confirmed at 31.000. The last paste is the one the
    // ledger confirmed last, never the one a clock dates last: its failure is seen and held.
    let (_c, phase, count) = script(&[
        Fail,
        DueDelivered,
        Fail,
        Restart,
        Rewind(1_001),
        PersonResume,
        Fail,
    ]);
    assert_eq!((held(phase), count), (true, 0));
}

#[test]
fn t_2305_finding_11_the_same_after_a_conversation_switch_with_no_restart() {
    let c = retrying();
    c.adapter.resumes_records.set(true);
    c.adapter.rereads_failures.set(true);
    let mut model = Model::new(&c);
    let mut rng = Rng(0);
    for step in [Fail, DueDelivered, Fail] {
        model.run(step, &mut rng);
        model.check();
    }
    c.adapter
        .switch_to(&model.window, "a-second-native-conversation");
    model.passes(2);
    for step in [Rewind(1_001), PersonResume, Fail] {
        model.run(step, &mut rng);
        model.check();
    }
    assert_eq!((held(model.phase), model.count), (true, 0));
}

#[test]
fn t_2305_finding_13_a_delivery_given_up_for_good_dates_the_withdrawal() {
    let (_c, phase, count) = script(&[Fail, DueQueued, GiveUp, PersonResume, Fail]);
    assert_eq!((held(phase), count), (true, 0));
}

#[test]
fn t_2305_finding_12_a_delivered_retry_cancelled_at_its_carrier_s_take_back_dates_it() {
    let c = retrying();
    let mut m = Model::new(&c);
    let mut rng = Rng(0);
    for step in [Fail, DueDelivered] {
        m.run(step, &mut rng);
        m.check();
    }
    let (id, _) = m.numbered().unwrap();
    let project = m.t.project.id;
    c.advance(1_300);
    c.ledger
        .borrow_mut()
        .pause_task(project, 1, Some("chief"), Some("hold the window"))
        .unwrap();
    let carrier = c
        .ledger
        .borrow_mut()
        .resume_task(
            project,
            1,
            Some("chief"),
            "Continue with the revised brief.",
        )
        .unwrap()
        .message
        .unwrap()
        .id;
    c.ledger
        .borrow_mut()
        .take_back(id, "the first paste was taken back")
        .unwrap();
    let begun = c.ledger.borrow_mut().begin_delivery(carrier).unwrap();
    let ids: Vec<i64> = begun.carried.into_iter().map(|m| m.id).collect();
    assert!(ids.contains(&id));
    c.ledger.borrow_mut().admit(carrier, &ids).unwrap();
    c.advance(3_210);
    c.ledger
        .borrow_mut()
        .cancel_message(id, "withdrawn after handover")
        .unwrap();
    c.advance(5_432);
    c.ledger
        .borrow_mut()
        .confirm_delivery(carrier, None)
        .unwrap();
    assert_eq!(c.message(id).state, "delivered");
    c.advance(1_700_000);
    c.ledger
        .borrow_mut()
        .take_back(carrier, "its window removed the carrier")
        .unwrap();
    assert_eq!(c.message(id).state, "cancelled");
    assert_eq!(
        m.stored_time().as_deref().map(ms),
        Some(m.now()),
        "the final withdrawal is the last moment, not the delivery's"
    );
}
