//! The order of a request's checks, end to end through [`handle`], as Node
//! has them: the screens first, then the window's token, then the routes, so
//! that a path that is no route at all, from a caller with no valid token, is
//! 401 and not 404.

use hyper::Method;
use serde_json::{json, Value};

use super::*;
use crate::testing::{request, said, scene, Scene};

fn screens() -> Screens {
    Screens {
        token: "the-ui-token".to_owned(),
        on_roster_change: Rc::new(|| Ok(())),
    }
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
        (Method::GET, "/"),
        (Method::GET, "/api/agents"),
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
async fn until_the_screens_land_their_paths_are_answered_as_any_other_unknown_path() {
    let scene = scene();
    for target in [
        "/",
        "/harnesses",
        "/api/agents",
        "/api/agents/mybuilder",
        "/api/preferences",
    ] {
        let (status, _) = through(&scene, Method::GET, target, Some("the-ui-token"), "").await;
        assert_eq!(status, 401, "the UI token is no window's: {target}");
        let (status, body) = through(&scene, Method::GET, target, Some(&scene.zeus), "").await;
        assert_eq!(status, 404, "{target}");
        assert_eq!(body["error"], "unknown-route");
    }
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
