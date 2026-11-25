//! The gate's host side (T-2126, T-2236): the pane host as the app runs it, driven line by line
//! from a browser page's harness (`app/tests/p5-gate/`). Build:
//!   cargo build -p cf-panes --example gate_bridge
//!
//! A window is a real pty whose program writes the bytes it is told on a fifo (raw, so the
//! page's own answers, which the harness keeps, are not echoed), opened with the room the
//! daemon's `pane.open` gives it. The consumer is `forward_journaled`, the one the app runs.
//! What the page asks for is answered by the functions the app's commands call
//! (`cf_panes::page`), under the page identity the app's `InputQueue` keeps: its start
//! (prepare, activate), its subscription, its pulls, their end, its acknowledgements and its
//! sizes; a navigation ends the page as the app's does. Every message the sink takes is printed
//! as a line.
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

use cf_panes::arbiter::{EnterTiming, InputArbiter};
use cf_panes::input_queue::InputQueue;
use cf_panes::journal::Journal;
use cf_panes::output_hub::{OutputHub, PaneOutputMessage};
use cf_panes::page;
use cf_panes::pane_handlers::{default_backlog_bytes, forward_journaled};
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
    let inputs = InputQueue::new(
        Arc::clone(&panes),
        Arc::new(InputArbiter::new(EnterTiming::fixed(0))),
    );
    let mut alive = Arc::new(AtomicBool::new(true));
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
                        default_backlog_bytes(),
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
                json!({"generation": generation})
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
            "page_prepare" => match inputs.prepare_page(command["page"].as_str().unwrap()) {
                Ok(()) => json!({"ok":true}),
                Err(error) => json!({"ok":false,"error":error}),
            },
            "page_activate" => match inputs.activate_page(command["page"].as_str().unwrap()) {
                Ok(()) => json!({"ok":true}),
                Err(error) => json!({"ok":false,"error":error}),
            },
            "navigate" => {
                inputs.end_page().unwrap();
                json!({})
            }
            "subscribe" => {
                let id = command["page"].as_str().unwrap().to_string();
                let flag = Arc::new(AtomicBool::new(true));
                alive = Arc::clone(&flag);
                let sink = {
                    let say = say.clone();
                    Arc::new(move |message: PaneOutputMessage| {
                        if !flag.load(Ordering::SeqCst) {
                            return false;
                        }
                        let v = serde_json::to_value(&message).unwrap();
                        say(json!({"msg": {
                            "page": id, "id": v["id"], "generation": v["generation"],
                            "seq": v["seq"], "offset": v["offset"], "hex": hex(message.bytes()),
                        }}));
                        true
                    })
                };
                let answer = page::subscribe(
                    &inputs,
                    &panes,
                    &hub,
                    command["page"].as_str().unwrap(),
                    sink,
                );
                json!({"answer": answer})
            }
            "gone" => {
                alive.store(false, Ordering::SeqCst);
                json!({})
            }
            "history" => {
                let frame = page::history(
                    &inputs,
                    &panes,
                    command["page"].as_str().unwrap(),
                    command["id"].as_str().unwrap(),
                    command["generation"].as_u64().unwrap(),
                    command["from"].as_u64(),
                );
                json!({"hex": hex(&frame)})
            }
            "done" => json!({"answer": page::history_done(
                &inputs,
                &panes,
                command["page"].as_str().unwrap(),
                command["id"].as_str().unwrap(),
                command["generation"].as_u64().unwrap(),
                command["consumed"].as_u64().unwrap(),
            )}),
            "ack" => json!({"answer": page::ack(
                &panes,
                command["id"].as_str().unwrap(),
                command["generation"].as_u64().unwrap(),
                command["seq"].as_u64().unwrap(),
            )}),
            "resize" => json!({"answer": page::resize(
                &inputs,
                &panes,
                command["page"].as_str(),
                command["id"].as_str().unwrap(),
                command["generation"].as_u64().unwrap(),
                (
                    command["cols"].as_u64().unwrap() as u16,
                    command["rows"].as_u64().unwrap() as u16,
                ),
                command["at"].as_u64(),
            )}),
            "info" => {
                let window = &windows[&name];
                json!({"end": window.journal.end()})
            }
            "close" => {
                if let Some(mut window) = windows.remove(&name) {
                    let _ = writeln!(window.fifo, "q");
                    let _ = panes.kill(&window.key);
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
