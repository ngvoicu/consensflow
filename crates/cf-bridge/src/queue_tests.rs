//! The writer queue's rule, on every platform: a full queue never ends the
//! bridge. What the peer has not read waits in order, an event for as long as
//! the transport is open, a response for [`RESPONSE_WAIT_MS`] at most (then it
//! is dropped and told of, never the transport), and the reader goes on
//! meanwhile. The peer here is a channel of bytes in and a gated writer out, so
//! the cases are the same on the Mac and on Windows.

use std::io::{Read, Write};
use std::sync::{mpsc, Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cf_proto::bridge::Role;
use serde_json::{json, Value};

use super::{Bridge, BridgeBuilder, BridgeError, MAX_WAITING_RESPONSES, WRITER_QUEUE_CAPACITY};

/// What the writer has been given, and whether the peer reads it yet.
struct GateState {
    open: bool,
    entered: bool,
    bytes: Vec<u8>,
}

type Shared = Arc<(Mutex<GateState>, Condvar)>;

/// The output: holds every write until the gate opens, as a peer that reads
/// nothing does.
struct GatedWriter(Shared);

impl Write for GatedWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let (state, changed) = &*self.0;
        let mut state = state.lock().expect("gate");
        state.entered = true;
        changed.notify_all();
        while !state.open {
            state = changed.wait(state).expect("gate");
        }
        state.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The input: the bytes the peer says, in the order it says them, and the end
/// when it is gone.
struct PeerInput {
    said: mpsc::Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for PeerInput {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if self.pending.is_empty() {
            match self.said.recv() {
                Ok(more) => self.pending = more,
                Err(_) => return Ok(0),
            }
        }
        let count = bytes.len().min(self.pending.len());
        bytes[..count].copy_from_slice(&self.pending[..count]);
        self.pending.drain(..count);
        Ok(count)
    }
}

struct Rig {
    bridge: Bridge,
    peer: mpsc::Sender<Vec<u8>>,
    gate: Shared,
    told: Arc<Mutex<Vec<BridgeError>>>,
}

impl Rig {
    fn new(mut builder: BridgeBuilder) -> Self {
        let told = Arc::new(Mutex::new(Vec::new()));
        let hear = Arc::clone(&told);
        builder.on_error(move |error| hear.lock().expect("told").push(error));
        let (peer, said) = mpsc::channel();
        peer.send(b"{\"url\":\"http://127.0.0.1:1\",\"token\":\"t\"}\n".to_vec())
            .expect("the handle line");
        let gate: Shared = Arc::new((
            Mutex::new(GateState {
                open: false,
                entered: false,
                bytes: Vec::new(),
            }),
            Condvar::new(),
        ));
        let connected = builder
            .connect_uninterruptible(
                PeerInput {
                    said,
                    pending: Vec::new(),
                },
                GatedWriter(Arc::clone(&gate)),
            )
            .expect("connect");
        Self {
            bridge: connected.bridge,
            peer,
            gate,
            told,
        }
    }

    /// A frame the peer says.
    fn say(&self, frame: Value) {
        let mut line = serde_json::to_vec(&frame).expect("frame");
        line.push(b'\n');
        let _ = self.peer.send(line);
    }

    /// An unknown op, which the bridge answers on its own.
    fn ask_the_unknown(&self, id: String) {
        self.say(json!({"v":1,"id":id,"kind":"req","op":"unknown","body":null}));
    }

    /// Waits until the writer holds a frame it cannot yet write.
    fn wait_until_the_writer_is_held(&self) {
        let (state, changed) = &*self.gate;
        let mut state = state.lock().expect("gate");
        let until = Instant::now() + Duration::from_secs(5);
        while !state.entered {
            let left = until.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "the writer never took a frame");
            state = changed.wait_timeout(state, left).expect("gate").0;
        }
    }

    /// The peer reads now.
    fn open_the_gate(&self) {
        let (state, changed) = &*self.gate;
        state.lock().expect("gate").open = true;
        changed.notify_all();
    }

    /// The frames written so far, once there are `count` of them (or its time is out).
    fn written(&self, count: usize) -> Vec<Value> {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let bytes = self.gate.0.lock().expect("gate").bytes.clone();
            let frames: Vec<Value> = bytes
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
                .map(|line| serde_json::from_slice(line).expect("a frame"))
                .collect();
            if frames.len() >= count || Instant::now() >= until {
                return frames;
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn told_of(&self, words: &str) -> bool {
        self.told
            .lock()
            .expect("told")
            .iter()
            .any(|error| error.to_string().contains(words))
    }

    fn open_for(&self, long: Duration) -> bool {
        let until = Instant::now() + long;
        while Instant::now() < until {
            if self.bridge.is_closed() {
                return false;
            }
            thread::sleep(Duration::from_millis(5));
        }
        !self.bridge.is_closed()
    }
}

#[test]
fn events_wait_for_a_peer_that_reads_nothing_and_arrive_in_order() {
    let rig = Rig::new(BridgeBuilder::new(Role::Host, 1024));
    let burst = 3 * WRITER_QUEUE_CAPACITY;
    let sender = rig.bridge.clone();
    let sending = thread::spawn(move || {
        (0..burst)
            .map(|seq| sender.event("pane.exit", json!({ "seq": seq })))
            .collect::<Vec<_>>()
    });
    rig.wait_until_the_writer_is_held();
    let open_while_the_queue_was_full = rig.open_for(Duration::from_millis(300));

    rig.open_the_gate();
    let sent = sending.join().expect("the sender");
    let frames = rig.written(burst);

    assert!(
        open_while_the_queue_was_full,
        "a full writer queue closed the bridge"
    );
    assert!(
        sent.iter().all(|sent| matches!(sent, Ok(true))),
        "an event was refused: {sent:?}"
    );
    let seqs: Vec<u64> = frames
        .iter()
        .map(|frame| frame["body"]["seq"].as_u64().expect("seq"))
        .collect();
    assert_eq!(seqs, (0..burst as u64).collect::<Vec<_>>());
    assert!(!rig.bridge.is_closed());
}

#[test]
fn responses_wait_for_room_and_the_reader_goes_on() {
    let rig = Rig::new(BridgeBuilder::new(Role::Host, 1024));
    // A request of this end, which the peer answers while its input is not
    // read: the reader must still settle it.
    let requester = rig.bridge.clone();
    let (answered, answer) = mpsc::channel();
    let request = thread::spawn(move || {
        let _ = answered.send(requester.request("consult", json!(null), None));
    });
    rig.wait_until_the_writer_is_held();

    let burst = 3 * WRITER_QUEUE_CAPACITY;
    for index in 0..burst {
        rig.ask_the_unknown(format!("n-{index}"));
    }
    rig.say(json!({"v":1,"id":"r-1","kind":"res","op":"consult","body":{"ok":true}}));
    let settled = answer.recv_timeout(Duration::from_secs(2));
    let open_meanwhile = rig.open_for(Duration::from_millis(200));

    rig.open_the_gate();
    request.join().expect("the requester");
    let frames = rig.written(burst + 1);

    assert_eq!(
        settled.expect("the reader was held by the responses waiting for room"),
        Ok(json!({"ok":true}))
    );
    assert!(
        open_meanwhile,
        "responses waiting for room closed the bridge"
    );
    let mut answered: Vec<String> = frames
        .iter()
        .filter(|frame| frame["kind"] == "res")
        .map(|frame| frame["id"].as_str().expect("id").to_owned())
        .collect();
    answered.sort();
    let mut every: Vec<String> = (0..burst).map(|index| format!("n-{index}")).collect();
    every.sort();
    assert_eq!(answered, every, "a response was lost or said twice");
}

#[test]
fn a_response_that_waits_out_its_time_is_dropped_and_told_never_the_bridge() {
    let rig = Rig::new(BridgeBuilder::new(Role::Host, 1024).with_response_wait_ms(60));
    assert_eq!(rig.bridge.event("occupy", json!(null)), Ok(true));
    rig.wait_until_the_writer_is_held();

    let burst = WRITER_QUEUE_CAPACITY + 8;
    for index in 0..burst {
        rig.ask_the_unknown(format!("n-{index}"));
    }
    let open = rig.open_for(Duration::from_millis(500));
    let told = rig.told_of("dropped");

    rig.open_the_gate();
    // The bridge is alive: a request after the stall is answered.
    rig.ask_the_unknown("n-after".to_owned());
    let frames = rig.written(WRITER_QUEUE_CAPACITY);
    let answered_after = rig
        .written(WRITER_QUEUE_CAPACITY + 1)
        .iter()
        .chain(frames.iter())
        .any(|frame| frame["id"] == "n-after");

    assert!(open, "a response that waited too long closed the bridge");
    assert!(told, "the dropped responses were not told of");
    assert!(answered_after, "the bridge did not answer after the stall");
}

#[test]
fn the_responses_waiting_for_room_are_capped() {
    let rig = Rig::new(BridgeBuilder::new(Role::Host, 1024));
    assert_eq!(rig.bridge.event("occupy", json!(null)), Ok(true));
    rig.wait_until_the_writer_is_held();

    let burst = WRITER_QUEUE_CAPACITY + 2 * MAX_WAITING_RESPONSES;
    for index in 0..burst {
        rig.ask_the_unknown(format!("n-{index}"));
    }
    let open = rig.open_for(Duration::from_millis(500));
    let waiting = rig.bridge.waiting_responses();
    let told = rig.told_of("dropped");

    rig.open_the_gate();
    assert!(open, "a burst of requests closed the bridge");
    assert!(
        waiting <= MAX_WAITING_RESPONSES,
        "{waiting} responses waited at once"
    );
    assert!(told, "the responses past the cap were not told of");
}
