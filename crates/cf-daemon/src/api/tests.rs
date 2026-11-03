//! The order of a request's checks, end to end through [`handle`], as Node
//! has them: the screens first, then the window's token, then the routes, so
//! that a path that is no route at all, from a caller with no valid token, is
//! 401 and not 404.

use hyper::Method;
use serde_json::{json, Value};

use super::answer::Content;
use super::*;
use crate::testing::{request, said, scene, Scene};

fn screens() -> Screens {
    crate::screens::testing::inert()
}

async fn through(
    scene: &Scene,
    method: Method,
    target: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, Value) {
    said(
        handle(
            &scene.context,
            &screens(),
            request(method, target, token, body),
        )
        .await,
    )
}

const NO_ACCESS: &str = "this window has no ConsensFlow access (it may have closed)";

fn unauthorized(message: &str) -> Value {
    json!({ "error": "unauthorized", "message": message })
}

#[tokio::test]
async fn a_path_that_is_no_route_from_a_caller_with_no_token_is_401_not_404() {
    let scene = scene();
    for (method, target) in [
        (Method::GET, "/api/nothing"),
        (Method::GET, "/api/whoami"),
        (Method::POST, "/api/answers"),
        (Method::DELETE, "/anything/at/all"),
        // Not `/` or `/api/agents`: those are the screens', whose 401 is their own.
        (Method::GET, "/api/agents/Upper"),
        (Method::GET, "/api/agents/"),
    ] {
        let (status, body) = through(&scene, method.clone(), target, None, "").await;
        assert_eq!(
            (status, body),
            (401, unauthorized(NO_ACCESS)),
            "{method} {target}"
        );
    }
}

#[tokio::test]
async fn the_ui_token_opens_none_of_the_agents_routes_and_a_made_up_one_none_at_all() {
    let scene = scene();
    for token in ["the-ui-token", "made-up", ""] {
        let (status, body) = through(&scene, Method::GET, "/api/whoami", Some(token), "").await;
        assert_eq!((status, body), (401, unauthorized(NO_ACCESS)), "{token:?}");
    }
}

#[tokio::test]
async fn a_token_that_was_revoked_has_no_access_and_one_whose_project_is_gone_says_so() {
    let scene = scene();
    scene.context.credentials.revoke(&scene.zeus);
    let (status, body) = through(&scene, Method::GET, "/api/nothing", Some(&scene.zeus), "").await;
    assert_eq!((status, body), (401, unauthorized(NO_ACCESS)));

    let orphan = scene.context.credentials.issue(9_999, 9_999);
    let (status, body) = through(&scene, Method::GET, "/api/nothing", Some(&orphan), "").await;
    assert_eq!(
        (status, body),
        (
            401,
            unauthorized("this window belongs to a project that no longer exists")
        )
    );
}

#[tokio::test]
async fn a_window_with_its_token_is_told_a_route_is_unknown_in_nodes_words() {
    let scene = scene();
    let (status, body) = through(
        &scene,
        Method::GET,
        "/api/nothing?x=1",
        Some(&scene.zeus),
        "",
    )
    .await;
    assert_eq!(
        (status, body),
        (
            404,
            json!({ "error": "unknown-route", "message": "no such command: GET /api/nothing" })
        )
    );
    // The right path and the wrong method is no route either.
    let (status, _) = through(
        &scene,
        Method::DELETE,
        "/api/answers",
        Some(&scene.zeus),
        "",
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn the_screens_answer_their_own_paths_before_any_window_s_token_is_looked_at() {
    let scene = scene();
    for target in [
        "/",
        "/harnesses",
        "/api/agents",
        "/api/agents/mybuilder",
        "/api/preferences",
    ] {
        // No token, and a window's: their own bare 401, neither the API's (which
        // says why) nor its 404 for a route it has none for.
        for token in [None, Some(scene.zeus.as_str())] {
            let (status, body) = through(&scene, Method::GET, target, token, "").await;
            assert_eq!(
                (status, body),
                (401, json!({ "error": "unauthorized" })),
                "{target}"
            );
        }
    }
    // The UI token opens a screen, which the API never sees.
    let answered = handle(
        &scene.context,
        &screens(),
        request(Method::GET, "/harnesses", Some("the-ui-token"), ""),
    )
    .await
    .unwrap();
    assert!(matches!(
        (answered.status, answered.content),
        (200, Content::Html(page)) if page.contains("<title>ConsensFlow Harnesses</title>")
    ));
}

#[tokio::test]
async fn a_route_that_has_landed_runs_after_the_token_is_checked() {
    let scene = scene();
    let body = json!({ "question": scene.question.id, "body": "The first." }).to_string();
    let (status, answered) = through(
        &scene,
        Method::POST,
        "/api/answers",
        Some(&scene.chief),
        &body,
    )
    .await;
    assert_eq!(status, 201, "{answered}");
    assert_eq!(answered["message"]["kind"], "answer");
    assert_eq!(scene.kicks.get(), 1);

    // The token's holder decides who may do what: zeus asked it, and may not wait for another's.
    let target = format!("/api/questions/{}", scene.question.id);
    let (status, body) = through(&scene, Method::GET, &target, Some(&scene.chief), "").await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("not-your-question"))
    );
    let (status, body) = through(&scene, Method::GET, &target, Some(&scene.zeus), "").await;
    assert_eq!(
        (status, body["answer"]["body"].as_str()),
        (200, Some("The first."))
    );
}

#[tokio::test]
async fn a_body_is_read_only_where_a_route_reads_one() {
    let scene = scene();
    let huge = "x".repeat(3 * 1024 * 1024);
    // A route that takes no body reads none: a huge one is no 413 there.
    let (status, _) = through(&scene, Method::GET, "/api/whoami", Some(&scene.zeus), &huge).await;
    assert_eq!(status, 200);
    // One that refuses the window before it reads its body does not read it.
    let (status, body) =
        through(&scene, Method::POST, "/api/tasks", Some(&scene.zeus), &huge).await;
    assert_eq!(
        (status, body["error"].as_str()),
        (403, Some("not-a-coordinator"))
    );
    // Those that read it refuse it.
    for (target, token) in [
        ("/api/notes", &scene.zeus),
        ("/api/answers", &scene.chief),
        ("/api/tasks", &scene.chief),
    ] {
        let (status, body) = through(&scene, Method::POST, target, Some(token), &huge).await;
        assert_eq!(
            (status, body["error"].as_str()),
            (413, Some("too-large")),
            "{target}"
        );
    }
}

/// The context of `scene`, with the tooling's look at the API switched on or off.
fn tooling(scene: &Scene, on: bool) -> Rc<context::Context> {
    let given = &scene.context;
    Rc::new(context::Context {
        ledger: Rc::clone(&given.ledger),
        credentials: Rc::clone(&given.credentials),
        kick: Rc::clone(&given.kick),
        finish: Rc::clone(&given.finish),
        closing: given.closing.clone(),
        roster: Rc::clone(&given.roster),
        log: Rc::clone(&given.log),
        trace: Rc::clone(&given.trace),
        tooling_calls: on,
    })
}

/// The lines of the trace of `context` that are the tooling's look at the API.
fn answered_lines(context: &context::Context) -> Vec<(String, Value)> {
    std::fs::read_to_string(context.trace.file())
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let parsed: Value = serde_json::from_str(line).ok()?;
            (parsed["kind"] == calls::ANSWERED).then(|| (line.to_owned(), parsed))
        })
        .collect()
}

/// T-1152 (T-1133's design, section 2): a test that seats a real chief tells what the chief's
/// own `cf` asked and how the daemon answered it, a refusal's code among it, though `cf` says a
/// refusal in words. Only a daemon the tooling started so writes it, and never the token.
#[tokio::test]
async fn a_tooling_daemon_writes_each_window_request_it_answered_with_its_refusal_code_and_no_token(
) {
    let scene = scene();
    let target = format!("/api/questions/{}", scene.question.id);
    let on = tooling(&scene, true);
    for (token, status) in [(&scene.chief, 403), (&scene.zeus, 200)] {
        let answered = handle(
            &on,
            &screens(),
            request(Method::GET, &target, Some(token), ""),
        )
        .await;
        assert_eq!(said(answered).0, status);
    }
    // A request with no token is nobody's, and is not written.
    let _ = handle(&on, &screens(), request(Method::GET, &target, None, "")).await;
    let lines = answered_lines(&on);
    let data: Vec<&Value> = lines.iter().map(|(_, parsed)| &parsed["data"]).collect();
    assert_eq!(
        data,
        [
            &json!({ "participant": "chief", "role": "chief", "method": "GET", "path": target, "status": 403, "error": "not-your-question" }),
            &json!({ "participant": "zeus", "role": "worker", "method": "GET", "path": target, "status": 200, "error": null }),
        ]
    );
    for (line, parsed) in &lines {
        assert_eq!(parsed["project"], scene.project.id);
        assert!(parsed["at"].is_string(), "{line}");
        assert!(
            !line.contains(scene.chief.as_str()) && !line.contains(scene.zeus.as_str()),
            "{line}"
        );
    }

    // A daemon the tooling did not start writes none.
    let quiet = crate::testing::scene();
    let off = tooling(&quiet, false);
    let _ = handle(
        &off,
        &screens(),
        request(Method::GET, &target, Some(&quiet.chief), ""),
    )
    .await;
    assert!(answered_lines(&off).is_empty());
}

#[test]
fn the_tooling_look_is_switched_on_by_its_variable_set_to_one_and_by_nothing_else() {
    use std::ffi::OsStr;
    assert_eq!(calls::TOOLING_API_CALLS, "CONSENSFLOW_TOOLING_API_CALLS");
    assert!(calls::switched_on(Some(OsStr::new("1"))));
    for other in [
        None,
        Some(OsStr::new("")),
        Some(OsStr::new("0")),
        Some(OsStr::new("yes")),
    ] {
        assert!(!calls::switched_on(other), "{other:?}");
    }
}
