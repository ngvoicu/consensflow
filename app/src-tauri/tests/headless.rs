#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use app_lib::bridge::{BridgeBuilder, BridgeError};

const FRAME_TIMEOUT: Duration = Duration::from_secs(3);

fn serial_headless_test() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|error| error.into_inner())
}

struct Headless {
    child: Child,
    input: Option<ChildStdin>,
    frames: Receiver<Result<Value, String>>,
    next_id: u64,
    pub handle: Value,
}

struct CoalescingReader<R> {
    inner: R,
    initial: Vec<u8>,
    initial_at: usize,
    wanted_newlines: usize,
}

struct FaultInjectingWriter<W> {
    inner: W,
    injected: bool,
}

impl<W> FaultInjectingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            injected: false,
        }
    }
}

impl<W: Write> Write for FaultInjectingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if !self.injected
            && bytes
                .windows(b"\"id\":\"n-1\",\"kind\":\"res\",\"op\":\"pane.open\"".len())
                .any(|window| window == b"\"id\":\"n-1\",\"kind\":\"res\",\"op\":\"pane.open\"")
        {
            self.injected = true;
            self.inner.write_all(
                b"not-json-from-rust\n\
                  {\"v\":1,\"id\":\"r-wrong-response\",\"kind\":\"res\",\"op\":\"pane.open\",\"body\":{\"injected\":\"wrong-namespace\"}}\n\
                  {\"v\":1,\"id\":\"n-1\",\"kind\":\"res\",\"op\":\"wrong.open\",\"body\":{\"injected\":\"wrong-op\"}}\n",
            )?;
        }
        self.inner.write(bytes)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

impl<W: AsRawFd> AsRawFd for FaultInjectingWriter<W> {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl<R> CoalescingReader<R> {
    fn new(inner: R, wanted_newlines: usize) -> Self {
        Self {
            inner,
            initial: Vec::new(),
            initial_at: 0,
            wanted_newlines,
        }
    }
}

impl<R: Read> Read for CoalescingReader<R> {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.initial_at < self.initial.len() {
            let available = &self.initial[self.initial_at..];
            let byte_count = available.len().min(bytes.len());
            bytes[..byte_count].copy_from_slice(&available[..byte_count]);
            self.initial_at += byte_count;
            return Ok(byte_count);
        }
        if self.wanted_newlines > 0 {
            let mut chunk = [0; 1024];
            while self.initial.iter().filter(|byte| **byte == b'\n').count() < self.wanted_newlines
            {
                let byte_count = self.inner.read(&mut chunk)?;
                if byte_count == 0 {
                    break;
                }
                self.initial.extend_from_slice(&chunk[..byte_count]);
            }
            self.wanted_newlines = 0;
            return self.read(bytes);
        }
        self.inner.read(bytes)
    }
}

impl<R: AsRawFd> AsRawFd for CoalescingReader<R> {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
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
        thread::spawn(move || {
            for line in reader.lines() {
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
            next_id: 0,
            handle,
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
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn open_body(script: &str, backlog_bytes: usize) -> Value {
    json!({
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

#[test]
fn claim_epoch_observes_intervening_typing_without_writing_to_the_pane() {
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

    let snapshot = helper.request(
        "pane.snapshot",
        json!({"id":pane_id,"generation":generation}),
        &mut events,
    );
    assert_eq!(snapshot["ok"], true);
    assert_eq!(snapshot["inputEpoch"], 0);
    assert_eq!(
        helper.request(
            "pane.input",
            json!({"id":pane_id,"generation":generation,"bytes":[120]}),
            &mut events,
        ),
        json!({"ok":true,"epoch":1})
    );
    let claimed = helper.request(
        "pane.claim_epoch",
        json!({"pane":pane_id,"generation":generation,"epoch":snapshot["inputEpoch"]}),
        &mut events,
    );
    assert_eq!(
        claimed,
        json!({"ok":false,"error":"human draft is latched"})
    );
    assert!(
        events
            .drain(..)
            .all(|event| output_bytes(&event, &pane_id, generation).is_none()),
        "claiming an epoch wrote a second byte before its response"
    );
    assert!(matches!(
        helper.receive_timeout(Duration::from_millis(150)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    let clean = helper.request(
        "pane.open",
        open_body("/bin/stty raw -echo; /bin/sleep 1000", 1024),
        &mut events,
    );
    let clean_id = clean["id"].as_str().expect("clean pane id").to_string();
    let clean_generation = clean["generation"].as_u64().expect("clean generation");
    assert_eq!(
        helper.request(
            "pane.claim_epoch",
            json!({"pane":clean_id,"generation":clean_generation,"epoch":0}),
            &mut events,
        ),
        json!({"ok":true})
    );
    let after_claim = helper.request(
        "pane.snapshot",
        json!({"id":clean_id,"generation":clean_generation}),
        &mut events,
    );
    assert_eq!(after_claim["inputEpoch"], 0);
    assert_eq!(
        after_claim["pasteInFlight"], false,
        "claiming an epoch does not reserve the paste writer"
    );
    assert_eq!(
        helper.request(
            "pane.claim_epoch",
            json!({"pane":clean_id,"generation":clean_generation,"epoch":1}),
            &mut events,
        ),
        json!({"ok":false,"error":"stale pane generation or input epoch"})
    );

    for (id, pane_generation) in [(pane_id, generation), (clean_id, clean_generation)] {
        assert_eq!(
            helper.request(
                "pane.kill",
                json!({"id":id,"generation":pane_generation}),
                &mut events,
            ),
            json!({"ok":true})
        );
    }
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
            json!({"id":pane_id,"generation":generation,"epoch":0,"body":"body"}),
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
    while received_bytes < OUTPUT_BYTES {
        if events.is_empty() {
            events.push(helper.receive());
        }
        let mut received = events
            .drain(..)
            .filter_map(|event| output_bytes(&event, &pane_id, generation))
            .collect::<Vec<_>>();
        for (seq, bytes) in received.drain(..) {
            received_bytes += bytes.len();
            assert!(received_bytes <= OUTPUT_BYTES);
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
            "epoch":0,
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

#[test]
fn real_node_bridge_conforms_with_rust_in_the_specified_nested_roles() {
    let _bridge_guard = serial_headless_test();
    let bridge_module = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../src/bridge.js")
        .canonicalize()
        .expect("locate real Node Bridge module");
    let script = r#"
import { pathToFileURL } from 'node:url'
const { Bridge } = await import(pathToFileURL(process.argv[1]).href)

process.stdout.write([
  JSON.stringify({ v: 1, kind: 'node-conformance' }),
  '{not-json',
  JSON.stringify({ v: 1, id: 'r-wrong', kind: 'evt', op: 'wrong.namespace', body: {} }),
].join('\n') + '\n')

const bridge = new Bridge({ input: process.stdin, output: process.stdout })
bridge.onError((cause) => {
  bridge.event('node.error', { message: String(cause) })
})
bridge.on('consult', async (body) => {
  process.stdout.write(JSON.stringify({
    v: 1, id: 'r-1', kind: 'res', op: 'wrong.consult', body: { ignored: true },
  }) + '\n')
  const opened = await bridge.request('pane.open', body.open, { deadlineMs: 2000 })
  const rustFailure = await bridge.request('rust.fail', {}, { deadlineMs: 2000 })
  return { ok: true, opened, rustFailure }
})
bridge.on('node.fail', () => { throw new Error('node-handler-error') })
bridge.on('shutdown', () => {
  setTimeout(() => process.exit(0), 25)
  return { ok: true }
})
process.stdin.resume()
"#;
    let mut node = Command::new("node")
        .args([
            "--input-type=module",
            "--eval",
            script,
            bridge_module.to_str().expect("UTF-8 Bridge path"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn real Node Bridge");
    let node_input = node.stdin.take().expect("piped Node stdin");
    let node_output = node.stdout.take().expect("piped Node stdout");

    let (error_sender, error_receiver) = mpsc::channel();
    let (open_sender, open_receiver) = mpsc::channel();
    let (node_error_sender, node_error_receiver) = mpsc::channel();
    let mut builder = BridgeBuilder::new(1024 * 1024);
    builder.on_error(move |error| {
        let _ = error_sender.send(error);
    });
    builder.on_event("node.error", move |body| {
        let _ = node_error_sender.send(body);
    });
    builder.on("pane.open", move |_bridge, body| {
        open_sender
            .send(body)
            .map_err(|_| "open-observer-disconnected".to_string())?;
        Ok(json!({"ok":true,"id":"pane-from-rust","generation":1}))
    });
    builder.on("rust.fail", |_bridge, _body| {
        Err("rust-handler-error".to_string())
    });
    let connected = builder
        .connect(
            CoalescingReader::new(node_output, 3),
            FaultInjectingWriter::new(node_input),
        )
        .expect("connect Rust to the real Node Bridge");
    assert_eq!(connected.handle, json!({"v":1,"kind":"node-conformance"}));

    let consult = connected
        .bridge
        .request(
            "consult",
            json!({"open":{"cwd":"/tmp","argv":["/bin/sh"]}}),
            Some(3_000),
        )
        .expect("Node consult response");
    assert_eq!(
        open_receiver
            .recv_timeout(FRAME_TIMEOUT)
            .expect("Node nested pane.open reached Rust"),
        json!({"cwd":"/tmp","argv":["/bin/sh"]})
    );
    assert_eq!(
        consult,
        json!({
            "ok":true,
            "opened":{"ok":true,"id":"pane-from-rust","generation":1},
            "rustFailure":{"ok":false,"error":"rust-handler-error"}
        })
    );
    assert_eq!(
        connected
            .bridge
            .request("node.fail", json!({}), Some(3_000))
            .expect("Node handler error response"),
        json!({"ok":false,"error":"node-handler-error"})
    );

    let mut errors = Vec::new();
    let deadline = Instant::now() + FRAME_TIMEOUT;
    while errors.len() < 3 && Instant::now() < deadline {
        if let Ok(error) = error_receiver.recv_timeout(Duration::from_millis(20)) {
            errors.push(error);
        }
    }
    assert_eq!(
        errors.len(),
        3,
        "all protocol violations are reported: {errors:?}"
    );
    assert!(errors
        .iter()
        .any(|error| matches!(error, BridgeError::MalformedFrame(_))));
    assert!(errors.iter().any(|error| matches!(error, BridgeError::MalformedFrame(message) if message.contains("wrong namespace"))));
    assert!(errors.iter().any(|error| matches!(error, BridgeError::MalformedFrame(message) if message.contains("wrong.consult"))));

    let mut node_errors = Vec::new();
    let deadline = Instant::now() + FRAME_TIMEOUT;
    while node_errors.len() < 3 && Instant::now() < deadline {
        if let Ok(error) = node_error_receiver.recv_timeout(Duration::from_millis(20)) {
            node_errors.push(error["message"].as_str().unwrap_or_default().to_string());
        }
    }
    assert!(node_errors
        .iter()
        .any(|error| error.contains("Unexpected token")));
    assert!(node_errors
        .iter()
        .any(|error| error.contains("wrong namespace")));
    assert!(node_errors.iter().any(|error| error.contains("wrong.open")));

    assert_eq!(
        connected
            .bridge
            .request("shutdown", json!({}), Some(3_000))
            .expect("Node shutdown response"),
        json!({"ok":true})
    );
    connected
        .bridge
        .wait_closed()
        .expect("Rust observes Node EOF");
    assert!(node.wait().expect("wait for Node").success());
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

#[test]
fn product_bridge_contract_preserves_app_identity_and_launch_deduplication() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let body = json!({
        "id":"product-pane", "generation":7, "launch":"product-launch",
        "cwd":"/tmp", "argv":["/bin/cat"], "env":{}
    });
    let opened = helper.request("pane.open", body.clone(), &mut events);
    assert_eq!(opened["ok"], true, "production open rejected: {opened}");
    assert_eq!(opened["id"], "product-pane");
    assert_eq!(opened["generation"], 7);
    let duplicate = helper.request("pane.open", body, &mut events);
    assert_eq!(
        duplicate["ok"], true,
        "duplicate launch rejected: {duplicate}"
    );
    assert_eq!(duplicate["id"], "product-pane");
    assert_eq!(duplicate["deduplicated"], true);
    helper.close_input_and_wait();
}

#[test]
fn product_bridge_cannot_clear_a_human_draft_even_with_the_exact_enter_epoch() {
    let _pty_guard = serial_headless_test();
    let mut helper = Headless::spawn();
    let mut events = Vec::new();
    let opened = helper.request("pane.open", open_body("exec /bin/cat", 1024), &mut events);
    assert_eq!(opened["ok"], true);
    let id = opened["id"].as_str().expect("pane id");
    let generation = opened["generation"].as_u64().expect("generation");
    let entered = helper.request(
        "pane.input",
        json!({
            "id":id, "generation":generation, "bytes":b"hello\r".to_vec()
        }),
        &mut events,
    );
    assert_eq!(entered["ok"], true);
    while !events.iter().any(|event| event["op"] == "pane.enter") {
        events.push(helper.receive());
    }
    let event = events
        .iter()
        .find(|event| event["op"] == "pane.enter")
        .expect("enter event");
    assert_eq!(event["body"]["id"], id);
    let epoch = event["body"]["epoch"].as_u64().expect("enter epoch");
    let cleared = helper.request("draft.clear", json!({
        "id":id, "generation":generation, "submittedEpoch":epoch, "submissionId":"submission-one"
    }), &mut events);
    assert_ne!(
        cleared["ok"], true,
        "legacy draft clear remained exposed: {cleared}"
    );
    let snapshot = helper.request(
        "pane.snapshot",
        json!({"id":id,"generation":generation}),
        &mut events,
    );
    assert_eq!(snapshot["draftLatched"], true);
    assert!(snapshot["lastSubmissionId"].is_null());
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
