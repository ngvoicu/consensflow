//! Codex's windows. Codex runs under ConsensFlow's supervisor, the bundle's
//! native `cf codex-session`: an app-server, a broker that knows the thread the
//! TUI shows and queues messages on it, and the TUI attached to both. The first
//! message is Codex's last argument; the broker names the thread once Codex
//! starts it, and again whenever the human starts or resumes another one in the
//! window (/new, /resume), so the window is followed to it. A Codex too old for
//! the native queue is refused: nothing could reach its window. Every window
//! starts with the MCP servers Codex has, the chief's and a member's alike (the
//! owner's choice, 2026-10-06): a launch never lists them, and its command line
//! switches none off.
//!
//! A thread is resumed by its id (`codex resume <id>`) and has a record to be
//! resumed from only once it has had a turn: a window resumed on one that never
//! had any ends at once (2026-10-13). The adapter says whether a thread has its
//! record ([`Adapter::resumable`]): a file under Codex's `sessions` named as Codex
//! names the rollout of that thread (`rollout-<timestamp>-<thread>.jsonl`). The
//! file is found and not read, so an empty or corrupt rollout counts as the
//! record, and a window resumed on it ends as it did before. The engine opens the
//! window afresh on a thread that has none; the launch itself resumes whatever
//! thread it is given. A window opened on a thread has come up once its broker
//! names it, as a new one has once it names the thread Codex started: within a
//! minute, or the launch is given up (`started`).
//!
//! A thread the app's quit cut in a turn is found with that turn open, and no
//! process in it. A window opened on one with no message to begin a turn (a
//! role's replacement is given to a resumed window by paste, once it is up: see
//! `keeps_first_instructions`) is read at rest once its broker names the thread,
//! while its record holds no more than it held when the window was opened
//! ([`LeftOpen`]), as Claude's and Pi's are. What Codex itself does with such a
//! thread is `cargo xtask live resume-cut codex`: one that goes on with the turn
//! or closes it writes to the record, and the record is then read as it always
//! was.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use cf_base::text::window_text;
use cf_proto::agents::Harness;
use cf_proto::models::ModelListing;
use cf_proto::panes::{Servers, SilentPaste};
use cf_proto::specs::Variant;

use super::channel::{send, Channel, Session, Shown, Target};
use super::{launch, paths, role};
use crate::contract::{
    Adapter, Admission, Agent, Dialog, FirstMessage, Held, Launch, LaunchId, NoDefault, Observed,
    Over, Pane, PaneHost, Prepared, Readiness, Records, Skill, Specs, Used, Window, Work,
};
use crate::detect::executable;
use crate::records::Options;
use crate::seams::{Loopback, Services, Time};
use crate::shared::admission::admission;
use crate::shared::left_open::LeftOpen;
use crate::shared::record_state::{record_state, switched_to, unnamed};
use crate::shared::resumable::transcript_kept;
use crate::shared::{compacting, specmint, window_args};

/// Codex's question tool (`request_user_input`) is behind a feature still
/// marked under development; the broker answers a member's from the board.
/// The chief has none: it asks the human in plain words in its window.
const QUESTION_TOOL: [&str; 4] = [
    "--enable",
    "default_mode_request_user_input",
    "-c",
    "suppress_unstable_features_warning=true",
];

/// A window that opens on Codex's "Update now?" prompt, whenever a newer
/// release exists, never starts its thread, so a chief opened without a first
/// message waited on it for good. And a login shell re-reads the user's
/// profile, which can put another install's `cf` ahead of this daemon's (an
/// eval worker's `cf` was the live app's older one): without it, commands keep
/// the PATH the daemon gave the window. Both probed on Codex 0.156.1.
const WINDOW: [&str; 4] = [
    "-c",
    "check_for_update_on_startup=false",
    "-c",
    "allow_login_shell=false",
];

/// Codex's offers to switch or install something, which wait in a window for a key nobody
/// there presses, turned off for the window's launch alone (T-1183, read from Codex 0.166.0):
/// the nudge near a rate limit to switch to a cheaper model ("Switch to … for lower credit
/// usage?", `notice.hide_rate_limit_model_nudge`), the model's suggestion to install a plugin
/// (`tool_suggest`), and the offer to install the MCP servers a skill names
/// (`skill_mcp_dependency_install`). The features go as configuration, never as `--disable`,
/// which refuses to start on a Codex that has no such feature; a key Codex does not know is
/// passed over. What the owner has installed and configured stays as it is.
const OFFERS: [&str; 6] = [
    "-c",
    "notice.hide_rate_limit_model_nudge=true",
    "-c",
    "features.tool_suggest=false",
    "-c",
    "features.skill_mcp_dependency_install=false",
];

/// The size the project chose for its members to compact at, as Codex is given it for the
/// launch alone (at launch and on resume): its `model_auto_compact_token_limit`, which
/// Codex caps at 90% of the model's window itself (`ModelInfo::auto_compact_token_limit`,
/// codex-rs 0.166.0), so a size above that leaves Codex's own point. None where the project
/// chose none, and for the chief, whose launch is never given one.
fn compaction_limit(size: Option<u32>) -> Vec<String> {
    size.map(|size| {
        vec![
            "-c".to_owned(),
            format!("model_auto_compact_token_limit={size}"),
        ]
    })
    .unwrap_or_default()
}

/// Why a message waits while the broker names no thread or cannot take one: a
/// refusal there would spend the message's attempts in seconds (an answer to a
/// Codex worker was lost that way), so it is held.
const HOLD: &str =
    "the Codex window cannot take a message yet: starting, switching conversations or reconnecting";

/// How often the broker is asked for the thread a new window opened.
const DISCOVER_EVERY: Duration = Duration::from_millis(250);

/// The most the broker has to name it, in milliseconds, the first time. A Codex
/// that has started no thread in a minute is most often waiting on something
/// more time does not give (a login, a prompt); a launch with less time than that
/// gives it less ([`Launch::first_message_ms`]).
const DISCOVER_FOR_MS: i64 = 60_000;

/// How many times its first time a window's may grow to.
const DISCOVER_GROWS_TO: i64 = 10;

/// How long the broker has to name the thread of a window: `first`, doubled for
/// each window of its participant given up before it in a row, to at most ten
/// times it (a minute, then 2, 4 and 8, then 10). Time alone proves no window
/// stuck: a resume that takes 65 s was given up at 60 s at every try, and what
/// waited for its chief never reached it (Astra on round 1 of the chief's restart).
fn discover_for(first: i64, given_up: u32) -> i64 {
    first
        .saturating_mul(2_i64.saturating_pow(given_up))
        .min(first.saturating_mul(DISCOVER_GROWS_TO))
}

/// Codex, as the engine launches its windows.
pub struct CodexAdapter {
    services: Services,
}

impl CodexAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            services: services.clone(),
        }
    }
}

/// How Codex on Windows reads a paste ([`CodexAdapter::silent_paste`]): about 0.6 ms a
/// character; its own draw of the paste comes 50 ms or more after it; then 120 ms of quiet;
/// and at the most a second and a millisecond a character.
const CODEX_ON_WINDOWS: SilentPaste = SilentPaste {
    pace_us_per_character: 600,
    drawn_after_ms: 50,
    quiet_ms: 120,
    most_ms: 1_000,
    most_us_per_character: 1_000,
};

impl Adapter for CodexAdapter {
    /// Codex keeps the machine's one login, which all its windows use.
    fn one_login(&self) -> bool {
        true
    }

    fn models(&self, _fetch: bool) -> Work<'_, Result<ModelListing, String>> {
        Box::pin(super::models::listed(
            &self.services.env,
            &*self.services.processes,
            &*self.services.time,
        ))
    }

    /// What its record says the conversation ran on last (CHF-05): read from its end,
    /// within a bound, as the reopen of a chief asks.
    fn last_used<'a>(
        &'a self,
        session: &'a str,
        _asked: Option<&'a str>,
    ) -> Work<'a, Option<Used>> {
        Box::pin(async move { super::last_used::last_used(session, &self.services.env) })
    }

    /// T-445's probe of each harness's own switch (brain `operations/work-in-flight/results/3.0.7/`).
    fn model_switch(&self) -> Option<&'static str> {
        Some("To change only the model, type /model in the chief's window and press s to keep it to that conversation: Enter there makes it your Codex default.")
    }

    fn model_gone(&self, words: &str, asked: &str) -> bool {
        super::models::gone(words, asked)
    }

    /// A Codex window is `cf codex-session`, which ends with the status of why when it could
    /// not get Codex ready (`cf_base::startup::StartKind::exit_status`). Those statuses are its
    /// own: a status Codex returned in that range is reported as 1
    /// (`cf_base::startup::harness_exit`), so that a window that ended with one could not have
    /// been ended by Codex.
    fn never_ran(&self, status: u32) -> Option<cf_base::startup::StartFailure> {
        cf_base::startup::StartKind::of_exit_status(i64::from(status))
            .map(cf_base::startup::StartFailure::of)
    }

    fn default_model(&self) -> Work<'_, Result<Option<String>, NoDefault>> {
        Box::pin(super::models::default_model(
            &self.services.env,
            &*self.services.processes,
            &*self.services.time,
        ))
    }
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let services = &self.services;
            let executable = executable(Harness::Codex, &services.env)?;
            let launched =
                launch::configuration(services, launch.id, launch.directory, &executable).await?;
            let texts = role::text(
                services,
                launch.role,
                &executable,
                launch.directory,
                launch.instructions,
            )
            .await?;
            // A chief given specs reads its Specmint skill by the path its role
            // text names: where Codex has none of its own, ConsensFlow's is
            // written where it is kept for every launch, which Codex is told of
            // by that path alone.
            specmint::write_own(Harness::Codex, &services.env, launch)?;
            // An image agent's window is Codex on its own default model, whose
            // image tool draws: it names no model or effort of its own.
            let agent = match launch.agent {
                Some(agent) if !agent.designer => agent,
                _ => Agent::default(),
            };
            let open = if launch.resume.is_none() {
                window_args::start
            } else {
                window_args::resume
            };
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Codex, agent, launch.resume, None)
                .ok_or_else(|| "a Codex window resumes a thread by its id".to_owned())?;
            // What the record holds before this window exists is the old
            // process's: a turn it left open, which the window is told apart from
            // by what the record holds later, until it is delivered a message.
            let left_open = match launch.resume {
                Some(thread) => {
                    let before = services
                        .records
                        .look(Harness::Codex, thread, &Options::default())
                        .await;
                    LeftOpen::of(&before)
                }
                None => None,
            };
            // The role goes by a file the window's broker reads, never its command line;
            // what the window's sub-agents start with in its place, by a file the
            // supervisor names to the app-server.
            let role = role::write(&services.env, launch.id, &texts.role)?;
            let children = role::write_children(&services.env, launch.id, &texts)?;
            let mut args = Vec::new();
            if launch.role != "chief" {
                args.extend(QUESTION_TOOL.map(str::to_owned));
            }
            args.extend(WINDOW.map(str::to_owned));
            args.extend(OFFERS.map(str::to_owned));
            args.extend(compaction_limit(launch.compaction));
            args.extend(invocation.args);
            Ok(Prepared {
                argv: launch::with_native_bridge(&services.bundle, &executable, args),
                env: launched
                    .env
                    .into_iter()
                    .chain([role])
                    .chain(children)
                    .collect(),
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: launch.resume.map(str::to_owned),
                window: Rc::new(CodexWindow {
                    records: Rc::clone(&services.records),
                    time: Rc::clone(&services.time),
                    loopback: Rc::clone(&services.loopback),
                    channel: launched.channel,
                    thread: RefCell::new(launch.resume.map(str::to_owned)),
                    named: Cell::new(false),
                    discover_for_ms: discover_for(
                        DISCOVER_FOR_MS.min(launch.first_message_ms),
                        launch.starts_given_up,
                    ),
                    left_open: RefCell::new(left_open),
                    at_work: Cell::new(false),
                }),
            })
        })
    }

    /// Codex keeps a thread's rollout once the thread has a turn.
    fn resumable<'a>(&'a self, session: &'a str) -> Work<'a, Option<bool>> {
        transcript_kept(&*self.services.records, Harness::Codex, session)
    }

    /// Codex opens the window's servers (its plugins': computer use, a REPL) as
    /// the window starts, each in a group of its own under `codex`, the harness's
    /// own process, which is in the window's group: they are there when the first
    /// turn begins, and what they start later (a REPL's workers, as a tool is
    /// called) is the harness's. What `codex` starts is the agent's: its helper,
    /// `codex-code-mode-host`, and its commands, which are believed to start
    /// under it (the tree was probed on Codex 0.162.1 with no command running).
    fn servers(&self) -> Servers {
        if self.services.env.on_windows() {
            Servers::Unknown
        } else {
            Servers::AtLaunch
        }
    }

    /// On Windows Codex reads a paste's key presses without drawing anything, and takes an
    /// Enter that comes while they are still coming, or within 120 ms of them, for a line break
    /// in the paste (its `paste_burst`). A measured heuristic (T-1048, Codex 0.166.0 on zeewin2,
    /// 2026-11-01): a few lines were drawn 150 to 205 ms after the paste; a paste of 7,775
    /// characters was answered at once by a print at about 10 ms (its reaction to the first
    /// key) and drawn as `[Pasted Content … chars]` only at about 4.5 s, about 0.6 ms a
    /// character, and an Enter before that stayed in it. Elsewhere the terminal hands Codex a
    /// paste whole, and it takes its Enter as it comes.
    fn silent_paste(&self) -> Option<SilentPaste> {
        self.services.env.on_windows().then_some(CODEX_ON_WINDOWS)
    }

    /// A resumed thread keeps the developer message its first turn began with:
    /// the role text a launch gives it (the developer instructions of a main
    /// start, see the module `role`) is not given to a resume, and would not be used. Found live on Codex 0.162.1,
    /// 2026-10-13: a chief's rollout held the one developer message of its start,
    /// with no section of the specs turned on before its Resume. And probed through
    /// `codex app-server` the same day: a thread started with the developer
    /// instructions "Your code word is ROLEONE." answered ROLEONE, and the same
    /// thread resumed by a second app-server (`thread/resume`) with "Your code
    /// word is now ROLETWO; this replaces any earlier code word." answered ROLEONE
    /// to the same question, so the field of the resume request replaces nothing
    /// either.
    fn keeps_first_instructions(&self) -> bool {
        true
    }

    /// Through the broker once it names the window's thread, as every later
    /// message is (`turn/start` to Codex's server): the TUI opened with no prompt
    /// starts its thread as it comes up, and its own prompt would have gone in by
    /// the same request.
    fn first_message(&self) -> FirstMessage {
        FirstMessage::Delivered
    }

    fn specmint(&self, variant: Variant, folder: &str) -> Result<Skill, String> {
        let env = &self.services.env;
        specmint::place(
            Harness::Codex,
            env,
            variant,
            folder,
            paths::global_specmint(env, variant),
        )
    }

    fn place_specmint(&self, specs: &Specs<'_>, id: &LaunchId) -> Result<(), String> {
        specmint::write_own_of(Harness::Codex, &self.services.env, Some(*specs), id).map(|_| ())
    }
}

/// A Codex window, and the channel to the broker of its supervisor.
struct CodexWindow {
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    loopback: Rc<dyn Loopback>,
    channel: Channel,
    /// The thread the window shows: its launch's, the one the broker named
    /// when Codex started it, or the one it was followed to. None until the
    /// broker has named it.
    thread: RefCell<Option<String>>,
    /// Whether the broker has named a thread since the window opened.
    named: Cell<bool>,
    /// How long the broker has to name the thread of a window, in milliseconds.
    discover_for_ms: i64,
    /// The turn the record had open when the window was opened, while the
    /// window has begun no turn of its own: the old process's.
    left_open: RefCell<Option<LeftOpen>>,
    /// The broker said, at the last look, that Codex called the thread busy
    /// (`Window::at_work`).
    at_work: Cell<bool>,
}

impl CodexWindow {
    /// The broker's word on the window now.
    async fn shown(&self) -> Option<Shown> {
        self.channel.shown(&*self.time, &*self.loopback).await
    }

    /// Whether the broker names a thread now: the window has come up, with the
    /// thread Codex started where it opened afresh (which is the window's from then
    /// on), or with none where it was opened on a thread (a thread not its own is
    /// followed by the next look).
    async fn named_now(&self) -> Option<Option<String>> {
        let Some(Shown {
            session: Session::Thread(thread),
            ..
        }) = self.shown().await
        else {
            return None;
        };
        self.named.set(true);
        if self.thread.borrow().is_some() {
            return Some(None);
        }
        *self.thread.borrow_mut() = Some(thread.clone());
        Some(Some(thread))
    }
}

impl Window for CodexWindow {
    /// The thread is named by the broker, never by the window's process.
    fn opened(&self, _pid: Option<u32>) {}

    /// Codex compacts a long thread at the start of a turn, its record an open turn
    /// and nothing more until it is done, the message that began it written after:
    /// its status line says it.
    fn compacting(&self, screen: &[String]) -> bool {
        compacting::drawn(screen, &compacting::CODEX)
    }

    /// Codex reads no dialog of its harness off its screen by shape (3.0.7-alpha.2, T-1240, T-1243).
    ///
    /// The engine reads the pane host's tail: no empty line and no rule that says no word. In it Codex's
    /// composer ("› Ask Codex to do anything"), its footer and the last line of the conversation ("  Worked
    /// for 5s • 16:00") are one under the other in one column, as a list of choices is, and by their shape
    /// the shared reader took them for three, with a worker's question above them for the dialog's
    /// words: a chief at rest was held for a dialog nobody had opened (the live matrix's
    /// `question-ask-answer`, 2026-11-05). No rule on the screen's words keeps a composer apart from a
    /// dialog, because the words are the user's: the footer is configured (`tui.status_line`, a
    /// branch's name that is `enter` or `esc`), Codex hides its line of keys while a draft is in the
    /// composer, and what the user typed is numbered or quoted as they like. A rule that took a list
    /// with its keys under it for a dialog (T-1243, `fd1837385`) read such a draft as one, where the
    /// release head read none.
    ///
    /// And the release head read no real Codex dialog off the tail at all: its nudge to switch models,
    /// its pickers and 27 upstream snapshots of its dialogs were unread, their title and hint lines
    /// being taken for choices. So reading none loses no dialog that was read, adds no false one, and
    /// fixes the field's bug. Codex's nudge and install offers stay off at launch (`OFFERS`), and its
    /// own question tool is the board's. Reading Codex's real dialogs from evidence beyond its footer
    /// is a follow-up: `issues/codex-dialogs-are-not-read-from-its-screen.md` in consensflow-sme.
    fn dialog(&self, _screen: &[String]) -> Option<Dialog> {
        None
    }

    fn follow(&self, session: &str) {
        *self.thread.borrow_mut() = Some(session.to_owned());
        // The turn the record had open was a thread's the window left.
        *self.left_open.borrow_mut() = None;
    }

    /// The window is up once its broker names a thread ([`CodexWindow::named_now`]),
    /// within the time its launch gives it ([`discover_for`]). A resumed window
    /// waits for it too: one whose broker never names it reads as starting, and
    /// has nothing delivered, for good (a Codex chief opened again after the app's
    /// restart on zeewin, 2026-10-19, read "starting" for 78 minutes).
    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(async move {
            if self.named.get() {
                return Ok(None);
            }
            let deadline = self.time.wall_ms().saturating_add(self.discover_for_ms);
            // The broker is asked at the deadline too: a thread named after the last
            // look before it was the window's, and its chief was closed at work (Sol
            // on round 1 of the chief's restart).
            loop {
                if let Some(thread) = self.named_now().await {
                    return Ok(thread);
                }
                let left = deadline.saturating_sub(self.time.wall_ms());
                if left <= 0 {
                    break;
                }
                let wait = u64::try_from(left).map_or(DISCOVER_EVERY, Duration::from_millis);
                self.time.sleep(DISCOVER_EVERY.min(wait)).await;
            }
            Err(match self.thread.borrow().as_deref() {
                Some(thread) => {
                    format!("the Codex broker never named the thread {thread} it was opened on")
                }
                None => "the Codex broker never named the thread it opened".to_owned(),
            })
        })
    }

    /// One more look at the broker.
    fn came_up(&self) -> Work<'_, Option<Option<String>>> {
        Box::pin(async move {
            if self.named.get() {
                return Some(None);
            }
            self.named_now().await
        })
    }

    /// A message waits until the broker can take one and the window shows its
    /// own thread.
    fn ready<'a>(
        &'a self,
        _host: &'a dyn PaneHost,
        _pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            Ok(match self.shown().await {
                Some(shown) if shown.available => {
                    if shown.session.is(self.thread.borrow().as_deref()) {
                        Readiness::Ready
                    } else {
                        Readiness::Held(Held::ShowsAnother)
                    }
                }
                _ => Readiness::Held(Held::Because(HOLD.to_owned())),
            })
        })
    }

    fn deliver<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>> {
        Box::pin(async move {
            let thread = self.thread.borrow().clone();
            let target = Target {
                channel: &self.channel,
                thread: thread.as_deref(),
                pane,
                host,
            };
            let sent = send(&*self.time, &*self.loopback, &target, &window_text(text)).await?;
            Ok(admission(
                &sent.reading(),
                "the Codex broker refused it",
                true,
            ))
        })
    }

    fn at_work(&self) -> bool {
        self.at_work.get()
    }

    fn recheck(&self) -> Work<'_, ()> {
        Box::pin(async move {
            let shown = self
                .channel
                .shown_refreshed(&*self.time, &*self.loopback)
                .await;
            // A broker that does not answer leaves the word as it was.
            if let Some(shown) = shown {
                self.at_work.set(shown.idle == Some(false));
            }
        })
    }

    /// Codex's background terminals, as its broker asks its server for them: a command it
    /// started with no shell, or kept when a pause interrupted it, is no process the host
    /// names as the turn's work (T-1137). None where the broker gave no list.
    fn kept_commands(&self) -> Work<'_, Option<Vec<String>>> {
        Box::pin(async move { self.channel.terminals(&*self.time, &*self.loopback).await })
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            let thread = self.thread.borrow().clone();
            let Some(thread) = thread else {
                return Ok(Observed {
                    reading: None,
                    settled: false,
                    over: Over::No,
                    waiting: None,
                    failed: false,
                    quota: None,
                    switched: None,
                    unnamed: false,
                    took_back: false,
                    at_prompt: false,
                    shared: false,
                    begun: Vec::new(),
                });
            };
            // The broker's word first, then the record, which is then never
            // older than the word: the order Node's reads finish in when the
            // broker answers beside a record. Kept from Node on purpose: Node
            // started both at once, and a record its look had read before the
            // broker answered could be older.
            let shown = self.shown().await;
            // Codex's own word that the thread is at work, which its rollout shows only
            // once the turn has written something (T-716 §3): kept for the pacing.
            self.at_work.set(
                shown
                    .as_ref()
                    .is_some_and(|shown| shown.idle == Some(false)),
            );
            let reading = self
                .records
                .look(Harness::Codex, &thread, &Options::default())
                .await;
            let mut observed = record_state(Arc::clone(&reading));
            Ok(match shown {
                Some(Shown {
                    session: Session::Thread(shown),
                    ..
                }) => {
                    // The thread the window was followed to meanwhile, if it was.
                    if self.thread.borrow().as_deref() == Some(shown.as_str()) {
                        // A turn the quit cut is over, though no record says so:
                        // the window, which names its thread, has been given no
                        // message since it was opened, and its record holds no
                        // more than it did.
                        if self
                            .left_open
                            .borrow()
                            .as_ref()
                            .is_some_and(|open| open.still(&reading))
                        {
                            observed.settled = true;
                        }
                        observed
                    } else {
                        switched_to(observed, shown)
                    }
                }
                _ => unnamed(observed, HOLD),
            })
        })
    }
}
