//! A request as a handler sees it. **Frozen**.
//!
//! The target is read as Node reads it (`new URL(request.url,
//! 'http://127.0.0.1')`, `api.js:80`): the WHATWG URL standard, so `path` is
//! `pathname` and `param` is `searchParams.get`. What a handler reads of the
//! headers is the bearer token alone.

use cf_base::refusal::Refusal;
use hyper::header::AUTHORIZATION;
use hyper::Method;
use serde_json::{Map, Value};
use url::Url;

use super::answer::Failure;
use super::body::{read_json, read_text, Body, Unread};

/// A request: what asked, of what, with which credential, and its body.
pub struct Request {
    pub method: Method,
    /// The target's path as the standard writes it (`pathname`): its
    /// percent-escapes are as the client wrote them.
    pub path: String,
    query: Vec<(String, String)>,
    authorization: Option<String>,
    /// The body, which only a handler that reads one touches: the agents' API
    /// reads it with [`Request::json`], the screens with [`Request::text`].
    pub body: Body,
}

impl Request {
    /// A request for `target` (the request line's, a path and a query, or a
    /// whole URL), which carries `authorization` as its header, if it does.
    /// A target the standard does not read is `Invalid URL`, which fails the
    /// request as Node's `new URL` threw (500, `internal`).
    pub fn new(
        method: Method,
        target: &str,
        authorization: Option<String>,
        body: Body,
    ) -> Result<Self, Failure> {
        let base = Url::parse("http://127.0.0.1")
            .map_err(|_| Failure::Internal("Invalid URL".to_owned()))?;
        let url = Url::options()
            .base_url(Some(&base))
            .parse(target)
            .map_err(|_| Failure::Internal("Invalid URL".to_owned()))?;
        Ok(Self {
            method,
            path: url.path().to_owned(),
            query: url
                .query_pairs()
                .map(|(name, value)| (name.into_owned(), value.into_owned()))
                .collect(),
            authorization,
            body,
        })
    }

    /// The request hyper read: its header as a text of Latin-1 characters
    /// (Node's), and its body as it arrives.
    pub fn from_hyper(request: hyper::Request<Body>) -> Result<Self, Failure> {
        let authorization = request.headers().get(AUTHORIZATION).map(|value| {
            // Node reads a header's bytes as Latin-1, each a character of its own.
            value
                .as_bytes()
                .iter()
                .map(|&byte| char::from(byte))
                .collect()
        });
        let method = request.method().clone();
        let target = request.uri().to_string();
        Self::new(method, &target, authorization, request.into_body())
    }

    /// `METHOD /path`, as the API words an unknown route.
    pub fn at(&self) -> String {
        format!("{} {}", self.method, self.path)
    }

    /// The first value of the query's `name` (`searchParams.get`).
    pub fn param(&self, name: &str) -> Option<&str> {
        self.query
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// The token of an `Authorization: Bearer <token>` header; none for any
    /// other header, or none. A token has what follows the word and one
    /// space, however odd.
    pub fn bearer(&self) -> Option<&str> {
        self.authorization
            .as_deref()
            .and_then(|header| header.strip_prefix("Bearer "))
    }

    /// The body as the one JSON object the agents' API reads ([`read_json`]).
    pub async fn json(&mut self) -> Result<Map<String, Value>, Failure> {
        read_json(&mut self.body).await
    }

    /// The body as the text a screen reads ([`read_text`]).
    pub async fn text(&mut self) -> Result<String, Unread> {
        read_text(&mut self.body).await
    }

    /// 404 `unknown-route` for this request, in Node's words
    /// (`no such command: GET /api/nothing`).
    pub fn unknown_route(&self) -> Failure {
        Failure::Refused(Refusal::with_status(
            "unknown-route",
            format!("no such command: {}", self.at()),
            404,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(target: &str, authorization: Option<&str>) -> Request {
        Request::new(
            Method::GET,
            target,
            authorization.map(str::to_owned),
            Body::empty(),
        )
        .unwrap()
    }

    #[test]
    fn the_path_is_the_pathname_and_the_query_is_the_search_params() {
        let request = get("/api/questions/12?wait=2000&wait=9&x=a%20b+c&empty=", None);
        assert_eq!(request.path, "/api/questions/12");
        assert_eq!(request.param("wait"), Some("2000"), "the first");
        assert_eq!(request.param("x"), Some("a b c"));
        assert_eq!(request.param("empty"), Some(""));
        assert_eq!(request.param("absent"), None);
        assert_eq!(request.at(), "GET /api/questions/12");
    }

    #[test]
    fn a_target_is_read_as_the_url_standard_reads_it() {
        assert_eq!(get("/a/../api/tasks", None).path, "/api/tasks");
        assert_eq!(get("/api/tasks/%35", None).path, "/api/tasks/%35");
        assert_eq!(
            get("http://127.0.0.1:9/api/staff?x=1", None).path,
            "/api/staff"
        );
        assert_eq!(get("/", None).path, "/");
        assert_eq!(get("", None).path, "/");
    }

    #[test]
    fn a_target_the_standard_does_not_read_fails_the_request_as_invalid_url() {
        let refused = Request::new(Method::GET, "http://", None, Body::empty());
        assert!(matches!(
            refused,
            Err(Failure::Internal(words)) if words == "Invalid URL"
        ));
    }

    #[test]
    fn a_bearer_token_is_what_follows_the_word_bearer_and_a_space() {
        assert_eq!(get("/", Some("Bearer abc")).bearer(), Some("abc"));
        assert_eq!(get("/", Some("Bearer ")).bearer(), Some(""));
        assert_eq!(get("/", Some("Bearer  two")).bearer(), Some(" two"));
        for header in ["bearer abc", "Bearer", "Basic abc", "abc", ""] {
            assert_eq!(get("/", Some(header)).bearer(), None, "{header:?}");
        }
        assert_eq!(get("/", None).bearer(), None);
    }

    #[test]
    fn an_unknown_route_says_what_asked_for_it_in_nodes_words() {
        let Failure::Refused(refusal) = get("/api/nothing?x=1", None).unknown_route() else {
            panic!("a refusal");
        };
        assert_eq!(
            (refusal.status, refusal.code, refusal.message.as_str()),
            (404, "unknown-route", "no such command: GET /api/nothing")
        );
    }
}
