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
use cf_proto::panes::Servers;
use cf_proto::specs::Variant;

use super::channel::{send, Channel, Session, Shown, Target};
use super::{launch, paths, role};
use crate::contract::{
    Adapter, Admission, Agent, Held, Launch, Observed, Over, Pane, PaneHost, Prepared, Readiness,
    Records, Skill, Window, Work,
};
use crate::detect::executable;
use crate::records::Options;
use crate::seams::{Loopback, Services, Time};
use crate::shared::admission::admission;
use crate::shared::left_open::LeftOpen;
use crate::shared::record_state::{record_state, switched_to, unnamed};
use crate::shared::resumable::transcript_kept;
use crate::shared::{specmint, window_args};

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

impl Adapter for CodexAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let services = &self.services;
            let executable = executable(Harness::Codex, &services.env)?;
            let launched =
                launch::configuration(services, launch.id, launch.directory, &executable).await?;
            let role = role::arguments(
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
            let message = launch.message.map(window_text);
            let open = if launch.resume.is_none() {
                window_args::start
            } else {
                window_args::resume
            };
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Codex, agent, launch.resume, message.as_deref())
                .ok_or_else(|| "a Codex window resumes a thread by its id".to_owned())?;
            // What the record holds before this window exists is the old
            // process's: a turn it left open, which a window with no message to
            // begin one is told apart from by what the record holds later. A
            // window given a message begins its own turn.
            let left_open = match launch.resume {
                Some(thread) if launch.message.is_none() => {
                    let before = services
                        .records
                        .look(Harness::Codex, thread, &Options::default())
                        .await;
                    LeftOpen::of(&before)
                }
                _ => None,
            };
            let mut args = role;
            if launch.role != "chief" {
                args.extend(QUESTION_TOOL.map(str::to_owned));
            }
            args.extend(WINDOW.map(str::to_owned));
            args.extend(invocation.args);
            Ok(Prepared {
                argv: launch::with_native_bridge(&services.bundle, &executable, args),
                env: launched.env,
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

    /// A resumed thread keeps the developer message its first turn began with:
    /// the role text a launch gives it (`developer_instructions`, see the
    /// module `role`) is read at a resume and not used. Found live on Codex 0.162.1,
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

    fn specmint(&self, variant: Variant) -> Result<Skill, String> {
        let env = &self.services.env;
        specmint::place(
            Harness::Codex,
            env,
            variant,
            paths::global_specmint(env, variant),
        )
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
    fn started<'a>(&'a self, _host: &'a dyn PaneHost) -> Work<'a, Result<Option<String>, String>> {
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
                });
            };
            // The broker's word first, then the record, which is then never
            // older than the word: the order Node's reads finish in when the
            // broker answers beside a record. Kept from Node on purpose: Node
            // started both at once, and a record its look had read before the
            // broker answered could be older.
            let shown = self.shown().await;
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
