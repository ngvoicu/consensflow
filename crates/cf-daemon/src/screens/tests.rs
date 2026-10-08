//! The screens' paths, and the order of their checks as Node's screens had
//! them: which paths are theirs, how the UI token is taken, what is read of a
//! request and when. (The routes' own answers are in `routes` and `harnesses`,
//! and every one Node recorded is held in `tests/screens`.)

use futures_util::stream;
use hyper::Method;
use serde_json::{json, Value};

use super::testing::{inert, Rig, TOKEN};
use super::*;
use crate::api::answer::Content;
use crate::api::body::Body;

mod harnesses;
mod routes;

/// What the screens answer, or none where the path is not theirs.
type Answered = Option<(u16, Content)>;

/// `method target`, with `authorization` as its header and `body` as the text sent.
async fn ask(
    screens: &Screens,
    method: Method,
    target: &str,
    authorization: Option<&str>,
    body: &str,
) -> Answered {
    let chunks: Vec<std::io::Result<bytes::Bytes>> = if body.is_empty() {
        Vec::new()
    } else {
        vec![Ok(bytes::Bytes::copy_from_slice(body.as_bytes()))]
    };
    let mut request = Request::new(
        method,
        target,
        authorization.map(str::to_owned),
        Body::new(stream::iter(chunks)),
    )
    .unwrap();
    screens
        .handle(&mut request)
        .await
        .map(|answer| (answer.status, answer.content))
}

/// `method target` as the app asks: the UI token as the bearer.
async fn ask_as_the_app(screens: &Screens, method: Method, target: &str, body: &str) -> Answered {
    ask(
        screens,
        method,
        target,
        Some(&format!("Bearer {TOKEN}")),
        body,
    )
    .await
}

/// The JSON of an answer, with its status.
fn json_of(answered: Answered) -> (u16, Value) {
    match answered {
        Some((status, Content::Json(body))) => (status, body),
        other => panic!("no JSON answer: {other:?}"),
    }
}

/// What the screens of `rig` answer the app, as JSON.
async fn the_app_asks(rig: &Rig, method: Method, target: &str, body: &str) -> (u16, Value) {
    json_of(ask_as_the_app(&rig.screens, method, target, body).await)
}

fn error(words: &str) -> Value {
    json!({ "error": words })
}

#[test]
fn the_screens_paths_are_the_seven_node_lists() {
    for (path, screen) in [
        ("/", Screen::Page),
        ("/harnesses", Screen::Page),
        ("/api/agents", Screen::Agents),
        (
            "/api/agents/mybuilder",
            Screen::Agent("mybuilder".to_owned()),
        ),
        ("/api/agents/a", Screen::Agent("a".to_owned())),
        (
            "/api/agents/my-agent-2",
            Screen::Agent("my-agent-2".to_owned()),
        ),
        ("/api/preferences", Screen::Preferences),
        ("/api/harnesses/check", Screen::HarnessCheck),
        ("/api/harnesses/update", Screen::HarnessUpdate),
    ] {
        assert_eq!(recognize(path), Some(screen), "{path}");
    }
}

#[test]
fn any_other_path_is_the_agents_api_s() {
    for path in [
        "",
        "/harnesses/",
        "/api",
        "/api/agents/",
        "/api/agents/Upper",
        "/api/agents/9lives",
        "/api/agents/-dash",
        "/api/agents/under_score",
        "/api/agents/a/b",
        "/api/agents/a b",
        "/api/agents/%6dine",
        "/api/harnesses",
        "/api/harnesses/check/",
        "/api/preferences/x",
        "/api/whoami",
        "/api/questions/1",
        "/index.html",
        "/library",
    ] {
        assert_eq!(recognize(path), None, "{path:?}");
    }
}

/// The paths the screens routed when Node was the daemon, frozen here: every
/// one is a path, so that none falls through unseen. They are `/`,
/// `/harnesses`, `/api/agents`, `/api/preferences`, `/api/harnesses/check` and
/// `/api/harnesses/update`, and an agent by its name, as a name is written
/// (`[a-z][a-z0-9-]*`; the paths that are not names are refused above).
#[test]
fn every_path_node_routed_is_one() {
    for path in [
        "/",
        "/harnesses",
        "/api/agents",
        "/api/preferences",
        "/api/harnesses/check",
        "/api/harnesses/update",
        "/api/agents/a",
        "/api/agents/my-maia",
        "/api/agents/pi-2-draw",
    ] {
        assert!(recognize(path).is_some(), "{path}");
    }
}

#[tokio::test]
async fn a_path_that_is_not_theirs_is_answered_none_whoever_asks() {
    let screens = inert();
    for target in [
        "/api/whoami",
        "/api/nothing",
        "/library",
        "/api/agents/Bad",
        "/api/agents/a/b",
    ] {
        for authorization in [None, Some("Bearer the-ui-token"), Some("Bearer other")] {
            assert_eq!(
                ask(&screens, Method::GET, target, authorization, "").await,
                None,
                "{target} {authorization:?}"
            );
        }
    }
}

#[tokio::test]
async fn without_the_token_every_screen_is_a_bare_401_and_nothing_is_read() {
    let screens = inert();
    for (method, target) in [
        (Method::GET, "/"),
        (Method::GET, "/harnesses"),
        (Method::GET, "/api/agents"),
        (Method::POST, "/api/agents"),
        (Method::PATCH, "/api/agents/mine"),
        (Method::DELETE, "/api/agents/mine"),
        (Method::POST, "/api/preferences"),
        (Method::POST, "/api/harnesses/check"),
        (Method::POST, "/api/harnesses/update"),
        (Method::HEAD, "/"),
        (Method::PUT, "/api/agents"),
    ] {
        for authorization in [None, Some("Bearer wrong"), Some("Basic the-ui-token")] {
            // A body that would fail the request if it were read.
            let answered = ask(&screens, method.clone(), target, authorization, "{").await;
            assert_eq!(
                json_of(answered),
                (401, error("unauthorized")),
                "{method} {target} {authorization:?}"
            );
        }
    }
}

#[tokio::test]
async fn the_token_is_the_bearer_or_else_the_first_token_of_the_query() {
    let screens = inert();
    let bearer = format!("Bearer {TOKEN}");
    for (authorization, target, status) in [
        // As a bearer, and in the query.
        (Some(bearer.as_str()), "/harnesses", 200),
        (None, "/harnesses?token=the-ui-token", 200),
        // A bearer that says something is the token, and the query is not looked at.
        (Some("Bearer wrong"), "/harnesses?token=the-ui-token", 401),
        (Some(bearer.as_str()), "/harnesses?token=wrong", 200),
        // A bearer that says nothing is none: the header without its space, or with
        // nothing after it, and a word that is not `Bearer` spelled as it is.
        (Some("Bearer"), "/harnesses?token=the-ui-token", 200),
        (Some("Bearer "), "/harnesses?token=the-ui-token", 200),
        (
            Some("bearer the-ui-token"),
            "/harnesses?token=the-ui-token",
            200,
        ),
        (Some("bearer the-ui-token"), "/harnesses", 401),
        (Some("Bearer"), "/harnesses", 401),
        // The first token of the query is the one.
        (None, "/harnesses?token=wrong&token=the-ui-token", 401),
        (None, "/harnesses?token=the-ui-token&token=wrong", 200),
        (None, "/harnesses?token=", 401),
        (None, "/harnesses?TOKEN=the-ui-token", 401),
        (None, "/harnesses?token=the-ui-token%20", 401),
    ] {
        let answered = ask(&screens, Method::GET, target, authorization, "").await;
        assert_eq!(
            answered.map(|(status, _)| status),
            Some(status),
            "{authorization:?} {target}"
        );
    }
}

#[tokio::test]
async fn a_get_reads_nothing_of_its_body_and_every_other_method_reads_it_before_any_route() {
    let screens = inert();
    // A body that panics if it is polled: a GET is never read.
    let mut get = Request::new(
        Method::GET,
        "/api/preferences?token=the-ui-token",
        None,
        Body::new(stream::poll_fn(
            |_| -> std::task::Poll<Option<std::io::Result<bytes::Bytes>>> {
                panic!("a GET's body is read")
            },
        )),
    )
    .unwrap();
    let answered = screens
        .handle(&mut get)
        .await
        .map(|answer| (answer.status, answer.content));
    assert_eq!(json_of(answered), (404, error("not found")));

    // The others are read first, whatever their route: a body that is no JSON fails
    // a request that has no route, and one that is JSON goes on to find none.
    for (method, target) in [
        (Method::POST, "/"),
        (Method::POST, "/harnesses"),
        (Method::HEAD, "/"),
        (Method::OPTIONS, "/api/agents"),
        (Method::PUT, "/api/agents/mine"),
        (Method::POST, "/api/agents/mine"),
        (Method::DELETE, "/api/agents"),
    ] {
        let (status, body) = json_of(ask_as_the_app(&screens, method.clone(), target, "{}").await);
        assert_eq!(
            (status, body),
            (404, error("not found")),
            "{method} {target}"
        );
        let (status, body) = json_of(ask_as_the_app(&screens, method.clone(), target, "{").await);
        assert_eq!(status, 400, "{method} {target}");
        assert!(
            body["error"]
                .as_str()
                .is_some_and(|words| words.starts_with("the request body is not valid JSON: ")),
            "{method} {target}: {body}"
        );
    }
}

#[tokio::test]
async fn a_body_over_64_k_units_is_refused_in_its_own_words_by_every_method_that_reads_one() {
    let screens = inert();
    let big = "x".repeat(64 * 1024 + 1);
    for (method, target) in [
        (Method::POST, "/api/preferences"),
        (Method::DELETE, "/api/agents/mine"),
        (Method::POST, "/"),
    ] {
        let answered = ask_as_the_app(&screens, method.clone(), target, &big).await;
        assert_eq!(
            json_of(answered),
            (400, error("body too large")),
            "{method} {target}"
        );
    }
}

#[tokio::test]
async fn a_page_is_html_and_a_delete_is_nothing() {
    let rig = Rig::new(&[]);
    let (status, content) = ask_as_the_app(&rig.screens, Method::GET, "/", "")
        .await
        .unwrap();
    assert_eq!(status, 200);
    let Content::Html(page) = content else {
        panic!("a page is HTML")
    };
    assert!(
        page.starts_with("<!DOCTYPE html>\n<html lang=\"en\">"),
        "{page:.80}"
    );
    assert!(page.contains("const TOKEN = \"the-ui-token\";"));
    let Some((200, Content::Html(harnesses))) =
        ask_as_the_app(&rig.screens, Method::GET, "/harnesses", "").await
    else {
        panic!("a page");
    };
    assert!(harnesses.contains("<title>ConsensFlow Harnesses</title>"));
}
