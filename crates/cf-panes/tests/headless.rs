#![cfg(unix)]
#![deny(clippy::undocumented_unsafe_blocks)]
// The tests start the helper themselves and read their own environment; their
// helpers unwrap and expect, as the tests do.
#![allow(clippy::disallowed_methods, clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

#[cfg(target_os = "macos")]
use cf_panes::pty::Sentinel;

const FRAME_TIMEOUT: Duration = Duration::from_secs(3);

fn serial_headless_test() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

struct Headless {
    child: Child,
    input: Option<ChildStdin>,
    frames: Receiver<Result<Value, String>>,
    /// The thread that reads the helper's output, which holds the pipe.
    reading: Option<thread::JoinHandle<()>>,
    /// Whether that thread has stopped taking what it reads (but holds the pipe).
    stalled: Arc<AtomicBool>,
    next_id: u64,
    pub handle: Value,
}

impl Headless {
    fn spawn() -> Self {
        Self::spawn_with_env(&[])
    }

    fn spawn_with_env(env: &[(&str, &str)]) -> Self {
        let mut command = Command::new(env!("CARGO_BIN_EXE_consensflow-bridge"));
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (name, value) in env {
            command.env(name, value);
        }
        let mut child = command.spawn().expect("spawn consensflow-bridge");
        let input = child.stdin.take().expect("piped helper stdin");
        let stdout = child.stdout.take().expect("piped helper stdout");
        let mut reader = BufReader::new(stdout);
        let mut handle_line = String::new();
        reader
            .read_line(&mut handle_line)
            .expect("read helper handle line");
        assert!(!handle_line.is_empty(), "helper omitted its handle line");
        let handle = serde_json::from_str(handle_line.trim_end()).expect("parse helper handle");

        let (sender, frames) = mpsc::channel();
        let stalled = Arc::new(AtomicBool::new(false));
        let stalling = Arc::clone(&stalled);
        let reading = thread::spawn(move || {
            for line in reader.lines() {
                while stalling.load(Ordering::Acquire) {
                    thread::sleep(Duration::from_millis(10));
                }
                let frame = line.map_err(|error| error.to_string()).and_then(|line| {
                    serde_json::from_str(&line).map_err(|error| error.to_string())
                });
                if sender.send(frame).is_err() {
                    break;
                }
            }
        });

        Self {
            child,
            input: Some(input),
            frames,
            reading: Some(reading),
            stalled,
            next_id: 0,
            handle,
        }
    }

    /// Stops reading the helper's output but holds its pipe, as a peer that hangs does:
    /// what the helper writes fills the pipe, and its next write waits for a reader.
    fn stop_reading(&self) {
        self.stalled.store(true, Ordering::Release);
    }

    /// Stops reading the helper's output and lets its pipe go, as a peer that has
    /// died does: the helper's next write fails. The helper answers a list once
    /// more, which the reading thread takes as its cue to stop.
    fn lose_output(&mut self) {
        let (_, unread) = mpsc::channel();
        drop(std::mem::replace(&mut self.frames, unread));
        self.send_request("pane.list", json!({}));
        if let Some(reading) = self.reading.take() {
            reading.join().expect("the reading thread ends");
        }
    }

    /// Every frame the helper writes until its output ends, which it does with its
    /// end; its end is waited for until `timeout`.
    fn frames_to_the_end(&self, timeout: Duration) -> Vec<Value> {
        let deadline = Instant::now() + timeout;
        let mut frames = Vec::new();
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.receive_timeout(left) {
                Ok(frame) => frames.push(frame),
                Err(mpsc::RecvTimeoutError::Disconnected) => return frames,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    panic!("the helper's output did not end: {frames:?}")
                }
            }
        }
    }

    fn request(&mut self, op: &str, body: Value, events: &mut Vec<Value>) -> Value {
        let id = self.send_request(op, body);

        loop {
            let frame = self.receive();
            if frame["kind"] == "res" && frame["id"] == id {
                assert_eq!(frame["op"], op, "response must echo request op");
                return frame["body"].clone();
            }
            events.push(frame);
        }
    }

    fn send_request(&mut self, op: &str, body: Value) -> String {
        self.next_id += 1;
        let id = format!("n-{}", self.next_id);
        let request = json!({"v":1,"id":id,"kind":"req","op":op,"body":body});
        let input = self.input.as_mut().expect("helper input is open");
        serde_json::to_writer(&mut *input, &request).expect("serialize request");
        input.write_all(b"\n").expect("terminate request");
        input.flush().expect("flush request");
        id
    }

    fn receive(&self) -> Value {
        self.receive_timeout(FRAME_TIMEOUT)
            .expect("receive helper frame before timeout")
    }

    fn receive_timeout(&self, timeout: Duration) -> Result<Value, mpsc::RecvTimeoutError> {
        let frame = match self.frames.recv_timeout(timeout)? {
            Ok(frame) => frame,
            Err(error) => panic!("helper stdout contains JSON frames only: {error}"),
        };
        let mut keys = frame
            .as_object()
            .expect("frame is an object")
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>();
        keys.sort_unstable();
        assert_eq!(keys, ["body", "id", "kind", "op", "v"]);
        assert_eq!(frame["v"], 1);
        let expected_prefix = if frame["kind"] == "res" { "n-" } else { "r-" };
        assert!(frame["id"]
            .as_str()
            .is_some_and(|id| id.starts_with(expected_prefix)));
        Ok(frame)
    }

    fn close_input_and_wait(&mut self) {
        drop(self.input.take());
        let status = self
            .wait_for_exit(FRAME_TIMEOUT)
            .expect("helper did not exit on stdin EOF");
        assert!(status.success(), "helper exited unsuccessfully: {status}");
    }

    fn wait_for_exit(&mut self, timeout: Duration) -> Option<ExitStatus> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait().expect("poll helper") {
                return Some(status);
            }
            if Instant::now() >= deadline {
                return None;
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Headless {
    fn drop(&mut self) {
        self.stalled.store(false, Ordering::Release);
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// A `pane.open` for a script, at an identity of its own, as the daemon names
/// every pane it opens.
fn open_body(script: &str, backlog_bytes: usize) -> Value {
    static OPENED: AtomicU64 = AtomicU64::new(0);
    json!({
        "id":format!("pane-{}", OPENED.fetch_add(1, Ordering::Relaxed) + 1),
        "generation":1,
        "cwd":"/tmp",
        "argv":["/bin/sh","-c",script],
        "env":{},
        "size":{"rows":24,"cols":80},
        "backlogBytes":backlog_bytes
    })
}

fn output_bytes(event: &Value, pane_id: &str, generation: u64) -> Option<(u64, Vec<u8>)> {
    if event["kind"] != "evt"
        || event["op"] != "pane.output"
        || event["body"]["id"] != pane_id
        || event["body"]["generation"] != generation
    {
        return None;
    }
    let seq = event["body"]["seq"].as_u64().expect("numeric output seq");
    let bytes = event["body"]["bytes"]
        .as_array()
        .expect("output bytes array")
        .iter()
        .map(|byte| byte.as_u64().expect("numeric output byte") as u8)
        .collect();
    Some((seq, bytes))
}

fn output_until(
    helper: &Headless,
    events: &mut Vec<Value>,
    pane_id: &str,
    generation: u64,
    expected: &[u8],
) -> Vec<u8> {
    let mut output = Vec::new();
    while !output
        .windows(expected.len())
        .any(|bytes| bytes == expected)
    {
        if events.is_empty() {
            events.push(helper.receive());
        }
        for event in events.drain(..) {
            if let Some((_seq, bytes)) = output_bytes(&event, pane_id, generation) {
                output.extend(bytes);
            }
        }
    }
    output
}

#[test]
fn pane_open_drop_env_removes_after_overlay_and_inherits_unlisted_parent() {
    const SENTINEL: &str = "CONSENSFLOW_HEADLESS_DROP_ENV_7FC9A1";

    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn_with_env(&[(SENTINEL, "from-parent")]);
    let mut events = Vec::new();
    let probe = format!(
        "if [ \"${{{SENTINEL}+set}}\" = set ]; then printf '%s' \"${SENTINEL}\"; else printf absent; fi"
    );

    let mut dropped = open_body(&probe, 1024);
    dropped["env"] = Value::Object(serde_json::Map::from_iter([(
        SENTINEL.to_string(),
        json!("from-frame"),
    )]));
    dropped["dropEnv"] = json!([SENTINEL]);
    let opened = helper.request("pane.open", dropped, &mut events);
    assert_eq!(opened["ok"], true, "dropEnv is part of the pane.open frame");
    let pane_id = opened["id"].as_str().expect("opened pane id");
    let generation = opened["generation"].as_u64().expect("opened generation");
    assert_eq!(
        output_until(&helper, &mut events, pane_id, generation, b"absent"),
        b"absent",
        "removal runs after the explicit env overlay"
    );
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );

    let inherited = helper.request("pane.open", open_body(&probe, 1024), &mut events);
    assert_eq!(inherited["ok"], true);
    let pane_id = inherited["id"].as_str().expect("opened pane id");
    let generation = inherited["generation"].as_u64().expect("opened generation");
    assert_eq!(
        output_until(&helper, &mut events, pane_id, generation, b"from-parent"),
        b"from-parent",
        "an unlisted parent variable remains available"
    );
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );

    for invalid_name in ["", "BAD=NAME", "BAD\0NAME"] {
        let mut invalid = open_body("printf should-not-run", 1024);
        invalid["dropEnv"] = json!([invalid_name]);
        let refused = helper.request("pane.open", invalid, &mut events);
        assert_eq!(refused["ok"], false, "accepted {invalid_name:?}");
        assert!(refused["error"]
            .as_str()
            .is_some_and(|error| error.contains("environment variable name")));
    }
    assert_eq!(
        helper.request("pane.list", json!({}), &mut events)["panes"],
        json!([]),
        "an invalid name is refused before a pane is spawned"
    );
    helper.close_input_and_wait();
}

/// A claim admits a native send whatever the human typed and left unsent (the
/// owner's choice, 2026-10-01). It writes nothing and reserves nothing; only a
/// replaced pane is refused.
#[test]
fn a_claim_after_unsent_typing_is_admitted_and_writes_nothing() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(
            "/bin/stty raw -echo; printf ready; /usr/bin/od -An -tx1 -N 2; printf done",
            1024,
        ),
        &mut events,
    );
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let _ = output_until(&helper, &mut events, &pane_id, generation, b"ready");

    assert_eq!(
        helper.request(
            "pane.input",
            json!({"id":pane_id,"generation":generation,"bytes":[120]}),
            &mut events,
        ),
        json!({"ok":true})
    );
    assert_eq!(
        helper.request(
            "pane.claim",
            json!({"pane":pane_id,"generation":generation}),
            &mut events,
        ),
        json!({"ok":true})
    );
    assert!(
        events
            .drain(..)
            .all(|event| output_bytes(&event, &pane_id, generation).is_none()),
        "a claim wrote a second byte before its response"
    );
    assert!(matches!(
        helper.receive_timeout(Duration::from_millis(150)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    let after_claim = helper.request(
        "pane.snapshot",
        json!({"id":pane_id,"generation":generation}),
        &mut events,
    );
    assert_eq!(
        after_claim["pasteInFlight"], false,
        "a claim does not reserve the paste writer"
    );
    assert_eq!(
        helper.request(
            "pane.claim",
            json!({"pane":pane_id,"generation":generation + 1}),
            &mut events,
        ),
        json!({"ok":false,"error":"stale pane generation"})
    );

    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events,
        ),
        json!({"ok":true})
    );
    helper.close_input_and_wait();
}

#[test]
fn stdio_protocol_opens_pastes_lists_and_kills_a_raw_recorder() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    assert_eq!(helper.handle, json!({"v":1,"kind":"consensflow-bridge"}));
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(
            "/bin/stty raw -echo; tree=/tmp/cf-tree-$$; \
             /bin/sh -c '/bin/sleep 1000 & grandchild=$!; \
             printf \"%s\n\" \"$grandchild\" >\"$1\"; wait' sh \"$tree\" & child=$!; \
             while [ ! -s \"$tree\" ]; do :; done; grandchild=$(/bin/cat \"$tree\"); \
             /bin/rm -f \"$tree\"; printf 'PIDS:%s,%s,%s\nready' \"$$\" \"$child\" \"$grandchild\"; \
             /usr/bin/od -An -tx1 -N 17; printf '\nRECORDER-DONE\n'; wait \"$child\"",
            4096,
        ),
        &mut events,
    );
    assert_eq!(opened["ok"], true);
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");

    let mut terminal_output = Vec::new();
    let ready_deadline = Instant::now() + FRAME_TIMEOUT;
    while !terminal_output
        .windows(b"ready".len())
        .any(|bytes| bytes == b"ready")
    {
        for event in events.drain(..) {
            if let Some((_seq, bytes)) = output_bytes(&event, &pane_id, generation) {
                terminal_output.extend(bytes);
            }
        }
        assert!(
            Instant::now() < ready_deadline,
            "raw recorder omitted readiness marker"
        );
        if !terminal_output
            .windows(b"ready".len())
            .any(|bytes| bytes == b"ready")
        {
            events.push(helper.receive());
        }
    }

    assert_eq!(
        helper.request(
            "pane.write_paste",
            json!({"id":pane_id,"generation":generation,"body":"body"}),
            &mut events,
        ),
        json!({"ok":true})
    );

    let wanted = "1b5b3230307e626f64791b5b3230317e0d";
    let deadline = Instant::now() + FRAME_TIMEOUT;
    while !terminal_output
        .windows(b"RECORDER-DONE".len())
        .any(|bytes| bytes == b"RECORDER-DONE")
    {
        for event in events.drain(..) {
            if let Some((_seq, bytes)) = output_bytes(&event, &pane_id, generation) {
                terminal_output.extend(bytes);
            }
        }
        assert!(
            Instant::now() < deadline,
            "raw recorder omitted completion marker"
        );
        if !terminal_output
            .windows(b"RECORDER-DONE".len())
            .any(|bytes| bytes == b"RECORDER-DONE")
        {
            events.push(helper.receive());
        }
    }
    assert!(terminal_output
        .windows(b"ready".len())
        .any(|bytes| bytes == b"ready"));
    let ready_at = terminal_output
        .windows(b"ready".len())
        .position(|bytes| bytes == b"ready")
        .expect("ready marker")
        + b"ready".len();
    let done_at = terminal_output
        .windows(b"RECORDER-DONE".len())
        .position(|bytes| bytes == b"RECORDER-DONE")
        .expect("completion marker");
    let recorder_hex = String::from_utf8_lossy(&terminal_output[ready_at..done_at])
        .split_whitespace()
        .collect::<String>();
    assert_eq!(recorder_hex, wanted);

    let output_text = String::from_utf8_lossy(&terminal_output);
    let pid_line = output_text
        .lines()
        .find(|line| line.starts_with("PIDS:"))
        .expect("recorder pid line");
    let pids = pid_line["PIDS:".len()..]
        .split(',')
        .map(|pid| pid.trim().parse::<i32>().expect("numeric recorder pid"))
        .collect::<Vec<_>>();
    assert_eq!(pids.len(), 3, "root, child, and grandchild pids");
    assert!(pids.iter().all(|pid| process_exists(*pid)));

    let listed = helper.request("pane.list", json!({}), &mut events);
    assert_eq!(listed["ok"], true);
    assert!(listed["panes"]
        .as_array()
        .expect("pane list")
        .iter()
        .any(|pane| {
            pane["id"] == pane_id && pane["generation"] == generation && pane["alive"] == true
        }));
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events,
        ),
        json!({"ok":true})
    );
    let listed = helper.request("pane.list", json!({}), &mut events);
    assert!(!listed["panes"]
        .as_array()
        .expect("pane list after kill")
        .iter()
        .any(|pane| pane["id"] == pane_id && pane["generation"] == generation));
    let deadline = Instant::now() + Duration::from_secs(2);
    while pids.iter().any(|pid| process_exists(*pid)) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(pids.iter().all(|pid| !process_exists(*pid)));
    helper.close_input_and_wait();
}

#[test]
fn output_window_resumes_only_after_a_wire_ack() {
    const BACKLOG_BYTES: usize = 4096;

    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body("/usr/bin/yes", BACKLOG_BYTES),
        &mut events,
    );
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let mut received_bytes = 0;
    let mut last_seq = 0;
    while received_bytes < BACKLOG_BYTES {
        if events.is_empty() {
            events.push(helper.receive());
        }
        for event in events.drain(..) {
            if let Some((seq, bytes)) = output_bytes(&event, &pane_id, generation) {
                assert!(seq > last_seq);
                last_seq = seq;
                received_bytes += bytes.len();
            }
        }
    }
    assert_eq!(received_bytes, BACKLOG_BYTES);
    assert!(matches!(
        helper.receive_timeout(Duration::from_millis(150)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    assert_eq!(
        helper.request(
            "pane.ack",
            json!({"id":pane_id,"generation":generation,"seq":last_seq}),
            &mut events,
        ),
        json!({"ok":true})
    );
    let resumed = events
        .drain(..)
        .find_map(|event| output_bytes(&event, &pane_id, generation))
        .unwrap_or_else(|| {
            let deadline = Instant::now() + FRAME_TIMEOUT;
            loop {
                assert!(
                    Instant::now() < deadline,
                    "output did not resume after wire ack"
                );
                if let Some(output) = output_bytes(&helper.receive(), &pane_id, generation) {
                    break output;
                }
            }
        });
    assert!(resumed.0 > last_seq);
    assert!(!resumed.1.is_empty());
    helper.close_input_and_wait();
}

#[test]
fn parked_page_probe_stops_at_1024_then_drains_14400_bytes_via_wire_acks() {
    const BACKLOG_BYTES: usize = 1024;
    const OUTPUT_BYTES: usize = 14_400;

    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(
            "/usr/bin/yes x | /usr/bin/tr -d '\\n' | /usr/bin/head -c 14400",
            BACKLOG_BYTES,
        ),
        &mut events,
    );
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let mut received_bytes = 0;
    let mut pending = Vec::new();
    while received_bytes < BACKLOG_BYTES {
        if events.is_empty() {
            events.push(helper.receive());
        }
        for event in events.drain(..) {
            if let Some((seq, bytes)) = output_bytes(&event, &pane_id, generation) {
                received_bytes += bytes.len();
                pending.push(seq);
            }
        }
    }
    assert_eq!(received_bytes, BACKLOG_BYTES);
    assert!(matches!(
        helper.receive_timeout(Duration::from_millis(150)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    let mut ack_count = 0;
    while let Some(seq) = pending.pop() {
        assert_eq!(
            helper.request(
                "pane.ack",
                json!({"id":pane_id,"generation":generation,"seq":seq}),
                &mut events,
            ),
            json!({"ok":true})
        );
        ack_count += 1;
    }
    // The window ends once its last output is read and leaves the table after
    // its exit, so an ack that comes later has nothing left to free.
    let mut ended = false;
    let gone =
        json!({"ok":false,"error":format!("pane {pane_id} generation {generation} is not open")});
    while received_bytes < OUTPUT_BYTES {
        if events.is_empty() {
            events.push(helper.receive());
        }
        let mut received = Vec::new();
        for event in events.drain(..) {
            ended |= event["op"] == "pane.exit";
            received.extend(output_bytes(&event, &pane_id, generation));
        }
        for (seq, bytes) in received {
            received_bytes += bytes.len();
            assert!(received_bytes <= OUTPUT_BYTES);
            let acked = helper.request(
                "pane.ack",
                json!({"id":pane_id,"generation":generation,"seq":seq}),
                &mut events,
            );
            ended |= events.iter().any(|event| event["op"] == "pane.exit");
            assert!(
                acked == json!({"ok":true}) || (ended && acked == gone),
                "{acked}"
            );
            ack_count += 1;
        }
    }

    assert_eq!(received_bytes, OUTPUT_BYTES);
    assert!(ack_count > 1, "finite flood drained without repeated acks");
    helper.close_input_and_wait();
}

#[test]
fn stdin_eof_reaps_every_spawned_process() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let helper_pid = helper.child.id() as i32;
    let mut events = Vec::new();
    let mut panes = Vec::new();
    for _ in 0..2 {
        let opened = helper.request(
            "pane.open",
            open_body(
                "tree=/tmp/cf-eof-tree-$$; \
                 /bin/sh -c '/bin/sleep 1000 & grandchild=$!; \
                 printf \"%s\\n\" \"$grandchild\" >\"$1\"; wait' sh \"$tree\" & child=$!; \
                 while [ ! -s \"$tree\" ]; do :; done; grandchild=$(/bin/cat \"$tree\"); \
                 /bin/rm -f \"$tree\"; printf '%s,%s,%s\\n' \"$$\" \"$child\" \"$grandchild\"; wait \"$child\"",
                1024,
            ),
            &mut events,
        );
        panes.push((
            opened["id"].as_str().expect("opened pane id").to_string(),
            opened["generation"].as_u64().expect("opened generation"),
            Vec::new(),
        ));
    }

    while panes.iter().any(|(_, _, bytes)| !bytes.contains(&b'\n')) {
        if events.is_empty() {
            events.push(helper.receive());
        }
        for event in events.drain(..) {
            for (pane_id, generation, bytes) in &mut panes {
                if let Some((_seq, output)) = output_bytes(&event, pane_id, *generation) {
                    bytes.extend(output);
                }
            }
        }
    }
    let mut spawned_pids = Vec::new();
    for (_, _, bytes) in &panes {
        let line = String::from_utf8_lossy(bytes);
        let pids = line
            .trim()
            .split(',')
            .map(|pid| pid.parse::<i32>().expect("numeric spawned pid"))
            .collect::<Vec<_>>();
        assert_eq!(pids.len(), 3);
        spawned_pids.extend(pids);
    }
    assert!(process_exists(helper_pid));
    assert!(spawned_pids.iter().all(|pid| process_exists(*pid)));

    helper.close_input_and_wait();
    let deadline = Instant::now() + Duration::from_secs(2);
    while (process_exists(helper_pid) || spawned_pids.iter().any(|pid| process_exists(*pid)))
        && Instant::now() < deadline
    {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(!process_exists(helper_pid), "helper survived EOF");
    assert!(
        spawned_pids.iter().all(|pid| !process_exists(*pid)),
        "a spawned pane process survived EOF"
    );
}

#[test]
fn stdin_eof_kills_a_pane_before_draining_its_blocked_paste_handler() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(
            "/bin/stty raw -echo; /bin/sleep 1000 & first=$!; /bin/sleep 1000 & second=$!; \
             printf 'PIDS:%s,%s,%s\\nREADY\\n' \"$$\" \"$first\" \"$second\"; /bin/sleep 1000",
            4096,
        ),
        &mut events,
    );
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");

    let mut output = Vec::new();
    let ready_deadline = Instant::now() + FRAME_TIMEOUT;
    while !output
        .windows(b"READY\n".len())
        .any(|bytes| bytes == b"READY\n")
    {
        for event in events.drain(..) {
            if let Some((_seq, bytes)) = output_bytes(&event, &pane_id, generation) {
                output.extend(bytes);
            }
        }
        assert!(
            Instant::now() < ready_deadline,
            "pane omitted readiness marker"
        );
        if !output
            .windows(b"READY\n".len())
            .any(|bytes| bytes == b"READY\n")
        {
            events.push(helper.receive());
        }
    }
    let pid_line = String::from_utf8_lossy(&output)
        .lines()
        .find(|line| line.starts_with("PIDS:"))
        .expect("pane pid line")
        .to_string();
    let pids = pid_line["PIDS:".len()..]
        .split(',')
        .map(|pid| pid.parse::<i32>().expect("numeric spawned pid"))
        .collect::<Vec<_>>();
    assert_eq!(pids.len(), 3);
    assert!(pids.iter().all(|pid| process_exists(*pid)));

    let paste_id = helper.send_request(
        "pane.write_paste",
        json!({
            "id":pane_id,
            "generation":generation,
            "body":"x".repeat(512 * 1024),
        }),
    );
    let response_deadline = Instant::now() + Duration::from_millis(150);
    let mut paste_replied = false;
    while Instant::now() < response_deadline {
        match helper.receive_timeout(Duration::from_millis(20)) {
            Ok(frame) if frame["kind"] == "res" && frame["id"] == paste_id => {
                paste_replied = true;
                break;
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    assert!(
        !paste_replied,
        "large paste did not block on the non-consuming pane"
    );

    drop(helper.input.take());
    let exited_before_cleanup = helper.wait_for_exit(Duration::from_millis(750));
    if exited_before_cleanup.is_none() {
        kill_process_group(pids[0]);
        let _ = helper.wait_for_exit(FRAME_TIMEOUT);
    }
    let gone_deadline = Instant::now() + Duration::from_secs(2);
    while pids.iter().any(|pid| process_exists(*pid)) && Instant::now() < gone_deadline {
        thread::sleep(Duration::from_millis(10));
    }

    let status =
        exited_before_cleanup.expect("helper waited for the blocked paste before killing its pane");
    assert!(status.success(), "helper exited unsuccessfully: {status}");
    assert!(pids.iter().all(|pid| !process_exists(*pid)));
}

/// The daemon ends the helper's input to stop it, and a window still open is ended with
/// it. The helper tells the exit of each, with what the window left running, as it
/// tells a window's close, before it ends its own output: the daemon reads that to
/// its end (the rig's last word), and what a window ran was otherwise known only from
/// earlier looks. (Where the helper cannot look at a window's tree, it has no
/// processes to name.)
#[test]
fn a_host_shut_down_with_windows_open_tells_the_exit_of_each_before_it_closes_its_output() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let mut windows = Vec::new();
    for job in ["sleep 61", "sleep 62"] {
        let script =
            format!("printf ready; read go; /bin/sh -c '{job} & wait' & printf started; wait");
        let opened = helper.request("pane.open", open_body(&script, 4096), &mut events);
        assert_eq!(opened["ok"], true, "{opened}");
        let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
        let generation = opened["generation"].as_u64().expect("opened generation");
        let pane = json!({"id":pane_id,"generation":generation});
        output_until(&helper, &mut events, &pane_id, generation, b"ready");
        // The turn begins, and the window starts its job.
        assert_eq!(
            helper.request("pane.mark", pane.clone(), &mut events),
            json!({"ok":true})
        );
        helper.request(
            "pane.input",
            json!({"id":pane_id,"generation":generation,"bytes":[103,111,10]}),
            &mut events,
        );
        output_until(&helper, &mut events, &pane_id, generation, b"started");
        #[cfg(target_os = "macos")]
        background_with(&mut helper, &mut events, &pane, job);
        windows.push((pane_id, generation, job));
    }

    drop(helper.input.take());
    let said = helper.frames_to_the_end(Duration::from_secs(20));
    let status = helper
        .wait_for_exit(FRAME_TIMEOUT)
        .expect("the helper did not exit when its output had ended");
    assert!(status.success(), "helper exited unsuccessfully: {status}");

    for (pane_id, generation, job) in windows {
        let exits: Vec<&Value> = said
            .iter()
            .chain(events.iter())
            .filter(|frame| frame["op"] == "pane.exit" && frame["body"]["id"] == pane_id.as_str())
            .map(|frame| &frame["body"])
            .collect();
        assert_eq!(exits.len(), 1, "{pane_id} has one exit: {exits:?}");
        assert_eq!(exits[0]["generation"], generation);
        // The job it left is named in its own exit and no other's.
        #[cfg(target_os = "macos")]
        {
            let named = |exit: &Value, command: &str| {
                exit["background"]
                    .as_array()
                    .is_some_and(|work| work.iter().any(|process| process["command"] == command))
            };
            assert!(named(exits[0], job), "{job} is named: {:?}", exits[0]);
        }
        #[cfg(not(target_os = "macos"))]
        let _ = job;
    }
}

/// The helper's peer is gone, as when the daemon died: the exits it tells have no one
/// to read them, and shutting down does not wait for one.
#[test]
fn a_host_whose_output_is_gone_shuts_down_without_hanging() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let mut spawned = Vec::new();
    for _ in 0..2 {
        let opened = helper.request(
            "pane.open",
            open_body("printf 'PID:%s\\n' $$; exec /bin/sleep 1000", 4096),
            &mut events,
        );
        assert_eq!(opened["ok"], true, "{opened}");
        let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
        let generation = opened["generation"].as_u64().expect("opened generation");
        let said = output_until(&helper, &mut events, &pane_id, generation, b"\n");
        let pid: i32 = String::from_utf8_lossy(&said)
            .trim()
            .trim_start_matches("PID:")
            .parse()
            .expect("the window's number");
        spawned.push(pid);
    }
    helper.lose_output();

    drop(helper.input.take());
    let status = helper
        .wait_for_exit(Duration::from_secs(20))
        .expect("the helper hung with no one to hear its exits");
    assert!(status.success(), "helper exited unsuccessfully: {status}");
    let gone = Instant::now() + Duration::from_secs(2);
    while spawned.iter().any(|pid| process_exists(*pid)) && Instant::now() < gone {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        spawned.iter().all(|pid| !process_exists(*pid)),
        "a window outlived the helper: {spawned:?}"
    );
}

/// A peer that holds its end of the pipe and reads nothing (hung, not dead): the exits the
/// helper has to tell cannot be written, and the helper does not wait for them for ever. It
/// waits for the exits as long as it can afford (`FAREWELL`) and for what it told to be
/// written as long again at most (`FLUSH`), and ends.
#[test]
fn a_host_whose_peer_reads_nothing_shuts_down_within_its_bounds() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    for _ in 0..2 {
        let opened = helper.request(
            "pane.open",
            open_body("exec yes", 8 * 1024 * 1024),
            &mut events,
        );
        assert_eq!(opened["ok"], true, "{opened}");
    }
    // The windows print without end, and the peer takes no more of it: the pipe fills.
    helper.stop_reading();
    thread::sleep(Duration::from_secs(1));

    drop(helper.input.take());
    let began = Instant::now();
    let status = helper
        .wait_for_exit(Duration::from_secs(20))
        .expect("the helper waited for a peer that reads nothing");
    assert!(status.success(), "helper exited unsuccessfully: {status}");
    assert!(
        began.elapsed() < Duration::from_secs(15),
        "the helper took {:?} to stop",
        began.elapsed()
    );
}

/// A paste says whether anything of it reached the window: one refused before
/// a byte was written was not sent, and the daemon may send it again; one
/// whose write failed partway is uncertain, and the daemon asks the harness.
#[test]
fn a_paste_answers_whether_anything_reached_the_window() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    // A window that reads nothing: a large paste fills its terminal and waits.
    let opened = helper.request(
        "pane.open",
        open_body(
            "/bin/stty raw -echo; printf ready; exec /bin/sleep 1000",
            4096,
        ),
        &mut events,
    );
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let _ = output_until(&helper, &mut events, &pane_id, generation, b"ready");

    fn refused(error: &str, cause: &str) -> Value {
        json!({"ok":false,"admitted":false,"bytesWritten":0,"error":error,"cause":cause})
    }
    assert_eq!(
        helper.request(
            "pane.write_paste",
            json!({"id":pane_id,"generation":generation + 1,"body":"late"}),
            &mut events,
        ),
        refused("stale-pane", "stale pane generation")
    );
    assert_eq!(
        helper.request(
            "pane.write_paste",
            json!({"id":pane_id,"generation":generation,"body":"\u{7}"}),
            &mut events,
        ),
        refused(
            "invalid-body",
            "invalid paste body: control byte 0x07 is not allowed"
        )
    );
    let unnamed = helper.request(
        "pane.write_paste",
        json!({"generation":generation,"body":"no pane"}),
        &mut events,
    );
    assert_eq!(
        (
            &unnamed["admitted"],
            &unnamed["bytesWritten"],
            &unnamed["error"]
        ),
        (&json!(false), &json!(0), &json!("invalid-body")),
        "{unnamed}"
    );
    assert!(unnamed["cause"].is_string(), "{unnamed}");

    // The window is closed while a paste is going into it: some of the paste
    // reached its terminal.
    let paste = helper.send_request(
        "pane.write_paste",
        json!({"id":pane_id,"generation":generation,"body":"x".repeat(512 * 1024)}),
    );
    assert!(
        matches!(
            helper.receive_timeout(Duration::from_millis(150)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "the paste did not wait on the window that reads nothing"
    );
    let kill = helper.send_request("pane.kill", json!({"id":pane_id,"generation":generation}));
    let mut answers = std::collections::HashMap::new();
    while answers.len() < 2 {
        let frame = helper.receive();
        if frame["kind"] == "res" {
            answers.insert(
                frame["id"].as_str().unwrap_or_default().to_string(),
                frame["body"].clone(),
            );
        }
    }
    assert_eq!(answers[&kill], json!({"ok":true}));
    let cut = &answers[&paste];
    assert_eq!(
        (&cut["ok"], &cut["admitted"], &cut["error"]),
        (&json!(false), &Value::Null, &json!("uncertain")),
        "{cut}"
    );
    assert!(cut["cause"].is_string(), "{cut}");
    assert!(cut.get("bytesWritten").is_none(), "{cut}");
    helper.close_input_and_wait();
}

#[test]
fn helper_is_inert_when_stdin_is_not_a_pipe() {
    let output = Command::new(env!("CARGO_BIN_EXE_consensflow-bridge"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .expect("run helper with non-pipe stdin");
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

fn process_exists(pid: i32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }

    // SAFETY: signal zero checks existence without delivering a signal.
    (unsafe { kill(pid, 0) } == 0) || std::io::Error::last_os_error().raw_os_error() == Some(1)
}

fn kill_process_group(process_group_id: i32) {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }

    const SIGKILL: i32 = 9;
    // SAFETY: the PTY child is its process-group leader and this is test-only cleanup.
    let _ = unsafe { kill(-process_group_id, SIGKILL) };
}

/// A window opens at the identity the daemon reserved for it, and only there:
/// the daemon names every pane it opens, once.
#[test]
fn product_bridge_contract_preserves_app_identity_and_opens_it_once() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let body = json!({
        "id":"product-pane", "generation":7, "cwd":"/tmp", "argv":["/bin/cat"], "env":{}
    });
    let opened = helper.request("pane.open", body.clone(), &mut events);
    assert_eq!(opened["ok"], true, "production open rejected: {opened}");
    let pid = opened["pid"].as_u64().filter(|pid| *pid > 0);
    assert!(pid.is_some(), "the window's process id: {opened}");
    assert_eq!(
        opened,
        json!({"ok":true,"id":"product-pane","generation":7,"pid":pid})
    );
    assert_eq!(
        helper.request("pane.open", body, &mut events),
        json!({"ok":false,"error":"pane product-pane generation 7 is already open"})
    );
    let unnamed = helper.request(
        "pane.open",
        json!({"cwd":"/tmp", "argv":["/bin/cat"], "env":{}}),
        &mut events,
    );
    assert_eq!(unnamed["ok"], false, "an open with no identity: {unnamed}");
    assert_eq!(
        helper.request("pane.list", json!({}), &mut events)["panes"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    helper.close_input_and_wait();
}

/// The answer to `pane.open` names the window's process, so the daemon can
/// find what the harness writes about itself from the window's first moment.
#[test]
fn a_pane_open_answers_the_process_id_of_the_windows_program() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body("printf 'PID:%s:' \"$$\"; exec /bin/sleep 30", 1024),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let printed = output_until(&helper, &mut events, &pane_id, generation, b":");
    let pid = String::from_utf8_lossy(&printed)
        .trim_start_matches("PID:")
        .trim_end_matches(':')
        .parse::<u64>()
        .expect("the pane's pid");
    assert_eq!(opened["pid"], pid, "{opened}");
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events,
        ),
        json!({"ok":true})
    );
    helper.close_input_and_wait();
}

/// A window whose program exits while something it started still holds its
/// terminal (a dev server deaf to the hangup) ends all the same, and what it
/// left running ends with it. The program is its terminal's controlling
/// process, and macOS revokes the terminal when it exits, so the output ends
/// and `pane.exit` follows; Linux keeps the terminal open for the child, and
/// would need an exit watcher as ConPTY does. The program exits on a line typed
/// to it once the window is open: the host knows a program by what it read of it
/// when the window opened, and on a loaded runner one that exits at once can be
/// gone before that (then what it started is no window's, and is left running).
#[cfg(target_os = "macos")]
#[test]
fn a_window_ends_when_its_program_exits_while_a_background_child_holds_its_terminal() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(
            "trap '' HUP; /bin/sleep 1000 & printf 'CHILD %s END' \"$!\"; read _; exit 0",
            1024,
        ),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let printed = output_until(&helper, &mut events, &pane_id, generation, b" END");
    let child = String::from_utf8_lossy(&printed)
        .trim_start_matches("CHILD ")
        .trim_end_matches(" END")
        .parse::<i32>()
        .expect("the background child's pid");
    assert_eq!(
        helper.request(
            "pane.input",
            json!({"id":pane_id,"generation":generation,"bytes":[10]}),
            &mut events,
        ),
        json!({"ok":true})
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while !events
        .iter()
        .any(|event| event["op"] == "pane.exit" && event["body"]["id"] == pane_id.as_str())
    {
        let left = deadline.saturating_duration_since(Instant::now());
        match helper.receive_timeout(left) {
            Ok(frame) => events.push(frame),
            Err(_) => panic!("no pane.exit while the background child holds the terminal"),
        }
    }
    let gone = Instant::now() + Duration::from_secs(2);
    while process_exists(child) && Instant::now() < gone {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !process_exists(child),
        "what the window left running survived it"
    );
    helper.close_input_and_wait();
}

/// A program that lets go of its terminal and opens it again: it takes the terminal
/// from everything that has it, itself included (`revoke`, which is what macOS does
/// to a terminal whose controlling process exits), ignoring the hangup that sends, so
/// the terminal's output ends; and two seconds later it opens the terminal again by its
/// name and goes on, as a program that resets its terminal while its work goes on does.
#[cfg(target_os = "macos")]
const OPENS_ITS_TERMINAL_AGAIN: &str = "exec /usr/bin/python3 -c '
import ctypes, os, signal, time
signal.signal(signal.SIGHUP, signal.SIG_IGN)
name = os.ttyname(0)
print(\"PROGRAM %d END\" % os.getpid(), flush=True)
libc = ctypes.CDLL(None, use_errno=True)
if libc.revoke(name.encode()) != 0:
    raise OSError(ctypes.get_errno(), \"revoke\")
time.sleep(2)
again = os.open(name, os.O_RDWR | os.O_NOCTTY)
while True:
    os.write(again, b\"BACK\\r\\n\")
    time.sleep(1)
'";

/// A window whose program lets go of its terminal and opens it again is read on: the
/// end of its output while the program runs is no end of the window, which is neither
/// told closed nor ended. Both reviews of 2026-10-16 reproduced it: the reader stopped at
/// the first end, the daemon was told the window closed, and the program was killed five
/// seconds later, though it went on writing to its terminal.
#[cfg(target_os = "macos")]
#[test]
fn a_program_that_lets_go_of_its_terminal_and_opens_it_again_is_read_on_and_not_ended() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(OPENS_ITS_TERMINAL_AGAIN, 1024),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let printed = output_until(&helper, &mut events, &pane_id, generation, b" END");
    let printed = String::from_utf8_lossy(&printed);
    let program = printed
        .trim()
        .trim_start_matches("PROGRAM ")
        .trim_end_matches(" END")
        .parse::<i32>()
        .unwrap_or_else(|_| panic!("the program's pid: {printed}"));

    // Ten seconds: past the reopen, the wait for an exit code and the look for an exit
    // that a window told closed is given before its program is ended.
    let mut back = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    while Instant::now() < until {
        let left = until.saturating_duration_since(Instant::now());
        let Ok(frame) = helper.receive_timeout(left) else {
            break;
        };
        if let Some((_, bytes)) = output_bytes(&frame, &pane_id, generation) {
            back.extend(bytes);
        }
        events.push(frame);
    }
    let told_closed = events
        .iter()
        .any(|event| event["op"] == "pane.exit" && event["body"]["id"] == pane_id.as_str());
    let runs = process_exists(program);
    // Whatever came of it, the program is the test's to end.
    kill_process_group(program);
    assert!(
        !told_closed,
        "the window was told closed while its program ran"
    );
    assert!(runs, "the program was ended while it ran");
    assert!(
        String::from_utf8_lossy(&back).matches("BACK").count() >= 3,
        "what it wrote to its terminal opened again was not read: {:?}",
        String::from_utf8_lossy(&back)
    );
    helper.close_input_and_wait();
}

/// Sol's sequence (the second review of 2026-10-16): the program lets go of its terminal
/// (the output ends) while a helper it started in a session of its own waits; the program
/// exits, and the helper, the moment it is an orphan, opens the terminal again and holds
/// it, saying nothing. The helper outlives it.
#[cfg(target_os = "macos")]
const EXITS_WHILE_AN_ORPHAN_HOLDS_ITS_TERMINAL: &str = "exec /usr/bin/python3 -c '
import ctypes, os, signal, time
signal.signal(signal.SIGHUP, signal.SIG_IGN)
name = os.ttyname(0)
program = os.getpid()
helper = os.fork()
if helper == 0:
    os.setsid()
    while os.getppid() == program:
        time.sleep(0.001)
    os.open(name, os.O_RDWR | os.O_NOCTTY)
    time.sleep(30)
    os._exit(0)
print(\"PROGRAM %d HELPER %d END\" % (program, helper), flush=True)
libc = ctypes.CDLL(None, use_errno=True)
libc.revoke(name.encode())
time.sleep(2)
os._exit(4)
'";

/// The window is told closed when its program exits, though a helper it left holds its
/// terminal with nothing to say: the reader that waited for the terminal to come back
/// blocked on the helper's silence for ever, and no `pane.exit` was sent (Sol, the second
/// review of 2026-10-16, reproduced it on macOS).
#[cfg(target_os = "macos")]
#[test]
fn a_program_that_exits_while_an_orphan_holds_its_terminal_is_told_closed() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(EXITS_WHILE_AN_ORPHAN_HOLDS_ITS_TERMINAL, 1024),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let printed = output_until(&helper, &mut events, &pane_id, generation, b" END");
    let printed = String::from_utf8_lossy(&printed);
    let numbers: Vec<i32> = printed
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    let [_, orphan] = numbers[..] else {
        panic!("the program's and its helper's numbers: {printed}")
    };

    let until = Instant::now() + Duration::from_secs(15);
    let mut exit = None;
    while exit.is_none() && Instant::now() < until {
        let left = until.saturating_duration_since(Instant::now());
        let Ok(frame) = helper.receive_timeout(left) else {
            break;
        };
        if frame["op"] == "pane.exit" && frame["body"]["id"] == pane_id.as_str() {
            exit = Some(frame["body"].clone());
        }
    }
    // The helper is the test's to end, whatever came of it.
    kill_process_group(orphan);
    let exit = exit.expect("no pane.exit for a program that exited");
    assert_eq!(exit["exitCode"], 4, "{exit}");
    helper.close_input_and_wait();
}

/// The pane's `pane.exit` from the frames that come within `wait`, and whether the process
/// `pid` was still there when it came.
#[cfg(target_os = "macos")]
fn exit_and_whether(
    helper: &Headless,
    pane_id: &str,
    pid: i32,
    wait: Duration,
) -> Option<(Value, bool)> {
    let until = Instant::now() + wait;
    loop {
        let left = until.saturating_duration_since(Instant::now());
        let frame = helper.receive_timeout(left).ok()?;
        if frame["op"] == "pane.exit" && frame["body"]["id"] == pane_id {
            return Some((frame["body"].clone(), process_exists(pid)));
        }
    }
}

/// A program that lets go of its terminal while a helper it started in a session of its
/// own opens it again and writes a byte to it every twenty milliseconds, for as long as
/// it is let; the program exits a moment later, and the helper, an orphan, writes on.
#[cfg(target_os = "macos")]
const EXITS_WHILE_AN_ORPHAN_WRITES_ON: &str = "exec /usr/bin/python3 -c '
import ctypes, os, signal, time
signal.signal(signal.SIGHUP, signal.SIG_IGN)
name = os.ttyname(0)
helper = os.fork()
if helper == 0:
    os.setsid()
    time.sleep(0.5)
    again = os.open(name, os.O_RDWR | os.O_NOCTTY)
    for _ in range(1500):
        os.write(again, b\".\")
        time.sleep(0.02)
    os._exit(0)
print(\"PROGRAM %d HELPER %d END\" % (os.getpid(), helper), flush=True)
ctypes.CDLL(None).revoke(name.encode())
time.sleep(1.5)
os._exit(4)
'";

/// The window is told closed when its program exits, though an orphan it left writes to
/// its terminal without a pause: the reader asked whether the program had gone only when
/// nothing came to read, and never asked (both reviews of round 3 reproduced it).
#[cfg(target_os = "macos")]
#[test]
fn a_program_that_exits_while_an_orphan_writes_on_to_its_terminal_is_told_closed() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(EXITS_WHILE_AN_ORPHAN_WRITES_ON, 1 << 20),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let printed = output_until(&helper, &mut events, &pane_id, generation, b" END");
    let printed = String::from_utf8_lossy(&printed);
    let numbers: Vec<i32> = printed
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .collect();
    let [program, orphan] = numbers[..] else {
        panic!("the program's and its helper's numbers: {printed}")
    };

    let exit = exit_and_whether(&helper, &pane_id, program, Duration::from_secs(15));
    // The helper is the test's to end, whatever came of it.
    kill_process_group(orphan);
    let (exit, _) = exit.expect("no pane.exit for a program that exited");
    assert_eq!(exit["exitCode"], 4, "{exit}");
    helper.close_input_and_wait();
}

/// A program that ignores the request to end, and, asked, lets go of its terminal and
/// cleans up for six seconds before it exits.
#[cfg(target_os = "macos")]
const CLEANS_UP_FOR_SIX_SECONDS: &str = "exec /usr/bin/python3 -c '
import ctypes, os, signal, time
name = os.ttyname(0)
def clean_up(number, frame):
    ctypes.CDLL(None).revoke(name.encode())
    time.sleep(6)
    os._exit(7)
signal.signal(signal.SIGHUP, signal.SIG_IGN)
signal.signal(signal.SIGTERM, clean_up)
print(\"PROGRAM %d END\" % os.getpid(), flush=True)
time.sleep(1000)
'";

/// The daemon is told a window closed only on proof that its program has gone: a close
/// takes the pane out of the table before it has ended its programs, and a program that
/// ignored the request, let go of its terminal and cleaned up through six seconds of the
/// close's grace was told gone after one, while it ran (both reviews of round 3).
#[cfg(target_os = "macos")]
#[test]
fn a_window_closed_while_its_program_cleans_up_is_told_closed_once_the_program_has_gone() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(CLEANS_UP_FOR_SIX_SECONDS, 1 << 20),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let printed = output_until(&helper, &mut events, &pane_id, generation, b" END");
    let printed = String::from_utf8_lossy(&printed);
    let program = printed
        .split_whitespace()
        .find_map(|word| word.parse::<i32>().ok())
        .unwrap_or_else(|| panic!("the program's number: {printed}"));

    let asked = Instant::now();
    helper.send_request("pane.kill", json!({"id":pane_id,"generation":generation}));
    let exit = exit_and_whether(&helper, &pane_id, program, Duration::from_secs(30));
    // Whatever came of it, the program's cleanup is over before the test is.
    let over = Instant::now() + Duration::from_secs(8);
    while process_exists(program) && Instant::now() < over {
        thread::sleep(Duration::from_millis(50));
    }
    let (exit, ran_when_told) = exit.expect("no pane.exit for the window closed");
    assert!(
        !ran_when_told,
        "the window was told closed after {:?}, while its program cleaned up",
        asked.elapsed()
    );
    assert!(
        asked.elapsed() >= Duration::from_secs(5),
        "{:?}",
        asked.elapsed()
    );
    assert_eq!(exit["exitCode"], 7, "{exit}");
    helper.close_input_and_wait();
}

#[test]
fn product_bridge_contract_forwards_a_natural_pane_exit() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request("pane.open", open_body("printf finished", 1024), &mut events);
    assert_eq!(opened["ok"], true);
    while !events.iter().any(|event| event["op"] == "pane.exit") {
        events.push(helper.receive());
    }
    let ended = events
        .iter()
        .find(|event| event["op"] == "pane.exit")
        .expect("exit event");
    assert_eq!(ended["body"]["id"], opened["id"]);
    assert_eq!(ended["body"]["generation"], opened["generation"]);
    helper.close_input_and_wait();
}

/// The `pane.exit` of the pane `opened` names, once the helper sends it.
fn exit_of(helper: &Headless, events: &mut Vec<Value>, opened: &Value) -> Value {
    while !events
        .iter()
        .any(|event| event["op"] == "pane.exit" && event["body"]["id"] == opened["id"])
    {
        events.push(helper.receive());
    }
    events
        .iter()
        .find(|event| event["op"] == "pane.exit" && event["body"]["id"] == opened["id"])
        .expect("exit event")["body"]
        .clone()
}

/// A window that fails to come up says why on its screen and ends: the exit
/// carries the code the program ended with and the last lines it showed, which
/// are all the daemon is left of it.
#[test]
fn a_pane_exit_says_how_the_program_ended_and_what_its_screen_showed() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(
            "printf 'No API key found for the selected model.\\nUse /login to log into a provider.\\n'; exit 3",
            1024,
        ),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    assert_eq!(
        exit_of(&helper, &mut events, &opened),
        json!({
            "id": opened["id"],
            "generation": opened["generation"],
            "exitCode": 3,
            "tail": [
                "No API key found for the selected model.",
                "Use /login to log into a provider.",
            ],
        })
    );
    helper.close_input_and_wait();
}

/// A screen that showed nothing is told as such, apart from a host that does
/// not say: the tail is there and empty.
#[test]
fn a_pane_exit_of_a_program_that_printed_nothing_has_an_empty_tail() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request("pane.open", open_body("exit 0", 1024), &mut events);
    assert_eq!(opened["ok"], true, "{opened}");
    let exit = exit_of(&helper, &mut events, &opened);
    assert_eq!((&exit["exitCode"], &exit["tail"]), (&json!(0), &json!([])));
    helper.close_input_and_wait();
}

/// A program a signal ended has the code 1 and the signal's name.
#[test]
fn a_pane_exit_of_a_program_a_signal_ended_names_the_signal() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body("printf 'about to go'; kill -9 $$", 1024),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let exit = exit_of(&helper, &mut events, &opened);
    assert_eq!(exit["exitCode"], 1, "{exit}");
    assert!(
        exit["signal"].as_str().is_some_and(|name| !name.is_empty()),
        "{exit}"
    );
    assert_eq!(exit["tail"], json!(["about to go"]), "{exit}");
    helper.close_input_and_wait();
}

/// A window the daemon closes while it is stuck ends with the screen it was
/// stuck on, in the same event.
#[test]
fn a_pane_the_daemon_kills_exits_with_the_screen_it_was_stuck_on() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body("printf 'waiting for a login\\n'; exec /bin/sleep 30", 1024),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id");
    let generation = opened["generation"].as_u64().expect("generation");
    output_until(&helper, &mut events, pane_id, generation, b"login");
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );
    let exit = exit_of(&helper, &mut events, &opened);
    assert_eq!(exit["tail"], json!(["waiting for a login"]), "{exit}");
    assert!(exit["signal"].is_string(), "{exit}");
    helper.close_input_and_wait();
}

/// The screen is what a person would see, not the bytes in the order they
/// were printed: a line drawn over another replaces it.
#[test]
fn a_pane_exit_has_the_screen_and_not_the_stream() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(
            "printf 'loading 10%%\\rready     \\n'; printf '\\033[2J\\033[Hlast screen'",
            1024,
        ),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let exit = exit_of(&helper, &mut events, &opened);
    assert_eq!(exit["tail"], json!(["last screen"]), "{exit}");
    helper.close_input_and_wait();
}

/// A snapshot carries the screen's last lines when it asks for them and is
/// what it was when it does not: it is polled all the time.
#[test]
fn a_snapshot_gives_the_screens_tail_only_when_asked_for_it() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body("printf 'ready\\nsteady\\n'; exec /bin/sleep 30", 1024),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id");
    let generation = opened["generation"].as_u64().expect("generation");
    output_until(&helper, &mut events, pane_id, generation, b"steady");

    let plain = helper.request(
        "pane.snapshot",
        json!({"id":pane_id,"generation":generation}),
        &mut events,
    );
    assert_eq!(plain["ok"], true, "{plain}");
    assert!(plain.get("tail").is_none(), "unasked: {plain}");
    let asked = |lines: u64, helper: &mut Headless, events: &mut Vec<Value>| {
        helper.request(
            "pane.snapshot",
            json!({"id":pane_id,"generation":generation,"tail":lines}),
            events,
        )
    };
    assert_eq!(
        asked(1, &mut helper, &mut events)["tail"],
        json!(["steady"])
    );
    let all = asked(1_000, &mut helper, &mut events);
    assert_eq!(all["tail"], json!(["ready", "steady"]));
    assert_eq!(
        all["unsent"], plain["unsent"],
        "the rest of the answer is as it was"
    );
    assert_eq!(asked(0, &mut helper, &mut events)["tail"], json!([]));

    let unknown = helper.request(
        "pane.snapshot",
        json!({"id":pane_id,"generation":generation,"lines":1}),
        &mut events,
    );
    assert_eq!(unknown["ok"], false, "{unknown}");
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );
    helper.close_input_and_wait();
}

/// A window whose program ends on its own leaves the pane table after its
/// `pane.exit`, its child reaped: nothing waits for a `pane.kill` the daemon
/// never sends, and an update is not held up by a window long gone.
#[test]
fn a_pane_whose_program_exits_leaves_the_table_after_its_exit_event() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body("printf 'PID:%s:' \"$$\"; read line", 1024),
        &mut events,
    );
    assert_eq!(opened["ok"], true);
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let printed = output_until(&helper, &mut events, &pane_id, generation, b":");
    let pid = String::from_utf8_lossy(&printed)
        .trim_start_matches("PID:")
        .trim_end_matches(':')
        .parse::<i32>()
        .expect("the pane's pid");
    assert!(process_exists(pid));

    assert_eq!(
        helper.request(
            "pane.input",
            json!({"id":pane_id,"generation":generation,"bytes":[13]}),
            &mut events,
        ),
        json!({"ok":true})
    );
    while !events.iter().any(|event| event["op"] == "pane.exit") {
        events.push(helper.receive());
    }
    let deadline = Instant::now() + FRAME_TIMEOUT;
    loop {
        let listed = helper.request("pane.list", json!({}), &mut events);
        if listed["panes"] == json!([]) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the ended window stayed in the table: {listed}"
        );
        thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !process_exists(pid),
        "the ended window's child was not reaped"
    );
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events,
        ),
        json!({"ok":true}),
        "closing a window that already ended is done, not an error"
    );
    helper.close_input_and_wait();
}

#[test]
fn pane_advertises_native_color_capability_and_preserves_ansi_bytes() {
    use std::ffi::OsString;

    let _pty_guard = serial_headless_test();
    // Simulate a Finder launch: strip color advertisement from this process
    // so the bridge helper inherits a color-blind parent environment.
    let saved_term = std::env::var_os("TERM");
    let saved_colorterm = std::env::var_os("COLORTERM");
    let saved_force_color = std::env::var_os("FORCE_COLOR");
    let saved_no_color = std::env::var_os("NO_COLOR");
    std::env::remove_var("TERM");
    std::env::remove_var("COLORTERM");
    std::env::remove_var("FORCE_COLOR");
    std::env::remove_var("NO_COLOR");
    struct RestoreColorEnv {
        term: Option<OsString>,
        colorterm: Option<OsString>,
        force_color: Option<OsString>,
        no_color: Option<OsString>,
    }
    impl Drop for RestoreColorEnv {
        fn drop(&mut self) {
            if let Some(value) = self.term.take() {
                std::env::set_var("TERM", value);
            }
            if let Some(value) = self.colorterm.take() {
                std::env::set_var("COLORTERM", value);
            }
            if let Some(value) = self.force_color.take() {
                std::env::set_var("FORCE_COLOR", value);
            }
            if let Some(value) = self.no_color.take() {
                std::env::set_var("NO_COLOR", value);
            }
        }
    }
    let _restore = RestoreColorEnv {
        term: saved_term,
        colorterm: saved_colorterm,
        force_color: saved_force_color,
        no_color: saved_no_color,
    };

    let mut helper = Headless::spawn();
    let mut events = Vec::new();

    // A color-blind parent must still yield a color-capable child: the PTY
    // advertises the native xterm capability instead of inheriting blankness.
    let opened = helper.request(
        "pane.open",
        open_body("printf '%s|%s' \"$TERM\" \"$COLORTERM\"", 1024),
        &mut events,
    );
    assert_eq!(opened["ok"], true);
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    assert_eq!(
        output_until(
            &helper,
            &mut events,
            &pane_id,
            generation,
            b"xterm-256color|truecolor"
        ),
        b"xterm-256color|truecolor",
        "Finder-launched panes must advertise TERM=xterm-256color and COLORTERM=truecolor"
    );
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );

    // Standard ANSI SGR bytes travel through the PTY unmodified so each
    // harness renders its own native styling.
    let opened = helper.request(
        "pane.open",
        open_body("printf '\\033[31mred\\033[0m'", 1024),
        &mut events,
    );
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    assert_eq!(
        output_until(&helper, &mut events, &pane_id, generation, b"\x1b[0m"),
        b"\x1b[31mred\x1b[0m",
        "standard ANSI SGR sequences must reach the renderer untouched"
    );
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );

    // Explicit user no-color choices stay untouched: an overlaid TERM=dumb
    // is honored, a dropped TERM stays absent, and the bridge never forces
    // color or clears NO_COLOR semantics.
    let mut dumb = open_body("printf '%s' \"$TERM\"", 1024);
    dumb["env"] = json!({"TERM":"dumb"});
    let opened = helper.request("pane.open", dumb, &mut events);
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    assert_eq!(
        output_until(&helper, &mut events, &pane_id, generation, b"dumb"),
        b"dumb",
        "an explicit TERM=dumb overlay must survive"
    );
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );

    // A dropped TERM stays removed: the bridge must not put back what the
    // frame explicitly removed. (macOS /bin/sh reports its own "dumb"
    // default when TERM is unset, so "dumb" -- never the bridge's
    // "xterm-256color" -- is the observable proof of removal.)
    let mut dropped = open_body("printf '%s' \"$TERM\"", 1024);
    dropped["dropEnv"] = json!(["TERM"]);
    let opened = helper.request("pane.open", dropped, &mut events);
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    assert_eq!(
        output_until(&helper, &mut events, &pane_id, generation, b"dumb"),
        b"dumb",
        "dropEnv TERM must stay removed"
    );
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );

    let opened = helper.request(
        "pane.open",
        open_body(
            "if [ \"${FORCE_COLOR+set}\" = set ]; then printf forced; else printf unforced; fi",
            1024,
        ),
        &mut events,
    );
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    assert_eq!(
        output_until(&helper, &mut events, &pane_id, generation, b"unforced"),
        b"unforced",
        "the bridge must never force FORCE_COLOR"
    );
    assert_eq!(
        helper.request(
            "pane.kill",
            json!({"id":pane_id,"generation":generation}),
            &mut events
        ),
        json!({"ok":true})
    );

    helper.close_input_and_wait();
}

#[test]
fn pane_replaces_a_noninteractive_launchers_dumb_term() {
    let _pty_guard = serial_headless_test();
    for inherited in ["dumb", ""] {
        let mut helper = Headless::spawn_with_env(&[("TERM", inherited)]);
        let mut events = Vec::new();
        let opened = helper.request(
            "pane.open",
            open_body("printf '%s:end' \"$TERM\"", 1024),
            &mut events,
        );
        assert_eq!(opened["ok"], true);
        let id = opened["id"].as_str().unwrap();
        let generation = opened["generation"].as_u64().unwrap();
        assert_eq!(
            output_until(&helper, &mut events, id, generation, b":end"),
            b"xterm-256color:end",
            "a noninteractive launcher does not describe the pane's xterm"
        );
    }
}

#[test]
fn pane_names_its_terminal_whatever_terminal_launched_the_app() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn_with_env(&[
        ("TERM_PROGRAM", "iTerm.app"),
        ("TERM_PROGRAM_VERSION", "3.5"),
    ]);
    let mut events = Vec::new();
    let mut said = |body: Value, end: &[u8]| {
        let opened = helper.request("pane.open", body, &mut events);
        assert_eq!(opened["ok"], true);
        let id = opened["id"].as_str().unwrap().to_owned();
        let generation = opened["generation"].as_u64().unwrap();
        output_until(&helper, &mut events, &id, generation, end)
    };
    let command = "printf '%s|%s:end' \"${TERM_PROGRAM-unset}\" \"${TERM_PROGRAM_VERSION-unset}\"";
    assert_eq!(
        said(open_body(command, 1024), b":end"),
        format!("ConsensFlow|{}:end", env!("CARGO_PKG_VERSION")).as_bytes(),
        "a pane is ConsensFlow's terminal, not the launcher's"
    );
    // What a frame says of it wins, and what it takes away stays away.
    let mut named = open_body(command, 1024);
    named["env"] = json!({"TERM_PROGRAM": "vscode"});
    assert_eq!(said(named, b":end"), b"vscode|3.5:end");
    let mut dropped = open_body(command, 1024);
    dropped["dropEnv"] = json!(["TERM_PROGRAM", "TERM_PROGRAM_VERSION"]);
    assert_eq!(said(dropped, b":end"), b"unset|unset:end");
    helper.close_input_and_wait();
}

#[test]
fn pane_does_not_inherit_a_launchers_disabled_colors() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn_with_env(&[
        ("TERM", "dumb"),
        ("COLORTERM", ""),
        ("NO_COLOR", "1"),
        ("FORCE_COLOR", "0"),
    ]);
    let mut events = Vec::new();
    let command = "printf '%s|%s|%s|%s:end' \"$TERM\" \"$COLORTERM\" \"${NO_COLOR-unset}\" \"${FORCE_COLOR-unset}\"";
    let opened = helper.request("pane.open", open_body(command, 1024), &mut events);
    assert_eq!(opened["ok"], true);
    let id = opened["id"].as_str().unwrap();
    let generation = opened["generation"].as_u64().unwrap();
    assert_eq!(
        output_until(&helper, &mut events, id, generation, b":end"),
        b"xterm-256color|truecolor|unset|unset:end",
        "the app's real terminal must not inherit a tool runner's plain-output flags"
    );
    let mut explicit = open_body(command, 1024);
    explicit["env"] =
        json!({"TERM":"vt100", "COLORTERM":"custom", "NO_COLOR":"1", "FORCE_COLOR":"0"});
    let opened = helper.request("pane.open", explicit, &mut events);
    let id = opened["id"].as_str().unwrap();
    let generation = opened["generation"].as_u64().unwrap();
    assert_eq!(
        output_until(&helper, &mut events, id, generation, b":end"),
        b"vt100|custom|1|0:end",
        "an explicit pane environment remains authoritative"
    );
    helper.close_input_and_wait();
}

/// What the window's tree has running that its turn started, once `command` is
/// among it: the program says it started a job as it forks it, and until the job
/// is executed it shows the command line of the program that forked it.
#[cfg(target_os = "macos")]
fn background_with(
    helper: &mut Headless,
    events: &mut Vec<Value>,
    pane: &Value,
    command: &str,
) -> Value {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let left = helper.request("pane.background", pane.clone(), events);
        let listed = left["processes"].as_array().is_some_and(|processes| {
            processes
                .iter()
                .any(|process| process["command"] == command)
        });
        if listed {
            return left;
        }
        assert!(
            Instant::now() < deadline,
            "{command} was never listed: {left}"
        );
        thread::sleep(Duration::from_millis(10));
    }
}

/// The daemon says where a window's turn begins and asks what the turn left
/// running; a window it closes with some of it running says so in its exit.
/// (On macOS the helper looks at the window's process tree; elsewhere it has
/// none to look at.)
#[cfg(target_os = "macos")]
#[test]
fn a_turn_marked_over_the_wire_lists_what_it_left_and_the_exit_of_the_window_closed_says_so() {
    let _pty_guard = serial_headless_test();
    let mut outsider = Sentinel::start();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request(
        "pane.open",
        open_body(
            "sleep 33 & printf ready; read go; /bin/sh -c 'sleep 31 & wait' & printf started; wait",
            4096,
        ),
        &mut events,
    );
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let pane = json!({"id":pane_id,"generation":generation});
    output_until(&helper, &mut events, &pane_id, generation, b"ready");

    assert_eq!(
        helper.request("pane.background", pane.clone(), &mut events),
        json!({"ok":true,"total":0,"processes":[]}),
        "a window never marked has no background work: nothing says where its turn began"
    );
    assert_eq!(
        helper.request("pane.mark", pane.clone(), &mut events),
        json!({"ok":true})
    );
    assert_eq!(
        helper.request(
            "pane.input",
            json!({"id":pane_id,"generation":generation,"bytes":[103,111,10]}),
            &mut events,
        ),
        json!({"ok":true})
    );
    output_until(&helper, &mut events, &pane_id, generation, b"started");

    let left = background_with(&mut helper, &mut events, &pane, "sleep 31");
    assert_eq!(left["ok"], true, "{left}");
    assert_eq!(
        left["total"], 2,
        "the shell the program ran and its job; the server it had before the mark is not the turn's: {left}"
    );
    assert!(left["processes"][0]["pid"].as_u64().is_some(), "{left}");

    assert_eq!(
        helper.request("pane.kill", pane, &mut events),
        json!({"ok":true})
    );
    let exit = exit_of(&helper, &mut events, &opened);
    assert_eq!(
        exit["background"].as_array().map(Vec::len),
        Some(2),
        "{exit}"
    );
    assert!(
        exit["background"]
            .as_array()
            .is_some_and(|work| work.iter().any(|process| process["command"] == "sleep 31")),
        "{exit}"
    );
    assert!(outsider.runs(), "a process outside the window");
    helper.close_input_and_wait();
}

/// The program of a window whose turn began: a command of the agent's that is a
/// shell in the program's group, as a server a user wrapped in one is; one that is a
/// shell in a group of its own, as a command a tool runs; and a job detached from
/// a shell that ended, which keeps the shell's group, and whose number is written to
/// the file `$JOB`.
#[cfg(target_os = "macos")]
const GROUPS: &str = "printf ready; read go; /bin/sh -c 'sleep 31 & wait' & \
     /usr/bin/python3 -c 'import os; os.setpgrp(); os.execv(\"/bin/sh\", [\"sh\", \"-c\", \"sleep 32 & wait\"])' & \
     /usr/bin/python3 -c 'import os; os.setpgrp(); os.system(\"nohup sleep 33 >/dev/null 2>&1 & echo $! > $JOB\")' & \
     printf started; wait";

/// The body of an open of [`GROUPS`], with the file the detached job writes its
/// number to, which is not there yet.
#[cfg(target_os = "macos")]
fn open_groups() -> (Value, std::path::PathBuf) {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let job = std::env::temp_dir().join(format!(
        "cf-headless-job-{}-{}",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_file(&job);
    let mut body = open_body(GROUPS, 4096);
    body["env"] = json!({"JOB": job});
    (body, job)
}

/// The number of the job detached from a shell that ended, waited for.
#[cfg(target_os = "macos")]
fn detached_job(file: &std::path::Path) -> i32 {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(pid) = std::fs::read_to_string(file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "the job never said its number");
        thread::sleep(Duration::from_millis(10));
    }
}

/// The daemon says, with the mark, that its harness keeps its servers in the
/// group of the window's program: what the program starts after it and keeps in
/// its group (a server a user wrapped in a shell) is not the turn's, in the
/// answer to a look and in the exit of a window closed; what a shell it started in
/// a group of its own runs (a command run by a tool) is. A job detached from a shell
/// that ended is the window's, but no work of the turn: no chain of processes that
/// run leads from it to a shell, and it is left alone and not named. A mark that
/// says nothing, as an older daemon's, is the turn's by what a shell started:
/// what the test above holds.
#[cfg(target_os = "macos")]
#[test]
fn a_mark_that_says_the_servers_are_in_the_group_leaves_what_stays_in_it_out_of_what_the_turn_left()
{
    let _pty_guard = serial_headless_test();
    let mut outsider = Sentinel::start();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let (body, job) = open_groups();
    let opened = helper.request("pane.open", body, &mut events);
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let pane = json!({"id":pane_id,"generation":generation});
    output_until(&helper, &mut events, &pane_id, generation, b"ready");

    let mut mark = pane.clone();
    mark["servers"] = json!("inGroup");
    mark["harness"] = json!("claude");
    assert_eq!(
        helper.request("pane.mark", mark, &mut events),
        json!({"ok":true})
    );
    helper.request(
        "pane.input",
        json!({"id":pane_id,"generation":generation,"bytes":[103,111,10]}),
        &mut events,
    );
    output_until(&helper, &mut events, &pane_id, generation, b"started");

    // The tool's shell and its job: the commands become what they are a moment after
    // they began (until they have, they are copies of what forked them). The detached
    // job is running by the time its number is written.
    background_with(&mut helper, &mut events, &pane, "sleep 32");
    let detached = detached_job(&job);
    assert!(process_exists(detached));
    let left = helper.request("pane.background", pane.clone(), &mut events);
    let commands: Vec<&str> = left["processes"]
        .as_array()
        .map(|work| {
            work.iter()
                .filter_map(|process| process["command"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        !commands.iter().any(|command| command.contains("sleep 31")),
        "the server stays in the window's group: {left}"
    );
    assert!(
        !commands.iter().any(|command| command.contains("sleep 33")),
        "the detached job has no shell that runs: {left}"
    );
    assert!(
        commands.iter().any(|command| command.contains("sleep 32")),
        "{left}"
    );

    assert_eq!(
        helper.request("pane.kill", pane, &mut events),
        json!({"ok":true})
    );
    let exit = exit_of(&helper, &mut events, &opened);
    assert!(
        exit["background"]
            .as_array()
            .is_some_and(|work| !work.is_empty()
                && work.iter().all(|process| {
                    let command = process["command"].as_str().unwrap_or("");
                    !command.contains("sleep 31") && !command.contains("sleep 33")
                })),
        "{exit}"
    );
    assert!(outsider.runs(), "a process outside the window");
    helper.close_input_and_wait();
    let _ = std::fs::remove_file(&job);
}

/// The daemon asks the host to end what the turn left running, once it has taken
/// the turn's answer for its result: the host ends what it proves the turn's (a
/// command's shell and its job), says what it ended, and the window stays open, its
/// program and what its harness keeps as they were, and with nothing left for its
/// exit to list. A job detached from a shell that ended runs on, unnamed, until the
/// window closes with it: it is in the session the window's program leads.
#[cfg(target_os = "macos")]
#[test]
fn an_ending_asked_over_the_wire_ends_what_the_turn_left_and_the_window_stays_open() {
    let _pty_guard = serial_headless_test();
    let mut outsider = Sentinel::start();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let (body, job) = open_groups();
    let opened = helper.request("pane.open", body, &mut events);
    assert_eq!(opened["ok"], true, "{opened}");
    let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
    let generation = opened["generation"].as_u64().expect("opened generation");
    let pane = json!({"id":pane_id,"generation":generation});
    output_until(&helper, &mut events, &pane_id, generation, b"ready");
    let mut mark = pane.clone();
    mark["servers"] = json!("inGroup");
    mark["harness"] = json!("claude");
    helper.request("pane.mark", mark, &mut events);
    helper.request(
        "pane.input",
        json!({"id":pane_id,"generation":generation,"bytes":[103,111,10]}),
        &mut events,
    );
    output_until(&helper, &mut events, &pane_id, generation, b"started");
    background_with(&mut helper, &mut events, &pane, "sleep 32");
    let detached = detached_job(&job);

    let ended = helper.request("pane.end_background", pane.clone(), &mut events);

    assert_eq!(ended["ok"], true, "{ended}");
    let said: Vec<&str> = ended["ended"]["processes"]
        .as_array()
        .map(|work| {
            work.iter()
                .filter_map(|process| process["command"].as_str())
                .collect()
        })
        .unwrap_or_default();
    assert!(
        said.iter().any(|command| command.contains("sleep 32")),
        "{ended}"
    );
    for kept in ["sleep 31", "sleep 33"] {
        assert!(
            !said.iter().any(|command| command.contains(kept)),
            "{kept} is not ended, nor named: {ended}"
        );
    }
    assert_eq!(ended["left"]["total"], 0, "{ended}");
    assert!(process_exists(detached), "the detached job runs on");
    assert!(outsider.runs(), "a process outside the window");
    assert_eq!(
        helper.request("pane.background", pane.clone(), &mut events)["total"],
        0,
        "what was ended is not listed after"
    );
    let panes = helper.request("pane.list", json!({}), &mut events);
    assert_eq!(
        panes["panes"][0]["alive"], true,
        "the window stays open: {panes}"
    );

    assert_eq!(
        helper.request("pane.kill", pane, &mut events),
        json!({"ok":true})
    );
    let exit = exit_of(&helper, &mut events, &opened);
    assert!(
        exit.get("background").is_none(),
        "the close has nothing left to list: {exit}"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    while process_exists(detached) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !process_exists(detached),
        "the window's close ended the job that was the window's"
    );
    assert!(outsider.runs(), "nor a close's");
    helper.close_input_and_wait();
    let _ = std::fs::remove_file(&job);
}

/// A folder, with a space in its name, holding the programs of a harness named
/// `codex`: its binary and, beside it, the helper it keeps (`codex-code-mode-host`).
/// Both are one small program of C, which runs the command it is given with `system`
/// and waits for it, or sleeps where it is given none.
#[cfg(target_os = "macos")]
fn codex_programs() -> std::path::PathBuf {
    static MADE: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "cf-headless-{}-{} codex bin",
        std::process::id(),
        MADE.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("a folder for the harness's programs");
    let source = dir.join("program.c");
    std::fs::write(
        &source,
        "#include <stdlib.h>\n#include <unistd.h>\n\
         int main(int argc, char **argv) {\n\
         \tif (argc > 1) return system(argv[1]);\n\
         \tsleep(60);\n\
         \treturn 0;\n}\n",
    )
    .expect("the source of the harness's programs");
    let helper = dir.join("codex-code-mode-host");
    let made = Command::new("cc")
        .arg(&source)
        .arg("-o")
        .arg(&helper)
        .status()
        .expect("compile the harness's programs");
    assert!(made.success(), "the harness's programs were not made");
    std::fs::copy(&helper, dir.join("codex")).expect("the harness's binary");
    dir
}

/// The daemon names the harness with the mark: a program named for it that runs beside
/// its binary (the helper Codex keeps for the session), and the shell it runs after the
/// mark, are the harness's and not the turn's; where the mark does not name it the
/// shell is a command's, as a shell a process from before the mark started is, and is
/// listed.
#[cfg(target_os = "macos")]
#[test]
fn a_mark_that_names_the_harness_leaves_its_programs_and_what_they_run_out_of_what_the_turn_left() {
    let _pty_guard = serial_headless_test();
    let mut outsider = Sentinel::start();
    let bins = codex_programs();
    for named in [true, false] {
        let mut helper = Headless::spawn();
        let mut events = Vec::new();
        let said = bins.join(format!("said-{named}"));
        let go = bins.join(format!("go-{named}"));
        let mut body = open_body(
            // The helper, which began before the turn, waits for the window to say it
            // has begun and runs a shell, which says so once it runs a job; the window
            // says it started once it has.
            r#""$BINS/codex" & "$BINS/codex-code-mode-host" 'while [ ! -e "$GO" ]; do sleep 0.05; done; /bin/sh -c "sleep 31 & echo inner-started; wait"' > "$SAID" & printf ready; read go; : > "$GO"; until [ -s "$SAID" ]; do sleep 0.05; done; printf started; wait"#,
            4096,
        );
        body["env"] = json!({"BINS": bins, "SAID": said, "GO": go});
        let opened = helper.request("pane.open", body, &mut events);
        assert_eq!(opened["ok"], true, "{opened}");
        let pane_id = opened["id"].as_str().expect("opened pane id").to_string();
        let generation = opened["generation"].as_u64().expect("opened generation");
        let pane = json!({"id":pane_id,"generation":generation});
        output_until(&helper, &mut events, &pane_id, generation, b"ready");

        let mut mark = pane.clone();
        if named {
            mark["harness"] = json!("codex");
        }
        assert_eq!(
            helper.request("pane.mark", mark, &mut events),
            json!({"ok":true})
        );
        helper.request(
            "pane.input",
            json!({"id":pane_id,"generation":generation,"bytes":[103,111,10]}),
            &mut events,
        );
        output_until(&helper, &mut events, &pane_id, generation, b"started");

        if named {
            assert_eq!(
                helper.request("pane.background", pane.clone(), &mut events)["total"],
                0,
                "the helper is the harness's, and so is the shell it runs"
            );
            assert_eq!(
                helper.request("pane.end_background", pane.clone(), &mut events)["ended"]["total"],
                0
            );
        } else {
            let left = background_with(&mut helper, &mut events, &pane, "sleep 31");
            assert!(left["total"].as_u64() >= Some(2), "{left}");
        }
        assert_eq!(
            helper.request("pane.kill", pane, &mut events),
            json!({"ok":true})
        );
        helper.close_input_and_wait();
        assert!(outsider.runs(), "a process outside the window");
    }
    let _ = std::fs::remove_dir_all(&bins);
}

/// An ending names its pane and no process, which it chooses itself; one that does
/// otherwise is refused as a request that is not one, and a window that is not open
/// has nothing to end.
#[test]
fn an_ending_that_says_more_or_otherwise_than_what_it_asks_to_end_is_refused() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    for body in [
        json!({"id":"nobody","generation":"1"}),
        json!({"id":"nobody","generation":1,"pids":[1]}),
        json!({"id":"nobody","generation":1,"all":true}),
        json!({"generation":1}),
    ] {
        let refused = helper.request("pane.end_background", body.clone(), &mut events);
        assert_eq!(refused["ok"], false, "{body}: {refused}");
        assert!(
            refused["error"]
                .as_str()
                .is_some_and(|error| error.contains("invalid")),
            "it is the request that is refused: {refused}"
        );
    }
    let nobody = json!({"id":"nobody","generation":1});
    let refused = helper.request("pane.end_background", nobody, &mut events);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|error| !error.contains("invalid")),
        "it is the pane that is not there: {refused}"
    );
    helper.close_input_and_wait();
}

/// A mark names its pane and says no more than how its harness keeps its servers,
/// in a word, and which harness it is, in a name; one that says more, or says it
/// otherwise, is refused as a request that is not one. A word this host does not
/// know is not a refusal: the mark stands, as for a harness it knows nothing of.
#[test]
fn a_mark_that_says_more_or_otherwise_than_how_the_servers_are_kept_is_refused() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    for body in [
        json!({"id":"nobody","generation":1,"harnessGroup":true}),
        json!({"id":"nobody","generation":1,"servers":7}),
        json!({"id":"nobody","generation":1,"harness":7}),
        json!({"id":"nobody","generation":1,"group":true}),
    ] {
        let refused = helper.request("pane.mark", body.clone(), &mut events);
        assert_eq!(refused["ok"], false, "{body}: {refused}");
        assert!(
            refused["error"]
                .as_str()
                .is_some_and(|error| error.contains("invalid")),
            "it is the request that is refused, not a pane that is not there: {refused}"
        );
    }
    let unknown = json!({"id":"nobody","generation":1,"servers":"somewhere-else"});
    let refused = helper.request("pane.mark", unknown, &mut events);
    assert_eq!(refused["ok"], false, "{refused}");
    assert!(
        refused["error"]
            .as_str()
            .is_some_and(|error| !error.contains("invalid")),
        "a word it does not know is read, and it is the pane that is not there: {refused}"
    );
    helper.close_input_and_wait();
}

/// A window that is not open has nothing to mark, and says so as any request
/// about a pane that is not there does.
#[test]
fn a_mark_or_a_look_at_a_window_that_is_not_open_is_refused() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let nobody = json!({"id":"nobody","generation":1});
    for op in ["pane.mark", "pane.background"] {
        let refused = helper.request(op, nobody.clone(), &mut events);
        assert_eq!(refused["ok"], false, "{op}: {refused}");
    }
    helper.close_input_and_wait();
}
