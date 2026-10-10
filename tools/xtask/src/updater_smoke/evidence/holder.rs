//! The ledger's one holder: a second ConsensFlow on the home, started while the
//! app's daemon runs, is refused by the ledger's own lock, which no other
//! evidence says as plainly (the daemon's start line is not the lock).

use std::collections::BTreeSet;
use std::io::Read;
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cf_base::env::Env;

use crate::process::{self, Invocation};
use crate::updater_smoke::bundle::{under, BundleInfo};
use crate::updater_smoke::processes::signal_of;
use crate::updater_smoke::sandbox::Sandbox;
use crate::updater_smoke::{Error, Result};

/// How long a second ConsensFlow tried on the home is given to be refused.
const SECOND_DAEMON_LIMIT: Duration = Duration::from_secs(30);

/// How long what a process wrote is waited for once it has ended.
const OUTPUT_GRACE: Duration = Duration::from_secs(2);

/// How a second ConsensFlow on the home ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attempt {
    pub pid: u32,
    pub code: Option<i32>,
    /// The signal that ended it, where one did.
    pub signal: Option<i32>,
    pub out: String,
    pub err: String,
}

/// A second ConsensFlow on the home, started the way the daemon is (the bundle's
/// `cf ui --json --no-open`, its input closed) while the app's daemon runs: the
/// ledger refuses it, in its own words, with no handle line out. Exit 1 alone
/// could be a program the bundle lacks, so the words are the proof.
pub fn assert_ledger_held(attempt: &Attempt, db: &str) -> Result {
    let said = format!("{}{}", attempt.out, attempt.err);
    ensure!(
        attempt.signal.is_none(),
        "the second cf ui never ended: {said}"
    );
    ensure!(
        attempt.code == Some(1),
        "the second cf ui ended {}: {said}",
        attempt
            .code
            .map_or_else(|| "null".to_string(), |code| code.to_string())
    );
    ensure!(
        attempt
            .err
            .contains(&format!("another ConsensFlow has {db} open")),
        "the second cf ui was refused, but not for the ledger's lock: {}",
        attempt.err
    );
    ensure!(
        attempt.out.is_empty(),
        "the second cf ui printed a handle line: {}",
        attempt.out
    );
    Ok(())
}

/// What a process wrote to a stream, kept as it is read.
struct Written {
    text: Arc<Mutex<Vec<u8>>>,
    reading: JoinHandle<()>,
}

impl Written {
    fn read(mut stream: impl Read + Send + 'static) -> Self {
        let text = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&text);
        let reading = thread::spawn(move || {
            let mut chunk = [0_u8; 4096];
            while let Ok(count) = stream.read(&mut chunk) {
                if count == 0 {
                    break;
                }
                kept.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .extend_from_slice(&chunk[..count]);
            }
        });
        Self { text, reading }
    }

    /// What was written, once the stream has closed, or what there is when it is
    /// still open after the grace a process that has ended is given.
    fn finish(self) -> String {
        let until = Instant::now() + OUTPUT_GRACE;
        while !self.reading.is_finished() && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
        let text = self.text.lock().unwrap_or_else(PoisonError::into_inner);
        String::from_utf8_lossy(&text).into_owned()
    }
}

/// Runs the bundle's `cf ui --json --no-open` on the box's home, as a second
/// ConsensFlow, and says how it ended.
pub fn second_daemon(bundle: &BundleInfo, sandbox: &Sandbox) -> Result<Attempt> {
    let mut vars: Vec<_> = sandbox
        .terminal_env(&sandbox.state)
        .iter()
        .map(|(name, value)| (name.to_os_string(), value.to_os_string()))
        .collect();
    // The way back runs on the bundle's Node, which the bundle's cf finds beside itself or is told.
    if bundle.node {
        vars.push((
            "CONSENSFLOW_NODE".into(),
            under(&bundle.app, &["Contents", "MacOS", "node"]).into(),
        ));
    }
    let invocation =
        Invocation::new(&bundle.cf, &sandbox.probe).args(["ui", "--json", "--no-open"]);
    let mut child = process::spawn(&invocation, &Env::from_vars(vars), Stdio::null(), false)?;
    let pid = child.id();
    let out = child.stdout.take().map(Written::read);
    let err = child.stderr.take().map(Written::read);
    let limit = Instant::now() + SECOND_DAEMON_LIMIT;
    let status = loop {
        let ended = child
            .try_wait()
            .map_err(|cause| Error::new(format!("could not wait for the second cf ui: {cause}")))?;
        if let Some(status) = ended {
            break status;
        }
        if Instant::now() >= limit {
            let _ = child.kill();
            break child.wait().map_err(|cause| {
                Error::new(format!("could not wait for the second cf ui: {cause}"))
            })?;
        }
        thread::sleep(Duration::from_millis(20));
    };
    Ok(Attempt {
        pid,
        code: status.code(),
        signal: signal_of(&status),
        out: out.map(Written::finish).unwrap_or_default(),
        err: err.map(Written::finish).unwrap_or_default(),
    })
}

/// The ledger is held by the app's daemon: a second one is refused. Said once the
/// daemon has answered the page, which is after it has the ledger: a probe
/// before that would be the one to take it. The probe's pid goes in `probes`.
pub fn ledger_held(bundle: &BundleInfo, sandbox: &Sandbox, probes: &mut BTreeSet<u32>) -> Result {
    let attempt = second_daemon(bundle, sandbox)?;
    probes.insert(attempt.pid);
    assert_ledger_held(
        &attempt,
        &sandbox.state.join("consensflow.db").to_string_lossy(),
    )
}

#[cfg(test)]
mod tests;
