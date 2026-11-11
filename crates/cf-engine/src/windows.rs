//! A window's lifecycle: launch (prepare, token, `pane.open`, conversation,
//! `started`), look (`observe`, `drawn`), and close (`retire`, `close_if_free`,
//! `close_own`). It owns the record's window part, the generation of each pane,
//! and the chief's relaunch backoff. What a window owes a stop of its task, and
//! how it is interrupted, is `stops`; the work its turn left running, which goes
//! on until its window closes, is `background`.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use cf_harness::contract::{
    CompactionWord, FirstMessage, Input, Interrupt, Launch, LaunchId, Observed, Pane, Window,
};
use cf_harness::seams::Time;
use cf_ledger::{
    launch_size, Begun, ConversationView, HandOver, MessageView, NewNote, ParticipantView,
    ProjectView, Stop,
};
use cf_proto::agents::Harness;
use cf_proto::bridge::DEADLINE;
use cf_proto::panes::{Servers, SilentPaste, BOTTOM_LINES, CLOSE_GRACE, SCREEN_QUIET_MS};
use cf_proto::trace::WindowEvent;
use futures_util::future::{select, Either};
use serde_json::{json, Value};

use crate::at_work::OnWork;
use crate::chief_model::ChiefRuns;
use crate::deliveries::Delivering;
use crate::delivery_text::brief_first;
use crate::dispatcher::Dispatcher;
use crate::host::{EngineHost, Killed, OpenPane, Opened};
use crate::ownership::Kept;
use crate::privacy::PrivacyPart;
use crate::record::Record;
use crate::role_change::{Compared, Prelude};
use crate::role_news::{NewsPart, Told};
use crate::runtime::{begin, caught, returning};
use crate::seams::EngineError;
use crate::shown::Shown;
use crate::specs::{ChiefSpecs, RoleText};
use crate::startup::{Stage, Startup, Why};
use crate::stops::{Interrupted, Unstopped};
use crate::trust::Trusting;
use crate::turn_end::HeldPart;
use crate::unstarted::GivenUp;

/// The key that interrupts a harness's current turn, and the gap of a double press.
const ESCAPE: u8 = 27;
const DOUBLE_PRESS: Duration = Duration::from_millis(150);

/// The screen's last lines as drawn, as a snapshot sends them (`bottom`): what a harness's
/// readers of its screen read, its prompt among them. None where it sent none.
fn drawn_lines(snapshot: &Value) -> Vec<String> {
    snapshot
        .get("bottom")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(|line| line.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// How long what a fresh window's screen shows must hold still before it counts as drawn
/// (the pane host's `screenQuietMs`: a screen printed again as it is, is still, T-1083).
/// The window of a task that is done is held to the same rest before it closes
/// ([`Dispatcher::still_drawing`], T-1051): one rest for "drawn", the number the engine
/// already read a fresh window by, not a new one. Devin's screen shows "Typing · 2s" while it
/// types an answer out, so it repaints at least as often as that counter counts; 1.5 s is
/// longer than a second, so a screen still typing is not taken for one at rest. Not yet
/// measured on a Devin window at rest (T-1051's live runs say what it prints then).
const DRAWN_QUIET_MS: f64 = 1_500.0;

/// The longest the window of a task that is done is held open for its screen to be drawn,
/// from the look that found the window free: where the screen has not held still by then
/// the window closes all the same ([`Dispatcher::still_drawing`], T-1051). From T-1038's A/B
/// (Devin 3000.11.228, 16 legs with the window kept open, logs in
/// `consensflow-trees/.runs/T-1038-logs/ab-*.log`): the screen drew the answer 5.0 to 6.8 s
/// after the task was given, and the agent had ended its turn a few seconds before that, so
/// the screen trailed the turn by about five seconds at most. This doubles that, and is the
/// host's whole close grace ([`CLOSE_GRACE`]). A
/// screen that never rests (a clock, a spinner that goes on at rest) holds its window no
/// longer, and the next task of its session waits no longer for a window of its own
/// (`ownership`).
const STILL_BOUND_MS: i64 = 10_000;

/// The longest the window of a task that is done is held open for a compaction that
/// ConsensFlow's own extension asked its harness for at the window's rest, for the
/// project's size ([`Window::compaction_asked`], [`Dispatcher::compaction_held`], T-1475):
/// 300 seconds counted from the engine's first sight of that word for that window, on the clock
/// that does not step ([`Time::monotonic_ms`]: the engine's other bounds are counted on the wall
/// clock, which a hold of this kind must not be, a step of it making the hold shorter or
/// longer). The compactions of a Pi member's real windows took 39 to 80 seconds (a cached
/// summary of 243,000 to 749,000 tokens); this is about four times the longest. Past it the
/// window closes as it does without one, the compaction ended by the close as before. The
/// word's own time is compared with no clock: it only tells one word from another.
pub(crate) const COMPACTION_HOLD_MS: i64 = 300_000;

/// What the engine made of a word the extension wrote ([`Dispatcher::compaction_held`]): the
/// time the word says it was written (to tell a new word from the one seen; none where the first
/// look met a word it could not read), and when the engine first looked on the clock that does
/// not step.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CompactionSeen {
    pub(crate) asked: Option<i64>,
    pub(crate) since: i64,
}

/// How soon a chief that could not start is tried again; the wait doubles
/// with each failure, up to the most.
const RELAUNCH_MS: i64 = 5_000;
const RELAUNCH_MAX_MS: i64 = 5 * 60_000;

/// How long a member's launch given up waits for its window's kill to be answered
/// before it settles without the exit ([`Dispatcher::close_own_within`]). The pane host
/// answers once the window's programs are gone: on the Mac in about 7 ms, and in the
/// whole close grace ([`CLOSE_GRACE`], 10 s; 10.02 s measured) where the window leaves
/// a job that ignores the request to end; on Windows (zeewin) in 2 to 10 ms, its tree
/// ended at once (T-77, `kill_answer_times_with_the_real_grace` in `cf-panes`). Three
/// times the longest, derived from the grace so that the two move together (T-414 P3-6).
const KILL_ANSWER_WAIT: Duration = Duration::from_secs(3 * CLOSE_GRACE.as_secs());

/// How long a window whose kill the host took is waited for its exit
/// ([`Dispatcher::exit_overdue`]). The host answers the kill once the window's programs are
/// gone, and tells the exit once its output has ended, a second at most after
/// (`cf-panes`' `ENDED_WAIT`); the kill's own wait is ample for that too.
const EXIT_WAIT: Duration = KILL_ANSWER_WAIT;

/// A close of the engine's own under way ([`Dispatcher::close_own`]): the window it
/// closes, and, where a member's give-up closes it, why the window was given up, which
/// the words told to the window are said to wait for the resume for, and the message
/// the give-up settles itself (`settling`), which they are not named with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnExit {
    pub(crate) pane: Pane,
    pub(crate) why: Option<String>,
    pub(crate) settling: Option<i64>,
}

impl WindowPart {
    /// The engine's own close of `pane`, where it is one.
    pub(crate) fn own_exit_of(&self, pane: &Pane) -> Option<&OwnExit> {
        self.own_exit.as_ref().filter(|own| own.pane == *pane)
    }
}

/// What a window is doing, as the board and the trace say it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityState {
    Starting,
    Working,
    Idle,
    Waiting,
    Out,
    Unknown,
    Closed,
}

impl ActivityState {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityState::Starting => "starting",
            ActivityState::Working => "working",
            ActivityState::Idle => "idle",
            ActivityState::Waiting => "waiting",
            ActivityState::Out => "out",
            ActivityState::Unknown => "unknown",
            ActivityState::Closed => "closed",
        }
    }
}

/// A window's activity, and why where it says (a question it waits on, the
/// reset it is out of quota until, a look that failed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activity {
    pub state: ActivityState,
    pub reason: Option<String>,
}

impl Activity {
    pub fn of(state: ActivityState) -> Self {
        Self {
            state,
            reason: None,
        }
    }

    pub fn because(state: ActivityState, reason: impl Into<String>) -> Self {
        Self {
            state,
            reason: Some(reason.into()),
        }
    }
}

/// A pane on its way open, and, if its exit came before the host's answer
/// (the host watches a window's process before it answers), what the host
/// said of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Opening {
    pub(crate) pane: Pane,
    pub(crate) exited: Option<Shown>,
    /// The conversation its program will write to, where it goes on with one the ledger
    /// holds: none for one opening on a new conversation, which no window wrote to yet.
    pub(crate) conversation: Option<i64>,
}

/// The engine's own kill of a window, on its way, and the task (its project, number
/// and the stop its pause is) whose pause an exit that came before the kill's answer
/// marked as the
/// suspension's (the host's close waits for the program to end, and tells the
/// exit as it ends): the mark stands if the kill was taken, and goes if it was
/// refused, for that exit was the window's own (`kill_and_settle`). Only that
/// window's: a give-up goes on while its kill waits, and the record's later window
/// may be killed meanwhile, whose answer is its own (T-509: the earlier kill, refused
/// at last, took back the later one's mark). A kill of the window that is taken ends
/// every kill of it under way: the engine ended the window, and no other kill's
/// refusal says otherwise (Calliope's P3-3 of T-510: a give-up's kill and the Close's
/// each kept an entry, the Close's answer took the give-up's, and its refusal later
/// took back the Close's mark).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Killing {
    pub(crate) pane: Pane,
    pub(crate) paused: Option<(i64, i64, i64)>,
}

/// The chief's window failing to start, tried again ever more slowly: how
/// often it failed, and when it may be tried next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Relaunch {
    pub(crate) failures: u32,
    pub(crate) at: i64,
    /// A window of the chief was given up for not coming up since one last came
    /// up: the next opens at its time whatever waits for it (`Dispatcher::step`).
    pub(crate) reopens: bool,
}

/// The record's window part.
pub(crate) struct WindowPart {
    /// A chief that needs an edit (T-1202) whose harness's record was read and named no
    /// model: it is held, and its record not read again, until the human switches it.
    pub(crate) no_model_in_record: bool,
    /// The harness's window on its launch (the adapter's `launch` bag).
    pub(crate) window: Option<Rc<dyn Window>>,
    /// The keys that interrupt a turn in it (its adapter's `interrupt`).
    pub(crate) keys: Option<Interrupt>,
    /// How it takes what it is handed (its adapter's `input`): pasted into its
    /// input box, or over a channel of its harness's own.
    pub(crate) input: Input,
    /// How its harness keeps its own processes (its adapter's `servers`), and
    /// which harness it is, which the pane host is told where a turn begins.
    pub(crate) servers: Servers,
    pub(crate) harness: String,
    pub(crate) pane: Option<Pane>,
    pub(crate) opening: Option<Opening>,
    /// How many times a window was asked for it, counted as its window is asked for
    /// ([`Dispatcher::open_soon`]) and as each launch begins, before either waits on
    /// anything. The exit of an earlier window that read its record with no window
    /// open compares the count it began with: a window asked for meanwhile, up, on its
    /// way or given up, is no business of that exit's (`in_flight`).
    pub(crate) launches: u64,
    pub(crate) launch_id: Option<LaunchId>,
    /// What a chief's window was told of its role, and what waits to tell it of a setting the
    /// human changed since (`role_news`).
    pub(crate) news: NewsPart,
    /// The window's token, for its `cf`.
    pub(crate) token: Option<String>,
    /// A kill is on its way.
    pub(crate) retiring: bool,
    /// When the host took the kill of `retire`, whose exit is waited for no longer than
    /// [`EXIT_WAIT`] ([`Dispatcher::exit_overdue`]): none where the exit came first.
    pub(crate) kill_taken: Option<i64>,
    /// When the engine asked the host to open the window (milliseconds): what the harness's
    /// record says was written before is history, not the window's (a move of the model, a
    /// refusal of an account it read from a transcript a Resume brought back, T-816).
    pub(crate) opened_at: Option<i64>,
    /// The window whose exit is the engine's own (`close_own`): a chief's closes no
    /// project. Only that window's: a close whose kill was never answered leaves it
    /// set, and a later window of the record is no close of the engine's (T-414 P3-8).
    pub(crate) own_exit: Option<OwnExit>,
    /// The engine's own kills of its windows that wait for the host's answer.
    pub(crate) killing: Vec<Killing>,
    /// The panes the engine sent a kill to: by id and generation, from the moment it asked
    /// (before the host's answer), until the window's exit is settled (`closed`). What ends a
    /// program with a signal is not told apart by its exit code, so whether the engine had a
    /// hand in it is told by this, not by the engine's own close alone: `retire` kills without
    /// one (the review of `a26e0848e`, P2). A stop's keys are Escape: they send no signal, and
    /// a stop that ends in a close ends in `retire`'s kill.
    pub(crate) sent_to: Vec<Pane>,
    /// Why the engine closes it, where the pause note of the task it was working
    /// on is to say more than that it closed: with its project, when the chief's
    /// window closed by itself.
    pub(crate) closed_with: Option<String>,
    /// The human opened it (Show): it stays open.
    pub(crate) pinned: bool,
    /// The human hid it since: it closes once free and its turn settled.
    pub(crate) hidden: bool,
    /// Whether its last look said its turn settled.
    pub(crate) settled: bool,
    pub(crate) relaunch: Option<Relaunch>,
    /// The model a chief's launch asked for, which its harness said it no longer has: the
    /// next launch starts on the harness's default (CHF-06).
    pub(crate) gone: Option<String>,
    /// How many of its windows in a row were given up for not coming up in the time
    /// they had: the next is given longer (`Launch::starts_given_up`). One that comes
    /// up starts the count again.
    pub(crate) starts_given_up: u32,
    /// Its record showed a message it was handed ([`Dispatcher::confirm_delivered`]): the
    /// window came up and took work. A chief's window that ends before is a launch its
    /// harness may have refused (CHF-06); the history a resumed record shows is no sign.
    pub(crate) took: bool,
    /// It printed, then held still: its screen is drawn.
    pub(crate) drawn: bool,
    /// It named its first conversation.
    pub(crate) named: bool,
    /// Where it stands with its harness's question about trusting its folder.
    pub(crate) trust: Trusting,
    /// How many keys ConsensFlow pressed into it toward a yes to its harness's questions.
    pub(crate) trust_keys: usize,
    /// Whether the first question was read with its choice placed: only that is a
    /// question whose yes is carried to the project's other windows, the only ones a key
    /// of ConsensFlow's can answer.
    pub(crate) trust_sure: bool,
    /// The last stop this window process paid: its task and the sequence. It
    /// dies with the window, and a window opened later owes nothing for the
    /// stops asked before it opened.
    ///
    /// One pair is enough because a member session holds at most one task at
    /// a time (`require_free` in the ledger): the stops a window pays are its
    /// task's, and a task that follows (a follow-up, in a window the human
    /// keeps open) has its own, which a pair of the one before never stands
    /// in for. A window that held two and paid a stop for each in turn would
    /// forget the first's, and be interrupted for it again on going back to
    /// it: only a member's own lane, given tasks by name, can.
    pub(crate) stopped: Option<(i64, i64)>,
    /// The stop it paid at rest with its turn's message not yet taken back: a harness that
    /// takes the message back writes so a moment after the turn's end (Muse's
    /// `run_retracted`, 5–11 ms after its `terminal cancelled`), and a look between the two
    /// pays the stop (`stops`). A later look that reads the message taken back takes it
    /// back then.
    pub(crate) paid_untaken: Option<Stop>,
    /// The stall ConsensFlow's own pause made of it for a message it began no turn on
    /// (`never_began`): bound to that message, to its item in the record and to this window's
    /// generation. A window that ignores every round of that stop, running nothing and still
    /// on nothing, is relaunched (`stops`), never kept.
    pub(crate) stalled: Option<crate::stops::Stalled>,
    /// The rounds the stop it owes has had.
    pub(crate) interrupted: Option<Interrupted>,
    /// The stop it ignored in every round, while it lasts.
    pub(crate) unstopped: Option<Unstopped>,
    pub(crate) activity: Activity,
    /// The conversation the window's program writes to, by its id: the one it opened
    /// on, or the one the human switched it to since (`transcripts`). Whether that
    /// program is gone is kept on it when the window ends ([`Shown::gone`],
    /// `ownership`); none for a window opened on a conversation that was never written
    /// down (its participant was forgotten as it opened).
    pub(crate) conversation: Option<i64>,
    /// The pane of the last window that ended and the conversation it wrote to,
    /// which the host's word on how it ended may come after, once the engine's own
    /// close has settled the window ([`Dispatcher::heard_exit`]).
    pub(crate) last_exit: Option<(Pane, Option<i64>)>,
    /// It opened on a conversation whose last window's program the host never proved
    /// gone: what that program had queued may stay in its record for good, and read its
    /// turns as never over (T-526). Its requester is told once where a turn looks finished
    /// all the same (`turn_end`).
    pub(crate) unproven: bool,
    /// The task whose work the window holds, by its number (`ownership`): no other
    /// task's message goes into it.
    pub(crate) owner: Option<i64>,
    /// The task whose requester was told it waits for this window to close.
    pub(crate) told_waiting: Option<i64>,
    /// When the engine first found the window free with its task done, wanting it closed:
    /// its screen is waited for from then, [`STILL_BOUND_MS`] at the most
    /// ([`Dispatcher::still_drawing`]). None until then, again once a task of the window's
    /// is in hand (the wait is over, the next time it is done is a wait of its own), and
    /// once the window closes.
    pub(crate) found_free: Option<i64>,
    /// When the engine first saw its extension's word that it asked the harness to compact
    /// for the window's rest, while the task the window was given is done: the hold's
    /// bound is counted from the word's own time, or from this where the word's time is
    /// ahead of the clock. None while there is no word, and once the window closes
    /// ([`Dispatcher::compaction_held`]).
    pub(crate) compaction_seen: Option<CompactionSeen>,
    /// The window of its last launch whose open the host never answered by the open's
    /// deadline: the host may have started its program since, or may yet, and it would
    /// run on the conversation the next window goes on with (a Codex thread one client
    /// holds alone). It is ended before another window of the participant opens
    /// ([`Dispatcher::end_abandoned`]).
    pub(crate) abandoned: Option<Abandoned>,
}

/// A window whose open the host never answered by its deadline, which the engine ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Abandoned {
    pub(crate) pane: Pane,
    /// Whether the trace said its kill failed: once, though it is asked at every step.
    pub(crate) told: bool,
}

impl Default for WindowPart {
    fn default() -> Self {
        Self {
            no_model_in_record: false,
            window: None,
            keys: None,
            input: Input::Channel,
            servers: Servers::Unknown,
            harness: String::new(),
            pane: None,
            opening: None,
            launches: 0,
            launch_id: None,
            news: NewsPart::default(),
            token: None,
            retiring: false,
            kill_taken: None,
            opened_at: None,
            own_exit: None,
            killing: Vec::new(),
            sent_to: Vec::new(),
            closed_with: None,
            pinned: false,
            hidden: false,
            settled: false,
            relaunch: None,
            gone: None,
            starts_given_up: 0,
            took: false,
            drawn: false,
            named: false,
            trust: Trusting::Past,
            trust_keys: 0,
            trust_sure: false,
            stopped: None,
            paid_untaken: None,
            stalled: None,
            interrupted: None,
            unstopped: None,
            activity: Activity::of(ActivityState::Closed),
            conversation: None,
            last_exit: None,
            unproven: false,
            owner: None,
            told_waiting: None,
            found_free: None,
            compaction_seen: None,
            abandoned: None,
        }
    }
}

/// What the windows keep across records: the last pane's generation.
#[derive(Debug, Default)]
pub(crate) struct WindowsState {
    generation: Cell<i64>,
}

/// A pane as the pane host's requests name it.
pub(crate) fn pane_body(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation })
}

/// [`pane_body`] for a `pane.snapshot` that is to hold the screen's last lines as drawn
/// (`bottom`), which a harness's readers of its screen read (its prompt, its input box): a
/// host sends them only when asked, and a reader given none reads no prompt and no box.
pub(crate) fn pane_body_drawn(pane: &Pane) -> Value {
    json!({ "id": pane.id, "generation": pane.generation, "bottom": BOTTOM_LINES })
}

/// A harness's interrupt as keys into its window (`pressInterrupt`): Escape
/// as many times in a row as it asks for, and, where those presses open a
/// dialog at a turn that ended just before them (Devin's rewind), one more
/// after a pause, which closes it and is nothing anywhere else. A key the
/// host did not take is not pressed again. Whether the host took any key at
/// all: one it refused, or never answered, is no press, whatever the harness
/// would have done with it.
pub(crate) async fn press_interrupt(
    host: &dyn EngineHost,
    time: &dyn Time,
    pane: &Pane,
    keys: Interrupt,
) -> bool {
    let escape = || async {
        let mut body = pane_body(pane);
        body["bytes"] = json!([ESCAPE]);
        let answer = host.request("pane.input", body).await;
        answer.is_ok_and(|answer| answer.get("ok") == Some(&Value::Bool(true)))
    };
    let mut taken = false;
    for press in 0..keys.presses {
        if press > 0 {
            time.sleep(DOUBLE_PRESS).await;
        }
        taken |= escape().await;
    }
    if let Some(after) = keys.close_after {
        time.sleep(after).await;
        taken |= escape().await;
    }
    taken
}

/// `env` with `key` set as an object spread sets it: in its first place
/// where it is there already, last where it is not.
fn assign(env: &mut Vec<(String, String)>, key: &str, value: &str) {
    match env.iter_mut().find(|(name, _)| name == key) {
        Some((_, old)) => value.clone_into(old),
        None => env.push((key.to_owned(), value.to_owned())),
    }
}

/// A launch's first message as its window is to be given it: one delivered once
/// the window is up is owed, with its text and the rows its paste was begun with,
/// which an Enter pressed once more for it sends with it (`arrival`).
fn owe(mut delivering: Delivering, planned: &Planned) -> Delivering {
    if let Some(text) = &planned.owed {
        delivering.owed = Some(text.clone());
        delivering.carried.clone_from(&planned.carried);
    }
    delivering
}

/// What a launch was prepared with: the window, the keys that interrupt it,
/// how it takes what it is handed, how its harness keeps its own processes and which harness it is, how its pane
/// opens, the conversation it goes on with if there is one, and the stop its task
/// had asked when the first message was begun, which the new window owes nothing
/// for.
struct Planned {
    window: Rc<dyn Window>,
    keys: Interrupt,
    input: Input,
    servers: Servers,
    /// How the harness reads a human's paste, where it reads one silently
    /// ([`Adapter::silent_paste`](cf_harness::contract::Adapter::silent_paste)).
    silent_paste: Option<SilentPaste>,
    harness: String,
    argv: Vec<String>,
    env: Vec<(String, String)>,
    drop_env: Vec<String>,
    native_session: Option<String>,
    conversation: Option<ConversationView>,
    stop: Option<Stop>,
    /// The rows the first message's paste was begun with.
    carried: Vec<i64>,
    /// How the window is given its first message ([`Adapter::first_message`]).
    first: FirstMessage,
    /// The first message's text, where the window is delivered it once it is up
    /// ([`FirstMessage::Delivered`]): it goes on the launch's [`Delivering`] as owed.
    owed: Option<String>,
    /// What goes into the window before anything else of its queue once it is up,
    /// in this order ([`Compared::prelude`]).
    prelude: Option<Prelude>,
    /// The text the window starts with, for a harness that keeps the first
    /// instructions of its conversations: what a conversation this launch starts
    /// is recorded as begun with.
    first_instructions: Option<String>,
    /// What a chief's window is told of its role by this launch, for the notes that tell it
    /// of a setting changed since (`role_news`): none for a member's.
    told: Option<Told>,
    /// The task whose work the window holds (`ownership`): its first message's, or
    /// the one its conversation was last given where it goes on with one.
    owner: Option<i64>,
}

impl Dispatcher {
    /// Refuses a harness ConsensFlow has no adapter for: none of its windows
    /// could open.
    pub(crate) fn require_adapter(&self, harness: &str) -> Result<(), EngineError> {
        match self.seams.adapters.adapter(harness) {
            Some(_) => Ok(()),
            None => Err(EngineError::said(
                "no-adapter",
                format!("ConsensFlow cannot open {harness} windows"),
            )),
        }
    }

    /// A window that exited: its token is revoked, its files in the home go
    /// with it, and its part of the record holds no window.
    pub(crate) fn closed(&self, record: &Record) {
        let (token, launch) = {
            let mut window = record.window.borrow_mut();
            let taken = (window.token.take(), window.launch_id.take());
            window.window = None;
            window.pane = None;
            window.retiring = false;
            window.kill_taken = None;
            window.sent_to.clear();
            window.hidden = false;
            window.settled = false;
            window.stopped = None;
            window.stalled = None;
            window.interrupted = None;
            window.unstopped = None;
            window.owner = None;
            window.news = NewsPart::default();
            window.conversation = None;
            window.unproven = false;
            window.told_waiting = None;
            window.found_free = None;
            window.compaction_seen = None;
            window.activity = Activity::of(ActivityState::Closed);
            window.trust = Trusting::Past;
            window.trust_keys = 0;
            window.trust_sure = false;
            taken
        };
        // What its task was held for went with it: a task held again in a window opened
        // again is told again (`turn_end`).
        *record.held.borrow_mut() = HeldPart::default();
        // A privacy dialog it waited on, and the services told of, went with the window: a
        // window opened again is told of them again (`privacy`).
        *record.privacy.borrow_mut() = PrivacyPart::default();
        if let Some(token) = token {
            self.seams.credentials.revoke(&token);
        }
        if let Some(launch) = launch {
            self.seams.launch_files.forget(&launch);
        }
    }

    /// The window of a record and its pane, as they are now.
    pub(crate) fn window_of(&self, record: &Record) -> (Option<Rc<dyn Window>>, Option<Pane>) {
        let part = record.window.borrow();
        (part.window.clone(), part.pane.clone())
    }

    /// The next pane's generation: past the last one, and no earlier than
    /// now, so a window opened after a restart takes no old one's.
    fn next_generation(&self) -> i64 {
        let next = (self.windows.generation.get() + 1).max(self.now());
        self.windows.generation.set(next);
        next
    }

    /// What a window does now, told to the trace and the board when it
    /// changed. A trace that could not be told fails it, the window's
    /// activity changed all the same.
    pub(crate) fn set_activity(
        &self,
        record: &Record,
        activity: Activity,
    ) -> Result<(), EngineError> {
        {
            let mut window = record.window.borrow_mut();
            if window.activity == activity {
                return Ok(());
            }
            window.activity = activity.clone();
        }
        self.trace_window(
            record,
            WindowEvent::Activity {
                state: activity.state.as_str().to_owned(),
                reason: activity.reason,
            },
        )?;
        self.changed();
        Ok(())
    }

    /// Launches `participant`'s window on its harness, `message` its first if
    /// there is one (a chief's is its handoff, most often). A conversation that
    /// is resumed and whose role changed, on a harness that keeps its first
    /// instructions, has no first message on its command line: the note that says
    /// the role goes in first once the window is up, and `message` after it
    /// ([`Dispatcher::compare_role`]); the launch owes its window that note until it
    /// is told, and a window that exits or is never ready before is a launch that
    /// did not come up ([`Prelude`]). Whether it is resumed is decided first: a
    /// conversation its harness holds nothing of opens afresh, and has no role to
    /// be told ([`Dispatcher::conversation_to_go_on_with`]). One that does not come
    /// up says why ([`Dispatcher::launch_failed`]); a participant forgotten while
    /// its launch waits gets nothing more under its ids. It is counted before
    /// anything ([`WindowPart::launches`]): its preparation waits on its harness, and
    /// an earlier window's exit that reads its record meanwhile is to see a window on
    /// its way (a review of 2026-10-16: the exit of a session's window paused the task
    /// the window the human opened again was being prepared for).
    /// A first message withdrawn before it is begun (a continuation whose task
    /// moved on) ends the launch too. One that goes in on the command line is
    /// handed over as the pane is asked for, one a window takes once it is up as it
    /// is posted, each as the ledger admits it ([`cf_ledger::Ledger::admit`]).
    pub(crate) async fn launch(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        message: Option<MessageView>,
    ) -> Result<(), EngineError> {
        // A window whose open went unanswered is ended before another opens; until the
        // host says it has, the launch waits for the next step.
        if !self.end_abandoned(record).await? {
            return Ok(());
        }
        // A chief the update could not tell the model of (T-1202) opens on no default,
        // whichever way its launch was asked for: it takes the model its harness's record
        // names, and opens on that, its row read again; with none, nothing is begun, and
        // what waits for it stays queued for Switch chief
        // ([`Dispatcher::chief_model_from_record`]).
        let resolved;
        let participant = if participant.role == "chief" && participant.needs_edit {
            if !self.chief_model_from_record(participant, record).await? {
                return Ok(());
            }
            let now = self.known_project(participant.project_id)?;
            let Some(chief) = now
                .participants
                .into_iter()
                .find(|chief| chief.id == participant.id)
            else {
                return Ok(());
            };
            resolved = chief;
            &resolved
        } else {
            participant
        };
        record.window.borrow_mut().launches += 1;
        let harness = participant.harness.clone().unwrap_or_default();
        // A role the window took before it closed, though its arrival was never
        // seen, is what the conversation was told: the ledger learns it first.
        let settled = self.settle_replacements(participant).await?;
        if self.forgotten(record) {
            return Ok(());
        }
        let message = message.filter(|message| !settled.contains(&message.id));
        // A chief's first message is its handoff or not by the conversation it goes
        // on with, so that is looked into first, and then the role its window is
        // given is compared with the one that conversation was told: a conversation
        // its harness holds nothing of is none the window goes on with, has nothing
        // to be told, and gets no note. A member's does not wait for it, and its
        // conversation is looked into once the message is begun, after which its
        // role is compared ([`Dispatcher::plan`]).
        let mut conversation = None;
        let mut compared = Compared {
            first: message,
            prelude: None,
            role: None,
        };
        if participant.role == "chief" {
            conversation = self.conversation_to_go_on_with(participant).await?;
            let first =
                self.chief_first(project, participant, conversation.as_ref(), compared.first)?;
            let resumed = conversation
                .as_ref()
                .filter(|conversation| conversation.native_session.is_some());
            compared = self.compare_role(project, participant, resumed, first)?;
        }
        let Compared {
            first,
            prelude,
            role,
        } = compared;
        let launch_id = self.seams.launch_ids.draw();
        let generation = self.next_generation();
        let delivering = first.as_ref().map(|first| Delivering {
            message: first.id,
            since: self.now(),
            // A launch's first message is waited for by the launch's own time.
            rested_since: None,
            launch: true,
            chief: participant.role == "chief",
            queued: false,
            free_since: None,
            turned: false,
            entered_again: false,
            carried: Vec::new(),
            session: None,
            // Owed once the plan says the window is delivered it (`owe`).
            owed: None,
            late: false,
            work: OnWork::default(),
            not_ready: None,
        });
        // What fails with a launch that does not come up: its first message, or
        // the one that would have been, which the role goes ahead of. A member's
        // is its first message in any case: where its role is to be said first,
        // that is found in the plan, and the message is given back to its queue.
        let failing = delivering
            .clone()
            .or_else(|| self.displaced(participant, prelude.as_ref()));
        // Boxed, so that the work of an operation that launches stays small
        // (`dispatcher::tests`): the plan asks the harness and prepares the window,
        // and what it comes to is held on the heap through the waits after it.
        let planned = match Box::pin(self.plan(
            record,
            project,
            participant,
            first.as_ref(),
            conversation,
            &launch_id,
            delivering.as_ref(),
            role,
            prelude,
        ))
        .await
        {
            Ok(Some(planned)) => Box::new(planned),
            Ok(None) => return Ok(()),
            Err(cause) => {
                // An adapter may fail after writing the launch's files; no window will read them.
                self.seams.launch_files.forget(&launch_id);
                // What the adapter said is its own: the log has it, and a note says the stage.
                let why = if cause.refusal().code == "no-adapter" {
                    Why::NoAdapter
                } else {
                    Why::Unsaid
                };
                let said = self.startup_words(
                    participant,
                    Startup::new(Stage::Prepare, why),
                    &cause.to_string(),
                );
                return self.launch_failed(record, project, participant, failing, &said);
            }
        };
        // A member's role was compared once its harness had said which conversation
        // its window goes on with, and where the role is to be said first the
        // message the launch was for went back to its queue: the window opens with
        // no first message, and that one goes after the note that says the role.
        let prelude = planned.prelude.clone();
        let delivering = delivering
            .filter(|_| {
                prelude
                    .as_ref()
                    .is_none_or(|prelude| prelude.displaced.is_none())
            })
            .map(|delivering| owe(delivering, &planned));
        if self.launch_over(
            record,
            participant,
            delivering.as_ref(),
            planned.stop,
            "a pause came while its window was prepared",
        )? || !self.handed_on_open(delivering.as_ref(), &planned)?
        {
            self.seams.launch_files.forget(&launch_id);
            return Ok(());
        }
        let resume = planned
            .conversation
            .as_ref()
            .and_then(|conversation| conversation.native_session.clone());
        let token = self.seams.credentials.issue(project.id, participant.id);
        let pane = Pane {
            id: format!("p{}-{}", project.id, participant.handle),
            generation: u64::try_from(generation).unwrap_or_default(),
        };
        let mut env = self.seams.pane_env.env(participant, project);
        for (key, value) in &planned.env {
            assign(&mut env, key, value);
        }
        assign(&mut env, "CONSENSFLOW_TOKEN", &token);
        // The program of a window that opens on a conversation will write to it: what
        // was proven of the last one on it holds no more, kept so before anything of
        // the new one runs, so that a restart while it is open does not take it for
        // gone (`ownership`). A host that refuses the open ran nothing, and the proof
        // is kept again; one whose answer never came may have, or may yet (kluris,
        // 2026-10-26: it started three seconds after the deadline). A window on a new
        // conversation leaves the one before as it was.
        let resumed = resume.is_some() && planned.native_session == resume;
        let resumed_on = planned
            .conversation
            .as_ref()
            .filter(|_| resumed)
            .map(|conversation| conversation.id);
        let proven = match resumed_on {
            Some(id) => self.seams.ledger.borrow().program_gone(id)?,
            None => false,
        };
        if let Some(id) = resumed_on {
            self.seams.ledger.borrow_mut().set_program_gone(id, false)?;
        }
        // Where the window's program finds its home: a window opened on it is told of (`privacy`).
        let home = env
            .iter()
            .find(|(name, _)| name == "HOME")
            .map(|(_, value)| value.clone());
        // The host watches a window's process before it answers the open, so
        // an exit can come first, even in the same read: the exit finds it here.
        let asked_at = self.now();
        record.window.borrow_mut().opening = Some(Opening {
            pane: pane.clone(),
            exited: None,
            conversation: resumed_on,
        });
        // `.catch(...)` made a promise of its own to wait on, a turn after
        // the host's answer.
        let opened = returning(self.seams.host.open(OpenPane {
            pane: pane.clone(),
            cwd: workspace(project),
            argv: planned.argv.clone(),
            env,
            drop_env: planned.drop_env.clone(),
            silent_paste: planned.silent_paste,
        }))
        .await;
        let exited = record
            .window
            .borrow_mut()
            .opening
            .take()
            .and_then(|opening| opening.exited);
        let pid = match opened {
            Ok(Opened::Open { pid }) => pid,
            // The host would not open it, in words of its own that no note carries; or the
            // window's program cannot be run as it is (an npm shim with no Node to run on), and
            // the person's own screen has the words, a note the kind of failure.
            Ok(refused @ (Opened::Refused { .. } | Opened::Unrunnable(_))) => {
                if let Some(id) = resumed_on.filter(|_| proven) {
                    self.seams.ledger.borrow_mut().set_program_gone(id, true)?;
                }
                let (why, said) = match &refused {
                    Opened::Unrunnable(refusal) => {
                        (Why::Program(refusal.failure()), refusal.words())
                    }
                    Opened::Refused { error } => (Why::HostRefused, error.as_str()),
                    _ => (Why::HostRefused, ""),
                };
                return self.not_opened(
                    record,
                    project,
                    participant,
                    failing,
                    &token,
                    &launch_id,
                    (why, said),
                );
            }
            // The host goes on with an open it did not answer in time: the window's
            // program may run, on the conversation and with whatever its command line
            // gave it, and nobody would see what it does. It is the participant's to
            // end before another of its windows opens.
            Ok(Opened::Unanswered) => {
                record.window.borrow_mut().abandoned = Some(Abandoned {
                    pane: pane.clone(),
                    told: false,
                });
                let failed = self.not_opened(
                    record,
                    project,
                    participant,
                    failing,
                    &token,
                    &launch_id,
                    (Why::HostSilent, DEADLINE),
                );
                self.end_abandoned(record).await?;
                return failed;
            }
            Err(error) => {
                return self.not_opened(
                    record,
                    project,
                    participant,
                    failing,
                    &token,
                    &launch_id,
                    (Why::Unsaid, &error.message),
                );
            }
        };
        // The window's own process: an adapter may find the harness's own
        // status by it from its first look.
        planned.window.opened(pid);
        self.home_note(project, participant, home.as_deref(), &workspace(project));
        if self.forgotten(record) {
            {
                let mut part = record.window.borrow_mut();
                part.pane = Some(pane.clone());
                part.launch_id = Some(launch_id);
                part.token = Some(token);
                part.conversation = resumed_on;
            }
            // One that exited before its open was answered has gone already.
            if let Some(shown) = exited {
                Box::pin(self.exited(&pane, shown)).await?;
            }
            return Ok(());
        }
        let mut conversation_id = planned
            .conversation
            .as_ref()
            .map(|conversation| conversation.id);
        if !resumed {
            let started = self
                .seams
                .ledger
                .borrow_mut()
                .start_conversation(participant.id, &harness)?;
            conversation_id = Some(started.id);
            if let Some(native) = planned.native_session.as_deref() {
                self.seams
                    .ledger
                    .borrow_mut()
                    .bind_conversation(started.id, native)?;
            }
            if let Some(instructions) = planned.first_instructions.as_deref() {
                self.seams
                    .ledger
                    .borrow_mut()
                    .set_conversation_role_text(started.id, instructions)?;
            }
        }
        // A turn begins with the first message: what the record lost so far is not its. A
        // conversation begun afresh has lost nothing, whatever the last one's record did
        // (Calliope's P3 (d), T-624).
        {
            let mut delivery = record.delivery.borrow_mut();
            if !resumed {
                delivery.lost = 0;
            }
            if let Some(first) = first.as_ref() {
                delivery.lost_before = Some((first.id, delivery.lost));
            }
        }
        {
            let mut part = record.window.borrow_mut();
            part.window = Some(Rc::clone(&planned.window));
            part.keys = Some(planned.keys);
            part.input = planned.input;
            part.servers = planned.servers;
            part.harness.clone_from(&planned.harness);
            part.pane = Some(pane.clone());
            part.opened_at = Some(asked_at);
            part.paid_untaken = None;
            record.quota.borrow_mut().drifts_told.clear();
            part.launch_id = Some(launch_id);
            part.news = NewsPart::launched(planned.told.clone());
            part.token = Some(token);
            part.activity = Activity::of(ActivityState::Starting);
            part.drawn = false;
            part.named = false;
            part.took = false;
            part.stopped = planned.stop.map(|stop| (stop.task_id, stop.seq));
            part.owner = planned.owner;
            part.conversation = conversation_id;
            part.unproven = resumed_on.is_some() && !proven;
        }
        // A window relaunched for a message it began no turn on (`stops`) came back: on its
        // conversation, or afresh where its harness held it no more. It is told once that
        // message has arrived.
        self.relaunched_back(record, resumed);
        {
            let mut delivery = record.delivery.borrow_mut();
            delivery.delivering = delivering;
            delivery.prelude = prelude;
        }
        // A window that exited before its open was answered goes as any exit does.
        if let Some(shown) = exited {
            Box::pin(self.exited(&pane, shown)).await?;
            return Ok(());
        }
        // A harness that asks whether to trust the window's folder is answered, or
        // the human told, as its window shows the question (`trust`).
        self.watch_trust(record, project, participant, &pane).await;
        // A Claude window where Claude Code's computer use is on, while ConsensFlow lacks
        // a permission it needs: the human is told once (`computer_use`). What fails to be
        // written down is the log's, not the launch's.
        if let Err(cause) = self.tell_computer_use_once(project, participant) {
            self.write_down(&cause);
        }
        // The adapter's `started` is an `async` function, and `.catch(...)`
        // made a promise of its own to wait on.
        let started = caught(planned.window.started()).await;
        let started = match started {
            // A window that never came up is given up, with its first message or
            // without one: a Codex chief opened again at rest after the app's restart
            // has none, and read "starting" for good (zeewin, 2026-10-19). What it
            // shows is asked first, for the screen goes with it, and its harness once
            // more after that: one that came up in that last moment is kept (Sol on
            // round 1 of the chief's restart).
            Err(error) => {
                let shown = self.look_at(&pane).await;
                match returning(planned.window.came_up()).await {
                    Some(native) => Ok(native),
                    None => {
                        let given_up = GivenUp {
                            pane,
                            failing,
                            error,
                            shown,
                        };
                        return self
                            .give_up_unstarted(record, project, participant, given_up)
                            .await;
                    }
                }
            }
            started => started,
        };
        record.window.borrow_mut().starts_given_up = 0;
        if self.forgotten(record) {
            return Ok(());
        }
        // A launch has not come up until its window is ready for work, and one that
        // is to say the role not until it has: a chief whose window was given up for
        // either, and is tried again, keeps its count of failures, so that its wait
        // grows and the human is told once. The human's Resume and switch clear it,
        // and so does the first readiness of a launch that has no role to say
        // ([`Dispatcher::came_up`]).
        if let (Ok(Some(native)), Some(conversation)) = (&started, conversation_id) {
            if !native.is_empty() && Some(native) != planned.native_session.as_ref() {
                self.seams
                    .ledger
                    .borrow_mut()
                    .bind_conversation(conversation, native)?;
            }
        }
        self.changed();
        Ok(())
    }

    /// The launch's plan, from its adapter, with its first message begun:
    /// none where the launch is over before its window is made
    /// ([`Dispatcher::launch_over`]), or why the launch failed. `conversation` is
    /// the chief's, which was looked into before its first message was chosen.
    /// `role` is the text the window starts with where it was asked for already
    /// ([`Dispatcher::compare_role`]), and `prelude` what goes into the window
    /// first where it was compared: a chief's was, before its first message was
    /// chosen, and a member's is, here, once its harness has said which
    /// conversation its window goes on with.
    #[allow(clippy::too_many_arguments)]
    async fn plan(
        self: &Rc<Self>,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        first: Option<&MessageView>,
        conversation: Option<ConversationView>,
        launch_id: &LaunchId,
        delivering: Option<&Delivering>,
        role: Option<RoleText>,
        prelude: Option<Prelude>,
    ) -> Result<Option<Planned>, EngineError> {
        // A member the update could not run on what it ran on (T-1202) opens no window on
        // its harness's default: its work goes back to the board, and the human hears why.
        if participant.role != "chief" && participant.needs_edit {
            self.needs_an_edit(record, project, participant, delivering)?;
            return Ok(None);
        }
        // A first message withdrawn while the launch waited (a continuation whose
        // task moved on, the ledger's) opens no window: the launch is over, and
        // what waits now is the next pass's.
        if let Some(first) = first {
            let message = self.seams.ledger.borrow().message(first.id)?;
            if message.is_none_or(|message| message.state != "queued") {
                self.changed();
                return Ok(None);
            }
        }
        // The task the session's conversation was last given, before this launch's first
        // message is begun (`ownership`).
        let before = if participant.role == "chief" {
            None
        } else {
            self.seams.ledger.borrow().task_given(participant.id)?
        };
        let begun = first
            .map(|first| self.seams.ledger.borrow_mut().begin_delivery(first.id))
            .transpose()?;
        // The stops this window owes are those asked before this look: a pause
        // that comes while it is prepared or opened is asked after it, and is
        // owed. Read in this turn, with the first message begun: the window
        // works on the task that message is about.
        let stop = self.seams.ledger.borrow().stop_of(participant.id)?;
        // A member's conversation is looked into now, which asks its harness and
        // may wait: a pause that comes during it is asked after the stop was read,
        // as one that comes while the window is prepared is, and the launch is over.
        let conversation = if participant.role == "chief" {
            conversation
        } else {
            let found = self.conversation_to_go_on_with(participant).await?;
            if self.launch_over(
                record,
                participant,
                delivering,
                stop,
                "a pause came while its harness was asked about its conversation",
            )? {
                return Ok(None);
            }
            match self.kept_apart(project, participant, found, before.as_ref(), first)? {
                Kept::On(conversation) => conversation,
                Kept::Held => {
                    // The task is held for the chief's word, which asks no stop (T-1199): the
                    // message the launch began goes back to wait for that word by hand, as
                    // `launch_over` gives back only a message a new stop took, and the launch
                    // is over.
                    if self.launch_over(
                        record,
                        participant,
                        delivering,
                        stop,
                        "its task waits for the chief's word: the conversation it follows cannot be gone on in",
                    )? {
                        return Ok(None);
                    }
                    if let Some(delivering) = delivering {
                        self.give_back(
                            delivering.message,
                            "its task waits for the chief's word: the conversation it follows cannot be gone on in",
                        )?;
                        self.seams
                            .ledger
                            .borrow_mut()
                            .launch_called_off(delivering.message)?;
                    }
                    return Ok(None);
                }
            }
        };
        // The task whose work the window holds: its first message's, or what its
        // conversation was last given where it goes on with one.
        let owner = if participant.role == "chief" {
            None
        } else {
            first.and_then(|first| first.task_number).or_else(|| {
                conversation
                    .as_ref()
                    .and(before.as_ref())
                    .map(|before| before.task.number)
            })
        };
        // A member's role is compared now that its conversation is known: one its
        // harness holds nothing of is not resumed, and has nothing to be told. Where
        // the role is to be said first, the message the launch was for is no first
        // message of its window: it goes back to its queue with its attempt, to go
        // in after the note that says the role. It is still what a launch that does
        // not come up fails ([`Dispatcher::launch_failed`]).
        let (begun, role, prelude) = if participant.role == "chief" {
            (begun, role, prelude)
        } else {
            let resumed = conversation
                .as_ref()
                .filter(|conversation| conversation.native_session.is_some());
            let compared = self.compare_role(project, participant, resumed, first.cloned())?;
            let begun = match self.displaced(participant, compared.prelude.as_ref()) {
                Some(displaced) => {
                    self.give_back(displaced.message, "its window is told its role first")?;
                    None
                }
                None => begun,
            };
            (begun, compared.role, compared.prelude)
        };
        let resume = conversation
            .as_ref()
            .and_then(|conversation| conversation.native_session.as_deref());
        // A harness that lost its adapter fails what came for it, and says why.
        let harness = participant.harness.as_deref().unwrap_or_default();
        self.require_adapter(harness)?;
        let Some(adapter) = self.seams.adapters.adapter(harness) else {
            return Ok(None);
        };
        // A chief whose last launch found its model gone starts on its harness's default.
        let started;
        let participant = if participant.role == "chief" {
            match self
                .start_on_default(record, project, participant, &*adapter, delivering)
                .await?
            {
                Some(chief) => {
                    started = chief;
                    &started
                }
                None => return Ok(None),
            }
        } else {
            participant
        };
        // A member runs on its own harness, model and effort (harness-models). A chief
        // runs on what its harness's record says it ran on last, or the model and effort
        // picked for it (CHF-05).
        let runs = if participant.role == "chief" {
            self.chief_runs(participant, &*adapter, resume).await
        } else {
            ChiefRuns::picked(participant)
        };
        // A session plays the role of the task it was started for; a member
        // or the chief its own.
        let text = begun
            .as_ref()
            .map(|begun| self.launch_text(begun, resume))
            .transpose()?;
        // What the window is given as it opens or posts once up; a window delivered
        // its first message is given none on its way (`FirstMessage`).
        let first_message = adapter.first_message();
        let delivered = first_message == FirstMessage::Delivered;
        // A chief whose project has specs on is told where its Specmint skill
        // is, and its window is given the skill there.
        let RoleText {
            specs,
            instructions,
        } = match role {
            Some(role) => role,
            None => self.role_text(project, participant, &*adapter)?,
        };
        // What a chief is told of its role by this launch, which a note tells it the change of
        // when the human changes it while the window is open (`role_news`).
        let told = (participant.role == "chief")
            .then(|| Told::of(project, specs.as_ref().map(ChiefSpecs::setting)));
        // The folder the window works in, as the system names it.
        let directory = workspace(project);
        // The program of the last window on its conversation was proven gone: nothing
        // of it writes on.
        let previous_gone = match &conversation {
            Some(conversation) => self.seams.ledger.borrow().program_gone(conversation.id)?,
            None => false,
        };
        let launch = Launch {
            id: launch_id,
            project: project.id,
            handle: &participant.handle,
            role: &participant.role,
            directory: &directory,
            resume,
            previous_gone,
            message: text.as_deref().filter(|_| !delivered),
            agent: if participant.role == "chief" {
                runs.agent()
            } else {
                Some(runs.member_agent(participant.designer))
            },
            instructions: &instructions,
            specs: specs.as_ref().map(ChiefSpecs::launch),
            // The size the project chose for its members' windows to compact at: every
            // member's launch is given it, whatever its harness, at launch and on resume
            // alike, and each adapter gives it its harness by its own route; the chief's
            // launch never is (2026-10-31). A size the project's row holds under the least
            // the ledger takes now (an earlier build took less) is given as that least: a
            // Claude Code window launched under it compacts again as soon as it can and ends
            // its turns with its own error, so it never reaches a harness. The row is not
            // changed (T-1439).
            compaction: project
                .compaction
                .filter(|_| participant.role != "chief")
                .map(launch_size),
            // The project leaves Claude Code's own advisor tool off until the human ticks it in
            // Project settings: every member's launch is told to switch it off, at launch and on
            // resume alike, and each adapter acts on it by its own harness (only Claude Code has
            // such a tool); the chief's launch never is, and keeps its harness's own setting
            // (2026-11-10).
            advisor_off: !project.advisor_tool && participant.role != "chief",
            first_message_ms: self.seams.limits.launch_ms,
            starts_given_up: record.window.borrow().starts_given_up,
        };
        // An `async` function: its answer, or its failure, reaches this
        // step a turn after it was made, though Pi's never waits.
        let prepared = returning(adapter.prepare(&launch))
            .await
            .map_err(|cause| EngineError::said("launch-failed", cause))?;
        if participant.role == "chief" {
            self.chief_launched(participant, &runs)?;
        }
        Ok(Some(Planned {
            window: prepared.window,
            keys: adapter.interrupt(),
            input: adapter.input(),
            servers: adapter.servers(),
            silent_paste: adapter.silent_paste(),
            // The name the harness's own programs go by (`codex-…`): the roster's kind
            // is `claude-code` for the harness whose binary is `claude`.
            harness: Harness::from_kind(harness)
                .map(Harness::as_str)
                .unwrap_or_default()
                .to_owned(),
            argv: prepared.argv,
            env: prepared.env,
            drop_env: prepared.drop_env,
            native_session: prepared.native_session,
            conversation,
            stop,
            carried: begun
                .iter()
                .flat_map(|begun| &begun.carried)
                .map(|row| row.id)
                .collect(),
            first: first_message,
            owed: text.filter(|_| delivered),
            prelude,
            first_instructions: adapter.keeps_first_instructions().then_some(instructions),
            told,
            owner,
        }))
    }

    /// Whether a launch's first message, which its window reads as it opens
    /// ([`FirstMessage::Opened`]), is handed over now, as its pane is asked for: as
    /// the ledger admits it, which marks it handed (the project is open, and nothing
    /// of its paste was withdrawn since it was begun). One it turns away goes back to
    /// its queue, unless it was withdrawn itself, and the launch is over. A launch
    /// whose window is delivered it once up hands nothing over now: it is admitted
    /// as it is delivered ([`FirstMessage::Delivered`]).
    fn handed_on_open(
        &self,
        delivering: Option<&Delivering>,
        planned: &Planned,
    ) -> Result<bool, EngineError> {
        let Some(first) = delivering.filter(|_| planned.first == FirstMessage::Opened) else {
            return Ok(true);
        };
        let handed = self
            .seams
            .ledger
            .borrow_mut()
            .admit(first.message, &planned.carried)?;
        if handed == HandOver::Admitted {
            return Ok(true);
        }
        let because = if handed == HandOver::Closed {
            "its project was closed before its window opened"
        } else {
            "what its paste held was withdrawn before its window opened"
        };
        self.give_back(first.message, because)?;
        Ok(false)
    }

    /// A window the host did not open: its token and files go, and the
    /// launch failed.
    #[allow(clippy::too_many_arguments)]
    fn not_opened(
        &self,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        delivering: Option<Delivering>,
        token: &str,
        launch_id: &LaunchId,
        (why, said): (Why, &str),
    ) -> Result<(), EngineError> {
        self.seams.credentials.revoke(token);
        self.seams.launch_files.forget(launch_id);
        // What the host said is its own: the log has it, and a note says the kind (T-1328).
        let said = self.startup_words(participant, Startup::new(Stage::Open, why), said);
        self.launch_failed(record, project, participant, delivering, &said)
    }

    /// Whether a launch that waited is over, with no window to open. A
    /// participant forgotten while its launch waits (it left, or its project was
    /// deleted) gets nothing more under its ids, whose rows may be gone. A project
    /// closed meanwhile opens no window, the chief's included: its first message,
    /// which would go in on its command line, waits for the Resume, never handed
    /// over (both reviews of 2026-10-16, round 4: a chief's handoff went into a
    /// window the Close then closed, and again after the Resume). A member's
    /// launch holds only while its first message is still on its way and no stop
    /// was asked of its window since `captured` was read, which is a pause that
    /// came in between: the message goes back to wait for the words that resume
    /// the task, with `because`. A task called off meanwhile took the message
    /// with it, and whoever told the window words that rode in it hears that the
    /// window never had them (`Ledger::launch_called_off`), in a project closed
    /// meanwhile too (Calliope's review of the merge, T-408).
    fn launch_over(
        &self,
        record: &Rc<Record>,
        participant: &ParticipantView,
        delivering: Option<&Delivering>,
        captured: Option<Stop>,
        because: &str,
    ) -> Result<bool, EngineError> {
        if self.forgotten(record) {
            return Ok(true);
        }
        if !self.in_open_project(participant.project_id)? {
            if let Some(delivering) = delivering {
                self.give_back(
                    delivering.message,
                    "its project was closed before its window opened",
                )?;
                // A task the human called off while it was closed took the message
                // with it, and said nothing then of the words that rode in it.
                self.seams
                    .ledger
                    .borrow_mut()
                    .launch_called_off(delivering.message)?;
            }
            return Ok(true);
        }
        let Some(delivering) = delivering.filter(|_| participant.role != "chief") else {
            return Ok(false);
        };
        let holds = {
            let ledger = self.seams.ledger.borrow();
            let on_its_way = ledger
                .message(delivering.message)?
                .is_some_and(|message| message.state == "delivering");
            on_its_way && ledger.stop_of(participant.id)? == captured
        };
        if holds {
            return Ok(false);
        }
        self.give_back(delivering.message, because)?;
        self.seams
            .ledger
            .borrow_mut()
            .launch_called_off(delivering.message)?;
        Ok(true)
    }

    /// What a window's first message reads, which `begin_delivery` told: a
    /// member's fresh session starts from nothing, so the brief its task's
    /// window received goes in first ([`Dispatcher::brief_given_first`]). A
    /// resumed conversation has it already.
    fn launch_text(&self, first: &Begun, resume: Option<&str>) -> Result<String, EngineError> {
        let text = self.delivery_text_of(&first.message, &first.carried)?;
        Ok(match self.brief_given_first(&first.message)? {
            Some(brief) if resume.is_none() => format!("{brief}{text}"),
            _ => text,
        })
    }

    /// What a member's fresh window is given ahead of `message`, its first, when
    /// that is not its task's own brief (a reopening, an answer): the brief it
    /// received, as it was received with what its paste carried, and a blank line
    /// ([`brief_first`]). A brief that was never received, held for the human's
    /// approval or not, is not the window's to read before it is; only a brief
    /// that arrived is found, so it is neither `message` nor one that rides in it.
    /// None for the chief's, and where no brief of the task was received. The
    /// paste of `message` in its record is behind it (`is_paste_of`).
    pub(crate) fn brief_given_first(
        &self,
        message: &MessageView,
    ) -> Result<Option<String>, EngineError> {
        let Some(number) = message
            .task_number
            .filter(|_| message.recipient_role != "chief")
        else {
            return Ok(None);
        };
        let cf = self.cf_for(message)?;
        let ledger = self.seams.ledger.borrow();
        let Some(thread) = ledger.task(message.project_id, number)? else {
            return Ok(None);
        };
        Ok(ledger
            .first_received(message.recipient_id, thread.task.id)?
            .map(|brief| brief_first(&brief.message, &brief.carried, &cf)))
    }

    /// A launch that did not come up. A member's first message fails with
    /// it, so its task fails and the requester hears why; a window the human
    /// opened with nothing to deliver tells them why it did not come. The
    /// chief's first message goes back to its queue with its attempt, and
    /// the chief is tried again, ever more slowly while it keeps failing;
    /// the human hears why once. Where a role was to be said first, a member's
    /// failing message is the one it went ahead of, and the note that says the
    /// role is withdrawn with it; a chief's stays in its queue (`role_change`).
    /// A member that needs an edit does not open: its tiered work goes back to the board
    /// for another member, what was on its way to it fails so its requester hears why,
    /// and a session's window the human opened, with nothing to deliver, says why it did
    /// not come.
    fn needs_an_edit(
        &self,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        delivering: Option<&Delivering>,
    ) -> Result<(), EngineError> {
        let because = crate::scheduler::needs_an_edit_because(
            participant.member.as_deref().unwrap_or(&participant.handle),
            participant.agent.as_deref(),
        );
        self.release_its_work(project, participant, &because, delivering.cloned())?;
        if delivering.is_none() {
            self.launch_failed(record, project, participant, None, &because)?;
        }
        Ok(())
    }

    /// A participant forgotten while it launched hears nothing and settles
    /// nothing.
    pub(crate) fn launch_failed(
        &self,
        record: &Rc<Record>,
        project: &ProjectView,
        participant: &ParticipantView,
        delivering: Option<Delivering>,
        because: &str,
    ) -> Result<(), EngineError> {
        if self.forgotten(record) {
            return Ok(());
        }
        if participant.role != "chief" {
            // What it was to say of the role first was written for a window that
            // never came up, and no queue of a member's would give it to another.
            self.withdraw_replacements(participant)?;
            match delivering {
                Some(delivering) => {
                    // One the role went ahead of waits still: it fails now, or
                    // the launch would be tried again at every pass.
                    self.begin_displaced(&delivering)?;
                    self.settle_failure(delivering, because, false)?;
                }
                None => {
                    self.seams.ledger.borrow_mut().note(
                        project.id,
                        &NewNote {
                            from: None,
                            to: "human".to_owned(),
                            task: None,
                            body: format!("@{} could not start: {because}.", participant.handle),
                        },
                    )?;
                    self.changed();
                }
            }
            return Ok(());
        }
        if let Some(delivering) = delivering {
            self.give_back(delivering.message, because)?;
        }
        let before = record.window.borrow().relaunch;
        let failures = before.map_or(0, |relaunch| relaunch.failures) + 1;
        let wait = RELAUNCH_MS
            .saturating_mul(2_i64.saturating_pow(failures - 1))
            .min(RELAUNCH_MAX_MS);
        record.window.borrow_mut().relaunch = Some(Relaunch {
            failures,
            at: self.now() + wait,
            // Until a window comes up, as a launch that fails for another reason after it.
            reopens: before.is_some_and(|relaunch| relaunch.reopens),
        });
        if failures == 1 {
            self.seams.ledger.borrow_mut().note(
                project.id,
                &NewNote {
                    from: None,
                    to: "human".to_owned(),
                    task: None,
                    body: format!(
                        "The chief could not start: {because}. What comes for the chief waits for it, and ConsensFlow tries again; you may also switch the chief."
                    ),
                },
            )?;
        }
        self.changed();
        Ok(())
    }

    /// A window said it is ready for a delivery: its launch came up, and a chief
    /// that failed to start before has its count of failures cleared, the next
    /// failure told the human again. A launch that is to say the role first has not
    /// come up until it has, and keeps its count (`role_change`).
    pub(crate) fn came_up(&self, record: &Record) {
        if record.delivery.borrow().prelude.is_none() {
            record.window.borrow_mut().relaunch = None;
        }
    }

    /// One look at a window: what its harness's record shows now, or why
    /// that could not be told. The window keeps whether the look said its
    /// agent's turn settled ([`Dispatcher::close_if_free`]).
    pub(crate) async fn observe(
        self: &Rc<Self>,
        _participant: &ParticipantView,
        record: &Rc<Record>,
    ) -> Result<Observed, String> {
        let Some(window) = record.window.borrow().window.clone() else {
            return Err("the window is closed".to_owned());
        };
        let observed = returning(window.observe()).await?;
        record.window.borrow_mut().settled = observed.settled;
        Ok(observed)
    }

    /// Whether a window has drawn its screen: it showed something, then held still for
    /// a moment (the pane host says how long what its screen shows has not changed). A host
    /// that cannot tell is taken as drawn. A window whose harness has no word of its
    /// own that it is at rest (`Window::idle_word`) is read at rest by the pacing's
    /// fallback, on its record and a quiet screen, and a screen held still is no
    /// harness that is up: Muse 1.4.14 held still six seconds as it started, before its
    /// first frame, and keys typed then were lost (T-946). Such a window is drawn only
    /// once its harness shows its own sign that it is up, its prompt
    /// (`Window::shows_prompt`), which a harness read by the fallback is to give.
    pub(crate) async fn drawn(self: &Rc<Self>, record: &Rc<Record>) -> bool {
        let (drawn, pane, window) = {
            let part = record.window.borrow();
            (part.drawn, part.pane.clone(), part.window.clone())
        };
        if drawn {
            return true;
        }
        let fallback = window.as_ref().is_some_and(|window| !window.idle_word());
        let snapshot = match pane {
            // `.catch(() => null)` made a promise of its own to wait on.
            Some(pane) => {
                let body = if fallback {
                    pane_body_drawn(&pane)
                } else {
                    pane_body(&pane)
                };
                returning(self.seams.host.request("pane.snapshot", body))
                    .await
                    .ok()
            }
            None => None,
        };
        let quiet = match snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.get(SCREEN_QUIET_MS))
        {
            None => true,
            Some(Value::Null) => false,
            Some(quiet) => quiet.as_f64().is_some_and(|quiet| quiet >= DRAWN_QUIET_MS),
        };
        let up = match (&window, &snapshot) {
            (Some(window), Some(snapshot)) if !window.idle_word() => {
                window.shows_prompt(&drawn_lines(snapshot))
            }
            (Some(window), None) if !window.idle_word() => false,
            _ => true,
        };
        let drawn = quiet && up;
        if drawn {
            record.window.borrow_mut().drawn = true;
        }
        drawn
    }

    /// A session's window closes with its task; its conversation stays, so a
    /// follow-up comes back on it. Says whether the window goes: one already
    /// closing had its kill, and one whose kill the pane host refused stays
    /// open, not closing, for the next close to try again.
    pub(crate) async fn retire(self: &Rc<Self>, record: &Rc<Record>) -> Result<bool, EngineError> {
        let pane = {
            let mut part = record.window.borrow_mut();
            if part.retiring {
                return Ok(true);
            }
            part.retiring = true;
            part.pane.clone()
        };
        let killed = match pane {
            Some(pane) => self.kill(record, &pane).await?,
            None => false,
        };
        {
            let mut part = record.window.borrow_mut();
            if !killed {
                part.retiring = false;
            } else if part.retiring {
                // Still closing: its exit has not come yet, which it may before the answer.
                part.kill_taken = Some(self.now());
            }
        }
        self.changed();
        Ok(killed)
    }

    /// A window closing by `retire` whose exit the host still owes, looked at in a pass:
    /// once [`EXIT_WAIT`] has gone by since the host took its kill, its exit is settled
    /// as the engine's own close settles one (`kill_and_settle`), knowing nothing of how
    /// it ended, and the host's word, should it come later, is kept as for that close
    /// (`heard_exit`). Without the bound, a close whose exit never came left the window
    /// retiring for good: never looked at, reused or released (Astraeus on turn-end-min,
    /// T-487).
    pub(crate) async fn exit_overdue(
        self: &Rc<Self>,
        record: &Rc<Record>,
    ) -> Result<(), EngineError> {
        let pane = {
            let part = record.window.borrow();
            let overdue = part
                .kill_taken
                .is_some_and(|taken| self.now() - taken >= EXIT_WAIT.as_millis() as i64);
            match (overdue, &part.pane) {
                (true, Some(pane)) => pane.clone(),
                _ => return Ok(()),
            }
        };
        // Boxed, as `kill_and_settle`'s: an exit may close windows in turn.
        Box::pin(self.exited(&pane, Shown::default())).await
    }

    /// Ends the window whose open went unanswered ([`WindowPart::abandoned`]): whether
    /// none is left. The host's kill ends it where it has opened, its programs and what
    /// they started, and where its open has not reached the host yet, the host never
    /// opens it (`cf_panes`' cancelled windows); either way the window is nothing more
    /// of the participant's. A kill the host refused, or did not answer, leaves it, and
    /// the participant's next step asks again.
    pub(crate) async fn end_abandoned(
        self: &Rc<Self>,
        record: &Rc<Record>,
    ) -> Result<bool, EngineError> {
        let Some(Abandoned { pane, told }) = record.window.borrow().abandoned.clone() else {
            return Ok(true);
        };
        let killed = returning(self.seams.host.kill(&pane)).await;
        let error = match killed {
            Ok(Killed::Killed) => {
                let mut window = record.window.borrow_mut();
                if window
                    .abandoned
                    .as_ref()
                    .is_some_and(|abandoned| abandoned.pane == pane)
                {
                    window.abandoned = None;
                }
                return Ok(true);
            }
            Ok(Killed::Refused { error }) => error,
            Err(error) => error.message,
        };
        // Said once: the kill is asked again at every step until the host takes it.
        if !told {
            if let Some(abandoned) = record.window.borrow_mut().abandoned.as_mut() {
                abandoned.told = abandoned.pane == pane;
            }
            self.trace_window(record, WindowEvent::KillFailed { error: Some(error) })?;
        }
        Ok(false)
    }

    /// Kills a window: whether the pane host took the kill; the trace says
    /// why not, and a trace that cannot be told fails the kill.
    async fn kill(self: &Rc<Self>, record: &Rc<Record>, pane: &Pane) -> Result<bool, EngineError> {
        // Told before the host is asked: its exit can come before its answer.
        record.window.borrow_mut().sent_to.push(pane.clone());
        // `.catch(...)` made a promise of its own to wait on, a turn after
        // the host's answer.
        let killed = returning(self.seams.host.kill(pane)).await;
        let error = match killed {
            Ok(Killed::Killed) => return Ok(true),
            Ok(Killed::Refused { error }) => error,
            Err(error) => error.message,
        };
        self.trace_window(record, WindowEvent::KillFailed { error: Some(error) })?;
        Ok(false)
    }

    /// A member's window closes once it has no task in hand, unless the human
    /// opened it or a message is still on its way in. A follow-up that waits
    /// on the board for what it needs is not in hand: the session is its
    /// already, but its window opens again, on its conversation, when the
    /// follow-up goes, and stays closed meanwhile. One the human opened and
    /// has hidden since waits for its agent's turn to end first. The window of
    /// a task that is done also waits for its screen to be drawn
    /// ([`Dispatcher::still_drawing`]); the task's result is taken meanwhile as ever,
    /// and the next step tries the close again. Says whether the step is done with the
    /// window: it closed, or it waits for its screen. One gone already has nothing to close.
    pub(crate) async fn close_if_free(
        self: &Rc<Self>,
        record: &Rc<Record>,
    ) -> Result<bool, EngineError> {
        let kept = {
            let part = record.window.borrow();
            part.pane.is_none() || part.pinned || (part.hidden && !part.settled)
        } || record.delivery.borrow().delivering.is_some()
            || self.seams.ledger.borrow().has_task_in_hand(record.id)?;
        if kept {
            // The wait is over with the task that is in hand again (resumed, or reopened):
            // the next time it is done, the window waits its own time. Not while the task
            // the window was given is still done: what keeps it is the next task of its
            // session, queued behind it, which waits for the same screen (`ownership`).
            if !self.given_task_finished(record)? {
                record.window.borrow_mut().found_free = None;
            }
            return Ok(false);
        }
        if self.still_drawing(record).await? {
            return Ok(true);
        }
        self.retire(record).await?;
        Ok(true)
    }

    /// Whether the task the window of `record` was last given is finished with its result:
    /// done, or accepted since (the chief that takes the result within seconds does not
    /// end the wait). A task message queued for it since is not given yet.
    fn given_task_finished(&self, record: &Record) -> Result<bool, EngineError> {
        Ok(self
            .seams
            .ledger
            .borrow()
            .task_given(record.id)?
            .is_some_and(|thread| matches!(thread.task.state.as_str(), "done" | "accepted")))
    }

    /// Whether the window of `record` is to wait before it closes because its screen is
    /// still being drawn: its task is done or accepted (the answer is the chief's already; a person
    /// watching the window sees it go mid-typing, frozen where it was, if it closes now:
    /// Devin's agent ends its turn some seconds before its screen has typed the answer
    /// out, T-1038), and what its screen shows changed within the last [`DRAWN_QUIET_MS`] (a
    /// screen printed again as it is, is still: one that repaints without end, as Muse Code
    /// 1.4.14 can, is not held the whole bound, T-1083). A window
    /// whose task is not done closes at once, whatever its screen shows: a pause, a cancel,
    /// a failure or a hold for quota take the task out of the window's hands, and a
    /// crash or the human closing the window is no close of the engine's to wait with. So
    /// does one whose host cannot say how long its screen has been quiet, or says it has
    /// shown nothing. The wait is bounded: it ends [`STILL_BOUND_MS`] after the window was first
    /// found free, a screen that never rests notwithstanding. The host is asked at each
    /// step while the window waits.
    pub(crate) async fn still_drawing(
        self: &Rc<Self>,
        record: &Rc<Record>,
    ) -> Result<bool, EngineError> {
        // A compaction its own extension asked for holds the window first, and its screen
        // is waited for after it (the wait of the screen begins when the compaction's ends).
        if self.given_task_finished(record)? && self.compaction_held(record) {
            return Ok(true);
        }
        if !self.given_task_finished(record)? {
            return Ok(false);
        }
        let now = self.now();
        let (since, pane) = {
            let mut part = record.window.borrow_mut();
            (*part.found_free.get_or_insert(now), part.pane.clone())
        };
        let Some(pane) = pane else {
            return Ok(false);
        };
        if now - since >= STILL_BOUND_MS {
            return Ok(false);
        }
        let quiet = self.seams.host.quiet(&pane).await;
        // A window that went while the host answered is the next step's.
        if record.window.borrow().pane.as_ref() != Some(&pane) {
            return Ok(true);
        }
        Ok(quiet
            .ok()
            .flatten()
            .is_some_and(|quiet| quiet < DRAWN_QUIET_MS))
    }

    /// Whether the window of `record`, whose task is done, is held open for a compaction its
    /// harness's own extension said it asked for at the window's rest for the project's size
    /// ([`Window::compaction_asked`]: Pi's, T-1475): a harness whose conversation the close
    /// would cut under a summary that takes tens of seconds (a Pi member whose task is one
    /// turn is at rest as its result is taken, and its summary 39 to 80 seconds long, which
    /// the 10 seconds of [`STILL_BOUND_MS`] cut, the signal of the close aborting it). Held
    /// while the word stands, at most [`COMPACTION_HOLD_MS`] from the engine's first sight of
    /// it, on the clock that does not step, so that no step of the wall clock lengthens or
    /// shortens it; then the close goes as it does without one. The word's own time is compared
    /// with no clock. A word that is not the one seen (another time) is a new first sight. Only a
    /// word that is absent holds nothing and clears the first sight: a word that is there and
    /// cannot be read this time keeps what the engine knows (a first sight saved stays; with
    /// none, this look is the first, and the window is held for it), so that a file that stays
    /// unreadable is bounded by the first look that met it. Only the ordinary close of a done task's window comes here: a pause, a cancel, a
    /// failure, the human ending the session and the app quitting close at once, and a task in
    /// hand again (a reopen) keeps its window, which its extension's own limit of the same
    /// length frees of its compaction.
    fn compaction_held(&self, record: &Record) -> bool {
        let word = self
            .window_of(record)
            .0
            .map_or(CompactionWord::Absent, |window| window.compaction_asked());
        let mono = self.seams.time.monotonic_ms();
        let mut part = record.window.borrow_mut();
        let seen = match (word, part.compaction_seen) {
            (CompactionWord::Absent, _) => {
                part.compaction_seen = None;
                return false;
            }
            (CompactionWord::Unreadable, Some(seen)) => seen,
            (CompactionWord::Unreadable, None) => CompactionSeen {
                asked: None,
                since: mono,
            },
            // The word read after a look that could not: the same first look.
            (CompactionWord::Asked(at), Some(seen)) if seen.asked.is_none_or(|was| was == at) => {
                CompactionSeen {
                    asked: Some(at),
                    since: seen.since,
                }
            }
            (CompactionWord::Asked(at), _) => CompactionSeen {
                asked: Some(at),
                since: mono,
            },
        };
        part.compaction_seen = Some(seen);
        mono - seen.since < COMPACTION_HOLD_MS
    }

    /// Closes a window whose exit is the engine's own (a switch, a Close, a
    /// chief that could not take its first message): the exit settles what
    /// the window was doing, as any exit does, but a chief's does not close
    /// its project. Says whether the window goes: one whose kill the pane
    /// host refused stays as it was, and no exit is made up for it. An exit
    /// that cannot be settled fails the close, which is the caller's to hear
    /// (the host's own exit event, with no caller, is told to the log).
    ///
    /// An exit the host tells before the kill is answered is settled as it comes,
    /// as the engine's own; where it paused the window's task as the suspension's,
    /// the kill's answer decides: a kill that was refused takes the mark back, for
    /// that exit was the window's own ([`Killing`]; both reviews of 2026-10-16: a
    /// worker that crashed while its Close's kill was pending, and whose kill then
    /// failed, was taken for one the Close closed, and its task went on with the
    /// Resume). An exit that comes with a kill that was taken is the kill's: the
    /// host, asked to end the window, says it has ended, and no proof it sends
    /// tells a program that ended by itself in that moment from one it ended.
    pub(crate) async fn close_own(
        self: &Rc<Self>,
        record: &Rc<Record>,
        pane: &Pane,
    ) -> Result<bool, EngineError> {
        self.close_own_for(record, pane, None, None).await
    }

    /// [`Dispatcher::close_own`], `why` the window is closed and the message it settles
    /// itself where a give-up closes it.
    async fn close_own_for(
        self: &Rc<Self>,
        record: &Rc<Record>,
        pane: &Pane,
        why: Option<String>,
        settling: Option<i64>,
    ) -> Result<bool, EngineError> {
        record.window.borrow_mut().own_exit = Some(OwnExit {
            pane: pane.clone(),
            why,
            settling,
        });
        let went = self.kill_and_settle(record, pane).await;
        let mut window = record.window.borrow_mut();
        if window.own_exit_of(pane).is_some() {
            window.own_exit = None;
        }
        went
    }

    /// [`Dispatcher::close_own`], its kill's answer waited for no longer than
    /// [`KILL_ANSWER_WAIT`]: none where the host gave none by then, and the close goes
    /// on apart, its exit settled whenever it comes and what it fails with told to the
    /// log, as the host's own exit's is. The pane host's kill is a request with no
    /// deadline (the chief's restart, 8bb0a465, left the kill unawaited for it).
    ///
    /// The one rule of every member's give-up of a window: one unfit for work
    /// ([`Dispatcher::give_up_unfit`]), one that never came up
    /// ([`Dispatcher::give_up_unstarted`]) and one that could not be told its role
    /// ([`Dispatcher::abandon_launch`]). Each closes the window with this before the
    /// message the launch was for goes, so that the exit pauses the task and words told
    /// to the window wait for its resume, said to its requester and the human; a kill
    /// refused, or not answered in its time, leaves no exit, and the message fails as
    /// before. None is held for a kill that never answers. A chief's give-up waits for
    /// its kill ([`Dispatcher::close_own`]). `why` the window is given up is what the
    /// words told to it are said to wait for the resume for, in the note of the pause
    /// its exit makes; `settling` the message the give-up settles itself, which they are
    /// not named with there.
    pub(crate) async fn close_own_within(
        self: &Rc<Self>,
        record: &Rc<Record>,
        pane: &Pane,
        why: &str,
        settling: Option<i64>,
    ) -> Result<Option<bool>, EngineError> {
        let (this, held, gone, why) = (
            Rc::clone(self),
            Rc::clone(record),
            pane.clone(),
            why.to_owned(),
        );
        let closing = begin(&*self.seams.spawn, async move {
            this.close_own_for(&held, &gone, Some(why), settling).await
        })
        .await;
        match select(closing, self.seams.time.sleep(KILL_ANSWER_WAIT)).await {
            Either::Left((went, _)) => went.map(Some),
            Either::Right(((), closing)) => {
                let this = Rc::clone(self);
                self.seams.spawn.spawn(Box::pin(async move {
                    if let Err(cause) = closing.await {
                        this.write_down(&cause);
                    }
                }));
                Ok(None)
            }
        }
    }

    async fn kill_and_settle(
        self: &Rc<Self>,
        record: &Rc<Record>,
        pane: &Pane,
    ) -> Result<bool, EngineError> {
        if !record.window.borrow().retiring {
            // The kill is marked before anything is awaited, the look at the screen
            // included: an exit that comes while the Close looks is the Close's, as one
            // that comes while its kill is answered is, and a kill that then fails takes
            // the mark off the task that exit paused (resume.rs, both reviews of 2026-10-16
            // and the re-check of 2026-10-25).
            record.window.borrow_mut().killing.push(Killing {
                pane: pane.clone(),
                paused: None,
            });
            // The exit is settled here, and says nothing of the screen the window ends
            // with: where it was asking, the human's yes in it is read off the screen now.
            self.bank_yes_before_close(record, pane).await;
            let killed = self.kill(record, pane).await;
            let taken = matches!(killed, Ok(true));
            let paused = {
                let mut window = record.window.borrow_mut();
                if taken {
                    window.killing.retain(|killing| killing.pane != *pane);
                    None
                } else {
                    let at = window
                        .killing
                        .iter()
                        .position(|killing| killing.pane == *pane);
                    at.and_then(|at| window.killing.remove(at).paused)
                }
            };
            if !taken {
                // Refused: an exit that came meanwhile was the window's own.
                if let Some((project, number, stop)) = paused {
                    self.seams
                        .ledger
                        .borrow_mut()
                        .unmark_pause(project, number, stop)?;
                }
                return killed;
            }
        }
        let open = record.window.borrow().pane.as_ref() == Some(pane);
        if open {
            // An exit may close windows in turn: boxed, so the futures of
            // closing and exiting are not each the other's inside. The engine
            // closed it, so it knows no more of what it showed than it did.
            Box::pin(self.exited(pane, Shown::default())).await?;
        }
        Ok(true)
    }
}

/// The folder a window of `project` works in: the project's, as the system names it, its
/// links resolved (a Mac's `/var/folders/…` is `/private/var/folders/…`, and `/tmp` is
/// `/private/tmp`). ConsensFlow's folder for a window is so the folder its harness has:
/// a window's program is given its folder only as its working folder, which the system
/// resolves (no `PWD` reaches it), so every harness already saw, and wrote in its records,
/// the real path (T-783, T-1036). The launch is prepared for that folder too. It changes
/// nothing a harness sees, and fixes no screen of Devin's (T-724's finding 1, T-1038). On
/// Windows a folder is as the project names it, its real path being a verbatim one
/// programs do not take (`cf_base::file::real_path`).
fn workspace(project: &ProjectView) -> String {
    cf_base::file::real_path(std::path::Path::new(&project.directory))
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kills_answer_is_waited_for_three_times_the_grace_the_host_gives_a_close() {
        assert_eq!(KILL_ANSWER_WAIT, CLOSE_GRACE * 3);
        assert!(KILL_ANSWER_WAIT > CLOSE_GRACE + Duration::from_secs(5));
    }
}
