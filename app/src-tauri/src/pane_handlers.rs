//! The pane operations the daemon asks of the app over the bridge, one set
//! for the window and the headless helper alike, and each pane's output and
//! exit on their way back.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;

use portable_pty::PtySize;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::arbiter::{InputArbiter, OutputClock};
use crate::bridge::{Bridge, BridgeBuilder};
use crate::input_queue::{wait_for_input_blocking, InputError, InputQueue};
use crate::output_hub::OutputHub;
use crate::pty::{validate_drop_env, PaneEnvironment, PaneKey, PaneTable, StreamedPane};
use crate::validation::{pane_key, validate_input, validate_seq, validate_size, INVALID_BODY};

const DEFAULT_BACKLOG_BYTES: usize = 1024 * 1024;

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
struct ResizeRequest {
    id: String,
    generation: u64,
    rows: u16,
    cols: u16,
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

pub(crate) fn register_pane_handlers(
    builder: &mut BridgeBuilder,
    panes: Arc<PaneTable>,
    arbiter: Arc<InputArbiter>,
    output: Arc<OutputHub>,
    inputs: Arc<InputQueue>,
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
        let streamed = open_panes
            .open_streamed_at(
                key,
                &request.cwd,
                &request.argv,
                PaneEnvironment::new(&request.env, &request.drop_env),
                size,
                request.backlog_bytes,
            )
            .map_err(|error| error.to_string())?;
        let printed = match open_arbiter
            .register(&streamed.key)
            .map_err(|error| error.to_string())
            .and_then(|printed| open_inputs.open(&streamed.key).map(|()| printed))
        {
            Ok(printed) => printed,
            Err(error) => {
                let _ = open_panes.kill(&streamed.key);
                open_inputs.retire(&streamed.key);
                return Err(error);
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

    // Keys typed into a pane and an emulator's replies (a page-less peer
    // answers a cursor query itself) are written alike.
    for operation in ["pane.input", "pane.reply"] {
        let input_queue = Arc::clone(&inputs);
        builder.on(operation, move |_bridge, body| {
            let request: BytesRequest = parse_body(body)?;
            validate_input(&request.bytes)?;
            let key = pane_key(&request.id, request.generation)?;
            input_queue
                .write(key, request.bytes)
                .and_then(wait_for_input_blocking)
                .map_err(|error| error.to_string())?;
            Ok(json!({"ok":true}))
        });
    }

    // Every way a paste fails is an answer that says whether anything of it
    // reached the window, a request it could not read included.
    let paste_queue = Arc::clone(&inputs);
    builder.on("pane.write_paste", move |_bridge, body| {
        let pasted = paste_request(body).and_then(|(key, body)| {
            wait_for_input_blocking(paste_queue.paste(key, body.into_bytes())?)
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

    let resize_panes = Arc::clone(&panes);
    builder.on("pane.resize", move |_bridge, body| {
        let request: ResizeRequest = parse_body(body)?;
        validate_size(request.cols, request.rows)?;
        resize_panes
            .resize(
                &pane_key(&request.id, request.generation)?,
                request.rows,
                request.cols,
            )
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
        kill_panes.kill(&key).map_err(|error| error.to_string())?;
        kill_inputs.retire(&key);
        Ok(json!({"ok":true}))
    });

    let list_panes = Arc::clone(&panes);
    builder.on("pane.list", move |_bridge, body| {
        let _: EmptyBody = parse_body(body)?;
        let panes = list_panes
            .list()
            .map_err(|error| error.to_string())?
            .into_iter()
            .map(|pane| {
                json!({
                    "id":pane.id,
                    "generation":pane.generation,
                    "alive":pane.alive,
                    "idleMs":pane.idle_ms,
                    "processGroupId":list_panes.process_group_id(&PaneKey { id: pane.id.clone(), generation: pane.generation }).ok(),
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({"ok":true,"panes":panes}))
    });

    let snapshot_arbiter = Arc::clone(&arbiter);
    builder.on("pane.snapshot", move |_bridge, body| {
        let request: PaneRequest = parse_body(body)?;
        let snapshot = snapshot_arbiter
            .snapshot(&pane_key(&request.id, request.generation)?)
            .map_err(|error| error.to_string())?;
        Ok(json!({
            "ok":true,
            "generation":snapshot.generation,
            "pasteInFlight":snapshot.paste_in_flight,
            "inputFailed":snapshot.input_failed,
            "outputQuietMs":snapshot.output_quiet_ms,
        }))
    });
}

/// A pane's output, on to the page, and its end: `pane.exit`, after which a
/// pane whose program has gone leaves the table.
fn stream_to_page(
    streamed: StreamedPane,
    bridge: Bridge,
    panes: Arc<PaneTable>,
    printed: Arc<OutputClock>,
    inputs: Arc<InputQueue>,
    output: Arc<OutputHub>,
) {
    thread::spawn(move || {
        let key = streamed.key;
        for message in streamed.output {
            printed.note();
            output.publish(message.into());
        }
        let _ = bridge.event(
            "pane.exit",
            json!({"id":key.id,"generation":key.generation}),
        );
        if matches!(panes.retire_exited(&key), Ok(true)) {
            inputs.retire(&key);
        }
    });
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyBody {}

fn parse_body<T: DeserializeOwned>(body: Value) -> Result<T, String> {
    serde_json::from_value(body).map_err(|error| format!("{INVALID_BODY}: {error}"))
}

/// A paste's pane and body. A request that does not name them was refused
/// before anything was written.
fn paste_request(body: Value) -> Result<(PaneKey, String), InputError> {
    let refused = |cause| InputError::refused(INVALID_BODY, cause);
    let request: PasteRequest = parse_body(body).map_err(refused)?;
    let key = pane_key(&request.id, request.generation).map_err(refused)?;
    Ok((key, request.body))
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
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::arbiter::EnterTiming;
    #[cfg(unix)]
    use std::sync::mpsc;
    #[cfg(unix)]
    use std::time::Duration;

    #[cfg(unix)]
    use crate::output_hub::PaneOutputMessage;
    #[cfg(unix)]
    use crate::runtime::MAX_FRAME_BYTES;

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

    /// Over the bridge as the daemon speaks it: text the human typed and never
    /// sent holds neither a paste nor a native send (the owner's choice,
    /// 2026-10-01), and the snapshot has no draft to wait on.
    #[cfg(unix)]
    #[test]
    fn unsent_typing_holds_no_paste_and_no_claim() {
        use std::io::{BufRead, BufReader, Write};
        use std::os::unix::net::UnixStream;

        let _pty_guard = crate::pty::serial_pty_test();
        let panes = Arc::new(PaneTable::new());
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = Arc::new(InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter)));
        let mut builder = BridgeBuilder::new(1024 * 1024);
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
        assert!(snapshot.get("draftLatched").is_none(), "{snapshot}");

        panes
            .kill(&PaneKey::new("typing-pane", 1))
            .expect("kill the pane");
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
        let mut builder = BridgeBuilder::new(MAX_FRAME_BYTES);
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
