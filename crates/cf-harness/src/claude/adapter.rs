//! Claude Code's windows. Each launch gets its own settings file: full
//! permission without the one-time dialog, and a Stop hook on every turn so
//! every finished turn is recorded.
//!
//! - The session id is ConsensFlow's: drawn for a fresh window, resumed for
//!   a known one.
//! - A message is pasted into a live window as if the human typed it, once
//!   its input box holds nothing the human typed and has not sent (the
//!   owner's choice, 2026-10-03: never pasted into their text). Claude's own
//!   peer inbox would bypass the input box, but Claude wraps each such
//!   message as a teammate's request from another Claude session, a hundred
//!   tokens of caution per delivery that misnames the human's own answers,
//!   so it is not used (2026-09-22).
//! - Claude's own `sessions/<pid>.json` says busy, idle or waiting (and
//!   why); the transcript holds the conversation and says whether the turn
//!   settled.
//! - A turn the daemon interrupted before Claude wrote a word of it leaves
//!   no record of its end, and Claude puts the message back in its input
//!   box, and out of the conversation it answers from: the window is read at
//!   rest where the daemon pressed the interrupt for that very turn
//!   ([`stopped`]), the look says it took the message back
//!   (`Observed::took_back`), and the box is cleared before the next paste,
//!   which would go in after the old text.
//! - A conversation the app's quit cut in a turn is found with that turn open,
//!   and no process in it. Resumed, Claude waits at its prompt, status idle,
//!   writes nothing of the old turn and closes it, with a message of its own
//!   (`No response requested.`), when the next one comes (Claude 2.1.296,
//!   `npm run live:resume-cut`). A window opened on such a conversation with no
//!   message to begin it is read at rest by that status while its record holds
//!   no more than it held when the window was opened ([`LeftOpen`]).
//! - A window opened again on a conversation runs a process of its own, the
//!   one before gone with its window where the pane host proved it gone: what
//!   that one had queued in its memory and not begun went with it, though its
//!   record still says it is queued (T-442). Where the transcript ended when the
//!   window is prepared, its length and the bytes before it, is where the new
//!   process began, and every look the window takes says so
//!   (`Options::relaunched`). Claude Code's own statuses
//!   say the same where the host could not: no Claude it runs shows the
//!   conversation then, so none writes it ([`status::holders`]; a window open when
//!   the app quit, ended after the daemon had gone, leaves no sign in the record
//!   and no proof in the ledger, T-526). Where a Claude still shows it (one the
//!   host could not end, or one the human started outside ConsensFlow), what it
//!   writes past the boundary would be read as the new process's, and none is
//!   told; so it is where neither the host nor the statuses can say.
//! - A Claude the human starts outside ConsensFlow on a window's conversation
//!   (`claude --resume <id>` in a terminal of their own) writes its turns into the
//!   same transcript, in records no different from the window's: nothing in them
//!   says which process wrote one. Its status names the conversation as the
//!   window's does: a window that has seen another Claude show its conversation, or
//!   one that showed it before the window opened, says so on every look
//!   (`Observed::shared`) from then on, and the engine takes none of the record's
//!   answers for its task's result.
//! - The window's Claude process is the pane's own child when a status names
//!   it, so the first look already sees a /clear. Otherwise (Claude may run
//!   as another process the child starts) it is the process whose status
//!   first named the launch's conversation, which a /clear before that look
//!   leaves unknown. A /clear or /resume in the window changes the
//!   conversation that status names, never the process, so the window is
//!   followed to it.
//! - Every window starts with the MCP servers, claude.ai connectors and
//!   Claude in Chrome the human's own Claude Code has, the chief's and a
//!   member's alike (the owner's choice, 2026-10-06): the command line
//!   switches none off. ConsensFlow's own hooks come from `--settings`, not
//!   from MCP.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cf_base::env::Env;
use cf_base::js;
use cf_base::text::window_text;
use cf_proto::agents::Harness;
use cf_proto::models::ModelListing;
use cf_proto::panes::{HumanInput, Servers};
use cf_proto::specs::Variant;
use serde_json::Value;

use super::status::{self, State, Status};
use super::stopped::{self, Pressed};
use super::{install, paths};
use crate::contract::{
    Adapter, Admission, Decline, Dialog, DialogSign, FirstMessage, Held, Input, Launch, LaunchId,
    NoDefault, Observed, Over, Pane, PaneHost, Prepared, Question, Readiness, Records,
    ServersChosen, Skill, Specs, Trusts, Used, Waiting, Window, Work,
};
use crate::detect::executable;
use crate::records::{Boundary, Options, Reading, Settlement};
use crate::seams::processes::Processes;
use crate::seams::{self, Entropy, Services, Time};
use crate::shared::admission::admission;
use crate::shared::input_box::{self, InputBox, Unsent};
use crate::shared::left_open::LeftOpen;
use crate::shared::record_state::{observed_from, switched_to};
use crate::shared::resumable::transcript_kept;
use crate::shared::trust;
use crate::shared::{compacting, pane, specmint, window_args};

/// What every window's Claude Code is started with in its environment, for its launch alone:
/// no survey of its own (Claude Code 2.1.317's session survey "How is Claude doing this
/// session?", its memory and plugin ratings), which waits under the prompt for a digit
/// nobody in the window types (T-1183). It is the one switch of its offers a launch can give:
/// the offer to install an LSP plugin, and a command's hint to install one, are turned off only
/// in the owner's own `~/.claude.json` (`lspRecommendationDisabled`, `claudeCodeHints`), which
/// ConsensFlow never writes; they close by themselves after 30 s, and the engine tells of them
/// meanwhile (`dialogs`).
const QUIET: [(&str, &str); 1] = [("CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY", "1")];

/// The questions of Claude Code's offers to install a plugin (2.1.317): of an LSP plugin, for a
/// file of an extension a marketplace has one for, and of a plugin a command's output hints at.
const OFFERS_CLOSING: [&str; 2] = [LSP_OFFER, "Would you like to install it?"];

/// The question of Claude Code's offer to install an LSP plugin, and its choice that declines
/// it and writes nothing (2.1.317).
pub(super) const LSP_OFFER: &str = "Would you like to install this LSP plugin?";
const LSP_DECLINE: &str = "No, not now";

/// What Claude Code 2.1.317 asks where a PreToolUse hook answers "ask" for a call, its words of
/// the hook above it ("Hook tool.check requires confirmation for this command: <the hook's
/// reason> [plugin:…]"), and its choice that refuses the call (T-1198).
const HOOK_QUESTION: &str = "Do you want to proceed?";
const HOOK_SAYS: [&str; 2] = ["Hook ", "requires confirmation for this"];
const HOOK_DECLINE: &str = "No";

/// How long a hook's ask may wait for the human before ConsensFlow answers it "No", in
/// milliseconds: ten minutes (the chief's decision of 2026-11-03, T-1198), or what
/// `CONSENSFLOW_HOOK_ASK_MS` says where it says a number (the rig's cases).
const HOOK_ASK_MS: i64 = 10 * 60_000;

/// The decline of `dialog` where it is a hook's ask on a call: "No", by its digit, once nobody
/// answered it for `after_ms`. "No" writes nothing, neither to the settings nor to
/// `~/.claude.json` (2.1.317's `nOr`: `"no"` → `{behavior:"deny"}`, where only "Yes, and don't
/// ask again" carries `permissionUpdates`), but a "No" with no words of the user's ends the
/// turn (`cancelAndAbort` aborts it, and the model is told to stop and wait for the user), so
/// the window is told once it rests to go on without the call, the hook's reason with it where
/// the screen showed one. "Yes" is never chosen, of any hook's or permission's prompt.
fn hook_ask_decline(dialog: &Dialog, after_ms: i64) -> Option<Decline> {
    let says = dialog.detail.join(" ");
    if dialog.question != HOOK_QUESTION || !HOOK_SAYS.iter().all(|words| says.contains(words)) {
        return None;
    }
    let at = dialog
        .options
        .iter()
        .position(|option| option == HOOK_DECLINE)?;
    let no = u8::try_from(at + 1).ok().filter(|digit| *digit <= 9)?;
    let span = if after_ms < 60_000 {
        format!("{} seconds", after_ms / 1_000)
    } else {
        format!("{} minutes", after_ms / 60_000)
    };
    let reason = hook_reason(&says).map_or_else(String::new, |reason| {
        format!(" The hook said: \u{201c}{reason}\u{201d}")
    });
    let then_tell = format!(
        "A hook asked for confirmation of your last call, and nobody answered it within {span}, so ConsensFlow answered \u{201c}No\u{201d} for you: that call did not run.{reason} Go on without that call, or say in your result that you could not do the task without it."
    );
    Some(Decline {
        choice: HOOK_DECLINE.to_owned(),
        key: vec![b'0' + no],
        after_ms: Some(after_ms),
        then_tell: Some(then_tell),
        what: "hook prompt on its call".to_owned(),
        listed: true,
        members_only: false,
    })
}

/// The hook's own reason in what a hook's ask says above its question: what follows the colon
/// after "requires confirmation for this <call>", up to the hook's source in brackets.
fn hook_reason(says: &str) -> Option<String> {
    let after = &says[says.find(HOOK_SAYS[1])? + HOOK_SAYS[1].len()..];
    let reason = after[after.find(':')? + 1..].trim();
    let reason = reason.split(" [").next().unwrap_or(reason).trim();
    (!reason.is_empty()).then(|| reason.to_owned())
}

/// The seconds after which Claude Code closes those offers by itself (`Zlt=30000`).
const OFFER_CLOSES_AFTER_S: u32 = 30;

/// Claude Code, as the engine launches its windows.
pub struct ClaudeAdapter {
    env: Env,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    entropy: Rc<dyn Entropy>,
    /// The bundle's `cf` as a window names it: what the launch's hooks run.
    pane_cf: String,
    processes: Rc<dyn Processes>,
    /// Where Claude keeps its own status of each of its processes, made
    /// whole once, as Node resolved it when it made the adapter; or why
    /// there is none.
    statuses: Result<String, String>,
}

impl ClaudeAdapter {
    /// The adapter of the windows the engine's `services` serve.
    pub fn new(services: &Services) -> Self {
        Self {
            env: services.env.clone(),
            records: Rc::clone(&services.records),
            time: Rc::clone(&services.time),
            entropy: Rc::clone(&services.entropy),
            pane_cf: services.bundle.pane_cf.clone(),
            processes: Rc::clone(&services.processes),
            statuses: status::folder(&services.env),
        }
    }
}

impl Adapter for ClaudeAdapter {
    fn models(&self, _fetch: bool) -> Work<'_, Result<ModelListing, String>> {
        Box::pin(super::models::listed(
            &self.env,
            &*self.processes,
            &*self.time,
        ))
    }

    /// What its record says the conversation ran on last (CHF-05): read from its end,
    /// within a bound, as the reopen of a chief asks.
    fn last_used<'a>(&'a self, session: &'a str, asked: Option<&'a str>) -> Work<'a, Option<Used>> {
        Box::pin(async move { super::last_used::last_used(session, asked, &self.env) })
    }

    /// T-445's probe of each harness's own switch (brain `operations/work-in-flight/results/3.0.7/`).
    fn model_switch(&self) -> Option<&'static str> {
        Some("To change only the model, type /model in the chief's window and press s to keep it to that conversation: Enter there makes it your Claude Code default.")
    }

    fn model_gone(&self, words: &str, asked: &str) -> bool {
        super::models::gone(words, asked)
    }

    fn default_model(&self) -> Work<'_, Result<Option<String>, NoDefault>> {
        Box::pin(super::models::default_model(
            &self.env,
            &*self.processes,
            &*self.time,
        ))
    }
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        Box::pin(async move {
            let executable = executable(Harness::Claude, &self.env)?;
            let settings = install::settings(
                &self.env,
                launch.id,
                launch.role != "chief",
                launch.compaction,
                launch.advisor_off,
                launch.suggestion_off,
                &self.pane_cf,
                Some(launch.directory),
            )?;
            let role = install::role(&self.env, launch)?;
            // Claude keeps a conversation only once something was said in
            // it, and what it kept can be gone since: the engine asks
            // `resumable` first, but a transcript that is not there has
            // nothing to resume, and `--resume` would exit at once. It starts
            // afresh under the same id, so the conversation stays bound.
            let session = match launch.resume {
                Some(resume) => resume.to_owned(),
                None => seams::uuid(&*self.entropy)?,
            };
            // Kept from Node on purpose: an environment that names no home
            // fails here, where Node read the process's own and opened a
            // window it would never see the status of.
            let statuses = self.statuses.clone()?;
            let resumable = match launch.resume {
                Some(resume) => self.records.has_transcript(Harness::Claude, resume).await?,
                None => false,
            };
            // The Claudes that show the conversation before this window exists:
            // none of them is the window's.
            let holders = if resumable {
                status::holders(&statuses, &session)
            } else {
                Some(Vec::new())
            };
            // What the record holds before this window exists is the old
            // process's, gone with its window where the pane host proved it
            // (`Launch::previous_gone`), or where Claude's statuses say no
            // Claude shows the conversation: what it had queued and not begun
            // went with it, and every look of this window says where the new
            // one began (`Options::relaunched`). One still showing it may
            // still write to the record, and what it writes past the boundary
            // taken would be read as the new process's: none is taken.
            let gone = holders.as_ref().map_or(launch.previous_gone, Vec::is_empty);
            let relaunched = if resumable && gone {
                let boundary = self
                    .records
                    .transcript_boundary(Harness::Claude, &session)
                    .await?;
                boundary.map(|boundary| (session.clone(), boundary))
            } else {
                None
            };
            let options = told(
                relaunched.as_ref(),
                launch
                    .agent
                    .and_then(|agent| agent.model)
                    .filter(|model| !model.is_empty()),
                &session,
            );
            // A turn the old process left open, which the window has to be told
            // apart from by what the record holds later: it begins its own only
            // once it is delivered a message.
            let left_open = if resumable {
                let before = self.records.look(Harness::Claude, &session, &options).await;
                LeftOpen::of(&before)
            } else {
                None
            };
            let agent = launch.agent.unwrap_or_default();
            let open = if resumable {
                window_args::resume
            } else {
                window_args::start
            };
            // None only for a conversation of an empty id, which no ledger holds.
            let invocation = open(Harness::Claude, agent, Some(&session), None)
                .ok_or_else(|| "a Claude window opens on a session id".to_owned())?;
            let mut argv = vec![executable];
            argv.extend(settings);
            argv.extend(role);
            argv.extend(invocation.args);
            Ok(Prepared {
                argv,
                env: QUIET
                    .iter()
                    .map(|&(name, value)| (name.to_owned(), value.to_owned()))
                    .collect(),
                drop_env: invocation
                    .drop_env
                    .iter()
                    .map(|&name| name.to_owned())
                    .collect(),
                native_session: Some(session.clone()),
                window: Rc::new(ClaudeWindow {
                    records: Rc::clone(&self.records),
                    time: Rc::clone(&self.time),
                    statuses,
                    session: RefCell::new(session),
                    pid: Cell::new(None),
                    claude_pid: Cell::new(None),
                    asked: RefCell::new(None),
                    pressed: RefCell::new(None),
                    restored: Cell::new(false),
                    put_back: Cell::new(false),
                    clear_first: Cell::new(false),
                    read_empty: Cell::new(None),
                    input: RefCell::new(None),
                    left_open: RefCell::new(left_open),
                    over_since: RefCell::new(None),
                    relaunched,
                    before: RefCell::new(holders.unwrap_or_default()),
                    shared: Cell::new(false),
                    model: launch
                        .agent
                        .and_then(|agent| agent.model)
                        .filter(|model| !model.is_empty())
                        .map(str::to_owned),
                    trusts: trust::trusts(launch.directory),
                    directory: launch.directory.to_owned(),
                    hook_ask_ms: self
                        .env
                        .text("CONSENSFLOW_HOOK_ASK_MS")
                        .and_then(|ms| ms.parse().ok())
                        .unwrap_or(HOOK_ASK_MS),
                }),
            })
        })
    }

    /// Claude keeps a conversation's transcript once something was said in it.
    fn resumable<'a>(&'a self, session: &'a str) -> Work<'a, Option<bool>> {
        transcript_kept(&*self.records, Harness::Claude, session)
    }

    /// Claude Code runs each command of its Bash tool, in the foreground and in
    /// the background alike, in a process group of its own (a
    /// `/bin/zsh -c source …/shell-snapshots/snapshot-zsh-….sh …` that leads its
    /// group, seen on macOS with 3.0.1's workers), while its MCP servers and its
    /// own helpers (`caffeinate`) stay in the group of its window. It starts its
    /// servers as it takes its first message, after the window's turn is marked.
    /// What a server starts is the harness's whatever group it starts it in: the
    /// browser of `@playwright/mcp` and `chrome-devtools-mcp` leaves it.
    ///
    /// On Windows a job object has no groups, and Claude Code runs every command of its
    /// agent in a shell it starts for it (Git Bash's `bash.exe`: T-375's scripts, T-858's
    /// `exec sleep 611` in a Monitor) and its servers as their programs (npm's are a
    /// `cmd.exe /c npx …`, which is no shell): [`Servers::ByShell`]. Said `Unknown` before
    /// T-906, it left the host telling none of its commands, and a Monitor that Claude Code
    /// had ended held its task until Finish.
    fn servers(&self) -> Servers {
        if self.env.on_windows() {
            Servers::ByShell
        } else {
            Servers::InGroup
        }
    }

    /// Claude Code keeps one login for the machine, in the config folder every
    /// window of ConsensFlow's shares, and each window follows a `/login` typed
    /// into any of them: on 2026-10-04 (poker-lab) every Claude window logged the
    /// change, and those at work went on on the new account.
    fn one_account(&self) -> bool {
        true
    }

    /// Pasted once the window is past the question of trusting its folder and has
    /// drawn, as every later message is: Claude Code 2.1.310 has no other way in
    /// for an interactive window's first message (a SessionStart hook's
    /// `initialUserMessage` is read and never sent, T-432). Its record wraps the
    /// paste (`<pasted_content id="…">`), as it wraps every later message's.
    fn first_message(&self) -> FirstMessage {
        FirstMessage::Delivered
    }

    /// Pasted, and its box cleared with Control-C ([`stopped::CLEAR_INPUT`]).
    fn input(&self) -> Input {
        Input::Pasted {
            clear: &stopped::CLEAR_INPUT,
        }
    }

    fn specmint(&self, variant: Variant, folder: &str) -> Result<Skill, String> {
        specmint::place(
            Harness::Claude,
            &self.env,
            variant,
            folder,
            paths::global_specmint(&self.env, variant),
        )
    }

    fn place_specmint(&self, specs: &Specs<'_>, id: &LaunchId) -> Result<(), String> {
        specmint::write_own_of(Harness::Claude, &self.env, Some(*specs), id).map(|_| ())
    }
}

/// A Claude window, and what the engine told it of itself.
struct ClaudeWindow {
    /// How long a hook's ask waits for the human before it is answered "No" (`HOOK_ASK_MS`).
    hook_ask_ms: i64,
    records: Rc<dyn Records>,
    time: Rc<dyn Time>,
    statuses: String,
    /// The conversation the window shows: its launch's, or the one it was
    /// followed to.
    session: RefCell<String>,
    /// The pane's own child, once the host named it.
    pid: Cell<Option<u32>>,
    /// The Claude whose status first named the window's conversation.
    claude_pid: Cell<Option<u32>>,
    /// The id of the user's last message in the latest look: the turn the
    /// window is in, and what an interrupt is pressed over.
    asked: RefCell<Option<Arc<str>>>,
    /// The interrupt the daemon pressed for that turn, if it did.
    pressed: RefCell<Option<Pressed>>,
    /// The latest look read the window at rest by that press: Claude has put
    /// the message back in its input box.
    restored: Cell<bool>,
    /// The latest look read the window at rest after a command Claude put back in
    /// its input box (a compaction cancelled with Escape, `Lines::put_back`).
    put_back: Cell<bool>,
    /// The latest readiness read that command still in the box on the screen, with
    /// no key of the human's since: the delivery's paste clears it first.
    clear_first: Cell<bool>,
    /// The human's keys left something unsent by the pane host's count, and the
    /// latest readiness read the input box empty on the screen, at this count of
    /// their keys: the delivery's paste says so ([`Unsent::ReadEmpty`]).
    read_empty: Cell<Option<u64>>,
    /// What the pane host said of the human's input at the latest readiness, where it
    /// read a snapshot of the pane ([`Window::human_input`]).
    input: RefCell<Option<HumanInput>>,
    /// The turn the record had open when the window was opened, while the
    /// window has begun no turn of its own: the old process's.
    left_open: RefCell<Option<LeftOpen>>,
    /// Since when the looks have read the turn of this last user message over by
    /// the record while the window was not at rest (the wall clock, in
    /// milliseconds): Claude says busy a moment before it appends the message of
    /// a turn it begins, and the turn before reads over meanwhile.
    over_since: RefCell<Option<(i64, Option<Arc<str>>)>>,
    /// The conversation the window was opened again on, and where its transcript
    /// ended then, the window it was on closed: where the program the window runs
    /// began writing.
    relaunched: Option<(String, Boundary)>,
    /// The Claudes that showed the launch's conversation before the window was
    /// opened: none of them is the window's.
    before: RefCell<Vec<u32>>,
    /// Another Claude than the window's has shown its conversation since it
    /// opened (`Observed::shared`).
    shared: Cell<bool>,
    /// The model the launch named, where it named one: an alias such as `opusplan` names the
    /// models it runs on by itself, which the reader takes for no change of model (T-841).
    model: Option<String>,
    /// What a yes to Claude Code's question about the window's folder trusts.
    trusts: Trusts,
    /// The window's folder, whose local settings keep what Claude Code was told of the
    /// project's servers.
    directory: String,
}

/// What a look at `session` is told: where the window's program began writing,
/// when the window was opened again on that conversation.
fn told(relaunched: Option<&(String, Boundary)>, model: Option<&str>, session: &str) -> Options {
    Options {
        relaunched: relaunched
            .filter(|(on, _)| on == session)
            .map(|(_, boundary)| boundary.clone()),
        launch_model: model.map(str::to_owned),
        ..Options::default()
    }
}

impl ClaudeWindow {
    /// Whether the record says the window's turn is over, though the look is not
    /// at rest (`reads_over`), and has said so for [`stopped::HOLD_MS`] of the same
    /// turn ([`Over`]): Claude's status goes busy a moment before the message of a
    /// turn it begins is in the record, and a look in that moment reads the turn
    /// before over (Calliope on T-551, T-644). A turn whose message came since
    /// begins the wait anew.
    fn held_over(&self, reads_over: bool) -> Over {
        let mut since = self.over_since.borrow_mut();
        if !reads_over {
            *since = None;
            return Over::No;
        }
        let now = self.time.wall_ms();
        let turn = self.asked.borrow().clone();
        match since.as_ref() {
            Some((at, of)) if *of == turn && now - at >= stopped::HOLD_MS => Over::Held,
            Some((_, of)) if *of == turn => Over::Pending,
            _ => {
                *since = Some((now, turn));
                Over::Pending
            }
        }
    }

    /// Claude's status of the window's process: the pane's own child's,
    /// when the host named it and Claude keeps one, else the status of the
    /// process that first named the window's conversation, but one that showed
    /// it before the window opened (`before`): that Claude is another's, the
    /// human's outside ConsensFlow or the program before the window's, however
    /// Claude's statuses are listed (T-800).
    fn status(&self) -> Option<Status> {
        let statuses = status::statuses(&self.statuses);
        let of = |pid: u32| {
            statuses
                .iter()
                .find(|(held, _)| *held == pid)
                .map(|(_, status)| status.clone())
        };
        if let Some(status) = self.pid.get().and_then(of) {
            return Some(status);
        }
        if self.claude_pid.get().is_none() {
            let session = self.session.borrow();
            let before = self.before.borrow();
            let named = statuses
                .iter()
                .find(|(pid, status)| status.session == *session && !before.contains(pid));
            self.claude_pid.set(named.map(|(pid, _)| *pid));
        }
        self.claude_pid.get().and_then(of)
    }

    /// Whether another Claude than the window's shows its conversation now: one
    /// that showed it before the window was opened, or a second beside the
    /// window's own. Which of two is the window's need not be known.
    fn shares(&self) -> bool {
        let session = self.session.borrow();
        status::holders(&self.statuses, &session).is_some_and(|holders| {
            let before = self.before.borrow();
            holders.len() > 1 || holders.iter().any(|pid| before.contains(pid))
        })
    }

    /// Whether `status` names another conversation than the window's.
    fn elsewhere(&self, status: &Status) -> bool {
        status.session != *self.session.borrow()
    }

    /// Whether the interrupt the daemon pressed for the turn the window is in
    /// is what ended it, though Claude wrote no record of that ([`stopped`]).
    fn stopped(&self, reading: &Reading) -> bool {
        let Reading::Known(record) = reading else {
            return false;
        };
        self.pressed
            .borrow()
            .as_ref()
            .is_some_and(|pressed| pressed.stopped(record, self.time.wall_ms()))
    }

    /// Whether a paste may go in after a compaction the human cancelled, the
    /// human's keys having left nothing unsent: the box on the screen empty, or
    /// holding what Claude put back, which the paste clears first (one Ctrl+C,
    /// only where the box shows text: at an empty box a second one in a row ends
    /// Claude). A box the screen does not show holds the paste as the human's own
    /// words do, for the `/compact` they typed may be there unsent, and the board
    /// tells them so (`holding`).
    async fn put_back_cleared(&self, host: &dyn PaneHost, pane: &Pane) -> Readiness {
        match input_box::on_screen(host, pane, Harness::Claude).await {
            Some(InputBox::Empty) => Readiness::Ready,
            Some(InputBox::Holds) => {
                self.clear_first.set(true);
                Readiness::Ready
            }
            None => Readiness::Held(Held::Unsent),
        }
    }
}

impl Window for ClaudeWindow {
    fn opened(&self, pid: Option<u32>) {
        if pid.is_some() {
            self.pid.set(pid);
        }
    }

    fn follow(&self, session: &str) {
        session.clone_into(&mut self.session.borrow_mut());
        // What was pressed for was a turn of the conversation the window left,
        // and so was the turn its record had open.
        *self.asked.borrow_mut() = None;
        *self.pressed.borrow_mut() = None;
        self.restored.set(false);
        self.put_back.set(false);
        *self.left_open.borrow_mut() = None;
        // Who showed the conversation the window left is no word on the one it
        // shows now: another Claude there is seen beside the window's.
        self.before.borrow_mut().clear();
        self.shared.set(false);
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        Box::pin(async { Ok(None) })
    }

    fn trusts(&self) -> Option<&Trusts> {
        Some(&self.trusts)
    }

    fn toward_trust(&self, screen: &[String]) -> Option<Question> {
        super::trust::toward_trust(screen)
    }

    fn shows_prompt(&self, screen: &[String]) -> bool {
        super::trust::shows_prompt(screen)
    }

    /// Claude Code's offers to install a plugin close by themselves after 30 s, answered
    /// "timeout" (an LSP plugin, which counts it as ignored) or "no" (a command's hint), as
    /// 2.1.317's dialog of them draws them (`Zlt=30000`).
    fn dialog(&self, screen: &[String]) -> Option<Dialog> {
        let shown = crate::shared::dialog::read(screen)?;
        let closes_after_s = OFFERS_CLOSING
            .contains(&shown.dialog.question.as_str())
            .then_some(OFFER_CLOSES_AFTER_S);
        Some(Dialog {
            prompt_below: self.shows_prompt(&shown.below),
            closes_after_s,
            asks: shown.dialog.asks || super::computer_use_prompt::is_prompt(&shown.dialog),
            ..shown.dialog
        })
    }

    /// What tells one of Claude Code's dialogs from another read of its screen: the time in its
    /// own warning line blanked, and its offer to install an LSP plugin from its own title down
    /// (`dialog_identity`, T-1277). Never a time in a command, a hook's reason or a question.
    fn dialog_identity(&self, dialog: &Dialog) -> Dialog {
        super::dialog_identity::identity(dialog)
    }

    /// Claude Code's list of safe declines: its offer to install an LSP plugin, answered "No, not
    /// now" by the digit its list numbers that choice with (2.1.317's lists take a digit for the
    /// choice of that number, at once). That answer writes nothing (`case"no":break`, where
    /// "Never for this plugin", "Disable all LSP recommendations" and an offer left to close
    /// by itself each write `~/.claude.json`), as `cargo xtask live harness-dialogs` showed on a
    /// throwaway configuration (T-1192). Not its hint to install a plugin a command named:
    /// every answer to it but a cancel writes the plugin's name in `claudeCodeHints.plugin`.
    fn safe_decline(&self, dialog: &Dialog) -> Option<Decline> {
        if let Some(decline) = hook_ask_decline(dialog, self.hook_ask_ms) {
            return Some(decline);
        }
        if let Some(decline) = super::computer_use_prompt::decline(dialog) {
            return Some(decline);
        }
        if dialog.question != LSP_OFFER {
            return None;
        }
        let at = dialog
            .options
            .iter()
            .position(|option| option == LSP_DECLINE)?;
        let digit = u8::try_from(at + 1).ok().filter(|digit| *digit <= 9)?;
        Some(Decline {
            choice: LSP_DECLINE.to_owned(),
            key: vec![b'0' + digit],
            after_ms: None,
            then_tell: None,
            what: "offer".to_owned(),
            listed: true,
            members_only: false,
        })
    }

    /// Claude Code's status says `waiting`, and why, for a dialog of its own (2.1.317's table of
    /// its dialogs): "input needed" for its question tool, which is the board's to answer in a
    /// member's window and the human's in the chief's; every other reason ("dialog open",
    /// "permission prompt", "sandbox request", "goal proposal") is a dialog of Claude Code's
    /// that waits for a key (T-1183). It does not always: with its offer to install an LSP
    /// plugin open in a turn at work it said `busy` (`cargo xtask live harness-dialogs`,
    /// 2026-11-03), so a window whose status says no wait is left to the screen.
    fn dialog_sign(&self, observed: &Observed) -> DialogSign {
        match &observed.waiting {
            Some(Waiting { reason }) if reason.as_deref() == Some(status::QUESTION_TOOL) => {
                DialogSign::Shut
            }
            Some(_) => DialogSign::Open,
            None => DialogSign::Screen,
        }
    }

    fn box_read_empty(&self, snapshot: &serde_json::Value) -> bool {
        input_box::read_empty(Harness::Claude, snapshot)
    }

    fn names_trust_choices(&self, screen: &[String]) -> bool {
        super::trust::names_choices(screen)
    }

    fn ready_sign(&self) -> Option<&str> {
        Some(super::trust::PROMPT)
    }

    fn compacting(&self, screen: &[String]) -> bool {
        compacting::drawn(screen, &compacting::CLAUDE)
    }

    fn trust_servers(&self, screen: &[String]) -> Vec<String> {
        super::trust::servers_named(screen)
    }

    fn servers_chosen(&self) -> ServersChosen {
        trust::chosen(&self.directory)
    }

    /// What the box on the screen shows, for the human told of a delivery held for it.
    fn unsent_words<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Option<String>> {
        Box::pin(input_box::words_on_screen(host, pane, Harness::Claude))
    }

    fn human_input(&self) -> Option<HumanInput> {
        self.input.borrow().clone()
    }

    /// A paste waits for the window to be readable, on its conversation,
    /// with no paste going in, and for what the human typed and did not send,
    /// unless the input box reads empty on the screen ([`input_box::unsent`]).
    /// After a compaction the human cancelled, Claude has the `/compact` back in
    /// its box, which the human's keys never typed: the box on the screen says
    /// whether it is still there, to be cleared first, and a box that cannot be
    /// read holds the paste, which would go in behind it (`put_back`).
    fn ready<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        Box::pin(async move {
            self.input.replace(None);
            if self.status().is_some_and(|live| self.elsewhere(&live)) {
                return Ok(Readiness::Held(Held::ShowsAnother));
            }
            let snapshot = pane::snapshot(host, pane)
                .await
                .map_err(|failed| failed.message)?;
            self.input.replace(HumanInput::of(&snapshot));
            if snapshot.get("ok") != Some(&Value::Bool(true)) {
                // The host's own word for why, as a template writes it.
                let said = snapshot
                    .get("error")
                    .filter(|error| !error.is_null())
                    .map_or(Cow::Borrowed("no answer"), |error| js::text(Some(error)));
                let reason = format!("the window cannot be read: {said}");
                return Ok(Readiness::Held(Held::Because(reason)));
            }
            if js::truthy(snapshot.get("pasteInFlight")) {
                let reason = "a paste is on its way to the window".to_owned();
                return Ok(Readiness::Held(Held::Because(reason)));
            }
            self.read_empty.set(None);
            self.clear_first.set(false);
            if !js::truthy(snapshot.get("unsent")) {
                return Ok(if self.put_back.get() {
                    self.put_back_cleared(host, pane).await
                } else {
                    Readiness::Ready
                });
            }
            // The count is the human's keys, a guess: the box on the screen says.
            Ok(
                match input_box::unsent(host, pane, Harness::Claude, &snapshot).await {
                    Unsent::ReadEmpty(typed) => {
                        self.read_empty.set(Some(typed));
                        Readiness::Ready
                    }
                    Unsent::Held => Readiness::Held(Held::Unsent),
                },
            )
        })
    }

    fn deliver<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>> {
        Box::pin(async move {
            // A window read at rest by an interrupt of ours has the message it
            // was given in its input box again, and one the human's cancel left
            // has the `/compact`: this one would be pasted after it, to go as one.
            // The box is cleared first, unless the engine just cleared it.
            if (self.restored.get() || self.clear_first.take()) && !host.cleared() {
                if let Err(cause) = pane::write_keys(host, pane, &stopped::CLEAR_INPUT).await {
                    let reason = format!("the window's input box could not be cleared: {cause}");
                    return Ok(Admission::Refused { reason });
                }
            }
            let read_empty = self.read_empty.take();
            let sent = pane::write_paste(host, pane, &window_text(text), read_empty).await;
            let admitted = admission(&sent, "the window refused the paste", false);
            if matches!(admitted, Admission::Admitted { .. }) {
                // The turn the paste begins is its own: no press was for it.
                *self.pressed.borrow_mut() = None;
                self.restored.set(false);
            }
            Ok(admitted)
        })
    }

    /// The box as the next paste meets it: one its own hand-over empties first (the
    /// message an interrupt of ours put back, a `/compact` the human's cancel left, which
    /// [`ClaudeWindow::ready`] found there) holds nothing the paste would go in behind.
    fn input_box<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Option<InputBox>> {
        if self.restored.get() || self.clear_first.get() {
            return Box::pin(async { Some(InputBox::Empty) });
        }
        Box::pin(input_box::on_screen(host, pane, Harness::Claude))
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        Box::pin(async move {
            // Claude's status first, then the conversation, which is then
            // never older than Claude's word: the order Node's reads finish
            // in when Claude's few small status files are read before a
            // transcript is. Kept from Node on purpose until the status read
            // leaves the event thread (3.5): Node started both at once, and
            // a transcript its look had read before the status came back
            // could be older than the status.
            let live = self.status();
            if self.shares() {
                self.shared.set(true);
            }
            let session = self.session.borrow().clone();
            let options = told(self.relaunched.as_ref(), self.model.as_deref(), &session);
            let reading = self.records.look(Harness::Claude, &session, &options).await;
            let (empty, settled, agents) = match &*reading {
                Reading::Known(record) => (
                    record.items.is_empty(),
                    record.settlement == Settlement::Settled,
                    record.pending.agents > 0,
                ),
                Reading::Unknown(_) | Reading::Missing(_) => (true, false, false),
            };
            // What the record says of the failure and the quota is the one place's
            // (`observed_from`); what Claude's own status says of the window is set on it.
            let observed = |settled, over, waiting, took_back, at_prompt| Observed {
                settled,
                over,
                waiting,
                took_back,
                at_prompt,
                shared: self.shared.get(),
                ..observed_from(Arc::clone(&reading), None)
            };
            Ok(match live {
                Some(live) if self.elsewhere(&live) => {
                    switched_to(observed(false, Over::No, None, false, false), live.session)
                }
                live => {
                    // Claude's own status is the word on whether the window
                    // is at its prompt: a new window has no transcript until
                    // its first message, and a resumed one carries a
                    // transcript that settled before this window opened, so
                    // a paste on its word alone lands before the prompt is up.
                    let idle = live.as_ref().is_some_and(|live| live.state == State::Idle);
                    // Claude says `busy` for as long as a background agent or
                    // workflow runs, though its turn is over and its prompt takes
                    // input: a turn the transcript says ended with some pending is
                    // at rest, and what is sent goes in between turns. The stop
                    // hook's summary comes a moment before the duration that says
                    // them, and Claude is busy through that moment: the look says
                    // at rest only once the duration says why. A window that said
                    // no status has said nothing, and one on its own dialog
                    // (waiting) is no agents' business.
                    let working = live
                        .as_ref()
                        .is_some_and(|live| live.state == State::Working);
                    let at_prompt = idle || (agents && working);
                    let waiting = match live {
                        Some(Status {
                            state: State::Waiting(reason),
                            ..
                        }) => Some(Waiting { reason }),
                        _ => None,
                    };
                    // A turn the daemon interrupted before a word of it was
                    // written has no end in the transcript: the press is it.
                    // Claude then has its message in the input box again, and
                    // out of the conversation it answers from: the look says so.
                    let by_press = idle && self.stopped(&reading);
                    self.restored.set(by_press);
                    self.put_back
                        .set(matches!(&*reading, Reading::Known(record) if record.lines.put_back));
                    *self.asked.borrow_mut() = stopped::last_user(&reading);
                    // A turn the quit cut is over, though its record never says
                    // so: Claude at its prompt has begun no other.
                    let left = self
                        .left_open
                        .borrow()
                        .as_ref()
                        .is_some_and(|open| open.still(&reading));
                    // The record's word that the turn is over stands whatever the
                    // status says: busy while a background agent runs, after a turn
                    // an interrupt or a refusal ended with no count of them (T-604),
                    // once it has stood a moment.
                    let at_rest = at_prompt && (settled || empty || left) || by_press;
                    let over = self.held_over(settled && !at_rest);
                    observed(at_rest, over, waiting, by_press, at_prompt)
                }
            })
        })
    }

    fn interrupted(&self) {
        let over = self.asked.borrow().clone();
        *self.pressed.borrow_mut() = over.map(|over| Pressed::new(self.time.wall_ms(), over));
    }
}
