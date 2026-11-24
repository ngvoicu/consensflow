//! The gate's host side (T-2126): the real pane host's API, driven line by line from a browser
//! page's harness. Copy to `crates/cf-panes/examples/gate_bridge.rs` (untracked) and build:
//!   cargo build -p cf-panes --example gate_bridge
//!
//! (Moved to the repository by T-2170; the page side is app/tests/p5-gate/.)
//!
//! A window is a real pty whose program writes the bytes it is told on a fifo (raw, so the
//! page's own answers, which the harness keeps in the page, are not echoed). The consumer is
//! `forward_journaled`, the one the app runs; the hub, the journal, `attach_page`, `history`
//! and `resize_at` are the host's own. Every message the sink takes is printed as a line.
// A test tool: a failed command stops the run, which is the report.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods
)]

use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cf_panes::journal::Journal;
use cf_panes::output_hub::{OutputHub, PaneOutputMessage};
use cf_panes::pane_handlers::forward_journaled;
use cf_panes::pty::{PaneEnvironment, PaneKey, PaneTable};
use serde_json::{json, Value};

const PROGRAM: &str = r#"
import os, sys, tty
tty.setraw(1)
fd = os.open(sys.argv[1], os.O_RDWR)
buf = b''
def out(data):
    v = memoryview(data)
    while v:
        v = v[os.write(1, v):]
while True:
    d = os.read(fd, 1 << 20)
    if not d:
        sys.exit(0)
    buf += d
    while b'\n' in buf:
        line, buf = buf.split(b'\n', 1)
        if line == b'q':
            sys.exit(0)
        if line.startswith(b'r '):
            _, h, n = line.split(b' ')
            out(bytes.fromhex(h.decode()) * int(n))
        else:
            out(bytes.fromhex(line.decode()))
"#;

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

struct Window {
    key: PaneKey,
    journal: Arc<Journal>,
    fifo: std::fs::File,
}

fn main() {
    let panes = Arc::new(PaneTable::new());
    panes.keep_journals();
    let hub = Arc::new(OutputHub::new());
    let stdout = Arc::new(Mutex::new(std::io::stdout()));
    let alive = Arc::new(Mutex::new(Arc::new(AtomicBool::new(true))));
    let page = Arc::new(Mutex::new(0u64));
    let mut windows: HashMap<String, Window> = HashMap::new();
    let scratch = std::env::temp_dir().join(format!("cf-gate-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).unwrap();
    let say = {
        let stdout = Arc::clone(&stdout);
        move |value: Value| {
            let mut out = stdout.lock().unwrap();
            writeln!(out, "{value}").unwrap();
            out.flush().unwrap();
        }
    };
    let mut generation = 0;
    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        let command: Value = serde_json::from_str(&line).unwrap();
        let re = command["re"].clone();
        let name = command["window"].as_str().unwrap_or("").to_string();
        let reply = match command["cmd"].as_str().unwrap() {
            "open" => {
                generation += 1;
                let key = PaneKey::new(name.clone(), generation);
                let path = scratch.join(format!("{name}.fifo"));
                let c = std::ffi::CString::new(path.to_str().unwrap()).unwrap();
                // SAFETY: a fresh path under this process's own scratch folder.
                assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);
                let program = panes
                    .open_streamed_at_while(
                        &|| true,
                        key.clone(),
                        &std::env::current_dir().unwrap(),
                        &[
                            "/usr/bin/python3".into(),
                            "-I".into(),
                            "-S".into(),
                            "-c".into(),
                            PROGRAM.into(),
                            path.to_str().unwrap().into(),
                        ],
                        PaneEnvironment::new(&HashMap::new(), &[]),
                        portable_pty::PtySize {
                            rows: command["rows"].as_u64().unwrap() as u16,
                            cols: command["cols"].as_u64().unwrap() as u16,
                            pixel_width: 0,
                            pixel_height: 0,
                        },
                        1 << 20,
                    )
                    .unwrap();
                let journal = program.journal.clone().unwrap();
                {
                    let (journal, hub) = (Arc::clone(&journal), Arc::clone(&hub));
                    std::thread::spawn(move || {
                        forward_journaled(&program.output, &journal, &hub, |_| {});
                    });
                }
                let fifo = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
                windows.insert(name, Window { key, journal, fifo });
                json!({})
            }
            "emit" => {
                let window = windows.get_mut(&name).unwrap();
                writeln!(window.fifo, "{}", command["hex"].as_str().unwrap()).unwrap();
                json!({})
            }
            "repeat" => {
                let window = windows.get_mut(&name).unwrap();
                writeln!(
                    window.fifo,
                    "r {} {}",
                    command["hex"].as_str().unwrap(),
                    command["count"]
                )
                .unwrap();
                json!({})
            }
            "sync" => {
                // Answered from a thread of its own: the page's acknowledgements, which the
                // program's credit needs, are read by this loop meanwhile.
                let journal = Arc::clone(&windows[&name].journal);
                let want = command["n"].as_u64().unwrap();
                let say = say.clone();
                std::thread::spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(180);
                    while journal.published_to() < want {
                        assert!(
                            Instant::now() < deadline,
                            "sync {want}: at {}",
                            journal.published_to()
                        );
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    say(json!({"re": re, "end": journal.end()}));
                });
                continue;
            }
            "subscribe" => {
                *page.lock().unwrap() += 1;
                let id = *page.lock().unwrap();
                let flag = Arc::new(AtomicBool::new(true));
                *alive.lock().unwrap() = Arc::clone(&flag);
                let sink = {
                    let (say, flag) = (say.clone(), Arc::clone(&flag));
                    Arc::new(move |message: PaneOutputMessage| {
                        if !flag.load(Ordering::SeqCst) {
                            return false;
                        }
                        let v = serde_json::to_value(&message).unwrap();
                        say(json!({"msg": {
                            "page": id, "id": v["id"], "seq": v["seq"], "offset": v["offset"],
                            "hex": hex(message.bytes()),
                        }}));
                        true
                    })
                };
                let boundaries = panes.attach_page(&hub, sink);
                let key = &windows[&name].key;
                json!({"page": id, "boundary": boundaries.get(key).copied().unwrap_or(0)})
            }
            "gone" => {
                alive.lock().unwrap().store(false, Ordering::SeqCst);
                json!({})
            }
            "history" => {
                let window = &windows[&name];
                let from = command["from"].as_u64();
                let piece = panes.history(&window.key, from).unwrap();
                json!({
                    "reset": piece.reset, "truncated": piece.truncated, "from": piece.from,
                    "size": [piece.size.0, piece.size.1], "seed": hex(&piece.seed),
                    "resizes": piece.resizes.iter().map(|r| json!([r.at, r.rows, r.cols])).collect::<Vec<_>>(),
                    "hex": hex(&piece.bytes), "boundary": piece.boundary, "done": piece.done,
                })
            }
            "ack" => {
                let window = &windows[&name];
                let _ = panes.ack(&window.key, command["seq"].as_u64().unwrap());
                json!({})
            }
            "resize" => {
                let window = &windows[&name];
                panes
                    .resize_at(
                        &window.key,
                        command["rows"].as_u64().unwrap() as u16,
                        command["cols"].as_u64().unwrap() as u16,
                        command["at"].as_u64(),
                    )
                    .unwrap();
                json!({})
            }
            "info" => {
                let window = &windows[&name];
                json!({"end": window.journal.end()})
            }
            "close" => {
                if let Some(mut window) = windows.remove(&name) {
                    let _ = writeln!(window.fifo, "q");
                    let _ = panes.kill(&window.key);
                    panes.forget_ended_journal(&window.key);
                }
                json!({})
            }
            "quit" => {
                say(json!({"re": re, "bye": true}));
                break;
            }
            other => panic!("unknown command {other}"),
        };
        let mut reply = reply;
        reply["re"] = re;
        say(reply);
    }
    let _ = std::fs::remove_dir_all(&scratch);
}
