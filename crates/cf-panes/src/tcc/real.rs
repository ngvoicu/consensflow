//! The reader of the system's log as it runs on a Mac: `log stream` started through
//! cf-process, the window table it asks, and the events it sends the daemon.

use std::ffi::OsString;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_bridge::Bridge;
use cf_process::{spawn, Ending, Run, Streams};
use cf_proto::panes::{TccPrompt, TccUnreadable, TCC_PROMPT_EVENT, TCC_UNREADABLE_EVENT};
use futures_channel::oneshot;
use serde_json::Value;

use super::lines::Prompt;
use super::watch::{Read, Stream, Tells, Time, Windows};
use crate::pty::{PaneKey, PaneTable};

/// The most bytes of a line the reader prints that are read: an event is a few thousand.
const LINE_LIMIT: usize = 1 << 20;
/// How long the reader has to start.
const START_WITHIN: Duration = Duration::from_secs(10);

/// The system's log reader.
const LOG: &str = "/usr/bin/log";

/// What `log stream` is asked: the dialogs raised (`AUTHREQ_PROMPTING`), and who asked each
/// (`AUTHREQ_ATTRIBUTION`, of the requests in the name of the process `app` alone, so that the
/// rest of the machine's requests are not even read).
pub(crate) fn predicate(app: u32) -> String {
    format!(
        "subsystem == \"com.apple.TCC\" AND (eventMessage CONTAINS \"AUTHREQ_PROMPTING\" OR \
         (eventMessage CONTAINS \"AUTHREQ_ATTRIBUTION\" AND eventMessage CONTAINS \"pid={app},\"))"
    )
}

/// The shell's arguments that start the reader with its error stream in its output: what it
/// says when it is refused (`Must be admin to run 'stream' command`) comes as a line.
pub(crate) fn log_arguments(app: u32) -> Vec<OsString> {
    [
        "-c",
        "exec \"$@\" 2>&1",
        "sh",
        LOG,
        "stream",
        "--level",
        "info",
        "--style",
        "ndjson",
        "--predicate",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([OsString::from(predicate(app))])
    .collect()
}

enum Said {
    Line(String),
    End,
}

/// A program running beside this one, read a line at a time, on a thread of its own.
pub(crate) struct LogStream {
    lines: mpsc::Receiver<Said>,
    end: Option<oneshot::Sender<()>>,
}

impl LogStream {
    /// The reader of the log for the dialogs raised in the name of `app`.
    pub(crate) fn of_the_log(app: u32) -> Result<Self, String> {
        Self::start(Path::new("/bin/sh"), log_arguments(app))
    }

    /// `program` started with `arguments`, its output read a line at a time.
    pub(crate) fn start(program: &Path, arguments: Vec<OsString>) -> Result<Self, String> {
        let (said, lines) = mpsc::channel();
        let (end, ended) = oneshot::channel();
        let (started, start) = mpsc::channel::<Result<(), String>>();
        let run = Run {
            program: program.to_path_buf(),
            args: arguments,
            verbatim: false,
        };
        thread::Builder::new()
            .name("privacy-log".to_owned())
            .spawn(move || read_to_the_end(&run, &started, &said, ended))
            .map_err(|why| why.to_string())?;
        match start.recv_timeout(START_WITHIN) {
            Ok(Ok(())) => Ok(Self {
                lines,
                end: Some(end),
            }),
            Ok(Err(why)) => Err(why),
            Err(_) => Err("the reader of the log did not start".to_owned()),
        }
    }
}

/// Starts `run`, tells `started`, and sends each line of its output to `said` until it ends or
/// `ended` fires, which asks it to end.
fn read_to_the_end(
    run: &Run,
    started: &mpsc::Sender<Result<(), String>>,
    said: &mpsc::Sender<Said>,
    mut ended: oneshot::Receiver<()>,
) {
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        let _ = started.send(Err("no runtime for the reader of the log".to_owned()));
        return;
    };
    runtime.block_on(async {
        let env = Env::from_vars([("PATH", "/usr/bin:/bin")]);
        let child = match spawn(run, None, &env, Streams::Lines) {
            Ok(child) => child,
            Err(why) => {
                let _ = started.send(Err(why));
                return;
            }
        };
        let _ = started.send(Ok(()));
        loop {
            tokio::select! {
                line = child.read_line(LINE_LIMIT) => match line {
                    Ok(Some(line)) => {
                        // Nobody reads any more: the supervisor is gone.
                        if said.send(Said::Line(line)).is_err() {
                            child.terminate(Ending::Asked);
                            break;
                        }
                    }
                    Ok(None) | Err(_) => break,
                },
                _ = &mut ended => {
                    child.terminate(Ending::Asked);
                    break;
                }
            }
        }
        child.ended().await;
        let _ = said.send(Said::End);
    });
}

impl Stream for LogStream {
    fn recv(&mut self, wait: Duration) -> Read {
        match self.lines.recv_timeout(wait) {
            Ok(Said::Line(line)) => Read::Line(line),
            Ok(Said::End) | Err(mpsc::RecvTimeoutError::Disconnected) => Read::End,
            Err(mpsc::RecvTimeoutError::Timeout) => Read::Quiet,
        }
    }

    fn end(&mut self) {
        if let Some(end) = self.end.take() {
            let _ = end.send(());
        }
    }
}

/// The pane table as the supervisor asks it.
pub(crate) struct TableWindows(pub(crate) std::sync::Arc<PaneTable>);

impl Windows for TableWindows {
    fn open(&self) -> usize {
        self.0.open_count()
    }
    fn holding(&self, pid: u32) -> Option<PaneKey> {
        self.0.pane_holding(pid)
    }
    fn any(&self) -> Option<PaneKey> {
        self.0.some_pane()
    }
}

/// The daemon's bridge as the supervisor tells it, whichever bridge is current when it does.
pub(crate) struct BridgeTells<F: Fn() -> Option<Bridge>>(pub(crate) F);

impl<F: Fn() -> Option<Bridge>> BridgeTells<F> {
    fn send(&self, event: &str, body: Value) {
        if let Some(bridge) = (self.0)() {
            let _ = bridge.event(event, body);
        }
    }
}

impl<F: Fn() -> Option<Bridge>> Tells for BridgeTells<F> {
    fn prompt(&self, window: Option<PaneKey>, prompt: &Prompt) {
        let body = TccPrompt {
            id: window.as_ref().map(|key| key.id.clone()),
            generation: window.map(|key| key.generation),
            service: prompt.service.clone(),
            pid: prompt.pid,
            program: prompt.program.clone(),
        };
        if let Ok(body) = serde_json::to_value(&body) {
            self.send(TCC_PROMPT_EVENT, body);
        }
    }

    fn unreadable(&self, window: Option<PaneKey>, said: &str) {
        let body = TccUnreadable {
            id: window.as_ref().map(|key| key.id.clone()),
            generation: window.map(|key| key.generation),
            said: said.to_owned(),
        };
        if let Ok(body) = serde_json::to_value(&body) {
            self.send(TCC_UNREADABLE_EVENT, body);
        }
    }
}

/// The clock and the waiting of the thread that runs the supervisor.
pub(crate) struct ThreadTime;

impl Time for ThreadTime {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, how_long: Duration) {
        thread::sleep(how_long);
    }
}

#[cfg(test)]
mod tests;
