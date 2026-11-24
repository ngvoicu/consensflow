//! The window of the model, its actors and its oracle: the real host and the real readers, the
//! program and the pty as stand-ins, the engine as a pass that takes a look, asks the licences and
//! acts, and every rule held after every step.

use std::collections::VecDeque;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cf_harness::{
    after_wait, licence_enter, licence_paste, AfterWait, At as Named, BoxState, EnterFor,
    EnterLicence, Look, NoLook, PasteLicence,
};
use cf_proto::agents::Harness;
use cf_proto::panes::BOTTOM_LINES;

use super::clock::ModelClock;
use super::program::{Echo, Program, Pty, Written};
use super::sched;
use crate::arbiter::{ArbiterError, At, EnterTiming, InputArbiter, OutputClock, OutputCount, Paste};
use crate::pane_handlers::{snapshot_of, took_in};
use crate::pty::{ActivityReader, Gate, Gated, PaneError, PaneInputWriter, PaneKey};
use crate::screen::{PaneScreen, Seen};

/// The host's wait of a delivery's Enter, shorter than the product's (2 s) so that a schedule
/// ends: its cap and grace are the arbiter's.
pub(crate) const ENTER_TIMING: EnterTiming = EnterTiming {
    least_ms: 10,
    drawn_after_ms: 0,
    quiet_ms: 120,
    most_ms: 400,
};

/// The engine's clocks, as the dispatcher's: how long before an Enter is pressed again for a
/// paste its record never showed, how long a whole wait is before it may be pasted again, and
/// how often it looks.
const ENTER_AGAIN: Duration = Duration::from_secs(10);
const ARRIVAL: Duration = Duration::from_secs(60);
pub(crate) const PASS_EVERY: Duration = Duration::from_secs(3);

/// What an Enter written into the window is, for the oracle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EnterKind {
    /// The paste's own, which waits for the output the host read, up to its cap and grace.
    First,
    /// `pane.enter`, never written while output is pending.
    Pane,
}

/// The pane as the host's writer sees it.
pub(crate) struct ModelPane {
    writer: Mutex<Box<dyn std::io::Write + Send>>,
    screen: Arc<PaneScreen>,
    clock: Arc<ModelClock>,
    printed: Arc<OutputClock>,
    kind: Mutex<EnterKind>,
    pasted_at: Mutex<Option<Duration>>,
    violations: Arc<Mutex<Vec<String>>>,
}

impl PaneInputWriter for ModelPane {
    fn write(&self, _key: &PaneKey, bytes: &[u8]) -> Result<(), PaneError> {
        match self.write_gated(_key, bytes, &mut || Gate::Go) {
            Ok(()) => Ok(()),
            Err(Gated::Pane(error)) => Err(error),
            Err(Gated::Refused(_)) => unreachable!("a gate that always goes refuses nothing"),
        }
    }

    fn write_gated(
        &self,
        _key: &PaneKey,
        bytes: &[u8],
        gate: &mut dyn FnMut() -> Gate,
    ) -> Result<(), Gated> {
        let enter = bytes == b"\r";
        // Whether output was pending where the last check let the write go: read in the same
        // turn as the check, with no point between.
        let pending = std::cell::Cell::new(false);
        let mut checked = || {
            let gated = gate();
            if enter && matches!(gated, Gate::Go) {
                pending.set(sched::without_points(|| self.printed.counts().pending()));
            }
            gated
        };
        let written = crate::pty::write_gated_in(&self.writer, bytes, &mut checked, &|rest| {
            crate::arbiter::Clock::rest(&*self.clock, rest)
        });
        if written.is_ok() {
            let now = self.clock.elapsed();
            if enter && pending.get() {
                let kind = *self.kind.lock().unwrap();
                let since = now.saturating_sub(self.pasted_at.lock().unwrap().unwrap_or(now));
                let cap = Duration::from_millis(ENTER_TIMING.most_ms + 3_000);
                if kind == EnterKind::Pane || since < cap {
                    self.violations.lock().unwrap().push(format!(
                        "an Enter ({kind:?}) was written at {now:?} with output pending, \
                         {since:?} after its paste"
                    ));
                }
            }
            if bytes.windows(6).any(|window| window == b"\x1b[200~") {
                *self.pasted_at.lock().unwrap() = Some(now);
            }
        }
        written
    }

    fn with_seen(&self, _key: &PaneKey, under: &mut dyn FnMut(Option<Seen>)) {
        self.screen.with_seen(&mut |seen| under(Some(seen)));
    }
}

/// What the worker of the pane does next, in order.
pub(crate) enum Job {
    /// The human's keys.
    Typed(Vec<u8>),
    Paste {
        id: u32,
        at: Option<At>,
        read_empty: Option<u64>,
        clear_first: Option<&'static [u8]>,
        reply: Reply,
    },
    Enter {
        at: At,
        reply: Reply,
    },
}

pub(crate) type Reply = Arc<Mutex<Option<Result<(), ArbiterError>>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Queued,
    /// Pasted, at this moment of the model's time.
    Pasted(Duration),
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub(crate) struct Msg {
    pub(crate) id: u32,
    pub(crate) state: State,
    pub(crate) attempts: u32,
}

pub(crate) struct World {
    pub(crate) harness: Harness,
    pub(crate) clock: Arc<ModelClock>,
    pub(crate) arbiter: Arc<InputArbiter>,
    pub(crate) key: PaneKey,
    pub(crate) printed: Arc<OutputClock>,
    pub(crate) screen: Arc<PaneScreen>,
    pub(crate) pty: Arc<Pty>,
    pub(crate) program: Arc<Mutex<Program>>,
    pub(crate) pane: Arc<ModelPane>,
    pub(crate) jobs: Mutex<VecDeque<Job>>,
    /// Reads counted and not yet fed into the screen.
    fed: Mutex<VecDeque<Vec<u8>>>,
    pub(crate) msgs: Mutex<Vec<Msg>>,
    pub(crate) violations: Arc<Mutex<Vec<String>>>,
    /// What each pass of the engine decided, for a failure to say.
    pub(crate) log: Mutex<Vec<String>>,
    stop: AtomicBool,
    /// The scenario's producers are done: the window is left to settle.
    pub(crate) settled: AtomicBool,
    settled_at: Mutex<Option<Duration>>,
}

impl World {
    pub(crate) fn new(harness: Harness, messages: u32) -> Arc<Self> {
        let clock = ModelClock::new();
        let pty = Arc::new(Pty::default());
        clock.watching(&pty);
        let arbiter = Arc::new(InputArbiter::with_clock(ENTER_TIMING, clock.clone()));
        let key = PaneKey::new("model", 1);
        let counts = Arc::new(OutputCount::default());
        let printed = arbiter
            .register_counted(&key, None, &counts)
            .expect("the pane registers");
        let screen = Arc::new(PaneScreen::new(24, 80));
        let program = Arc::new(Mutex::new(Program::new(harness, &clock, &pty)));
        let violations = Arc::new(Mutex::new(Vec::new()));
        let pane = Arc::new(ModelPane {
            writer: Mutex::new(Box::new(Written(Arc::clone(&program)))),
            screen: Arc::clone(&screen),
            clock: Arc::clone(&clock),
            printed: Arc::clone(&printed),
            kind: Mutex::new(EnterKind::First),
            pasted_at: Mutex::new(None),
            violations: Arc::clone(&violations),
        });
        // The window's first screen, drawn before anything is asked of it.
        let frame = program.lock().unwrap().frame();
        printed.counts().took(frame.len());
        took_in(&screen, &printed, &frame);
        let msgs = (1..=messages)
            .map(|id| Msg {
                id,
                state: State::Queued,
                attempts: 0,
            })
            .collect();
        Arc::new(Self {
            harness,
            clock,
            arbiter,
            key,
            printed,
            screen,
            pty,
            program,
            pane,
            jobs: Mutex::new(VecDeque::new()),
            fed: Mutex::new(VecDeque::new()),
            msgs: Mutex::new(msgs),
            violations,
            log: Mutex::new(Vec::new()),
            stop: AtomicBool::new(false),
            settled: AtomicBool::new(false),
            settled_at: Mutex::new(None),
        })
    }

    /// The reader: the pty's reads, counted the moment they are in hand, and handed on.
    pub(crate) fn reader(self: &Arc<Self>) {
        let due = Arc::new(Mutex::new(None));
        struct FromPty {
            pty: Arc<Pty>,
            due: Arc<Mutex<Option<Duration>>>,
        }
        impl Read for FromPty {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                let Some(chunk) = self.pty.next() else {
                    return Ok(0);
                };
                *self.due.lock().unwrap() = chunk.due;
                let length = chunk.bytes.len().min(buffer.len());
                buffer[..length].copy_from_slice(&chunk.bytes[..length]);
                Ok(length)
            }
        }
        let mut reader = ActivityReader {
            inner: Box::new(FromPty {
                pty: Arc::clone(&self.pty),
                due: Arc::clone(&due),
            }),
            last_activity: Arc::new(Mutex::new(Instant::now())),
            counts: Arc::clone(self.printed.counts()),
        };
        let mut buffer = [0; 8192];
        loop {
            let Ok(length) = reader.read(&mut buffer) else {
                break;
            };
            if length == 0 {
                break;
            }
            let due = due.lock().unwrap().take();
            self.fed.lock().unwrap().push_back(buffer[..length].to_vec());
            self.pty.counted(due);
        }
    }

    /// The consumer: each read fed into the screen, as the pane host feeds it.
    pub(crate) fn consumer(self: &Arc<Self>) {
        loop {
            sched::wait_until("a read to feed", || {
                !self.fed.lock().unwrap().is_empty() || self.stopping()
            });
            let next = self.fed.lock().unwrap().pop_front();
            let Some(bytes) = next else {
                break;
            };
            took_in(&self.screen, &self.printed, &bytes);
        }
    }

    /// The pane's worker: the keys, pastes and Enters, one at a time, in order.
    pub(crate) fn worker(self: &Arc<Self>) {
        loop {
            sched::wait_until("a job", || !self.jobs.lock().unwrap().is_empty() || self.stopping());
            let job = self.jobs.lock().unwrap().pop_front();
            let Some(job) = job else {
                break;
            };
            match job {
                Job::Typed(bytes) => {
                    let _ = self.arbiter.write_typed_via(&*self.pane, &self.key, &bytes);
                }
                Job::Paste {
                    id,
                    at,
                    read_empty,
                    clear_first,
                    reply,
                } => {
                    *self.pane.kind.lock().unwrap() = EnterKind::First;
                    let body = format!("MSG{id} the message");
                    let done = self.arbiter.write_paste_via(
                        &*self.pane,
                        &self.key,
                        Paste {
                            body: body.as_bytes(),
                            at,
                            read_empty,
                            clear_first,
                        },
                    );
                    *reply.lock().unwrap() = Some(done);
                }
                Job::Enter { at, reply } => {
                    *self.pane.kind.lock().unwrap() = EnterKind::Pane;
                    let done = self.arbiter.enter(&*self.pane, &self.key, at);
                    *reply.lock().unwrap() = Some(done);
                }
            }
        }
    }

    /// How many reads are counted and not yet fed.
    pub(crate) fn fed_len(&self) -> usize {
        self.fed.lock().unwrap().len()
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    /// Ends the service actors once they are idle.
    pub(crate) fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
        self.pty.close();
    }

    /// The human's keys, as the window's page sends them.
    pub(crate) fn type_keys(&self, bytes: &[u8]) {
        self.jobs.lock().unwrap().push_back(Job::Typed(bytes.to_vec()));
    }

    /// What the host says of the window now, as a look.
    pub(crate) fn look(&self) -> Result<Look, NoLook> {
        let snapshot = snapshot_of(
            &self.arbiter,
            &self.key,
            Some(Arc::clone(&self.screen)),
            None,
            Some(BOTTOM_LINES),
        )
        .map_err(|_| NoLook::NoAnswer)?;
        Look::of(&snapshot)
    }

    fn named(&self, at: &Named) -> At {
        At {
            seen: Seen::parse(&at.seen).expect("the host's own name"),
            typed: at.typed,
        }
    }

    /// The paste of the head message on the licence of `look`, the worker's answer; none where the
    /// look licenses no paste.
    pub(crate) fn paste_on(&self, look: &Look) -> Option<Result<(), ArbiterError>> {
        let PasteLicence::Go {
            at,
            read_empty,
            clear_first,
        } = licence_paste(look, self.harness, BoxState::default(), &[3])
        else {
            return None;
        };
        let (named, typed) = (self.named(&at), at.typed);
        Some(self.ask(|reply| Job::Paste {
            id: 1,
            at: Some(named),
            read_empty: read_empty.then_some(typed),
            clear_first,
            reply,
        }))
    }

    /// An Enter of a delivery that is not a paste's first, on the licence of `look`; none where the
    /// look licenses none.
    pub(crate) fn enter_on(&self, look: &Look, needs: EnterFor) -> Option<Result<(), ArbiterError>> {
        let EnterLicence::Go(named) = licence_enter(look, self.harness, needs) else {
            return None;
        };
        let at = self.named(&named);
        Some(self.ask(|reply| Job::Enter { at, reply }))
    }

    /// Hands a job to the worker and waits for its answer.
    fn ask(&self, make: impl FnOnce(Reply) -> Job) -> Result<(), ArbiterError> {
        let reply: Reply = Arc::new(Mutex::new(None));
        self.jobs.lock().unwrap().push_back(make(Arc::clone(&reply)));
        sched::wait_until("the worker answers", || reply.lock().unwrap().is_some());
        let answer = reply.lock().unwrap().take().expect("an answer");
        answer
    }

    fn set(&self, id: u32, state: State) {
        for msg in self.msgs.lock().unwrap().iter_mut() {
            if msg.id == id {
                msg.state = state;
            }
        }
    }

    /// One pass of the engine: the arrival of a message pasted, and the paste of the next.
    /// `blind` is a look that fails (the host did not answer).
    pub(crate) fn pass(&self, blind: bool) {
        let now = self.clock.elapsed();
        let msgs = self.msgs.lock().unwrap().clone();
        let looked = |this: &Self| {
            if blind {
                Err(NoLook::NoAnswer)
            } else {
                this.look()
            }
        };
        for msg in msgs.iter().filter(|msg| matches!(msg.state, State::Pasted(_))) {
            let State::Pasted(at) = msg.state else {
                continue;
            };
            if self.program.lock().unwrap().taken.contains(&msg.id) {
                self.set(msg.id, State::Done);
                continue;
            }
            let waited = now.saturating_sub(at);
            if waited < ENTER_AGAIN {
                continue;
            }
            let look = looked(self);
            let whole = waited >= ARRIVAL;
            // A whole wait with no look begins again, and nothing goes.
            let after = after_wait(look.as_ref(), self.harness);
            let press = |this: &Self, look: &Look, needs: EnterFor| {
                if let EnterLicence::Go(named) = licence_enter(look, this.harness, needs) {
                    let at = this.named(&named);
                    let _ = this.ask(|reply| Job::Enter { at, reply });
                }
            };
            match (&look, after) {
                (Ok(look), AfterWait::PasteAgain) if whole => {
                    let _ = look;
                    self.set(msg.id, State::Queued);
                }
                (Ok(look), AfterWait::EnterAgain(_)) => {
                    press(self, look, EnterFor::PasteStillThere);
                    self.set(msg.id, State::Pasted(self.clock.elapsed()));
                }
                (Ok(look), AfterWait::PasteAgain) => {
                    // Not yet a whole wait: one more Enter, as a box may hold what the screen
                    // does not show.
                    press(self, look, EnterFor::PasteNotSent);
                }
                (_, _) => {
                    if whole {
                        self.set(msg.id, State::Pasted(self.clock.elapsed()));
                    }
                }
            }
        }
        let msgs = self.msgs.lock().unwrap().clone();
        if msgs.iter().any(|msg| matches!(msg.state, State::Pasted(_))) {
            return;
        }
        let Some(head) = msgs.iter().find(|msg| msg.state == State::Queued) else {
            return;
        };
        let look = match looked(self) {
            Ok(look) => look,
            Err(no_look) => {
                self.log.lock().unwrap().push(format!("{now:?}: no look: {no_look:?}"));
                return;
            }
        };
        let state = BoxState::default();
        let licence = licence_paste(&look, self.harness, state, &[3]);
        self.log
            .lock()
            .unwrap()
            .push(format!(
                "{now:?}: {licence:?}, box {:?}, lines {:?}",
                look.reads(self.harness),
                look.lines()
            ));
        let PasteLicence::Go {
            at,
            read_empty,
            clear_first,
        } = licence
        else {
            return;
        };
        let (named, typed) = (self.named(&at), at.typed);
        let id = head.id;
        let done = self.ask(|reply| Job::Paste {
            id,
            at: Some(named),
            read_empty: read_empty.then_some(typed),
            clear_first,
            reply,
        });
        match done {
            Ok(()) => {
                self.log.lock().unwrap().push(format!("{now:?}: m-{id} pasted"));
                self.set(id, State::Pasted(self.clock.elapsed()));
            }
            // The look was no longer true, or the human typed: nothing was written, and no attempt
            // is spent.
            Err(refused @ (ArbiterError::LookAgain(_) | ArbiterError::Unsent)) => {
                self.log.lock().unwrap().push(format!("{now:?}: refused: {refused}"));
            }
            Err(other) => {
                let mut msgs = self.msgs.lock().unwrap();
                if let Some(msg) = msgs.iter_mut().find(|msg| msg.id == id) {
                    msg.attempts += 1;
                    if msg.attempts >= 3 {
                        msg.state = State::Failed;
                    }
                }
                self.violations
                    .lock()
                    .unwrap()
                    .push(format!("a delivery of m-{id} failed: {other}"));
            }
        }
    }

    /// The engine: a pass every few seconds, until its messages are all in and the window has
    /// settled, or five minutes of the model's time have passed since it did.
    pub(crate) fn engine(self: &Arc<Self>, blind: Vec<bool>) {
        let mut pass = 0;
        loop {
            let blind_now = blind.get(pass).copied().unwrap_or(false);
            pass += 1;
            self.pass(blind_now);
            let all = self
                .msgs
                .lock()
                .unwrap()
                .iter()
                .all(|msg| matches!(msg.state, State::Done | State::Failed));
            if self.settled.load(Ordering::SeqCst) {
                let mut since = self.settled_at.lock().unwrap();
                let at = *since.get_or_insert(self.clock.elapsed());
                if all || self.clock.elapsed().saturating_sub(at) >= Duration::from_secs(300) {
                    break;
                }
            }
            crate::arbiter::Clock::rest(&*self.clock, PASS_EVERY);
        }
    }

    /// The scenario's producers say they are done.
    pub(crate) fn settle(&self) {
        self.settled.store(true, Ordering::SeqCst);
    }

    /// The rules, held after every step: what has gone wrong so far, or nothing.
    pub(crate) fn check(&self) -> Result<(), String> {
        let mut said = self.violations.lock().unwrap().clone();
        said.extend(self.program.lock().unwrap().violations.clone());
        if said.is_empty() {
            Ok(())
        } else {
            Err(said.join("\n"))
        }
    }

    /// At the end of a schedule: every message was taken once, unless the human still holds unsent
    /// text, in which case it is neither pasted (but in a box it was pasted into before) nor
    /// failed.
    pub(crate) fn finish(&self) -> Result<(), String> {
        self.check()?;
        let program = self.program.lock().unwrap();
        let draft = !program.draft.is_empty();
        for msg in self.msgs.lock().unwrap().iter() {
            let taken = program.taken.iter().filter(|id| **id == msg.id).count();
            let fine = match (msg.state, taken) {
                (State::Done, 1) => true,
                (State::Queued, 0) => draft,
                (State::Pasted(_), 0) => draft && program.held.contains(&msg.id),
                _ => false,
            };
            if !fine {
                return Err(format!(
                    "m-{} ends {:?} (taken {taken} times, attempts {}) with the human's draft {:?}, held {:?}",
                    msg.id, msg.state, msg.attempts, program.draft, program.held
                ));
            }
        }
        Ok(())
    }

    /// The echo styles the next keys use.
    pub(crate) fn echo_as(&self, styles: impl IntoIterator<Item = Echo>) {
        self.program.lock().unwrap().echo_as(styles);
    }
}
