//! The URLs the channel asks OpenCode's server, written as `new URL` and
//! `searchParams` write them. The server's origin is its own, never given in
//! anything that needs escaping, but the working folder the requests name is a
//! path, and the two builders write it differently:
//! - `searchParams.set` writes it as a form does: a space as `+`, and
//!   everything but a letter, a digit and `*-._` as percent-encoded;
//! - `` `session?directory=${encodeURIComponent(…)}` `` keeps `!~*'()` as
//!   they are, which the URL parser then leaves, but for `'`, which it
//!   escapes as `%27`: the one character of the query set that
//!   `encodeURIComponent` leaves.

use url::form_urlencoded::byte_serialize;

/// `encodeURIComponent(text)`: every character but a letter, a digit and
/// `-_.!~*'()` as the percent-encoding of its UTF-8.
fn component(text: &str) -> String {
    let mut written = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => written.push(char::from(byte)),
            other => written.push_str(&format!("%{other:02X}")),
        }
    }
    written
}

/// `url.searchParams.set('directory', directory)` on a URL with no query:
/// the query it makes.
fn directory_param(directory: &str) -> String {
    let value: String = byte_serialize(directory.as_bytes()).collect();
    format!("directory={value}")
}

/// `GET /session/<id>?directory=…`: the session's own record, which a
/// resumed conversation's first message reads its model and effort from.
pub(super) fn session(endpoint: &str, session: &str, directory: &str) -> String {
    format!(
        "{endpoint}/session/{}?{}",
        component(session),
        directory_param(directory)
    )
}

/// `POST /session/<id>/prompt_async?directory=…`: the first message.
pub(super) fn prompt(endpoint: &str, session: &str, directory: &str) -> String {
    format!(
        "{endpoint}/session/{}/prompt_async?{}",
        component(session),
        directory_param(directory)
    )
}

/// `POST /session?directory=…`: a new empty session in `directory`.
pub(super) fn creation(endpoint: &str, directory: &str) -> String {
    format!(
        "{endpoint}/session?directory={}",
        component(directory).replace('\'', "%27")
    )
}

#[cfg(test)]
mod tests;
