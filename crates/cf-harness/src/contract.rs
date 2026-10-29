//! What the engine asks of a harness: prepare a launch, and of the window
//! opened on it, whether it is ready, a delivery into it, and a look at it.
//!
//! The engine runs on one thread (step 3.5: tokio's `current_thread`), so
//! what may wait (on the pane host, a harness's server, a child, a timer) is
//! a future that need not be `Send`. A window keeps what JavaScript kept in
//! its `launch` bag, in cells never borrowed across an await, behind `&self`:
//! the engine may close a window while a delivery into it still waits.

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cf_proto::agents::Harness;
use cf_proto::panes::{HumanInput, Servers};
use cf_proto::specs::Variant;
use serde_json::Value;

use crate::records::{
    Boundary, Item, Monitor, Options, Pending, Quota, Reading, Record, Role, Settlement,
};
/// What an input box on the screen holds ([`Window::input_box`]).
pub use crate::shared::input_box::InputBox;

/// What waits, on the engine's one thread.
pub type Work<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// A harness's side of the engine: how its windows are launched, whether a
/// conversation of its can be gone on with, and how one is interrupted. A
/// conversation's record with no window open is the engine's own look through
/// [`Records`], the same for every harness.
pub trait Adapter {
    /// Prepares a window on `launch`: writes the files it runs with and
    /// says how it opens, or why it cannot (the launch then fails with that
    /// sentence).
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>>;

    /// Whether a window can be opened again on its conversation `session`, as
    /// the harness's own record of it says: yes where it holds the conversation,
    /// no where it holds nothing a resume could find (Codex writes a thread's
    /// record at its first turn, and a window resumed on a thread with none ends
    /// at once, which closes a chief's project), and none where it cannot tell
    /// (its record cannot be read, or says nothing of whether there is anything
    /// to go on with). Only no opens the window afresh; the engine goes on with
    /// the conversation on a yes and on a none, as it did before it asked. So a
    /// no is for a harness that is sure of it: a conversation wrongly said to be
    /// empty is left behind.
    fn resumable<'a>(&'a self, _session: &'a str) -> Work<'a, Option<bool>> {
        Box::pin(async { None })
    }

    /// The keys that interrupt a turn in this harness's window
    /// (`adapter.interrupt`): Escape once, unless the harness says more.
    fn interrupt(&self) -> Interrupt {
        Interrupt {
            presses: 1,
            close_after: None,
        }
    }

    /// How the harness keeps its own processes (its MCP servers, its helpers),
    /// which the pane host tells a turn's work from them by (`pane.mark`'s
    /// `servers`): what a server is, and so what it starts, is the harness's and
    /// no work a turn left, to be listed or ended. Where it is not known the host
    /// still leaves alone what it can prove the harness's (a program named for the
    /// harness that runs beside its own binary, and what descends from one), and
    /// takes for the turn's only what it can prove: a shell the harness started after
    /// the mark and what the shell started, by a chain of processes that run. What it
    /// cannot trace so (a job detached with `nohup … &` from a shell that then ended)
    /// and a server that begins after the mark are left alone, and are not named.
    ///
    /// Only what someone has looked at is said: what a window's tree holds with a
    /// command of its agent running (`ps -axo pid,pgid,ppid,command`). Claude Code
    /// and Pi keep their servers in the group of the window's program, however
    /// late each begins, and run every command their agents start in a group of its
    /// own ([`Servers::InGroup`]). Codex's supervisor opens its servers with the
    /// window, each in a group of its own, under the harness's own process, which
    /// starts the agent's commands ([`Servers::AtLaunch`]). Windows has no process
    /// groups to tell by, and OpenCode and Devin have not been probed.
    fn servers(&self) -> Servers {
        Servers::Unknown
    }

    /// Whether a resumed conversation keeps the instructions it began with. Every
    /// launch gives a window its role text ([`Launch::instructions`]), and a
    /// harness that takes it again at each launch (Claude Code, OpenCode, Pi,
    /// Devin) needs nothing more to read a role that changed since its conversation
    /// began. One that keeps the first instructions of the conversation it resumes
    /// reads the new text and does not use it: a change never reaches it, and
    /// whoever launches it must say the new text itself, as a message.
    fn keeps_first_instructions(&self) -> bool {
        false
    }

    /// Whether every window of this harness is logged into the one account, so
    /// that a window whose turn gets through after a refusal for quota says the
    /// quota is back for every other window of it: a login in one is a login in
    /// all, and a spent quota is spent for all. Not, where nobody has looked: Pi
    /// and OpenCode log into a provider for each model, so two of their windows
    /// may be on two accounts, and Codex and Devin have not been probed for a
    /// login changed under a window that is open.
    fn one_account(&self) -> bool {
        false
    }

    /// How a window is given its first message ([`FirstMessage`]).
    fn first_message(&self) -> FirstMessage;

    /// How its windows take the words they are handed: over a channel of the
    /// harness's own, or pasted into the window's input box ([`Input`]).
    fn input(&self) -> Input {
        Input::Channel
    }

    /// Where the window of a chief given specs finds the Specmint skill of
    /// `variant`, asked before the window is prepared, as its role text names
    /// the file. The copy the human installed where this harness looks for
    /// skills, if it is the chosen variant: used where it is. Otherwise the
    /// copy ConsensFlow keeps for this harness, at the place [`Adapter::prepare`]
    /// will have it written: the same place for every launch, so that the role
    /// text that names it is the same from one launch to the next. An installed
    /// copy of any other variant is no copy of this one, and is left alone.
    fn specmint(&self, variant: Variant) -> Result<Skill, String>;
}

/// Where a window finds the Specmint skill of its chief's variant: the file
/// that is its `SKILL.md`, which every harness can be told to read, and which
/// is next to the `references` (and `assets`) it names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skill {
    /// The human's own, installed where the harness looks for skills itself:
    /// the harness has it, so nothing is registered and nothing is written.
    Global(String),
    /// ConsensFlow's, written when the window is prepared where the harness has
    /// none: in a folder of its own, outside every launch's, that lasts from one
    /// launch to the next.
    Own(String),
}

impl Skill {
    /// The path of the skill's `SKILL.md`.
    pub fn path(&self) -> &str {
        match self {
            Skill::Global(path) | Skill::Own(path) => path,
        }
    }
}

/// What a chief's launch is given of specs: the variant it plans with, and
/// where the skill of it is ([`Adapter::specmint`]).
#[derive(Debug, Clone, Copy)]
pub struct Specs<'a> {
    pub variant: Variant,
    pub skill: &'a Skill,
}

/// A harness's interrupt as keys into its window: Escape `presses` times in a
/// row, and where those presses open a dialog at a turn that has just ended,
/// once more `close_after` later, which closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Interrupt {
    pub presses: u8,
    pub close_after: Option<Duration>,
}

/// How a window is given its first message. None is on its command line: what
/// is there is read by every process on the machine (`ps` is set-uid root on
/// macOS), and a kill or wait by pattern whose words a message holds matches the
/// window (T-432: a SIGINT to "xtask plants" closed two workers' windows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstMessage {
    /// In a file of the launch's own that the command line names, read as the
    /// window opens ([`Launch::message`]), and handed over as the window's pane is
    /// asked for: Devin's `--prompt-file`.
    Opened,
    /// Posted through the harness's own server once the window is up, which asks
    /// [`PaneHost::admits`] then ([`Window::started`]): OpenCode's.
    Started,
    /// Handed over as any later message is, by the window's own
    /// [`Window::deliver`], once the window is past any question of trusting its
    /// folder and has drawn its screen: Claude Code's pasted (its record wraps it
    /// as it wraps every paste), Pi's through its extension's inbox, Codex's
    /// through its broker. The launch is given no [`Launch::message`].
    Delivered,
}

/// How a harness's windows take the words they are handed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Input {
    /// Over a channel of the harness's own (Codex's broker, OpenCode's server,
    /// Pi's inbox): nothing handed over waits in the window's input box, and no
    /// key the engine presses sends it.
    #[default]
    Channel,
    /// Pasted into the window's input box, and sent by an Enter (Claude Code,
    /// Devin): what an Enter withheld leaves in the box, `clear` takes out, the
    /// keys that empty the box and do no harm to an empty one.
    Pasted { clear: &'static [u8] },
}

/// A window to launch (`adapter.prepare`'s argument).
#[derive(Debug, Clone)]
pub struct Launch<'a> {
    pub id: &'a LaunchId,
    /// The participant's project and handle, which Pi names its window by.
    pub project: i64,
    pub handle: &'a str,
    /// `chief`, `worker`, `advisor`, `reviewer`, `tester` or `designer`.
    pub role: &'a str,
    /// The project's folder, where the window opens.
    pub directory: &'a str,
    /// The conversation to open again, when the participant has one.
    pub resume: Option<&'a str>,
    /// The program of the participant's last window was proven gone when that
    /// window ended: the pane host named no program it could not end. False
    /// where it named one, and where the engine never heard how it ended (a
    /// window of a daemon before this one): what the record holds then may still
    /// be written to by a process ConsensFlow does not hold.
    pub previous_gone: bool,
    /// The window's first message, as the engine wrote it, where its harness takes
    /// it as its window opens or once it is up ([`FirstMessage::Opened`],
    /// [`FirstMessage::Started`]): none for a harness whose windows are delivered
    /// their first message.
    pub message: Option<&'a str>,
    /// The agent it runs on: none for a chief from before every chief had one.
    pub agent: Option<Agent<'a>>,
    /// The role's text.
    pub instructions: &'a str,
    /// The specs a chief plans with, when its project has them on: only a
    /// chief's launch is given any, and with none the launch is as it always was.
    pub specs: Option<Specs<'a>>,
    /// How long the window has to show its first message, in milliseconds: the
    /// engine's limit for a launch (`CONSENSFLOW_LAUNCH_TIMEOUT_MS`). What a
    /// window waits for before that message can show, such as the conversation
    /// its harness names, ends by it.
    pub first_message_ms: i64,
    /// How many of the participant's windows in a row were given up before this
    /// one for not coming up in the time they had: a harness whose window takes
    /// as long as it takes (a Codex resuming a long thread) gives this one longer.
    pub starts_given_up: u32,
}

/// The model a window runs on and the levels its harness reads, as the
/// launch gives them: `Some("")` is an empty one, which a window's
/// arguments take for none (`if (model)`), and OpenCode's first message is
/// refused for (`variant: ''`).
#[derive(Debug, Clone, Copy, Default)]
pub struct Agent<'a> {
    pub model: Option<&'a str>,
    pub effort: Option<&'a str>,
    pub thinking: Option<&'a str>,
    /// An image agent: Codex on its own default model, whose image tool
    /// draws, naming no model or effort of its own.
    pub designer: bool,
}

/// A launch's id: a uuid in lowercase, the name of the folders its files are
/// written in.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LaunchId(String);

impl LaunchId {
    /// The launch id `text` is, if it is one.
    pub fn new(text: &str) -> Option<Self> {
        let groups: Vec<&str> = text.split('-').collect();
        let shaped = groups.iter().map(|group| group.len()).eq([8, 4, 4, 4, 12])
            && text
                .bytes()
                .all(|byte| byte == b'-' || byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        shaped.then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// How a prepared window opens.
pub struct Prepared {
    pub argv: Vec<String>,
    /// What its environment adds, in the order its harness built it.
    pub env: Vec<(String, String)>,
    /// The keys it must not inherit.
    pub drop_env: Vec<String>,
    /// The conversation it opens on, where ConsensFlow names it.
    pub native_session: Option<String>,
    pub window: Rc<dyn Window>,
}

/// What a yes to a harness's question about trusting a window's folder trusts: the
/// folder, as the system names it, and whether it is the project's folder or one
/// below it, which ConsensFlow answers for; one outside it is the human's. And the
/// nearest folder above the project's whose `.mcp.json` Claude Code reads as well and
/// names a server the project's settings neither use nor leave out, where there is one: its dialog of a new server asks of that file's servers as of
/// the project's own, and they are the human's ([`Question::Servers`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trusts {
    pub folder: String,
    pub inside: bool,
    pub servers_above: Option<String>,
}

/// The servers of `.mcp.json` the settings of a window's folder say it uses, and those
/// they say it leaves out, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ServersChosen {
    pub used: Vec<String>,
    pub left_out: Vec<String>,
}

/// Whether `key`, a key toward a yes ([`Question`]), may be pressed again into a screen
/// that did not move after it: Down, which only moves a cursor. Enter answers whatever
/// is under the cursor by then, the next question perhaps, and Up wraps from a
/// dialog's first choice to its last (T-415).
pub fn presses_again(key: &[u8]) -> bool {
    key == crate::shared::trust::DOWN
}

/// Whether `key`, a key toward a yes ([`Question`]), answers the question: Enter.
pub fn confirms(key: &[u8]) -> bool {
    key == crate::shared::trust::ENTER
}

/// What a window's last lines show of its harness's question about trusting its
/// folder, where they show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// The key a person presses next toward a yes: Down, or Enter once the choice is
    /// on yes.
    Toward(&'static [u8]),
    /// The question, its choice drawn where the reader cannot place it (a cursor
    /// drawn otherwise, or a redraw caught halfway): no key is known.
    Unplaced,
    /// The question asked of a folder whose trust is already accepted, by a folder
    /// above it, where the folder's own settings grant more than trusting it does
    /// (Claude Code 2.1.309: tools its `.claude/settings.json` pre-approves, folders it
    /// adds, commands that mint the headers of `.mcp.json`'s servers), and the key a
    /// person presses next toward its yes where its choice is placed. Its yes lets them
    /// apply without asking, and records the folder [`Trusts`] names as trusted: it is
    /// answered as the first question is, only where that folder is the project's.
    Grants(Option<&'static [u8]>),
    /// Claude Code's dialog of a server of `.mcp.json` it was never told to use, asked
    /// once the folder is trusted (2.1.309, T-383), and the key a person presses next
    /// toward using it where its choice is placed. Using one starts its command and is
    /// kept in the local settings of the window's folder; ConsensFlow approves it where
    /// the folder [`Trusts`] names is the project's and no folder above the project's
    /// has servers of its own the project never settled (the owner's decision of
    /// 2026-10-23).
    Servers(Option<&'static [u8]>),
}

/// A window open on its harness.
pub trait Window {
    /// Its pane opened, the window's own process `pid` when the host knew it.
    fn opened(&self, pid: Option<u32>);
    /// The window shows `session` now (a `/clear` or `/resume` in it): later
    /// looks follow it there.
    fn follow(&self, session: &str);
    /// The conversation the window started on, where its harness says it
    /// itself; none where ConsensFlow named it. A window that waits for its
    /// harness to say it gives up by [`Launch::first_message_ms`], if not
    /// before. A window whose first message goes in once it is up (OpenCode's,
    /// through its server) hands it over only if `host` admits it then
    /// ([`PaneHost::admits`]).
    fn started<'a>(&'a self, host: &'a dyn PaneHost) -> Work<'a, Result<Option<String>, String>>;
    /// Once [`Window::started`] gave up, and right before the engine closes the
    /// window: whether its harness says it came up since, with the conversation it
    /// started on where it says that itself (`Some(None)` where ConsensFlow named
    /// it). None where it has not, and where the harness has no such look (the
    /// default): a window that came up in that last moment is kept.
    fn came_up(&self) -> Work<'_, Option<Option<String>>> {
        Box::pin(async { None })
    }
    /// What a yes to its harness's question about trusting the window's folder would
    /// trust, where the harness may ask one before it starts (Claude Code does in a
    /// git repository: the repository's root). None where it never asks (the
    /// default).
    fn trusts(&self) -> Option<&Trusts> {
        None
    }
    /// That question, where the window's last screen lines `screen` show it: the
    /// key a person presses next to answer yes (Down, or Enter once the choice is on
    /// yes), or that the reader cannot place its choice; none where they show no such
    /// question.
    fn toward_trust(&self, _screen: &[String]) -> Option<Question> {
        None
    }
    /// Whether `screen` (the window's last lines) is its harness's prompt: the
    /// window is past any question it asks as it starts.
    fn shows_prompt(&self, _screen: &[String]) -> bool {
        false
    }
    /// The servers its harness's dialog of new servers on `screen` (the window's last
    /// lines) would use, as its lines name them; none where they show none.
    fn trust_servers(&self, _screen: &[String]) -> Vec<String> {
        Vec::new()
    }
    /// The servers the settings of the window's folder say it uses and leaves out,
    /// which its harness writes when its dialog of new servers is answered; none where
    /// it keeps no such settings.
    fn servers_chosen(&self) -> ServersChosen {
        ServersChosen::default()
    }
    /// Whether `screen` (the window's last lines) names that question's choices, as
    /// the question does and a conversation that quotes it may: a screen at its
    /// prompt that still names them may never have shown the question itself.
    fn names_trust_choices(&self, _screen: &[String]) -> bool {
        false
    }
    /// Whether a delivery may go in now, or why that could not be told.
    fn ready<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>>;
    /// What the window's input box shows, for the human told of a delivery held for
    /// words typed there and not sent ([`Held::Unsent`]): its words, empty where it
    /// shows none, or none where no reader of the harness's reads a box on the screen.
    fn unsent_words<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        _pane: &'a Pane,
    ) -> Work<'a, Option<String>> {
        Box::pin(async { None })
    }
    /// What the pane host said of the human's input into the window at its last
    /// [`Window::ready`], for the trace of a delivery that look held; none where that look
    /// read no snapshot of the pane, or the harness's reads none.
    fn human_input(&self) -> Option<HumanInput> {
        None
    }
    /// Whether the engine asks [`Window::ready`] of the window before it has drawn its
    /// screen too, only for a window that says it never will be fit for work
    /// ([`Readiness::Unfit`]), which is given up then: a harness whose readiness has
    /// a bound that runs whether or not the window draws (Devin's normal permission
    /// mode, which held a window that never drew for ever). The others are asked
    /// once it has drawn.
    fn ready_while_drawing(&self) -> bool {
        false
    }
    /// Hands `text` to the window's harness: what became of it, or why it
    /// failed before the hand-over, which the engine says the delivery
    /// failed for. A failure after the hand-over is uncertain, never an
    /// error.
    fn deliver<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>>;
    /// What the window's harness says of it now.
    fn observe(&self) -> Work<'_, Result<Observed, String>>;
    /// Whether the harness says, after the hand-over, that it put the message last
    /// handed to it to its agent: its record shows it when it gets to it, however long
    /// that is. A harness that says so at the hand-over ([`Admission::Admitted`]'s
    /// `queued`) needs no answer here; one that can say it only later, by what it
    /// writes (Devin's store), does. The engine asks at each look until the answer
    /// is yes.
    fn took(&self) -> bool {
        false
    }
    /// Whether the message last handed to the window is in a turn the harness is in
    /// still: one it went into, which a harness that holds what it is given in a turn
    /// runs it after (Devin), or the one the harness began with it. Its record shows
    /// it when the turn ends, however long the turn says nothing that would read as
    /// work: until then, its record has had no time to show it. Once the turn ends,
    /// its record has the whole wait any record has. Asked at the hand-over too.
    fn held(&self) -> bool {
        false
    }
    /// The engine pressed the interrupt keys into the window, and the host
    /// took them: the turn it shows at work is the one they were for. A
    /// harness whose record says how an interrupted turn ended has no use for
    /// it; one that writes nothing of a turn it was interrupted in (Claude,
    /// before its first word) reads the looks that follow by it.
    fn interrupted(&self) {}
    /// Whether the harness has a word of its own that its window is at rest, apart
    /// from its record (Claude Code's status, Codex's broker, Pi's extension, Devin's
    /// store and wire log, OpenCode's TUI status). One that has none is read at rest
    /// for a delivery only by the fallback: its record's turn ended, its screen quiet
    /// and no key of the human's, each for the settle (`pacing`), and the board says
    /// the fallback is in use.
    fn idle_word(&self) -> bool {
        true
    }
    /// Whether the harness's live word, apart from its record, said at the last look
    /// that the window is at work (Codex's broker: Codex called the thread busy before
    /// its rollout shows the turn). Nothing is typed into a window at work (`pacing`);
    /// the record's word stands for everything else. False from a harness that has no
    /// such word, or said none.
    fn at_work(&self) -> bool {
        false
    }
    /// The harness is asked its live word again, where [`Window::at_work`] said the
    /// window is at work while its record has read the turn over for the settle: a
    /// notice the harness's word missed leaves it at work for good otherwise (Codex's
    /// broker and a lost `turn/completed`, T-842's P2). A harness with no such word asks
    /// nothing.
    fn recheck(&self) -> Work<'_, ()> {
        Box::pin(async {})
    }
    /// What the window's input box holds on its screen now, where its harness takes a
    /// message pasted into that box (Claude Code, Devin): a paste whose Enter did not take
    /// still sits there (3.0.7 chief-meeting, CMT-24). None where the screen shows no box
    /// its reader knows, or the harness takes its messages through a channel of its own.
    fn input_box<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        _pane: &'a Pane,
    ) -> Work<'a, Option<InputBox>> {
        Box::pin(async { None })
    }
}

/// A window's pane, by the pane host's id and the generation it opened in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pane {
    pub id: String,
    pub generation: u64,
}

/// The pane host, as an adapter asks it: its harness's own requests
/// (`pane.snapshot`, `pane.write_paste`, `pane.claim`), passed straight
/// through. Opening and ending panes are the engine's.
pub trait PaneHost {
    fn request<'a>(&'a self, op: &'a str, body: Value) -> Work<'a, Result<Value, HostError>>;

    /// Whether text may be handed to the window now, asked at the moment of the
    /// hand-over, after every wait before it: a paste as it is asked for (after
    /// Claude's keys that clear its input box), Pi's inbox rename once the pane's
    /// claim is answered, and a request to a peer on loopback (a Codex or OpenCode
    /// send, OpenCode's first message) inside the send, once its connection is up
    /// ([`Loopback::hand_over`](crate::seams::loopback::Loopback::hand_over)). The
    /// engine's host for a window says no once the window's project is closed (the
    /// human's Close, or the chief's window ending), and the text goes back to wait
    /// for the Resume, never handed over ([`NOT_ADMITTED`]); and once what it was
    /// to hand over was withdrawn since it was begun. A yes is the hand-over's
    /// moment: the engine counts the text the window's from then on. A host that
    /// says nothing admits everything.
    ///
    /// What an adapter must keep: no wait comes between the answer and the moment
    /// the text is the transport's, so nothing else of the engine runs in between.
    /// The bridge queues a paste's frame where the paste is asked for; the rename
    /// is a call; a request is handed to its connection, which writes it as it is
    /// next driven, in the same poll. A send's work polled in the same poll as the
    /// answer is not enough: it may wait inside, as a connection is made (Astra's
    /// review of 2026-10-16, round 5). A hand-over begun is the window's, whatever
    /// comes while it is on its way.
    fn admits(&self) -> bool {
        true
    }

    /// Whether the engine emptied the window's input box just before this
    /// hand-over, with the keys of its [`Input::Pasted`]: an adapter that would
    /// empty it itself presses nothing more (a second Control-C in a row ends
    /// Claude Code). A host that says nothing emptied nothing.
    fn cleared(&self) -> bool {
        false
    }
}

/// What a hand-over the pane host did not admit ([`PaneHost::admits`]) says:
/// nothing of it reached the window.
pub const NOT_ADMITTED: &str = "the window is handed nothing while its project is closed";

/// A request the pane host never answered: the bridge ended first, `error` its
/// word for it (`eof`). A passed deadline and a frame too large are answers
/// (`{ok: false, error: 'deadline'}`), which the adapter reads as it reads any
/// other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    pub error: Option<String>,
    pub message: String,
}

/// Whether a window may take a delivery now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    Held(Held),
    /// It never will be, and says why: the window came up unfit for work and
    /// did not come right in the time it was given (a Devin left in its normal
    /// permission mode, where every call waits for a permission nobody gives).
    /// The engine gives the launch up, as one that did not come up.
    Unfit(String),
}

/// Whether the turn a window was last in is over by its harness's own record,
/// though its live status kept the look from reading it settled
/// ([`Observed::over`]): nothing in it is then left for an interrupt to stop. Claude
/// says it is busy for as long as a background agent runs, and a turn an interrupt or
/// a refusal ended writes no count of them (T-604, 2026-10-25); and it says busy a
/// moment before the message of a turn it begins is in its record, when the turn
/// before still reads over (Calliope on T-551, T-644). So the record's word counts
/// once it has stood a moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Over {
    /// The record says no more than the look did.
    #[default]
    No,
    /// The record says the turn is over, and has not said so long enough to count:
    /// neither at rest nor at work yet.
    Pending,
    /// The record has said so a moment: the turn is over.
    Held,
}

/// Why a delivery waits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Held {
    /// The human typed into the window and has not sent it: the board says so.
    Unsent,
    /// The window shows another conversation than its launch's.
    ShowsAnother,
    /// A harness that said no more than "not yet" (Devin's `false`).
    Unsaid,
    Because(String),
}

impl Held {
    /// The sentence the engine says it in.
    pub fn sentence(&self) -> &str {
        match self {
            Held::Unsent => "you have typed in this window and not sent it",
            Held::ShowsAnother => "the window shows another conversation",
            Held::Unsaid => "a paste is on its way",
            Held::Because(reason) => reason,
        }
    }
}

/// What became of a delivery: only one refused before the harness could take it
/// is known not to have reached it; any other failure may have, and the
/// harness's own record decides (`admission`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admission {
    /// Handed over; `queued` where the harness's own queue took it.
    Admitted {
        queued: bool,
    },
    Refused {
        reason: String,
    },
    /// Refused because the harness is at work: nothing went in, and the message
    /// waits for the window's next rest, with no attempt spent (Pi's extension
    /// sends nothing while Pi is busy; 3.0.7 chief-meeting, D9).
    Busy {
        reason: String,
    },
    Uncertain {
        reason: String,
    },
}

/// What a look at a window found.
#[derive(Debug, Clone, PartialEq)]
pub struct Observed {
    /// The harness's record of the conversation.
    /// None where the window has not named a conversation to read yet.
    pub reading: Option<Arc<Reading>>,
    pub settled: bool,
    /// Whether the turn the window was last in is over by its harness's own
    /// record, though its live status kept the look from reading it `settled`
    /// ([`Over`]). A harness whose `settled` is its record's word says no.
    pub over: Over,
    pub waiting: Option<Waiting>,
    pub failed: bool,
    pub quota: Option<Arc<Quota>>,
    /// The window shows another conversation, this one: the look is the
    /// old conversation's last, and nothing in it settles.
    pub switched: Option<String>,
    /// The window has not said which conversation it shows (it is
    /// starting, switching conversations or reconnecting).
    pub unnamed: bool,
    /// The look read the window at rest because the interrupt the engine
    /// pressed stopped its turn before a word of its answer (Claude): the
    /// message that began the turn is in the window's input box again, and out
    /// of the conversation the window answers from, though its record still
    /// shows it.
    pub took_back: bool,
    /// The harness's own word that the window waits at its prompt, whatever its record
    /// says of the turn (Claude Code's status): what tells a window at rest whose record
    /// never reads its turn as over (T-526: a conversation opened again with no proof that
    /// the program before was gone keeps what that program had queued) from one still at
    /// work. False from a harness that says no such thing apart from its record.
    pub at_prompt: bool,
    /// Another program of the harness's, not the window's, has shown the window's
    /// conversation since the window opened (a Claude Code the human started outside
    /// ConsensFlow on it, `claude --resume <id>`): the record holds that one's turns
    /// too, written as the window's are, and nothing in it tells them apart. False
    /// from a harness that cannot tell.
    pub shared: bool,
}

impl Observed {
    /// The conversation's items: none where its record could not be read.
    pub fn items(&self) -> &[Item] {
        match self.reading.as_deref() {
            Some(Reading::Known(record)) => &record.items,
            Some(Reading::Unknown(_) | Reading::Missing(_)) | None => &[],
        }
    }

    /// Why the look's turn failed, in its harness's own words where its record
    /// has them: none where the turn did not fail, or its record says no more.
    pub fn failure(&self) -> Option<&str> {
        match self.reading.as_deref() {
            Some(Reading::Known(record)) if self.failed => record.failure.as_deref(),
            _ => None,
        }
    }

    /// The conversation's last word ([`last_word`]): none where its record says
    /// nothing, or could not be read.
    pub fn last_word(&self) -> Option<&Item> {
        last_word(self.items())
    }

    /// The lines of the conversation's record that were torn and passed over,
    /// each by the place among the records it stood at: none where its record
    /// could not be read.
    pub fn lost(&self) -> &[usize] {
        match self.reading.as_deref() {
            Some(Reading::Known(record)) => &record.lines.lost,
            Some(Reading::Unknown(_) | Reading::Missing(_)) | None => &[],
        }
    }

    /// The place among the records of the record each item began in, where the
    /// harness says it: none for Devin, whose items are its store's, and where
    /// the record could not be read.
    pub fn places(&self) -> Option<&[usize]> {
        match self.reading.as_deref() {
            Some(Reading::Known(record)) => record.lines.places.as_deref(),
            Some(Reading::Unknown(_) | Reading::Missing(_)) | None => None,
        }
    }

    /// What the conversation's turn ended and left running: the window takes
    /// input between turns, and its agent is woken as each ends.
    pub fn pending(&self) -> Pending {
        match self.reading.as_deref() {
            Some(Reading::Known(record)) => record.pending,
            Some(Reading::Unknown(_) | Reading::Missing(_)) | None => Pending::default(),
        }
    }

    /// The monitors among what the conversation's turn left running, as its record says
    /// them: none where it could not be read.
    pub fn monitors(&self) -> Vec<Monitor> {
        match self.reading.as_deref() {
            Some(Reading::Known(record)) => record.monitors.clone(),
            Some(Reading::Unknown(_) | Reading::Missing(_)) | None => Vec::new(),
        }
    }

    /// Whether the conversation's turn ended and left background agents or
    /// workflows running.
    pub fn agents_pending(&self) -> bool {
        self.pending().agents > 0
    }
}

/// A window that waits on something its own dialog holds, and why where its
/// harness said: nothing is pasted into it meanwhile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Waiting {
    pub reason: Option<String>,
}

/// What a conversation's record, read with no window open, says of the turn its
/// window was left in ([`Records::turn_cut`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnLeft {
    /// Its last turn ended: whatever background work it left running, and whatever
    /// the harness's own notices of it queued, the window was at rest.
    AtRest,
    /// It was in a turn with no end: the one the message there began, where the
    /// record holds it.
    Cut(Option<Opening>),
}

/// The message a turn began with, as its record holds it: when it came, where the
/// record says a time, and its words, its whitespace made single spaces and cut short.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Opening {
    /// `2026-10-15 20:41 UTC`.
    pub at: Option<String>,
    pub words: String,
}

/// How many characters of a turn's first message [`Opening`] keeps.
const OPENING_WORDS: usize = 100;

impl Opening {
    /// The message the turn at the end of `items` began with: the last of the user's
    /// (none of the harness's own, and no compaction's summary), if one is there. None
    /// where the last word is a whole reply: that turn ended, and what holds the record
    /// in flight is a message taken and not begun, which no item shows.
    fn of(items: &[Item]) -> Option<Self> {
        if last_word(items).is_some_and(|item| item.role == Role::Assistant && item.complete) {
            return None;
        }
        let item = items
            .iter()
            .rev()
            .find(|item| item.role == Role::User && !item.compaction)?;
        let words: Vec<&str> = item.text.split_whitespace().collect();
        let words = words.join(" ");
        let mut kept: String = words.chars().take(OPENING_WORDS).collect();
        if kept.len() < words.len() {
            kept.push('…');
        }
        let at = item
            .at
            .as_ref()
            .and_then(Value::as_str)
            .and_then(|at| at.parse::<jiff::Timestamp>().ok())
            .map(|at| at.strftime("%Y-%m-%d %H:%M UTC").to_string());
        Some(Self { at, words: kept })
    }
}

/// A harness's own record of a conversation, as the engine serves it to
/// the windows: a worker that owns the readers reads it off the engine's
/// thread, where a first look at a long transcript takes a second.
pub trait Records {
    /// What `harness`'s record of `session` says now (`cachedAnswers`).
    fn look<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
        options: &'a Options,
    ) -> Work<'a, Arc<Reading>>;
    /// Whether `harness` kept a transcript of `session` at all
    /// (`hasTranscript`), or why that cannot be told.
    fn has_transcript<'a>(
        &'a self,
        harness: Harness,
        session: &'a str,
    ) -> Work<'a, Result<bool, String>>;

    /// Where `harness`'s transcript of `session` ends now, how many bytes it
    /// holds and the last of them, where a reader of it reads a relaunch
    /// (`Options::relaunched`, Claude's alone): none where it keeps no
    /// transcript of it, or a seam that keeps no files says none.
    fn transcript_boundary<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<Option<Boundary>, String>> {
        Box::pin(async { Ok(None) })
    }

    /// Where `harness`'s transcript of `session` ends now, where the harness's own
    /// word says no program of its writes it any more (Claude's statuses): what is
    /// before it was written by programs that are gone, and what they had queued
    /// went with them. None where one may, where it cannot say, or a seam that
    /// keeps no files says none.
    fn transcript_left<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
    ) -> Work<'a, Result<Option<Boundary>, String>> {
        Box::pin(async { Ok(None) })
    }

    /// Whether `harness`'s own store says, with no window open, that the window of
    /// `session` took a paste that begins with `pasted`, handed over at `at_ms` or
    /// later, though its record may not show it yet (Devin holds a paste queued behind
    /// a turn, and writes it to the record when the turn takes it in): none where the
    /// harness keeps no such word, as all but Devin.
    fn took<'a>(
        &'a self,
        _harness: Harness,
        _session: &'a str,
        _pasted: &'a str,
        _at_ms: i64,
    ) -> Work<'a, Option<bool>> {
        Box::pin(async { None })
    }

    /// Whether `harness`'s record of `session`, read with no window open, ends in
    /// a turn with no end (the turn the app's quit cut, or a Close), with the message
    /// that began it ([`TurnLeft::Cut`]); at rest where it ends at rest, or where all
    /// that holds its turn is the harness's notices of background work
    /// ([`Lines::held_by_notices`](crate::records::Lines)): background work is rest;
    /// and none where the record cannot tell. Devin's
    /// cannot: it reads a turn as going on only while its window shows it working,
    /// and one whose last word is the message it was given as settled, which with
    /// no window open is a turn never answered as much as one at rest. Where no
    /// program writes the record any more ([`Records::transcript_left`]), what they
    /// had queued and not begun went with them and is no turn: a Claude that quit
    /// idle with a message queued leaves no sign of its quit in its record, and was
    /// cut in nothing (T-442's three).
    fn turn_cut<'a>(&'a self, harness: Harness, session: &'a str) -> Work<'a, Option<TurnLeft>> {
        Box::pin(async move {
            let options = Options {
                relaunched: self.transcript_left(harness, session).await.ok().flatten(),
                ..Options::default()
            };
            match &*self.look(harness, session, &options).await {
                Reading::Known(record) => match record.settlement {
                    Settlement::InFlight if record.lines.held_by_notices => Some(TurnLeft::AtRest),
                    Settlement::InFlight => Some(TurnLeft::Cut(Opening::of(&record.items))),
                    Settlement::Settled if ends_in_a_reply(record) => Some(TurnLeft::AtRest),
                    Settlement::Settled | Settlement::Unknown => None,
                },
                _ => None,
            }
        })
    }
}

/// Whether the last word of `record` (what the harness put in it itself aside)
/// is the agent's reply: what a turn at rest ends in.
fn ends_in_a_reply(record: &Record) -> bool {
    last_word(&record.items).is_some_and(|item| item.role == Role::Assistant)
}

/// The last word of a conversation's `items`: its last item but what the harness
/// put in it itself (Devin's system and custom messages, Pi's extensions' messages,
/// a hook's context, a compaction's summary), which can come after a reply. An idle
/// compaction's summary is no word of the user's: a record that ends in one after a
/// reply ends at rest (the sites chief's of 2026-10-23, T-814; a chief's undecided
/// record was given no note, a member's was told its turn was cut).
fn last_word(items: &[Item]) -> Option<&Item> {
    items
        .iter()
        .rev()
        .find(|item| item.role != Role::Custom && !item.compaction)
}

#[cfg(test)]
mod tests;
