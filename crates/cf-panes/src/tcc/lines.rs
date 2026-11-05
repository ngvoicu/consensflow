//! The lines of tccd's log (`log stream --style ndjson`) that tell a privacy dialog was raised,
//! and the joining of the ones that tell the same request.
//!
//! A request that raises a dialog is told by three lines, tied by one `msgID` that tccd counts
//! from the number of the process that asked it (sandboxd's for a file, the process's own for
//! an API): `AUTHREQ_CTX` names the service, `AUTHREQ_ATTRIBUTION` names the process that
//! touched the file and the responsible process (the app), and `AUTHREQ_PROMPTING` says a
//! dialog was raised. Only the last two are read here. The system's tccd and the user's both
//! count their messages, so a request is told by the number of the tccd and its `msgID`
//! together. A request whose lines say no dialog (a check an app makes of its own grant, the
//! queries sandboxd makes where no dialog can be drawn) is never a prompt.

use std::collections::{HashMap, VecDeque};

use serde_json::Value;

/// How many requests are kept while their lines come: a dialog's lines come within the same
/// moment, and the attributions of requests that raise none are the rest.
const KEPT: usize = 512;

/// What the reader printed for one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Line {
    /// A log event: the number of the process that wrote it (tccd) and its message.
    Event { tccd: u64, message: String },
    /// Anything else: the banner the reader prints first, an error it prints, a line cut
    /// short when the reader was ended.
    Text(String),
}

/// Reads one line of the reader's output.
pub(crate) fn read_line(line: &str) -> Line {
    let text = line.trim_end();
    if let Ok(Value::Object(event)) = serde_json::from_str::<Value>(text) {
        let tccd = event.get("processID").and_then(Value::as_u64);
        let message = event.get("eventMessage").and_then(Value::as_str);
        if let (Some(tccd), Some(message)) = (tccd, message) {
            return Line::Event {
                tccd,
                message: message.to_owned(),
            };
        }
    }
    Line::Text(text.to_owned())
}

/// A dialog raised in the app's name for a process that is not the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Prompt {
    /// The service without its `kTCCService` (`MediaLibrary`).
    pub service: String,
    /// The process that touched what the dialog guards.
    pub pid: u32,
    /// The program it runs, by its file's name.
    pub program: String,
}

/// Who asked: the process that touched the file, and the process responsible for it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Who {
    pid: u32,
    program: String,
    responsible: u32,
}

/// What the lines of one request have said so far.
#[derive(Debug, Default)]
struct Partial {
    service: Option<String>,
    who: Option<Who>,
    /// Whether this request has been decided: told, or not to be.
    settled: bool,
}

/// Joins the lines of one request, however they come.
#[derive(Debug)]
pub(crate) struct Joiner {
    app: u32,
    requests: HashMap<(u64, String), Partial>,
    /// The keys of `requests`, oldest first.
    order: VecDeque<(u64, String)>,
}

impl Joiner {
    /// A joiner for the dialogs raised in the name of the process `app`.
    pub(crate) fn new(app: u32) -> Self {
        Self {
            app,
            requests: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    /// The prompt `message` of the tccd `tccd` completes, if it does: the dialog is for a
    /// process the app is responsible for, and is not the app's own access.
    pub(crate) fn feed(&mut self, tccd: u64, message: &str) -> Option<Prompt> {
        let (kind, rest) = message.split_once(": msgID=")?;
        let id = rest.split(',').next()?.to_owned();
        let key = (tccd, id);
        match kind {
            "AUTHREQ_ATTRIBUTION" => {
                let who = who_of(rest)?;
                // Of the app's name only: the rest of the machine's requests are not kept.
                if who.responsible != self.app {
                    return None;
                }
                self.partial(&key).who = Some(who);
            }
            "AUTHREQ_PROMPTING" => {
                let service = field(rest, "service=")?
                    .strip_prefix("kTCCService")?
                    .to_owned();
                self.partial(&key).service = Some(service);
            }
            _ => return None,
        }
        self.settle(&key)
    }

    fn partial(&mut self, key: &(u64, String)) -> &mut Partial {
        if !self.requests.contains_key(key) {
            if self.order.len() >= KEPT {
                if let Some(oldest) = self.order.pop_front() {
                    self.requests.remove(&oldest);
                }
            }
            self.order.push_back(key.clone());
        }
        self.requests.entry(key.clone()).or_default()
    }

    /// The prompt the request `key` is, once it is known both who asked and that a dialog was
    /// raised; told once.
    fn settle(&mut self, key: &(u64, String)) -> Option<Prompt> {
        let partial = self.requests.get_mut(key)?;
        if partial.settled {
            return None;
        }
        let (Some(service), Some(who)) = (&partial.service, &partial.who) else {
            return None;
        };
        partial.settled = true;
        // The app's own access is not a window's: a folder it opens for the human who chose it.
        if who.pid == self.app {
            return None;
        }
        Some(Prompt {
            service: service.clone(),
            pid: who.pid,
            program: who.program.clone(),
        })
    }

    /// How many requests are kept while their lines come.
    #[cfg(test)]
    pub(crate) fn kept(&self) -> usize {
        self.requests.len()
    }
}

/// The text of `name` in `rest`, up to the next comma.
fn field<'a>(rest: &'a str, name: &str) -> Option<&'a str> {
    let at = rest.find(name)? + name.len();
    rest[at..].split(',').next()
}

/// Who an `AUTHREQ_ATTRIBUTION` names: the process that accessed, and the responsible process
/// where it says one (the accessing process is its own where it says none).
fn who_of(rest: &str) -> Option<Who> {
    let accessing = process_of(rest, "accessing")?;
    let responsible = process_of(rest, "responsible").map_or(accessing.pid, |process| process.pid);
    Some(Who {
        pid: accessing.pid,
        program: program_of(&accessing),
        responsible,
    })
}

/// A process an attribution names.
struct Process<'a> {
    identifier: &'a str,
    pid: u32,
    binary_path: &'a str,
}

/// The process the attribution `rest` gives to `role` (`accessing={TCCDProcess: identifier=…,
/// pid=…, auid=…, euid=…, binary_path=…}`). A path may hold commas and spaces: it is the last
/// field, and runs to the brace that closes the process.
fn process_of<'a>(rest: &'a str, role: &str) -> Option<Process<'a>> {
    let needle = format!("{role}={{TCCDProcess: ");
    let start = rest.find(&needle)? + needle.len();
    let mut depth = 1_usize;
    let mut end = None;
    for (at, character) in rest[start..].char_indices() {
        match character {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = Some(start + at);
                    break;
                }
            }
            _ => {}
        }
    }
    let body = &rest[start..end?];
    let identifier = body.strip_prefix("identifier=")?.split(", pid=").next()?;
    let pid = body
        .split(", pid=")
        .nth(1)?
        .split(',')
        .next()?
        .parse()
        .ok()?;
    let binary_path = body
        .rsplit_once(", binary_path=")
        .map_or("", |(_, path)| path);
    Some(Process {
        identifier,
        pid,
        binary_path,
    })
}

/// The program a process runs: its file's name, else the identifier the system gives it.
fn program_of(process: &Process<'_>) -> String {
    let file = process.binary_path.rsplit('/').next().unwrap_or("");
    if file.is_empty() {
        process.identifier.to_owned()
    } else {
        file.to_owned()
    }
}
