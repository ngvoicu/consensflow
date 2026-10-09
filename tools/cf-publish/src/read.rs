//! Where a release's files and feeds are read from: `curl`, as a process,
//! asked again as often and as long as the caller says. A feed that was just
//! replaced can be served stale for a moment, and GitHub's download links catch
//! up with a new file after a few minutes; a file published long ago is read
//! past a blip and never waited on for a change.

use std::collections::HashMap;
use std::io::Read;
use std::thread;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::digest::hex;
use crate::process;
use crate::version::{later_release, named_release};

/// How long a body is read for, and how long a hash: the archive is the
/// biggest file, and is read as it streams.
const BODY_LIMIT: Duration = Duration::from_secs(30);
const HASH_LIMIT: Duration = Duration::from_secs(15 * 60);

/// A read that got no success: the status it got (none where nothing
/// answered) and why, as a problem says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Refusal {
    pub status: Option<u16>,
    pub why: String,
}

/// What a read found: the thing read, or why there is none.
pub type Fetched<T> = Result<T, Refusal>;

/// How often a read is asked, and how long it waits between asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Patience {
    /// At least one.
    pub attempts: u32,
    pub wait: Duration,
}

impl Patience {
    /// `attempts` asks (one at least), `wait` apart.
    pub fn new(attempts: u32, wait: Duration) -> Self {
        Self {
            attempts: attempts.max(1),
            wait,
        }
    }
}

impl Default for Patience {
    /// Twelve asks, five seconds apart.
    fn default() -> Self {
        Self::new(12, Duration::from_secs(5))
    }
}

fn refused(status: u16) -> Refusal {
    Refusal {
        status: Some(status),
        why: format!("HTTP {status}"),
    }
}

fn unreachable(why: &str) -> Refusal {
    Refusal {
        status: None,
        why: format!("unreachable: {why}"),
    }
}

/// `url`, read by `curl` to its end: what the body is, as it arrives, goes to
/// `on_body`. The status comes back after the body, from curl's `--write-out`,
/// so the last three bytes of what curl writes are held back from `on_body`.
/// Redirects are followed (a release download is a redirect to where GitHub
/// keeps the file), and no proxy of the environment is asked: the reads go to
/// GitHub, as the `fetch` this replaces did.
fn fetch(url: &str, limit: Duration, mut on_body: impl FnMut(&[u8])) -> Fetched<()> {
    let limit = limit.as_secs().to_string();
    let args = [
        "--disable",
        "--silent",
        "--show-error",
        "--location",
        "--noproxy",
        "*",
        "--proto",
        "=http,https",
        "--proto-redir",
        "=http,https",
        "--max-time",
        limit.as_str(),
        "--write-out",
        "%{http_code}",
        "--url",
        url,
    ];
    let mut child = process::start("curl", args)
        .map_err(|cause| unreachable(&format!("curl could not be started: {cause}")))?;
    let mut tail: Vec<u8> = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    if let Some(mut stdout) = child.stdout.take() {
        loop {
            let read = match stdout.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => read,
                Err(cause) if cause.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            };
            tail.extend_from_slice(&chunk[..read]);
            if tail.len() > 3 {
                let body = tail.len() - 3;
                on_body(&tail[..body]);
                tail.drain(..body);
            }
        }
    }
    let mut said = Vec::new();
    if let Some(mut stderr) = child.stderr.take() {
        let _ = stderr.read_to_end(&mut said);
    }
    let ended = child
        .wait()
        .map_err(|cause| unreachable(&format!("curl could not be waited for: {cause}")))?;
    if !ended.success() {
        let said = String::from_utf8_lossy(&said);
        let said = said.trim();
        return Err(unreachable(&if said.is_empty() {
            format!("curl exited with {}", ended.code().unwrap_or(1))
        } else {
            said.to_string()
        }));
    }
    let status = std::str::from_utf8(&tail)
        .ok()
        .and_then(|digits| digits.parse::<u16>().ok())
        .ok_or_else(|| unreachable("curl said no status"))?;
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(refused(status))
    }
}

/// The body of `url`.
fn fetch_body(url: &str) -> Fetched<Vec<u8>> {
    let mut body = Vec::new();
    fetch(url, BODY_LIMIT, |bytes| body.extend_from_slice(bytes))?;
    Ok(body)
}

/// The SHA-256 of what `url` serves, read as it streams.
fn fetch_sha256(url: &str) -> Fetched<String> {
    let mut hash = Sha256::new();
    fetch(url, HASH_LIMIT, |bytes| hash.update(bytes))?;
    Ok(hex(&hash.finalize()))
}

/// What `read` answered once `accept` took it, or what it answered last, after
/// `patience.attempts` asks `patience.wait` apart.
fn until<T>(
    patience: Patience,
    mut read: impl FnMut() -> Fetched<T>,
    accept: impl Fn(&Fetched<T>) -> bool,
) -> Fetched<T> {
    let mut attempt = 1;
    loop {
        let found = read();
        if accept(&found) || attempt >= patience.attempts {
            return found;
        }
        attempt += 1;
        thread::sleep(patience.wait);
    }
}

/// Reads a release's files and feeds with the patience it was made with. A hash
/// is read once for the life of the reader.
pub struct Reader {
    patience: Patience,
    /// Past a blip, never waited on for a change: a few asks, not all.
    blips: Patience,
    hashes: HashMap<String, Fetched<String>>,
}

impl Reader {
    /// A reader that asks `patience.attempts` times, `patience.wait` apart,
    /// where a read may be out of date, and at most three where it is a file
    /// published long ago.
    pub fn new(patience: Patience) -> Self {
        Self {
            patience,
            blips: Patience::new(patience.attempts.min(3), patience.wait),
            hashes: HashMap::new(),
        }
    }

    /// A feed's latest.json, read again while it is not `expected` and names no
    /// later release: a replaced asset can be served stale for a moment, and no
    /// wait turns a later release into `expected`.
    pub fn feed(&self, url: &str, expected: &[u8]) -> Fetched<Vec<u8>> {
        let own = named_release(expected);
        until(
            self.patience,
            || fetch_body(url),
            |found| match found {
                Ok(body) => {
                    body == expected
                        || own
                            .as_ref()
                            .is_some_and(|own| later_release(body, own).is_some())
                }
                Err(_) => false,
            },
        )
    }

    /// A published file's body.
    pub fn file(&self, url: &str) -> Fetched<Vec<u8>> {
        until(self.blips, || fetch_body(url), |found| found.is_ok())
    }

    /// What a published file hashes to.
    pub fn hash(&mut self, url: &str) -> Fetched<String> {
        if let Some(known) = self.hashes.get(url) {
            return known.clone();
        }
        let found = until(self.blips, || fetch_sha256(url), |found| found.is_ok());
        self.hashes.insert(url.to_string(), found.clone());
        found
    }

    /// A feed that may not be there: asked once.
    pub fn once(&self, url: &str) -> Fetched<Vec<u8>> {
        fetch_body(url)
    }
}
