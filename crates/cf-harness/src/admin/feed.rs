//! What the admin asks of a release feed and what it reads from the answer
//! (`latestRelease`, `src/harness-admin.js`), all of it but the network, so
//! that the client the daemon gives (the HTTPS of the platform, which this
//! crate has none of) holds to Node's rules by doing nothing but move bytes
//! ([`Network`]):
//!
//! - a GET of [`Source::url`] ([`request`]), answered within [`TIMEOUT`] as
//!   a whole, head and body together, with no redirect followed: one is
//!   refused whatever it carries ([`REDIRECTS`]);
//! - a status that is not a success ends it ([`unsuccessful`]);
//! - the body is read a chunk at a time into a [`Body`], which stops it at
//!   [`MAX_UNITS`];
//! - the release is read from the text ([`release_version`]).
//!
//! [`Feed`] does all of it, over a [`Network`], as the admin's [`Latest`].
//! Where the connection fails, the network says what Node's `fetch` said
//! ([`FETCH_FAILED`], [`TERMINATED`]), so that the page reads as it did. The
//! recorded goldens (`tests/goldens/admin`) hold each of these to Node's
//! answer.

use std::rc::Rc;
use std::time::Duration;

use cf_base::json::from_slice_lossy;
use cf_base::{js, text::utf16_len};
use cf_proto::agents::Harness;
use serde_json::Value;

use super::source::{Format, Source};
use super::version::version_of;
use super::Latest;
use crate::contract::Work;
use crate::seams::{within, Time};

/// How long the whole request may take, as `AbortSignal.timeout(5000)`
/// bounds it: waiting for the answer and reading its body together.
pub const TIMEOUT: Duration = Duration::from_secs(5);

/// The most the body may hold, in UTF-16 code units of the text read so far
/// (`text.length > 2_000_000`).
pub const MAX_UNITS: usize = 2_000_000;

/// What the page says of a feed that answered with a body past [`MAX_UNITS`].
pub const TOO_BIG: &str = "Release metadata exceeds size limit";

/// What the page says of an answer that holds no release.
pub const UNAVAILABLE: &str = "Release version is unavailable";

/// What the page says of an answer that is not JSON where JSON is read.
///
/// Kept from Node on purpose: Node said what V8's `JSON.parse` said, which
/// quotes the start of the text (`Unexpected token '<', "<html>…" is not
/// valid JSON`); no Rust parser says that.
pub const NOT_JSON: &str = "Release metadata is not valid JSON";

/// What Node's `fetch` said of a connection that failed to be made, and of a
/// redirect it was told to refuse (`redirect: 'error'`): `TypeError: fetch
/// failed`.
pub const FETCH_FAILED: &str = "fetch failed";

/// The statuses of a redirect. Node's `fetch`, told to refuse them
/// (`redirect: 'error'`), failed on each of these whatever came with it, a
/// `Location` or none, and answered every other status, 300, 304 and 305
/// among them.
pub const REDIRECTS: [u16; 5] = [301, 302, 303, 307, 308];

/// What Node's `fetch` said of a connection that was closed while the body
/// was being read: `TypeError: terminated`.
pub const TERMINATED: &str = "terminated";

/// What Node's `fetch` said of a request that took longer than [`TIMEOUT`]:
/// the timeout signal's `TimeoutError`.
pub const TIMED_OUT: &str = "The operation was aborted due to timeout";

/// What the page says of a feed that answered with `status`, a status that
/// is not a success (`!response.ok`: not 200 to 299).
pub fn unsuccessful(status: u16) -> String {
    format!("Release service returned HTTP {status}")
}

/// A body as it arrives, a chunk at a time, read as Node read it: each chunk
/// decoded as UTF-8 on its own, so a character cut between two chunks is two
/// replacement characters, and the length counted in UTF-16 code units as
/// JavaScript counts a text.
#[derive(Debug, Default)]
pub struct Body {
    text: String,
    units: usize,
}

impl Body {
    /// Adds the next chunk, or says [`TOO_BIG`] once the text read holds more
    /// than [`MAX_UNITS`]: the body is read no further.
    pub fn push(&mut self, chunk: &[u8]) -> Result<(), &'static str> {
        let decoded = String::from_utf8_lossy(chunk);
        self.units += utf16_len(&decoded);
        self.text.push_str(&decoded);
        if self.units > MAX_UNITS {
            return Err(TOO_BIG);
        }
        Ok(())
    }

    /// The text read.
    pub fn text(&self) -> &str {
        &self.text
    }
}

/// The latest release a feed's body says, read as `format` says: the body's
/// text with its white space taken off for [`Format::Text`], else the JSON's
/// `version` (`versions.stable` of a formula). It is a release only when
/// some version is in it.
///
/// Kept from Node on purpose: JSON that is `null` has no `version` to read,
/// and Node's `TypeError` said so in V8's words; here it is [`UNAVAILABLE`],
/// as for JSON with no release in it.
pub fn release_version(format: Format, text: &str) -> Result<String, String> {
    let found = if format == Format::Text {
        Some(js::trim(text).to_owned())
    } else {
        let data = from_slice_lossy(text.as_bytes()).map_err(|_| NOT_JSON.to_owned())?;
        let value = match format {
            Format::Formula => data
                .get("versions")
                .and_then(|versions| versions.get("stable")),
            _ => data.get("version"),
        };
        value.and_then(Value::as_str).map(str::to_owned)
    };
    found
        .filter(|value| version_of(value).is_some())
        .ok_or_else(|| UNAVAILABLE.to_owned())
}

/// What is asked of a feed: a GET of `url`, to be answered within `timeout`,
/// following a redirect or, as Node's `redirect: 'error'` has it, none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request<'a> {
    pub url: &'a str,
    /// How long the whole request may take: [`TIMEOUT`].
    pub timeout: Duration,
    /// Whether a redirect is followed: it is not. The network answers one as
    /// it is, its status among [`REDIRECTS`], and the feed refuses it in
    /// [`FETCH_FAILED`]'s words.
    pub follow_redirects: bool,
}

/// What the admin asks of the feed `source` names.
pub fn request(source: &Source) -> Request<'_> {
    Request {
        url: &source.url,
        timeout: TIMEOUT,
        follow_redirects: false,
    }
}

/// Where a feed is asked: the platform's HTTPS, in the daemon. A GET that
/// does not follow a redirect, answered once its head has arrived.
pub trait Network {
    /// Asks as `request` says, following no redirect (one is an answer like
    /// another, with its status): the answer once its head is in, or the words
    /// of why not ([`FETCH_FAILED`] where the connection could not be made).
    fn get<'a>(&'a self, request: &'a Request<'a>) -> Work<'a, Result<Box<dyn Answer>, String>>;
}

/// A feed's answer, its body still arriving.
pub trait Answer {
    /// The status of the answer.
    fn status(&self) -> u16;

    /// The next chunk of the body, none at its end, or the words of why it
    /// ended otherwise ([`TERMINATED`] where the connection closed).
    fn chunk(&mut self) -> Work<'_, Result<Option<Vec<u8>>, String>>;
}

/// The latest release of a harness, asked of its feed over a [`Network`]
/// (`latestRelease`): Node's own rules, whichever network answers.
pub struct Feed {
    time: Rc<dyn Time>,
    network: Rc<dyn Network>,
}

impl Feed {
    /// A feed asked over `network`, its time bound by `time`'s clock.
    pub fn new(time: Rc<dyn Time>, network: Rc<dyn Network>) -> Self {
        Self { time, network }
    }

    /// The release `source`'s feed says, within the request's time.
    async fn read(&self, source: &Source, asked: &Request<'_>) -> Result<String, String> {
        let mut answer = self.network.get(asked).await?;
        let status = answer.status();
        if !asked.follow_redirects && REDIRECTS.contains(&status) {
            return Err(FETCH_FAILED.to_owned());
        }
        if !(200..300).contains(&status) {
            return Err(unsuccessful(status));
        }
        let mut body = Body::default();
        while let Some(chunk) = answer.chunk().await? {
            body.push(&chunk).map_err(str::to_owned)?;
        }
        release_version(source.format, body.text())
    }
}

impl Latest for Feed {
    fn latest<'a>(&'a self, _id: Harness, source: &'a Source) -> Work<'a, Result<String, String>> {
        Box::pin(async move {
            let asked = request(source);
            let millis = u64::try_from(asked.timeout.as_millis()).unwrap_or(u64::MAX);
            within(&*self.time, millis, self.read(source, &asked))
                .await
                .unwrap_or_else(|| Err(TIMED_OUT.to_owned()))
        })
    }
}

#[cfg(test)]
mod tests;
