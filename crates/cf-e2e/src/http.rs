//! Plain HTTP to a server on this machine: the daemon's screens and API, which
//! the suites ask as the page and the app do. A status other than 2xx is an
//! answer to judge, not a failure; only a server that does not answer is one.

pub mod server;

use std::time::Duration;

use serde_json::Value;

use crate::{Error, Result};

/// How long a request has to be answered.
const TIMEOUT: Duration = Duration::from_secs(30);

/// What a server answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub status: u16,
    /// The `content-type` header, if it sent one.
    pub content_type: Option<String>,
    pub text: String,
}

impl Reply {
    /// What it answered, read as JSON.
    pub fn json(&self) -> Result<Value> {
        serde_json::from_str(&self.text).map_err(|source| Error::Json {
            text: self.text.clone(),
            source,
        })
    }
}

/// A client of servers on loopback.
#[derive(Debug, Clone)]
pub struct Http {
    agent: ureq::Agent,
}

impl Default for Http {
    fn default() -> Self {
        Self::new()
    }
}

impl Http {
    /// A client that takes no proxy of the machine's: what it asks is on
    /// loopback.
    pub fn new() -> Self {
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(TIMEOUT))
            .http_status_as_error(false)
            .proxy(None)
            .build()
            .into();
        Self { agent }
    }

    /// Asks `url` with `method` (GET, POST, PATCH or DELETE), as the bearer
    /// `bearer` if one is given, with `body` as JSON if one is.
    pub fn send(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        body: Option<&Value>,
    ) -> Result<Reply> {
        let failed = |message: String| Error::Http {
            url: url.to_owned(),
            message,
        };
        let authorization = bearer.map(|token| format!("Bearer {token}"));
        macro_rules! with_header {
            ($request:expr) => {{
                let request = $request;
                match &authorization {
                    Some(value) => request.header("authorization", value),
                    None => request,
                }
            }};
        }
        let sent = match (method, body) {
            ("GET", None) => with_header!(self.agent.get(url)).call(),
            ("DELETE", None) => with_header!(self.agent.delete(url)).call(),
            ("POST", Some(body)) => with_header!(self.agent.post(url))
                .header("content-type", "application/json")
                .send(body.to_string()),
            ("PATCH", Some(body)) => with_header!(self.agent.patch(url))
                .header("content-type", "application/json")
                .send(body.to_string()),
            (method, body) => {
                return Err(failed(format!(
                    "no way to send a {method} {}",
                    if body.is_some() {
                        "with a body"
                    } else {
                        "without one"
                    }
                )))
            }
        };
        let mut response = sent.map_err(|cause| failed(cause.to_string()))?;
        let status = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|cause| failed(cause.to_string()))?;
        Ok(Reply {
            status,
            content_type,
            text,
        })
    }

    /// GET `url` as the bearer `bearer`, if one is given.
    pub fn get(&self, url: &str, bearer: Option<&str>) -> Result<Reply> {
        self.send("GET", url, bearer, None)
    }
}

/// The part of `url` before its path: `http://127.0.0.1:51234` for
/// `http://127.0.0.1:51234/`.
pub fn origin(url: &str) -> &str {
    let after_scheme = url.find("://").map_or(0, |at| at + 3);
    match url[after_scheme..].find('/') {
        Some(end) => &url[..after_scheme + end],
        None => url,
    }
}

/// `text` as a URL component takes it: everything but letters, digits and
/// `-_.!~*'()` as percent escapes of its UTF-8 bytes, as JavaScript's
/// `encodeURIComponent` writes it.
pub fn encode_component(text: &str) -> String {
    let mut encoded = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_origin_is_the_url_without_its_path() {
        assert_eq!(origin("http://127.0.0.1:51234/"), "http://127.0.0.1:51234");
        assert_eq!(
            origin("http://127.0.0.1:51234/api/agents"),
            "http://127.0.0.1:51234"
        );
        assert_eq!(origin("http://127.0.0.1:51234"), "http://127.0.0.1:51234");
    }

    #[test]
    fn a_component_is_escaped_as_encode_uri_component_does() {
        assert_eq!(encode_component("proof-ui-token"), "proof-ui-token");
        assert_eq!(encode_component("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encode_component("-_.!~*'()"), "-_.!~*'()");
        assert_eq!(encode_component(""), "");
    }

    #[test]
    fn a_method_with_no_way_to_send_it_is_an_error_that_says_so() {
        let failed = Http::new()
            .send("POST", "http://127.0.0.1:1/", None, None)
            .unwrap_err();
        assert!(
            failed
                .to_string()
                .contains("no way to send a POST without one"),
            "{failed}"
        );
    }

    #[test]
    fn a_server_that_is_not_there_is_an_error_that_names_the_address() {
        let failed = Http::new().get("http://127.0.0.1:1/", None).unwrap_err();
        assert!(
            matches!(&failed, Error::Http { url, .. } if url == "http://127.0.0.1:1/"),
            "{failed}"
        );
    }
}
