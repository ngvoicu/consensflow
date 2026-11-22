//! The pane operations the daemon asks of the app over the bridge, one set
//! for the window and the headless helper alike, and each pane's output and
//! exit on their way back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::Duration;

use portable_pty::PtySize;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::arbiter::{ArbiterSnapshot, InputArbiter, OutputClock};
use crate::ended::Ended;
use crate::input_queue::{wait_for_input_blocking, InputError, InputQueue};
use crate::output_hub::OutputHub;
use crate::pty::{
    validate_drop_env, Handed, PaneEnvironment, PaneError, PaneKey, PaneOutput, PaneTable,
    StreamedPane,
};
use crate::screen::PaneScreen;
use crate::validation::{pane_key, validate_input, validate_seq, validate_size, INVALID_BODY};
use cf_bridge::{Bridge, BridgeBuilder};
use cf_proto::panes::{
    BackgroundProcess, CommandsRequest, MarkRequest, PaneExit, PossiblyLeft, Rest, RestRequest,
    SilentPaste, SnapshotRequest, BOTTOM_LINES, TAIL_LINES,
};

const DEFAULT_BACKLOG_BYTES: usize = 1024 * 1024;
/// A bound on a page message, even if a pane has much more output waiting.
const MAX_OUTPUT_MESSAGE_BYTES: usize = 64 * 1024;
/// A screen update hashes the whole screen, so bound the number before a handoff too.
const MAX_OUTPUT_READS_PER_BATCH: usize = 4;

/// How long `pane.exit` waits, once a program has closed its terminal, for
/// the program's exit code to be readable: it closes the terminal a moment
/// before it has gone, and the event does not wait beyond this for a code.
const ENDED_WAIT: Duration = Duration::from_secs(1);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OpenRequest {
    /// The identity the daemon reserved for the window: it names every pane
    /// it opens, once.
    id: String,
    generation: u64,
    cwd: PathBuf,
    argv: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    drop_env: Vec<String>,
    #[serde(default)]
    size: SizeRequest,
    #[serde(default = "default_backlog_bytes")]
    backlog_bytes: usize,
    /// How the window's harness reads a human's paste, where its adapter says it reads one
    /// silently: the human's keys after a paste then wait until it has drawn it.
    #[serde(default)]
    silent_paste: Option<SilentPaste>,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct SizeRequest {
    rows: u16,
    cols: u16,
}

impl Default for SizeRequest {
    fn default() -> Self {
        Self { rows: 24, cols: 80 }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaneRequest {
    id: String,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BytesRequest {
    id: String,
    generation: u64,
    bytes: Vec<u8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasteRequest {
    id: String,
    generation: u64,
    body: String,
    /// The count of the human's keys (`pane.snapshot`'s `typed`) at which the
    /// window's adapter read its input box empty on the screen.
    #[serde(default, rename = "readEmpty")]
    read_empty: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClaimRequest {
    pane: String,
    generation: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AckRequest {
    id: String,
    generation: u64,
    seq: u64,
}

fn default_backlog_bytes() -> usize {
    DEFAULT_BACKLOG_BYTES
}

pub fn register_pane_handlers(
    builder: &mut BridgeBuilder,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    output: Arc<OutputHub>,
    inputs: Arc<InputQueue>,
) {
    register(builder, panes, arbiter, output, inputs, false);
}

/// [`register_pane_handlers`] for a host that may outlive its daemon and start another (the
/// app): a launch the daemon asked for is not opened once its bridge has closed, however late
/// its handler reaches the table. A launch is admitted when it is asked for, and the bridge's
/// closing stops no handler already admitted; without this one slow to begin (a loaded Mac)
/// inserted a pane of a daemon that had stopped, and nothing owned it. The headless host
/// does without: it ends every window it holds when its peer's input ends, the launches
/// admitted before waited for ([`crate::headless::shut_down`]).
pub fn register_fenced_pane_handlers(
    builder: &mut BridgeBuilder,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    output: Arc<OutputHub>,
    inputs: Arc<InputQueue>,
) {
    register(builder, panes, arbiter, output, inputs, true);
}

fn register(
    builder: &mut BridgeBuilder,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    output: Arc<OutputHub>,
    inputs: Arc<InputQueue>,
    fenced: bool,
) {
    let open_panes = Arc::clone(&panes);
    let open_arbiter = Arc::clone(&arbiter);
    let open_inputs = Arc::clone(&inputs);
    let open_output = Arc::clone(&output);
    builder.on_launch("pane.open", move |bridge, body| {
        let request: OpenRequest = parse_body(body)?;
        let key = validate_open_request(&request)?;
        let size = PtySize {
            rows: request.size.rows,
            cols: request.size.cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let streamed = match open_panes.open_streamed_at_while(
            // Asked with the table's lock held, at the insertion: a bridge that has
            // closed has no daemon to own the window.
            &|| !fenced || !bridge.is_closed(),
            key,
            &request.cwd,
            &request.argv,
            PaneEnvironment::new(&request.env, &request.drop_env),
            size,
            request.backlog_bytes,
        ) {
            Ok(streamed) => streamed,
            // The system would not start the program, and named why: the answer says so beside
            // its words, as a name of libuv's table and nothing else of anyone's.
            Err(error) if error.code().is_some() => {
                return Ok(json!({"ok":false,"error":error.to_string(),"code":error.code()}));
            }
            Err(error) => return Err(error.to_string()),
        };
        let printed = match open_arbiter
            .register_with(&streamed.key, request.silent_paste)
            .map_err(|error| error.to_string())
            .and_then(|printed| open_inputs.open(&streamed.key).map(|()| printed))
        {
            Ok(printed) => printed,
            Err(error) => {
                return Err(end_unregistered(
                    &open_panes,
                    &open_inputs,
                    &streamed.key,
                    error,
                ));
            }
        };
        let key = streamed.key.clone();
        // The window's process, so the daemon can find what the harness
        // writes about itself from the window's first moment.
        let mut opened = json!({"ok":true,"id":key.id,"generation":key.generation});
        if let Some(pid) = streamed.pid {
            opened["pid"] = json!(pid);
        }
        stream_to_page(
            streamed,
            bridge,
            Arc::clone(&open_panes),
            printed,
            Arc::clone(&open_inputs),
            Arc::clone(&open_output),
        );
        Ok(opened)
    });

    // How many alerts of the system's notification centre are on screen: a privacy dialog
    // raised in the app's name is one, and its being gone is how the daemon knows a window's wait
    // on it is over (T-1194). Null where the system gives no list of windows.
    builder.on("pane.alerts", |_bridge, _body| {
        Ok(json!({"ok":true,"alerts":crate::alerts::on_screen()}))
    });

    // Keys typed into a pane, in their order; and an emulator's replies (a page-less peer
    // answers a cursor query itself), past whatever the pane's keys wait on
    // (`InputWork::Reply`).
    for (operation, reply) in [("pane.input", false), ("pane.reply", true)] {
        let input_queue = Arc::clone(&inputs);
        builder.on(operation, move |_bridge, body| {
            let request: BytesRequest = parse_body(body)?;
            validate_input(&request.bytes)?;
            let key = pane_key(&request.id, request.generation)?;
            if reply {
                input_queue.reply(key, request.bytes)
            } else {
                input_queue.write(key, request.bytes)
            }
            .and_then(wait_for_input_blocking)
            .map_err(|error| error.to_string())?;
            Ok(json!({"ok":true}))
        });
    }

    // Every way a paste fails is an answer that says whether anything of it
    // reached the window, a request it could not read included.
    let paste_queue = Arc::clone(&inputs);
    builder.on("pane.write_paste", move |_bridge, body| {
        let pasted = paste_request(body).and_then(|(key, body, read_empty)| {
            let pending = paste_queue.paste(key, body.into_bytes(), read_empty)?;
            wait_for_input_blocking(pending)
        });
        Ok(match pasted {
            Ok(()) => json!({"ok":true}),
            Err(error) => error.paste_answer(),
        })
    });

    let claim_queue = Arc::clone(&inputs);
    builder.on("pane.claim", move |_bridge, body| {
        let request: ClaimRequest = parse_body(body)?;
        let key = pane_key(&request.pane, request.generation)?;
        claim_queue
            .claim(key)
            .and_then(wait_for_input_blocking)
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });

    let ack_panes = Arc::clone(&panes);
    builder.on("pane.ack", move |_bridge, body| {
        let request: AckRequest = parse_body(body)?;
        validate_seq(request.seq)?;
        ack_panes
            .ack(&pane_key(&request.id, request.generation)?, request.seq)
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });

    let kill_panes = Arc::clone(&panes);
    let kill_inputs = Arc::clone(&inputs);
    builder.on("pane.kill", move |_bridge, body| {
        let request: PaneRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        // The daemon is done with the window whatever its kill answers: its
        // input goes with it, and the daemon hears the failure.
        let killed = kill_panes.kill(&key);
        kill_inputs.retire(&key);
        killed.map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });

    // A turn is about to begin in a window: what its tree has now came before
    // it, and so does what its harness's servers start later, where the daemon
    // says how the harness keeps them, and the programs the harness is named for.
    let mark_panes = Arc::clone(&panes);
    builder.on("pane.mark", move |_bridge, body| {
        let request: MarkRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        mark_panes
            .mark(&key, request.servers, request.harness.as_deref())
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });

    // What the window's tree has running that is the turn's work: what began after
    // its last mark and the host can prove its; and what may be the window's outside it.
    let background_panes = Arc::clone(&panes);
    builder.on("pane.background", move |_bridge, body| {
        let request: PaneRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        let background = background_panes
            .background(&key)
            .map_err(|error| error.to_string())?;
        let mut answer =
            json!({"ok":true,"total":background.total,"processes":background.processes});
        // Only where it names any, as an exit says it.
        if !background.possibly_left.is_empty() {
            answer["possiblyLeft"] = json!(background.possibly_left);
        }
        Ok(answer)
    });

    // Which commands of the window's harness run in its tree now, less what ran at the
    // rest the request names, where the host can tell them: `null` where it cannot, said
    // `unreadable` where a process of the window cannot be read.
    let commands_panes = Arc::clone(&panes);
    builder.on("pane.commands", move |_bridge, body| {
        let request: CommandsRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        let answer = commands_panes
            .commands(&key, request.since, request.after)
            .map_err(|error| error.to_string())?;
        let mut body = serde_json::to_value(&answer).map_err(|error| error.to_string())?;
        body["ok"] = json!(true);
        Ok(body)
    });

    // The window is at rest: what its tree runs now is kept, under the number answered,
    // for a look at its commands to leave out; `null` where the host tells none of them.
    let rest_panes = Arc::clone(&panes);
    builder.on("pane.rest", move |_bridge, body| {
        let request: RestRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        let rest: Rest = rest_panes
            .rest(&key, &request.keep)
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true,"rest":rest}))
    });

    let list_panes = Arc::clone(&panes);
    builder.on("pane.list", move |_bridge, body| {
        let _: EmptyBody = parse_body(body)?;
        let panes = list_panes
            .list()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|pane| {
                Ok(json!({
                    "id":pane.id,
                    "generation":pane.generation,
                    "alive":pane.alive,
                    "idleMs":pane.idle_ms,
                    "processGroupId":group_id_answer(list_panes.process_group_id(&PaneKey { id: pane.id.clone(), generation: pane.generation }))?,
                }))
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(json!({"ok":true,"panes":panes}))
    });

    let snapshot_arbiter = Arc::clone(&arbiter);
    let snapshot_panes = Arc::clone(&panes);
    builder.on("pane.snapshot", move |_bridge, body| {
        let request: SnapshotRequest = parse_body(body)?;
        let key = pane_key(&request.id, request.generation)?;
        let snapshot = snapshot_arbiter
            .snapshot(&key)
            .map_err(|error| error.to_string())?;
        let mut answer = snapshot_answer(&snapshot);
        // The screen's last lines, for a request that asks for them. A pane
        // the table no longer holds has none to give.
        if let Some(lines) = request.tail {
            if let Some(tail) = screen_read(snapshot_panes.tail(&key, lines.min(TAIL_LINES)))? {
                answer["tail"] = json!(tail);
            }
        }
        // The screen's last lines as drawn, for an adapter that reads its
        // harness's input box.
        if let Some(lines) = request.bottom {
            if let Some(drawn) = screen_read(snapshot_panes.bottom(&key, lines.min(BOTTOM_LINES)))?
            {
                answer["bottom"] = json!(drawn.lines);
                answer["cursor"] = drawn.cursor.map_or(Value::Null, |cursor| {
                    json!({
                        "line": cursor.line, "col": cursor.col, "shown": cursor.shown,
                        "plainAfter": cursor.plain_after,
                    })
                });
            }
        }
        Ok(answer)
    });
}

/// A launch whose input could not be registered ends its window, and the answer says how
/// that went: the cause alone when the window is gone (one already retired is), the cause
/// and the failed ending when it is not proved gone, so that the caller does not take a
/// window it cannot see for one that was cleaned up. Both go to the log.
fn end_unregistered(
    panes: &PaneTable,
    inputs: &InputQueue,
    key: &PaneKey,
    cause: String,
) -> String {
    let ended = match panes.kill(key) {
        Ok(()) | Err(PaneError::NotFound(_)) => None,
        Err(error) => Some(error),
    };
    inputs.retire(key);
    match ended {
        None => cause,
        Some(error) => {
            let said = format!(
                "{cause}; and the window {}#{} could not be ended: {error}",
                key.id, key.generation
            );
            say(&said);
            said
        }
    }
}

#[allow(clippy::print_stderr)] // The pane host's log is its error output.
fn say(line: &str) {
    eprintln!("consensflow-bridge: {line}");
}

/// A pane's process group for `pane.list`: `null` where it has none to give (a pane
/// retired since the list was read, or a host with no process groups), an error for any
/// other failure.
fn group_id_answer(read: Result<i32, PaneError>) -> Result<Value, String> {
    match read {
        Ok(group) => Ok(json!(group)),
        Err(PaneError::NotFound(_) | PaneError::Pty(_)) => Ok(Value::Null),
        Err(error) => Err(error.to_string()),
    }
}

/// A screen read of `pane.snapshot`: absent for a pane the table no longer holds, an error
/// for any other failure, so that an unreadable screen is not taken for one the request
/// did not get.
fn screen_read<T>(read: Result<T, PaneError>) -> Result<Option<T>, String> {
    match read {
        Ok(value) => Ok(Some(value)),
        Err(PaneError::NotFound(_)) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

/// What `pane.snapshot` says of a pane's input: the human's last writes as how long ago,
/// how many bytes and of what kinds, never a byte of them (`HumanWrite`).
fn snapshot_answer(snapshot: &ArbiterSnapshot) -> Value {
    let writes: Vec<Value> = snapshot
        .human_writes
        .iter()
        .map(|write| {
            json!({ "agoMs": write.ago_ms, "length": write.length, "classes": write.classes })
        })
        .collect();
    json!({
        "ok":true,
        "generation":snapshot.generation,
        "pasteInFlight":snapshot.paste_in_flight,
        "inputFailed":snapshot.input_failed,
        "unsent":snapshot.unsent,
        "typed":snapshot.typed,
        "typedQuietMs":snapshot.typed_quiet_ms,
        "outputQuietMs":snapshot.output_quiet_ms,
        "screenQuietMs":snapshot.screen_quiet_ms,
        "pasteQuietMs":snapshot.paste_quiet_ms,
        "humanWrites":writes,
    })
}

/// The human's own keys over the bridge (`pane.typed`), for the headless host,
/// which has no page: its peer types as the window's page does
/// (`pane_input_enqueue`), and the keys are counted for what they leave unsent.
/// The window never takes it: there, the bridge's peer is the daemon, whose
/// keys are never the human's; nor does the headless host, but where the rig
/// asks for it (`headless::HUMAN_KEYS`).
pub(crate) fn register_typed(builder: &mut BridgeBuilder, inputs: Arc<InputQueue>) {
    builder.on("pane.typed", move |_bridge, body| {
        let request: BytesRequest = parse_body(body)?;
        validate_input(&request.bytes)?;
        let key = pane_key(&request.id, request.generation)?;
        inputs
            .typed(key, request.bytes)
            .and_then(wait_for_input_blocking)
            .map_err(|error| error.to_string())?;
        Ok(json!({"ok":true}))
    });
}

/// What a test is told at the moment a `pane.exit` is handed to the bridge, which
/// sends it later: the pane it is of. A test of the order of the end of a program and
/// the daemon's word of it watches this, not when the word arrives.
#[cfg(test)]
pub(crate) static EXIT_HANDED: std::sync::Mutex<Option<ExitHanded>> = std::sync::Mutex::new(None);

/// What [`EXIT_HANDED`] holds.
#[cfg(test)]
pub(crate) type ExitHanded = Box<dyn Fn(&PaneKey) + Send>;

/// Join a few reads already waiting. Take them before feeding the screen, so more
/// reads arriving during screen work cannot extend the batch. The first that would
/// cross the byte bound is kept for the next handoff. The last sequence acknowledges
/// every read in the batch.
fn next_output(
    stream: &mpsc::Receiver<PaneOutput>,
    pending: &mut Option<PaneOutput>,
    mut feed: impl FnMut(&[u8]),
) -> Option<PaneOutput> {
    let mut message = pending.take().or_else(|| stream.recv().ok())?;
    let mut size = message.bytes.len();
    let mut joined = Vec::new();
    while joined.len() + 1 < MAX_OUTPUT_READS_PER_BATCH {
        let Ok(next) = stream.try_recv() else {
            break;
        };
        if next.bytes.len() > MAX_OUTPUT_MESSAGE_BYTES.saturating_sub(size) {
            *pending = Some(next);
            break;
        }
        size += next.bytes.len();
        joined.push(next);
    }
    feed(&message.bytes);
    for next in joined {
        feed(&next.bytes);
        message.bytes.extend_from_slice(&next.bytes);
        message.seq = next.seq;
    }
    // Extending a Vec can reserve more than its contents. The page's message,
    // like each read waiting in the channel, holds only the bytes it contains.
    message.bytes = message.bytes.into_boxed_slice().into_vec();
    Some(message)
}

/// A read of the window's output, taken into its screen, and the clocks told: it printed
/// now, and whether it changed what the screen shows. One call for each read, in the
/// order the reads came: the paste's Enter and the engine's quiet checks read these.
fn took_in(screen: &PaneScreen, printed: &OutputClock, bytes: &[u8]) {
    let changed = screen.feed(bytes);
    printed.note(changed);
}

fn forward_output(
    stream: &mpsc::Receiver<PaneOutput>,
    mut feed: impl FnMut(&[u8]),
    mut publish: impl FnMut(PaneOutput),
) {
    let mut pending = None;
    while let Some(message) = pending.take().or_else(|| stream.recv().ok()) {
        // A ready byte goes on after its own screen update, before any work on
        // another read. In particular, a writer that was idle must not wait for
        // reads that arrive while this update is in progress.
        feed(&message.bytes);
        publish(message);
        // There is no wait for another read. When output is already queued, join
        // the next few reads before going back to the immediate handoff above.
        if let Ok(next) = stream.try_recv() {
            pending = Some(next);
            if let Some(batch) = next_output(stream, &mut pending, &mut feed) {
                publish(batch);
            }
        }
    }
}

/// A pane's output, on to the page and into its screen, and its end, which is
/// for good (`pty`'s `output_end`): `pane.exit`, which says how the program
/// ended and what the screen showed last, once the program is proven gone (one
/// that outlived its output, on Windows a console host that died, is ended first),
/// or names it as one that could not be ended; for the daemon forgets a window it
/// is told has closed and may open another on its conversation. And the pane
/// leaves the table.
fn stream_to_page(
    streamed: StreamedPane,
    bridge: Bridge,
    panes: Arc<PaneTable>,
    printed: Arc<OutputClock>,
    inputs: Arc<InputQueue>,
    output: Arc<OutputHub>,
) {
    thread::spawn(move || {
        let StreamedPane {
            key,
            pid,
            output: stream,
            screen,
            ended,
            // Paid, with this thread's end, once the exit is told.
            owed: _owed,
            #[cfg(target_os = "macos")]
            tree,
        } = streamed;
        forward_output(
            &stream,
            |bytes| took_in(&screen, &printed, bytes),
            |message| output.publish(message.into()),
        );
        let status = panes.wait_ended(&key, &ended, ENDED_WAIT);
        // What the program left running goes with the exit, before the close
        // that follows ends it.
        let background = panes.left_running_at_exit(&key, &ended);
        let told = |handed: Handed| {
            // How it ended by itself, where that came after the output did; not the
            // host's own end of it.
            let status = status.or_else(|| (!handed.ended_by_host).then(|| ended.get()).flatten());
            // What may be the window's and runs on, which no close signals: named as the
            // program is proven gone, which the close's ending of what is the window's does
            // not change.
            let left = Left {
                background,
                possibly: possibly_left(
                    #[cfg(target_os = "macos")]
                    &tree,
                ),
            };
            let exit = exit_body(&key, &screen, status, left, handed.survivor);
            #[cfg(test)]
            if let Some(handed) = EXIT_HANDED
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .as_ref()
            {
                handed(&key);
            }
            let _ = bridge.event("pane.exit", exit);
        };
        if matches!(panes.output_ended(&key, &ended, pid, told), Ok(true)) {
            inputs.retire(&key);
        }
    });
}

/// What may be the window's and runs on, not ended, for its exit to name: on macOS what its
/// process tree names (`ProcessTree::possibly_left`); elsewhere none, and on Windows the
/// pane's job ends its whole tree as its last handle closes, with the pane.
fn possibly_left(
    #[cfg(target_os = "macos")] tree: &crate::process_tree::ProcessTree,
) -> Vec<PossiblyLeft> {
    #[cfg(target_os = "macos")]
    {
        tree.possibly_left()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Vec::new()
    }
}

/// What a window left running, for its exit: the turn's work, which its close ended, and
/// what may be the window's and was not ended.
struct Left {
    background: Vec<BackgroundProcess>,
    possibly: Vec<PossiblyLeft>,
}

/// `pane.exit`'s body: the pane, how its program ended where that was read,
/// the last lines its screen showed, what it left running, and the program
/// where it could not be proven gone.
fn exit_body(
    key: &PaneKey,
    screen: &PaneScreen,
    ended: Option<Ended>,
    left: Left,
    survivor: Option<u32>,
) -> Value {
    let exit = PaneExit {
        id: key.id.clone(),
        generation: key.generation,
        exit_code: ended.as_ref().map(|ended| ended.code),
        signal: ended.and_then(|ended| ended.signal),
        tail: Some(screen.tail(TAIL_LINES)),
        background: left.background,
        possibly_left: left.possibly,
        survivor,
    };
    serde_json::to_value(&exit).unwrap_or_else(|_| json!({"id":key.id,"generation":key.generation}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

fn parse_body<T: DeserializeOwned>(body: Value) -> Result<T, String> {
    serde_json::from_value(body).map_err(|error| format!("{INVALID_BODY}: {error}"))
}

/// A paste's pane and body. A request that does not name them was refused
/// before anything was written.
fn paste_request(body: Value) -> Result<(PaneKey, String, Option<u64>), InputError> {
    let refused = |cause| InputError::refused(INVALID_BODY, cause);
    let request: PasteRequest = parse_body(body).map_err(refused)?;
    let key = pane_key(&request.id, request.generation).map_err(refused)?;
    Ok((key, request.body, request.read_empty))
}

/// Everything a `pane.open` asks for, checked before anything is spawned; the
/// answer is the pane's key.
fn validate_open_request(request: &OpenRequest) -> Result<PaneKey, String> {
    let key = pane_key(&request.id, request.generation)?;
    if !request.cwd.is_absolute() {
        return Err("pane cwd must be absolute".to_string());
    }
    if request.argv.is_empty() || !Path::new(&request.argv[0]).is_absolute() {
        return Err("pane argv[0] must be absolute".to_string());
    }
    if request.backlog_bytes == 0 {
        return Err("backlogBytes must be greater than zero".to_string());
    }
    validate_drop_env(&request.drop_env).map_err(|error| error.to_string())?;
    validate_size(request.size.cols, request.size.rows)?;
    Ok(key)
}

#[cfg(test)]
#[cfg(windows)]
mod windows_tests;

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::arbiter::EnterTiming;
    #[cfg(unix)]
    use cf_proto::bridge::Role;
    #[cfg(unix)]
    use std::sync::mpsc;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    use crate::headless::MAX_FRAME_BYTES;
    #[cfg(unix)]
    use crate::output_hub::PaneOutputMessage;

    /// The sink accepts output, but processing the second read is held. A ready first
    /// read must reach the sink before the publisher begins work on the second.
    fn first_read_is_published_before_slow_second_read(online: bool) {
        use std::sync::mpsc;
        use std::time::Duration;

        let (sender, stream) = mpsc::channel();
        let key = PaneKey::new("latency", 1);
        sender
            .send(PaneOutput {
                key: key.clone(),
                seq: 1,
                bytes: vec![b'a'],
            })
            .unwrap();
        if !online {
            sender
                .send(PaneOutput {
                    key: key.clone(),
                    seq: 2,
                    bytes: vec![b'b'],
                })
                .unwrap();
        }
        let (entered, seen) = mpsc::channel();
        let (release, held) = mpsc::channel();
        let (published, received) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            forward_output(
                &stream,
                |bytes| {
                    entered.send(bytes[0]).unwrap();
                    held.recv_timeout(Duration::from_secs(5))
                        .expect("release the screen feed");
                },
                |message| {
                    published.send(message).unwrap();
                },
            );
        });
        assert_eq!(seen.recv_timeout(Duration::from_secs(5)).unwrap(), b'a');
        if online {
            sender
                .send(PaneOutput {
                    key,
                    seq: 2,
                    bytes: vec![b'b'],
                })
                .unwrap();
        }
        drop(sender);
        release.send(()).unwrap();
        assert_eq!(seen.recv_timeout(Duration::from_secs(5)).unwrap(), b'b');
        let first = received.recv_timeout(Duration::from_millis(250));
        release.send(()).unwrap();
        worker.join().expect("output thread ends");
        let first = first.expect("the first byte waited for another read's screen update");
        assert_eq!((first.seq, first.bytes.as_slice()), (1, &b"a"[..]));
    }

    #[test]
    fn tiny_read_queued_first_byte_is_published_before_later_screen_work() {
        first_read_is_published_before_slow_second_read(false);
    }

    #[test]
    fn tiny_read_online_first_byte_is_published_before_a_new_read_is_fed() {
        first_read_is_published_before_slow_second_read(true);
    }

    #[test]
    fn tiny_read_batch_limits_how_many_screen_updates_precede_publication() {
        let (sender, stream) = std::sync::mpsc::channel();
        let key = PaneKey::new("latency-bound", 1);
        for seq in 1..=4096 {
            sender
                .send(PaneOutput {
                    key: key.clone(),
                    seq,
                    bytes: vec![b'x'],
                })
                .unwrap();
        }
        drop(sender);
        let mut fed = 0;
        let first = next_output(&stream, &mut None, |_| fed += 1).unwrap();
        assert_eq!(fed, 4, "a batch has a fixed bound on screen updates");
        assert_eq!(first.bytes.len(), fed);
    }

    /// A clock that moves only when the test moves it.
    struct Manual {
        base: std::time::Instant,
        elapsed: std::sync::Mutex<Duration>,
    }

    impl Manual {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                base: std::time::Instant::now(),
                elapsed: std::sync::Mutex::new(Duration::ZERO),
            })
        }

        fn advance(&self, by: Duration) {
            *self.elapsed.lock().unwrap() += by;
        }
    }

    impl crate::arbiter::Clock for Manual {
        fn now(&self) -> std::time::Instant {
            self.base + *self.elapsed.lock().unwrap()
        }

        fn rest(&self, duration: Duration) {
            self.advance(duration);
        }
    }

    /// Queue `reads` as a pane's output and forward them as the host does, each read
    /// taking `each` of the clock's time to take in. `between` runs before each read is
    /// taken in; `during` is told, before each read and at each publication, the quiet
    /// times the arbiter reports then: (output, screen).
    fn forwarded_with_clocks(
        reads: &[&[u8]],
        each: Duration,
        mut between: impl FnMut(&PaneScreen, usize),
        during: impl FnMut(&str, usize, (Option<u64>, Option<u64>)),
    ) {
        let during = std::cell::RefCell::new(during);
        let clock = Manual::new();
        let arbiter =
            InputArbiter::with_clock(crate::arbiter::EnterTiming::fixed(0), clock.clone());
        let key = PaneKey::new("clocks", 1);
        let printed = arbiter.register(&key).expect("register clock");
        let screen = PaneScreen::new(24, 80);
        let (sender, stream) = std::sync::mpsc::channel();
        for (seq, bytes) in reads.iter().enumerate() {
            sender
                .send(PaneOutput {
                    key: key.clone(),
                    seq: seq as u64 + 1,
                    bytes: bytes.to_vec(),
                })
                .unwrap();
        }
        drop(sender);
        let quiet = || {
            let snapshot = arbiter.snapshot(&key).expect("snapshot");
            (snapshot.output_quiet_ms, snapshot.screen_quiet_ms)
        };
        let taken = std::cell::Cell::new(0);
        forward_output(
            &stream,
            |bytes| {
                let at = taken.get();
                taken.set(at + 1);
                (during.borrow_mut())("before a read", at, quiet());
                between(&screen, at);
                clock.advance(each);
                took_in(&screen, &printed, bytes);
            },
            |message| (during.borrow_mut())("publication", message.seq as usize, quiet()),
        );
    }

    /// Each read stamps the output clock as it is taken in, as before reads were joined,
    /// so a batch that takes long still prints as it goes: the paste's Enter waits for
    /// the quiet of the last read, not of the first (T-1941 round 2).
    #[test]
    fn tiny_read_every_read_of_a_batch_stamps_the_output_clock_as_it_is_taken_in() {
        let reads: Vec<&[u8]> = vec![b"x"; 4];
        let mut seen = Vec::new();
        forwarded_with_clocks(
            &reads,
            Duration::from_millis(100),
            |_, _| {},
            |when, at, quiet| seen.push((when.to_owned(), at, quiet.0)),
        );
        for (when, at, output) in &seen {
            if when == "publication" {
                assert_eq!(*output, Some(0), "at the publication of read {at}");
            } else if *at > 0 {
                assert_eq!(
                    *output,
                    Some(0),
                    "before read {at}: the one before it printed"
                );
            }
        }
        assert_eq!(
            seen.iter().filter(|seen| seen.0 == "publication").count(),
            2
        );
    }

    /// A screen that changes and changes back across the reads of a batch moves the
    /// screen's clock with each read, as it did when each read was hashed alone: 70
    /// cycles of a spinner's frame drawn and erased.
    #[test]
    fn tiny_read_a_screen_that_changes_and_changes_back_across_reads_moves_the_screen_clock() {
        // The first read alone, then batches of four reads that each end on the
        // screen they began with.
        let mut reads: Vec<&[u8]> = vec![b"\x1b[0m"];
        reads.extend((0..70).flat_map(|_| [&b"|"[..], b"\r \r", b"|", b"\r \r"]));
        let mut at_publication = Vec::new();
        forwarded_with_clocks(
            &reads,
            Duration::from_millis(10),
            |_, _| {},
            |when, _, quiet| {
                if when == "publication" {
                    at_publication.push(quiet.1);
                }
            },
        );
        assert!(at_publication.len() >= 2);
        assert_eq!(at_publication[0], None, "the first read changed nothing");
        assert!(
            at_publication[1..].iter().all(|screen| *screen == Some(0)),
            "the last read of every batch changed the screen: {at_publication:?}"
        );
    }

    /// A resize between two reads of a batch leaves the change the reads before it made
    /// in the screen's clock, and a resize alone changes nothing (T-1941 round 2).
    #[test]
    fn tiny_read_a_resize_between_reads_keeps_the_change_made_before_it_and_adds_none() {
        let reads: Vec<&[u8]> = vec![b"\x1b[0m", b"Trust this folder?", b"\x1b[0m", b"\x1b[0m"];
        let mut at_publication = Vec::new();
        forwarded_with_clocks(
            &reads,
            Duration::from_millis(10),
            |screen, at| {
                if at == 2 {
                    screen.resize(25, 81);
                }
            },
            |when, at, quiet| {
                if when == "publication" && at > 1 {
                    at_publication.push(quiet.1);
                }
            },
        );
        // The change was the batch's first read's: two reads ago.
        assert_eq!(at_publication, [Some(20)], "the batch changed the screen");

        let reads: Vec<&[u8]> = vec![b"\x1b[0m", b"\x1b[1m", b"\x1b[0m"];
        let mut at_publication = Vec::new();
        forwarded_with_clocks(
            &reads,
            Duration::from_millis(10),
            |screen, at| {
                if at == 1 {
                    screen.resize(25, 81);
                }
            },
            |when, at, quiet| {
                if when == "publication" && at > 1 {
                    at_publication.push(quiet.1);
                }
            },
        );
        assert_eq!(at_publication, [None], "a resize alone shows nothing");
    }

    #[test]
    fn tiny_read_batch_does_not_grow_while_its_screen_is_fed() {
        let (sender, stream) = std::sync::mpsc::channel();
        let key = PaneKey::new("snapshot", 1);
        sender
            .send(PaneOutput {
                key: key.clone(),
                seq: 1,
                bytes: vec![b'a'],
            })
            .unwrap();
        let (entered, seen) = std::sync::mpsc::channel();
        let (release, held) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            next_output(&stream, &mut None, |bytes| {
                if bytes == b"a" {
                    entered.send(()).unwrap();
                    held.recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                }
            })
            .unwrap()
        });
        seen.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        sender
            .send(PaneOutput {
                key,
                seq: 2,
                bytes: vec![b'b'],
            })
            .unwrap();
        drop(sender);
        release.send(()).unwrap();
        let batch = worker.join().expect("batch thread");
        assert_eq!((batch.seq, batch.bytes.as_slice()), (1, &b"a"[..]));
    }

    #[test]
    fn tiny_read_batches_obey_the_bound_and_keep_the_last_sequence() {
        let (sender, stream) = std::sync::mpsc::channel();
        let key = PaneKey::new("burst", 1);
        for (seq, byte, len) in [(1, b'a', 40 * 1024), (2, b'b', 40 * 1024), (3, b'c', 1)] {
            sender
                .send(PaneOutput {
                    key: key.clone(),
                    seq,
                    bytes: vec![byte; len],
                })
                .unwrap();
        }
        drop(sender);
        let mut pending = None;
        let first = next_output(&stream, &mut pending, |_| {}).expect("first batch");
        let second = next_output(&stream, &mut pending, |_| {}).expect("second batch");
        assert!(next_output(&stream, &mut pending, |_| {}).is_none());
        assert_eq!((first.seq, first.bytes), (1, vec![b'a'; 40 * 1024]));
        let mut expected = vec![b'b'; 40 * 1024];
        expected.push(b'c');
        assert_eq!(
            second.seq, 3,
            "the cumulative acknowledgement covers both reads"
        );
        assert_eq!(second.bytes.capacity(), expected.len());
        assert_eq!(second.bytes, expected);
    }

    #[test]
    fn tiny_read_is_published_without_waiting_for_another_read() {
        let (sender, stream) = std::sync::mpsc::channel();
        sender
            .send(PaneOutput {
                key: PaneKey::new("interactive", 1),
                seq: 1,
                bytes: vec![b'x'],
            })
            .unwrap();
        let (done, result) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let message = next_output(&stream, &mut None, |_| {}).expect("the first byte");
            done.send(message).unwrap();
        });
        let message = result.recv_timeout(std::time::Duration::from_secs(5));
        drop(sender);
        worker.join().expect("reader thread");
        let message = message.expect("an interactive byte waited for another read");
        assert_eq!(message.bytes, b"x");
        assert_eq!(message.seq, 1);
    }

    #[test]
    fn tiny_read_batch_keeps_each_screens_change_boundary() {
        let (sender, stream) = std::sync::mpsc::channel();
        let key = PaneKey::new("screen", 1);
        for (seq, bytes) in [(1, &b"a"[..]), (2, &b"\r \r"[..])] {
            sender
                .send(PaneOutput {
                    key: key.clone(),
                    seq,
                    bytes: bytes.to_vec(),
                })
                .unwrap();
        }
        drop(sender);
        let screen = PaneScreen::new(24, 80);
        let mut changes = Vec::new();
        let message = next_output(&stream, &mut None, |bytes| changes.push(screen.feed(bytes)))
            .expect("batch");
        assert_eq!(message.bytes, b"a\r \r");
        assert_eq!(
            changes,
            [true, true],
            "the screen changed and then changed back"
        );
    }

    /// Start the page's output thread only after a real pane's burst and exit have
    /// reached its channel. This makes the burst deterministic, including its last
    /// output, without delaying an interactive window in production.
    #[cfg(unix)]
    #[test]
    fn tiny_read_queued_burst_is_joined_in_order_before_the_exit() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let output = Arc::new(OutputHub::new());
        let (published, received) = mpsc::channel();
        output.register_sink(Arc::new(move |message: PaneOutputMessage| {
            published.send(message).is_ok()
        }));

        let (host, mut peer) = UnixStream::pair().expect("bridge socket pair");
        peer.write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("bridge handle");
        peer.set_read_timeout(Some(Duration::from_secs(30)))
            .unwrap();
        let connected = BridgeBuilder::new(Role::Host, MAX_FRAME_BYTES)
            .connect(host.try_clone().unwrap(), host)
            .expect("connect bridge");

        let pattern = "0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ-_";
        let script = format!("i=0; while [ $i -lt 2048 ]; do printf '{pattern}'; i=$((i+1)); done");
        let mut streamed = panes
            .open_streamed(
                Path::new("/tmp"),
                &["/bin/sh".to_owned(), "-c".to_owned(), script],
                PaneEnvironment::new(&HashMap::new(), &[]),
                PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                DEFAULT_BACKLOG_BYTES,
            )
            .expect("open burst pane");
        let mut reads = Vec::new();
        loop {
            match streamed.output.recv_timeout(Duration::from_secs(30)) {
                Ok(message) => reads.push(message),
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(error) => panic!("the burst did not finish: {error}"),
            }
        }
        let read_count = reads.len();
        let (sender, replay) = mpsc::channel();
        for message in reads {
            sender.send(message).unwrap();
        }
        drop(sender);
        streamed.output = replay;
        let clock = arbiter.register(&streamed.key).expect("register clock");
        stream_to_page(
            streamed,
            connected.bridge.clone(),
            Arc::clone(&panes),
            clock,
            inputs,
            output,
        );

        let mut reader = BufReader::new(peer);
        loop {
            let mut line = String::new();
            assert_ne!(reader.read_line(&mut line).expect("bridge event"), 0);
            let frame: Value = serde_json::from_str(&line).expect("event JSON");
            if frame["op"] == "pane.exit" {
                break;
            }
        }
        let messages: Vec<_> = received.try_iter().collect();
        let mut bytes = Vec::new();
        let mut sequences = Vec::new();
        for message in &messages {
            assert!(message.bytes.len() <= 64 * 1024);
            bytes.extend_from_slice(&message.bytes);
            sequences.push(
                serde_json::to_value(message).unwrap()["seq"]
                    .as_u64()
                    .unwrap(),
            );
        }
        drop(reader);
        connected.bridge.wait_closed().expect("bridge closes");

        assert_eq!(
            bytes,
            pattern.repeat(2048).as_bytes(),
            "every byte precedes exit in order"
        );
        assert!(sequences.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(
            read_count >= 16,
            "the burst did not produce enough reads: {read_count}"
        );
        assert!(
            messages.len() * 2 < read_count,
            "{} reads became {} messages",
            read_count,
            messages.len()
        );
    }

    /// What a test of a window that repaints what it shows reads of it, on either
    /// platform's pane host (and `windows_tests`' twin).
    pub(super) mod quiet_by_screen {
        use std::time::{Duration, Instant};

        use serde_json::Value;

        /// What the stand-in's box reads before its character is changed, and after.
        pub(crate) const BOX: &str = "> Ask anything";
        pub(crate) const CHANGED: &str = "> Tsk anything";

        /// Two looks at the stand-in: two seconds after its box shows, while it repaints
        /// it; and once its character has changed. `look` is a `pane.snapshot` with its
        /// tail, and `shows` whether a snapshot's tail holds a line.
        pub(crate) fn watch(
            look: &mut dyn FnMut() -> Value,
            shows: &dyn Fn(&Value, &str) -> bool,
        ) -> (Value, Value) {
            let mut until = |text: &str| {
                let deadline = Instant::now() + Duration::from_secs(45);
                loop {
                    let snapshot = look();
                    if shows(&snapshot, text) {
                        return snapshot;
                    }
                    assert!(Instant::now() < deadline, "{text} never showed: {snapshot}");
                    std::thread::sleep(Duration::from_millis(50));
                }
            };
            until(BOX);
            std::thread::sleep(Duration::from_secs(2));
            let repainting = until(BOX);
            (repainting, until(CHANGED))
        }

        /// The stand-in's screen was quiet while it repainted its box, though (where
        /// `printed_on`, as on a Unix pty, which passes every byte) its output never was;
        /// and its changed character is a change.
        pub(crate) fn check(repainting: &Value, changed: &Value, printed_on: bool) {
            let quiet = |snapshot: &Value, name: &str| snapshot[name].as_u64();
            if printed_on {
                assert!(
                    quiet(repainting, "outputQuietMs").is_some_and(|ms| ms < 1_000),
                    "the stand-in printed all along: {repainting}"
                );
            }
            assert!(
                quiet(repainting, "screenQuietMs").is_some_and(|ms| ms >= 1_500),
                "a screen printed again as it is, is quiet: {repainting}"
            );
            assert!(
                quiet(changed, "screenQuietMs").is_some_and(|ms| ms < 1_500),
                "a character changed is a change: {changed}"
            );
        }
    }

    #[test]
    fn open_request_requires_absolute_launch_inputs() {
        // A directory and a program that are absolute here: `/tmp` and
        // `/bin/sh` are not, on Windows.
        let (directory, program) = if cfg!(windows) {
            ("C:\\Windows", "C:\\Windows\\System32\\cmd.exe")
        } else {
            ("/tmp", "/bin/sh")
        };
        let relative: OpenRequest = parse_body(json!({
            "id":"p-1",
            "generation":1,
            "cwd":"relative",
            "argv":["sh"],
            "size":{"rows":24,"cols":80},
        }))
        .unwrap();
        assert!(validate_open_request(&relative).is_err());

        let absolute: OpenRequest = parse_body(json!({
            "id":"p-1",
            "generation":1,
            "cwd":directory,
            "argv":[program],
            "dropEnv":["OPENAI_API_KEY"],
            "size":{"rows":24,"cols":80},
        }))
        .unwrap();
        assert_eq!(validate_open_request(&absolute), Ok(PaneKey::new("p-1", 1)));

        assert!(parse_body::<OpenRequest>(json!({
            "id":"p-1",
            "cwd":directory,
            "argv":[program],
            "size":{"rows":24,"cols":80},
        }))
        .is_err());
        let first_generation: OpenRequest = parse_body(json!({
            "id":"p-1",
            "generation":0,
            "cwd":directory,
            "argv":[program],
        }))
        .unwrap();
        assert!(validate_open_request(&first_generation).is_err());

        let invalid_drop_env: OpenRequest = parse_body(json!({
            "id":"p-1",
            "generation":1,
            "cwd":directory,
            "argv":[program],
            "dropEnv":["BAD=NAME"],
        }))
        .unwrap();
        assert!(validate_open_request(&invalid_drop_env)
            .unwrap_err()
            .contains("environment variable name"));

        assert!(parse_body::<OpenRequest>(json!({
            "id":"p-1",
            "generation":1,
            "cwd":directory,
            "argv":[program],
            "dropEnv":[],
            "silentlyIgnoredSecurityField":true,
        }))
        .is_err());
    }

    /// The human's last writes are said as how long ago, how many bytes and of what kinds
    /// (Calliope's T-844, T-883).
    #[test]
    fn a_snapshot_says_the_humans_last_writes_by_age_length_and_kinds() {
        let snapshot = ArbiterSnapshot {
            generation: 1,
            paste_in_flight: false,
            input_failed: false,
            unsent: true,
            typed: 2,
            typed_quiet_ms: Some(40),
            output_quiet_ms: None,
            screen_quiet_ms: None,
            paste_quiet_ms: None,
            human_writes: vec![
                crate::arbiter::HumanWrite {
                    ago_ms: 90,
                    length: 25,
                    classes: vec!["string"],
                },
                crate::arbiter::HumanWrite {
                    ago_ms: 40,
                    length: 5,
                    classes: vec!["text", "enter"],
                },
            ],
        };
        let answer = super::snapshot_answer(&snapshot);
        assert_eq!(
            answer["humanWrites"],
            json!([
                { "agoMs": 90, "length": 25, "classes": ["string"] },
                { "agoMs": 40, "length": 5, "classes": ["text", "enter"] },
            ])
        );
        assert_eq!(
            (&answer["typed"], &answer["typedQuietMs"], &answer["unsent"]),
            (&json!(2), &json!(40), &json!(true))
        );
    }

    /// The daemon asks how many alerts of the system's notification centre are on screen (a
    /// privacy dialog is one) to end the wait it holds a window in (T-1194): the host answers a
    /// number, or null where the system gives no list of windows.
    #[cfg(unix)]
    #[test]
    fn the_host_says_how_many_alerts_the_system_has_on_screen() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            arbiter,
            Arc::new(OutputHub::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut frame = serde_json::to_vec(
            &json!({"v":1,"id":"n-alerts-1","kind":"req","op":"pane.alerts","body":{}}),
        )
        .expect("serialize request");
        frame.push(b'\n');
        node_stream.write_all(&frame).expect("write request");
        let answer = loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("read response");
            let frame: Value = serde_json::from_str(line.trim()).expect("response JSON");
            if frame["kind"] == "res" && frame["id"] == "n-alerts-1" {
                break frame["body"].clone();
            }
        };
        assert_eq!(answer["ok"], true, "{answer}");
        assert!(
            answer
                .get("alerts")
                .is_some_and(|alerts| alerts.is_null() || alerts.is_u64()),
            "{answer}"
        );
        drop(connected);
    }

    /// Over the bridge as the daemon speaks it: its own keys are not the
    /// human's unsent text, so they hold neither a paste nor a native send,
    /// and the snapshot says nothing is unsent.
    #[cfg(unix)]
    #[test]
    fn the_daemons_own_keys_hold_no_paste_and_no_claim() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::new(OutputHub::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut number = 0;
        let mut ask = |op: &str, body: Value| -> Value {
            number += 1;
            let id = format!("n-typing-{number}");
            let mut frame =
                serde_json::to_vec(&json!({"v":1,"id":id,"kind":"req","op":op,"body":body}))
                    .expect("serialize request");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("write request");
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read response");
                let frame: Value = serde_json::from_str(line.trim()).expect("response JSON");
                if frame["kind"] == "res" && frame["id"] == id.as_str() {
                    return frame["body"].clone();
                }
            }
        };
        let opened = ask(
            "pane.open",
            json!({"id":"typing-pane","generation":1,"cwd":"/tmp","argv":["/bin/sh","-c","sleep 30"],
                   "env":{},"size":{"rows":24,"cols":80},"backlogBytes":1024}),
        );
        assert_eq!(opened["ok"], true, "{opened}");
        let typing = PaneKey::new("typing-pane", 1);
        assert_eq!(
            arbiter.silent_paste(&typing),
            None,
            "the daemon said nothing of it"
        );
        // A window whose harness reads a paste silently is opened saying how (T-1048).
        let declared = ask(
            "pane.open",
            json!({"id":"declared-pane","generation":1,"cwd":"/tmp","argv":["/bin/sh","-c","sleep 30"],
                   "env":{},"size":{"rows":24,"cols":80},"backlogBytes":1024,
                   "silentPaste":{"paceUsPerCharacter":600,"drawnAfterMs":50,"quietMs":120,
                                  "mostMs":1000,"mostUsPerCharacter":1000}}),
        );
        assert_eq!(declared["ok"], true, "{declared}");
        let declared_pane = PaneKey::new("declared-pane", 1);
        assert_eq!(
            arbiter.silent_paste(&declared_pane),
            Some(SilentPaste {
                pace_us_per_character: 600,
                drawn_after_ms: 50,
                quiet_ms: 120,
                most_ms: 1_000,
                most_us_per_character: 1_000,
            })
        );
        panes.kill(&declared_pane).expect("kill the declared pane");
        let typed = ask(
            "pane.input",
            json!({"id":"typing-pane","generation":1,"bytes":b"half a thought".to_vec()}),
        );
        assert_eq!(typed, json!({"ok":true}));
        assert_eq!(
            ask("pane.claim", json!({"pane":"typing-pane","generation":1})),
            json!({"ok":true})
        );
        assert_eq!(
            ask(
                "pane.write_paste",
                json!({"id":"typing-pane","generation":1,"body":"result"})
            ),
            json!({"ok":true})
        );
        let snapshot = ask("pane.snapshot", json!({"id":"typing-pane","generation":1}));
        assert_eq!(snapshot["pasteInFlight"], false, "{snapshot}");
        // And says how long since it wrote that paste.
        assert!(snapshot["pasteQuietMs"].is_u64(), "{snapshot}");
        // Keys over the bridge are the daemon's, never the human's unsent text.
        assert_eq!(snapshot["unsent"], false, "{snapshot}");

        panes
            .kill(&PaneKey::new("typing-pane", 1))
            .expect("kill the pane");
        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// The terminal's answer over the bridge (`pane.reply`) goes past the human's keys held
    /// after their paste (`pane.typed`), in a window whose harness reads a paste silently
    /// (T-1048): the program reads the paste and says so by a file (the human's Enter is held
    /// behind it), reads the answer, and draws only then. So the reply sent once the program has
    /// the paste is answered and read before the Enter, or the program never draws and the
    /// Enter waits for its cap, two minutes: no time of the machine's is in it.
    #[cfg(unix)]
    #[test]
    fn a_reply_over_the_bridge_goes_past_the_humans_keys_held_after_their_paste() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        use crate::testing::{hex, SilentReader, HELD_FOR_MS, LIVENESS};

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::new(OutputHub::new()),
            Arc::clone(&inputs),
        );
        register_typed(&mut builder, Arc::clone(&inputs));
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        // A read that waits past the liveness bound is a hang: the answers come at once.
        node_stream
            .set_read_timeout(Some(LIVENESS))
            .expect("a read timeout");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut send = |id: &str, op: &str, body: Value| {
            let mut frame =
                serde_json::to_vec(&json!({"v":1,"id":id,"kind":"req","op":op,"body":body}))
                    .expect("serialize request");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("write request");
        };
        let answered = |reader: &mut BufReader<UnixStream>| -> String {
            loop {
                let mut line = String::new();
                reader
                    .read_line(&mut line)
                    .expect("a response within the liveness bound");
                let frame: Value = serde_json::from_str(line.trim()).expect("response JSON");
                if frame["kind"] == "res" {
                    assert_eq!(frame["body"]["ok"], true, "{frame}");
                    return frame["id"].as_str().unwrap_or_default().to_owned();
                }
            }
        };
        let paste: &[u8] = b"\x1b[200~ab\x1b[201~";
        let program = SilentReader::new("reply-bridge", paste.len());
        send(
            "n-open",
            "pane.open",
            json!({"id":"replied-pane","generation":1,"cwd":"/tmp",
                   "argv":["/bin/sh","-c",program.script()],
                   "env":{},"size":{"rows":24,"cols":80},"backlogBytes":1024,
                   "silentPaste":{"paceUsPerCharacter":0,"drawnAfterMs":0,"quietMs":100,
                                  "mostMs":HELD_FOR_MS,"mostUsPerCharacter":0}}),
        );
        assert_eq!(answered(&mut reader), "n-open");
        // Once the program reads raw and echoes nothing: an echo of the paste would be the
        // window's draw of it, and let the human's Enter go before the reply is sent.
        let key = PaneKey::new("replied-pane", 1);
        let shows = |words: &str| {
            panes
                .tail(&key, 5)
                .is_ok_and(|lines| lines.iter().any(|line| line.contains(words)))
        };
        let deadline = std::time::Instant::now() + LIVENESS;
        while !shows("ready") {
            assert!(
                std::time::Instant::now() < deadline,
                "the program never got ready"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let pane = json!({"id":"replied-pane","generation":1});
        let with = |bytes: &[u8]| {
            let mut body = pane.clone();
            body["bytes"] = json!(bytes);
            body
        };
        send("n-typed", "pane.typed", with(&[paste, b"\r"].concat()));
        // The paste is in the program: the human's Enter is held behind it. Each request is
        // handled on a thread of its own, so a reply sent sooner could be written ahead of the
        // human's keys, and prove nothing of them.
        program.wait_pasted();
        send("n-reply", "pane.reply", with(b"R"));
        // Neither answer is awaited in an order: a reply queued behind the Enter is answered a
        // moment after it, and the program's record says which was written first.
        let mut answers = vec![answered(&mut reader), answered(&mut reader)];
        answers.sort();
        assert_eq!(answers, ["n-reply", "n-typed"]);

        let record = format!("drawn{}520d", hex(paste));
        let deadline = std::time::Instant::now() + LIVENESS;
        loop {
            let seen: String = panes
                .tail(&key, 5)
                .unwrap_or_default()
                .concat()
                .split_whitespace()
                .collect();
            if seen.contains(&record) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the program's record is not the paste, the answer and then the Enter: {seen}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }

        panes.kill(&key).expect("kill the pane");
        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// A window that prints every 50 ms and changes nothing it shows is quiet by its
    /// screen, though never by its output: the stand-in draws its box, then writes what
    /// Muse Code 1.4.14 wrote at rest after a command it ran in the background was stopped
    /// (colour resets and a move of the cursor to where it is) and its box again, every 50
    /// ms, as T-1081's pty probe saw. Then it changes one character, and its screen is not
    /// quiet (T-1083: the daemon judged such a window by its output, so it never let its
    /// next message in).
    #[cfg(unix)]
    #[test]
    fn a_window_that_repaints_what_it_shows_is_quiet_by_its_screen_and_not_by_its_output() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            arbiter,
            Arc::new(OutputHub::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut number = 0;
        let mut ask = |op: &str, body: Value| -> Value {
            number += 1;
            let id = format!("n-quiet-{number}");
            let mut frame =
                serde_json::to_vec(&json!({"v":1,"id":id,"kind":"req","op":op,"body":body}))
                    .expect("serialize request");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("write request");
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read response");
                let frame: Value = serde_json::from_str(line.trim()).expect("response JSON");
                if frame["kind"] == "res" && frame["id"] == id.as_str() {
                    return frame["body"].clone();
                }
            }
        };
        // The box, then 80 repaints 50 ms apart (4 s); then one character of it changed,
        // and that box repainted on. Nothing here reads its output, so its backlog holds
        // all of it.
        let script = r#"repaint() { printf '\033[17;1H%s\033[18;3H' "$2"; i=0; while [ $i -lt $1 ]; do
  printf '\033[39m\033[49m\033[59m\033[0m\033[18;3H\033[17;1H%s\033[18;3H' "$2"
  /bin/sleep 0.05; i=$((i+1)); done; }
repaint 80 '> Ask anything'; repaint 1200 '> Tsk anything'"#;
        let pane = json!({"id":"repaint-pane","generation":1});
        let opened = ask(
            "pane.open",
            json!({"id":"repaint-pane","generation":1,"cwd":"/tmp","argv":["/bin/sh","-c",script],
                   "env":{},"size":{"rows":24,"cols":80},"backlogBytes":1 << 20}),
        );
        assert_eq!(opened["ok"], true, "{opened}");
        let shows = |snapshot: &Value, text: &str| {
            snapshot["tail"]
                .as_array()
                .is_some_and(|lines| lines.iter().any(|line| line.as_str() == Some(text)))
        };
        let mut look = || {
            let mut body = pane.clone();
            body["tail"] = json!(4);
            ask("pane.snapshot", body)
        };
        let (repainting, changed) = quiet_by_screen::watch(&mut look, &shows);
        quiet_by_screen::check(&repainting, &changed, true);

        panes
            .kill(&PaneKey::new("repaint-pane", 1))
            .expect("kill the pane");
        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// What `pane.snapshot` answers for a pane's screen: a pane the table no longer holds
    /// has none to give and the answer goes without it; a table that cannot be read says so.
    #[cfg(unix)]
    #[test]
    fn a_snapshot_tells_an_unreadable_screen_from_a_retired_pane() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let key = PaneKey::new("p1-zeus", 1);
        arbiter.register(&key).expect("register the pane");
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::new(OutputHub::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut ask = |id: &str, body: Value| -> Value {
            let mut frame = serde_json::to_vec(&json!({
                "v":1,"id":id,"kind":"req","op":"pane.snapshot","body":body,
            }))
            .expect("serialize the snapshot");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("ask for the snapshot");
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read a frame");
                let frame: Value = serde_json::from_str(line.trim()).expect("frame JSON");
                if frame["kind"] == "res" && frame["id"] == id {
                    break frame["body"].clone();
                }
            }
        };
        let request = json!({"id":key.id,"generation":key.generation,"tail":5,"bottom":5});

        let retired = ask("n-retired", request.clone());
        assert_eq!(retired["ok"], true, "{retired}");
        assert!(retired.get("tail").is_none() && retired.get("bottom").is_none());

        panes.poison();
        let unreadable = ask("n-poisoned", request);
        assert_eq!(
            unreadable,
            json!({"ok":false,"error":"pane table lock is poisoned"})
        );

        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// A window whose input could not be registered is ended, and a failure to end it is
    /// said with the cause, not dropped.
    #[cfg(unix)]
    #[test]
    fn a_window_that_could_not_be_ended_after_a_failed_open_says_so() {
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let key = PaneKey::new("p1-zeus", 1);

        // Gone (never held, or retired): the cause alone.
        assert_eq!(
            end_unregistered(&panes, &inputs, &key, "the cause".to_string()),
            "the cause"
        );

        panes.poison();
        let said = end_unregistered(&panes, &inputs, &key, "the cause".to_string());
        assert_eq!(
            said,
            "the cause; and the window p1-zeus#1 could not be ended: pane table lock is poisoned"
        );
        inputs.close_and_drain();
    }

    /// `pane.list`'s process group: null where a pane has none to give, an error where the
    /// table could not be read.
    #[cfg(unix)]
    #[test]
    fn a_process_group_is_null_only_where_there_is_none() {
        let panes = PaneTable::new();
        let key = PaneKey::new("p1-zeus", 1);
        assert_eq!(
            group_id_answer(panes.process_group_id(&key)),
            Ok(Value::Null)
        );
        assert_eq!(group_id_answer(Ok(42)), Ok(json!(42)));
        assert_eq!(
            group_id_answer(Err(PaneError::Pty("the PTY has no process group".into()))),
            Ok(Value::Null)
        );
        panes.poison();
        assert_eq!(
            group_id_answer(panes.process_group_id(&key)),
            Err("pane table lock is poisoned".to_string())
        );
    }

    /// A pane whose kill failed takes its input with it all the same, and the
    /// daemon hears the failure. Its input worker used to stay until the app
    /// quit, admitting input for a window the daemon had closed.
    #[cfg(unix)]
    #[test]
    fn a_kill_that_fails_still_takes_the_panes_input() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        use crate::arbiter::ArbiterError;

        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let key = PaneKey::new("p1-zeus", 1);
        arbiter.register(&key).expect("register the pane");
        inputs.open(&key).expect("open the pane's input");
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::new(OutputHub::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        panes.poison();

        let mut frame = serde_json::to_vec(&json!({
            "v":1,"id":"n-kill","kind":"req","op":"pane.kill",
            "body":{"id":key.id,"generation":key.generation},
        }))
        .expect("serialize the kill");
        frame.push(b'\n');
        node_stream.write_all(&frame).expect("ask for the kill");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let answer = loop {
            let mut line = String::new();
            reader.read_line(&mut line).expect("read a frame");
            let frame: Value = serde_json::from_str(line.trim()).expect("frame JSON");
            if frame["kind"] == "res" && frame["id"] == "n-kill" {
                break frame["body"].clone();
            }
        };
        assert_eq!(
            answer,
            json!({"ok":false,"error":"pane table lock is poisoned"})
        );
        assert!(
            matches!(
                inputs.write(key.clone(), b"x".to_vec()),
                Err(InputError::Refused {
                    code: "stale-pane",
                    ..
                })
            ),
            "the pane kept its input"
        );
        assert!(matches!(arbiter.snapshot(&key), Err(ArbiterError::Stale)));

        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// T-1328: a program the system will not start is answered with the system's name for why,
    /// beside the host's words (which name the program), so that the daemon can say the kind of
    /// failure in a note and no word of the file; any other refusal names none.
    #[cfg(unix)]
    #[test]
    fn a_program_the_system_cannot_start_answers_with_its_code() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(Role::Host, 1024 * 1024);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            Arc::clone(&arbiter),
            Arc::new(OutputHub::new()),
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut number = 0;
        let mut ask = |argv: Value| -> Value {
            number += 1;
            let id = format!("n-open-{number}");
            let mut frame = serde_json::to_vec(&json!({
                "v":1,"id":id,"kind":"req","op":"pane.open",
                "body":{"id":format!("p-{number}"),"generation":1,"cwd":"/tmp","argv":argv,
                        "env":{},"size":{"rows":24,"cols":80},"backlogBytes":1024},
            }))
            .expect("serialize the open");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("ask for the open");
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read a frame");
                let frame: Value = serde_json::from_str(line.trim()).expect("frame JSON");
                if frame["kind"] == "res" && frame["id"] == id.as_str() {
                    return frame["body"].clone();
                }
            }
        };

        let missing = ask(json!(["/nonexistent/hunter22 program"]));
        assert_eq!(missing["ok"], false, "{missing}");
        assert_eq!(missing["code"], "ENOENT", "{missing}");
        assert!(missing["error"].is_string(), "{missing}");
        // A refusal the system did not make names no code.
        let relative = ask(json!(["program"]));
        assert_eq!(relative["ok"], false, "{relative}");
        assert!(relative.get("code").is_none(), "{relative}");

        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }

    /// A paste says whether anything of it reached the window. Refused before
    /// a byte was written, it was not sent and may be sent again; cut short
    /// in its write, it is uncertain. Both used to answer a bare error, which
    /// the daemon read as not sent.
    #[cfg(unix)]
    #[test]
    fn a_paste_answers_whether_anything_reached_the_window() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let output = Arc::new(OutputHub::new());
        let (printed, seen) = mpsc::channel();
        output.register_sink(Arc::new(move |message: PaneOutputMessage| {
            printed.send(message.bytes).is_ok()
        }));
        let mut builder = BridgeBuilder::new(Role::Host, MAX_FRAME_BYTES);
        register_pane_handlers(
            &mut builder,
            Arc::clone(&panes),
            arbiter,
            output,
            Arc::clone(&inputs),
        );
        let (rust_stream, mut node_stream) = UnixStream::pair().expect("bridge socket pair");
        node_stream
            .write_all(b"{\"url\":\"http://localhost:1/\",\"token\":\"test\"}\n")
            .expect("write bridge handle");
        let connected = builder
            .connect(rust_stream.try_clone().expect("clone socket"), rust_stream)
            .expect("connect bridge");
        let mut reader = BufReader::new(node_stream.try_clone().expect("clone node reader"));
        let mut number = 0;
        let mut send = |op: &str, body: Value| -> String {
            number += 1;
            let id = format!("n-paste-{number}");
            let mut frame =
                serde_json::to_vec(&json!({"v":1,"id":id,"kind":"req","op":op,"body":body}))
                    .expect("serialize request");
            frame.push(b'\n');
            node_stream.write_all(&frame).expect("write request");
            id
        };
        // Answers are kept by request, whatever order they come in.
        let mut answers = HashMap::new();
        let mut answer = |id: &str| -> Value {
            while !answers.contains_key(id) {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read a frame");
                let frame: Value = serde_json::from_str(line.trim()).expect("frame JSON");
                if frame["kind"] == "res" {
                    let answered = frame["id"].as_str().expect("response id").to_string();
                    answers.insert(answered, frame["body"].clone());
                }
            }
            answers.remove(id).expect("the answer")
        };

        // A window that reads nothing: a large paste fills its terminal and
        // waits, once the terminal is raw.
        let opened = send(
            "pane.open",
            json!({"id":"pasted-pane","generation":1,"cwd":"/tmp",
                   "argv":["/bin/sh","-c","/bin/stty raw -echo; printf ready; exec /bin/sleep 1000"],
                   "env":{},"size":{"rows":24,"cols":80},"backlogBytes":4096}),
        );
        assert_eq!(answer(&opened)["ok"], true);
        let mut terminal = Vec::new();
        while !terminal.ends_with(b"ready") {
            terminal.extend(
                seen.recv_timeout(Duration::from_secs(5))
                    .expect("the window says it is ready"),
            );
        }

        let stale = send(
            "pane.write_paste",
            json!({"id":"pasted-pane","generation":2,"body":"late"}),
        );
        assert_eq!(
            answer(&stale),
            json!({"ok":false,"admitted":false,"bytesWritten":0,
                   "error":"stale-pane","cause":"stale pane generation"})
        );
        let unreadable = send(
            "pane.write_paste",
            json!({"id":"pasted-pane","generation":1}),
        );
        let unreadable = answer(&unreadable);
        assert_eq!(
            (
                &unreadable["admitted"],
                &unreadable["bytesWritten"],
                &unreadable["error"]
            ),
            (&json!(false), &json!(0), &json!("invalid-body")),
            "{unreadable}"
        );

        let paste = send(
            "pane.write_paste",
            json!({"id":"pasted-pane","generation":1,"body":"x".repeat(512 * 1024)}),
        );
        thread::sleep(Duration::from_millis(150));
        let kill = send("pane.kill", json!({"id":"pasted-pane","generation":1}));
        assert_eq!(answer(&kill), json!({"ok":true}));
        let cut = answer(&paste);
        assert_eq!(
            (&cut["ok"], &cut["admitted"], &cut["error"]),
            (&json!(false), &Value::Null, &json!("uncertain")),
            "{cut}"
        );
        assert!(cut["cause"].is_string(), "{cut}");
        assert!(cut.get("bytesWritten").is_none(), "{cut}");

        inputs.close_and_drain();
        drop(reader);
        drop(node_stream);
        connected.bridge.wait_closed().expect("bridge closes");
    }
}
