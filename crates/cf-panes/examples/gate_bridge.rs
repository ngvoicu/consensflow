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
//! as a line. What a page answers for the program goes in through the real `InputQueue`
//! (`reply`: its page, pane, generation and number are checked as the app's command checks
//! them), and the program keeps what reaches its input in a file (`received`).
// A test tool: a failed command stops the run, which is the report.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::disallowed_methods,
    clippy::print_stderr
)]

// A fifo is its program's input: Unix only.
#[cfg(unix)]
mod bridge {
    use std::collections::HashMap;
    use std::io::{BufRead, Write};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    use cf_panes::arbiter::{EnterTiming, InputArbiter};
    use cf_panes::input_queue::{InputQueue, InputWork};
    use cf_panes::journal::Journal;
    use cf_panes::output_hub::{OutputHub, PaneOutputMessage};
    use cf_panes::page;
    use cf_panes::pane_handlers::{default_backlog_bytes, forward_journaled};
    use cf_panes::pty::{PaneEnvironment, PaneKey, PaneTable};
    use serde_json::{json, Value};

    const PROGRAM: &str = r#"
import os, select, sys, tty
owner = os.getppid()
tty.setraw(1)
fd = os.open(sys.argv[1], os.O_RDWR)
got = os.open(sys.argv[1] + '.in', os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
buf = b''
def out(data):
    v = memoryview(data)
    while v:
        v = v[os.write(1, v):]
while True:
    ready = select.select([fd, 0], [], [])[0]
    if 0 in ready:
        os.write(got, os.read(0, 1 << 16))
    if fd not in ready:
        continue
    d = os.read(fd, 1 << 20)
    if not d or os.getppid() != owner:
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
        /// What reached the program's input, as the program wrote it down.
        received: PathBuf,
    }

    pub(super) fn main() {
        let panes = Arc::new(PaneTable::new());
        panes.keep_journals();
        let hub = Arc::new(OutputHub::new());
        let stdout = Arc::new(Mutex::new(std::io::stdout()));
        let arbiter = Arc::new(InputArbiter::new(EnterTiming::fixed(0)));
        let inputs = InputQueue::new(Arc::clone(&panes), Arc::clone(&arbiter));
        // Every wait of the gate is bounded; so is this one's (the gate's, in seconds).
        let wait: u64 = std::env::var("CF_GATE_WAIT_S")
            .ok()
            .and_then(|seconds| seconds.parse().ok())
            .unwrap_or(180);
        let mut alive = Arc::new(AtomicBool::new(true));
        let mut windows: HashMap<String, Window> = HashMap::new();
        // The replies admitted and not yet written, and the outcomes that failed.
        let owed = Arc::new((
            Mutex::new((0usize, Vec::<String>::new())),
            std::sync::Condvar::new(),
        ));
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
                    arbiter.register(&key).unwrap();
                    inputs.open(&key).unwrap();
                    {
                        let (journal, hub) = (Arc::clone(&journal), Arc::clone(&hub));
                        std::thread::spawn(move || {
                            forward_journaled(&program.output, &journal, &hub, |_| {});
                        });
                    }
                    let fifo = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
                    windows.insert(
                        name,
                        Window {
                            key,
                            journal,
                            fifo,
                            received: PathBuf::from(format!("{}.in", path.display())),
                        },
                    );
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
                        let deadline = Instant::now() + Duration::from_secs(wait);
                        while journal.published_to() < want {
                            if Instant::now() >= deadline {
                                // The gate reports it and ends: no reader may hang on a program
                                // that stopped at its room.
                                say(json!({"re": re, "error": format!(
                                    "the program's output stopped at {} of {want} bytes ({wait} s)",
                                    journal.published_to()
                                )}));
                                return;
                            }
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        say(json!({"re": re, "end": journal.end()}));
                    });
                    continue;
                }
                "page_prepare" => match inputs.prepare_page(
                    command["page"].as_str().unwrap(),
                    command["nonce"].as_str().unwrap(),
                ) {
                    Ok(epoch) => json!({"ok":true,"epoch":epoch,"nonce":command["nonce"]}),
                    Err(error) => json!({"ok":false,"error":error}),
                },
                "page_activate" => match inputs.activate_page(
                    command["page"].as_str().unwrap(),
                    command["nonce"].as_str().unwrap(),
                    command["epoch"].as_u64().unwrap(),
                ) {
                    Ok(_) => json!({"ok":true,"epoch":command["epoch"],"notices":[]}),
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
                "reply" => {
                    // The page's answer to a question, admitted as the app's command admits it.
                    let bytes: Vec<u8> = (0..command["hex"].as_str().unwrap().len() / 2)
                        .map(|i| {
                            u8::from_str_radix(
                                &command["hex"].as_str().unwrap()[i * 2..i * 2 + 2],
                                16,
                            )
                            .unwrap()
                        })
                        .collect();
                    let key = PaneKey::new(
                        command["id"].as_str().unwrap(),
                        command["generation"].as_u64().unwrap(),
                    );
                    match inputs.enqueue_page(
                        command["page"].as_str().unwrap(),
                        command["epoch"].as_u64().unwrap(),
                        key,
                        command["sequence"].as_u64().unwrap(),
                        InputWork::Reply(bytes),
                        false,
                    ) {
                        Ok(ticket) => {
                            // The host owns the outcome of what it admitted: a page that is cut
                            // before it is written is told it was lost. `replies_settled` waits
                            // for every outcome, so that the gate cuts the page only after.
                            if let Ok(done) =
                                inputs.page_completion(command["page"].as_str().unwrap(), &ticket)
                            {
                                owed.0.lock().unwrap().0 += 1;
                                let owed = Arc::clone(&owed);
                                std::thread::spawn(move || {
                                    let outcome = futures_executor::block_on(done.receiver.clone());
                                    let mut state = owed.0.lock().unwrap();
                                    state.0 -= 1;
                                    if let Err(error) = outcome {
                                        state.1.push(error.to_string());
                                    }
                                    owed.1.notify_all();
                                });
                            }
                            json!({"answer": {"ok": true, "ticket": ticket}})
                        }
                        Err(error) => json!({"answer": {"ok": false, "error": error}}),
                    }
                }
                "replies_settled" => {
                    let deadline = Instant::now() + Duration::from_secs(wait);
                    let mut state = owed.0.lock().unwrap();
                    while state.0 > 0 && Instant::now() < deadline {
                        state = owed
                            .1
                            .wait_timeout(state, Duration::from_millis(50))
                            .unwrap()
                            .0;
                    }
                    let lost = std::mem::take(&mut state.1);
                    json!({"pending": state.0, "lost": lost})
                }
                "received" => {
                    let window = &windows[&name];
                    json!({"hex": hex(&std::fs::read(&window.received).unwrap_or_default())})
                }
                "size" => {
                    let window = &windows[&name];
                    let (rows, cols) = panes.terminal_size(&window.key).unwrap();
                    json!({"rows": rows, "cols": cols})
                }
                "info" => {
                    let window = &windows[&name];
                    json!({"end": window.journal.end()})
                }
                "close" => {
                    if let Some(mut window) = windows.remove(&name) {
                        let _ = writeln!(window.fifo, "q");
                        let _ = panes.kill(&window.key);
                        panes.forget_consumed_journal(&window.key, window.journal.end());
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
}

// The window's program writes on a fifo, which only Unix has: elsewhere the tool says so and stops.
#[cfg(not(unix))]
fn main() {
    eprintln!("gate_bridge drives a window through a fifo: it runs on Unix only");
    std::process::exit(2);
}

#[cfg(unix)]
fn main() {
    bridge::main();
}
