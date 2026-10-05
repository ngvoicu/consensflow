//! The route table, held to Node's: which request is which route, in what
//! order they are matched, and that nothing Node routes is left out.

use super::*;
use crate::testing::{request, said, scene};

fn route(method: Method, path: &str) -> Option<Route> {
    recognize(&method, path)
}

fn task(number: &str, action: Option<TaskAction>) -> Option<Route> {
    Some(Route::Task {
        number: number.to_owned(),
        action,
    })
}

#[test]
fn each_route_is_the_one_node_matches_for_its_method_and_path() {
    assert_eq!(route(Method::GET, "/api/whoami"), Some(Route::Whoami));
    assert_eq!(route(Method::GET, "/api/history"), Some(Route::History));
    assert_eq!(route(Method::GET, "/api/staff"), Some(Route::Staff));
    assert_eq!(route(Method::GET, "/api/tasks"), Some(Route::Tasks));
    assert_eq!(route(Method::POST, "/api/tasks"), Some(Route::CreateTask));
    assert_eq!(route(Method::GET, "/api/inbox"), Some(Route::Inbox));
    assert_eq!(
        route(Method::GET, "/api/inbox/12"),
        Some(Route::Message {
            id: "12".to_owned()
        })
    );
    assert_eq!(
        route(Method::POST, "/api/questions"),
        Some(Route::AskQuestion)
    );
    assert_eq!(route(Method::POST, "/api/notes"), Some(Route::Note));
    assert_eq!(
        route(Method::GET, "/api/questions/7"),
        Some(Route::Question { id: "7".to_owned() })
    );
    assert_eq!(route(Method::POST, "/api/answers"), Some(Route::Answers));
}

#[test]
fn a_task_route_is_the_task_and_the_word_after_it_whatever_the_method() {
    assert_eq!(route(Method::GET, "/api/tasks/5"), task("5", None));
    assert_eq!(route(Method::DELETE, "/api/tasks/5"), task("5", None));
    assert_eq!(route(Method::PUT, "/api/tasks/5"), task("5", None));
    for (word, action) in [
        ("done", TaskAction::Done),
        ("accept", TaskAction::Accept),
        ("reopen", TaskAction::Reopen),
        ("cancel", TaskAction::Cancel),
        ("pause", TaskAction::Pause),
        ("resume", TaskAction::Resume),
        ("tell", TaskAction::Tell),
        ("transcript", TaskAction::Transcript),
    ] {
        assert_eq!(
            route(Method::POST, &format!("/api/tasks/12/{word}")),
            task("12", Some(action)),
            "{word}"
        );
        assert_eq!(
            route(Method::GET, &format!("/api/tasks/12/{word}")),
            task("12", Some(action)),
            "the route is the same for any method: {word}"
        );
    }
    // The digits stay as the path had them.
    assert_eq!(route(Method::GET, "/api/tasks/007"), task("007", None));
}

#[test]
fn a_path_that_is_not_the_regular_expression_of_node_is_no_task_route() {
    for path in [
        "/api/tasks/",
        "/api/tasks/abc",
        "/api/tasks/5x",
        "/api/tasks/-5",
        "/api/tasks/5.0",
        "/api/tasks/5/",
        "/api/tasks/5/nothing",
        "/api/tasks/5/DONE",
        "/api/tasks/5/done/",
        "/api/tasks/5/done/more",
        "/api/tasks//done",
        "/api/tasks/\u{665}",
        "/api/tasks/5%2Fdone",
        "/api/taskss/5",
        "/api/task/5",
    ] {
        assert_eq!(route(Method::POST, path), None, "{path}");
    }
}

#[test]
fn only_a_get_reads_a_message_or_a_door_and_only_a_post_gives_the_rest() {
    for (method, path) in [
        (Method::POST, "/api/inbox/5"),
        (Method::DELETE, "/api/inbox/5"),
        (Method::POST, "/api/inbox"),
        (Method::GET, "/api/inbox/5x"),
        (Method::GET, "/api/inbox/"),
        (Method::POST, "/api/questions/7"),
        (Method::GET, "/api/questions"),
        (Method::GET, "/api/questions/"),
        (Method::GET, "/api/questions/7a"),
        (Method::GET, "/api/answers"),
        (Method::GET, "/api/notes"),
        (Method::POST, "/api/whoami"),
        (Method::POST, "/api/history"),
        (Method::POST, "/api/staff"),
        (Method::DELETE, "/api/tasks"),
        (Method::PUT, "/api/answers"),
    ] {
        assert_eq!(route(method.clone(), path), None, "{method} {path}");
    }
}

#[test]
fn a_path_is_matched_whole_and_as_written() {
    for path in [
        "/api/whoami/",
        "/api/whoami/x",
        "/API/whoami",
        "/api/Whoami",
        "/api",
        "/",
        "",
        "/api/agents",
        "/api/preferences",
        "/harnesses",
        "api/whoami",
    ] {
        assert_eq!(route(Method::GET, path), None, "{path}");
    }
}

/// Every route `api.js` matches by its `at === 'METHOD /path'` is a route
/// here: a route Node gains cannot be left out unseen.
#[test]
fn every_route_node_names_by_its_method_and_path_is_one() {
    let source = include_str!("../../../../../src/core/api.js");
    let mut named = 0;
    for piece in source.split("at === '").skip(1) {
        let Some((route, _)) = piece.split_once('\'') else {
            continue;
        };
        let (method, path) = route.split_once(' ').unwrap();
        let method = Method::from_bytes(method.as_bytes()).unwrap();
        assert!(recognize(&method, path).is_some(), "{route}");
        named += 1;
    }
    assert_eq!(
        named, 9,
        "api.js names nine routes by their method and path"
    );
}

#[tokio::test]
async fn a_route_with_no_handler_yet_answers_as_node_answers_one_it_has_none_for() {
    let scene = scene();
    let caller = crate::api::callers::caller_of(
        &scene.context,
        &request(Method::GET, "/api/whoami", Some(&scene.zeus), ""),
    )
    .unwrap();
    for (method, path, route) in [
        (Method::GET, "/api/whoami", Route::Whoami),
        (Method::GET, "/api/history", Route::History),
        (Method::GET, "/api/staff", Route::Staff),
        (Method::GET, "/api/tasks", Route::Tasks),
        (Method::POST, "/api/tasks", Route::CreateTask),
        (
            Method::GET,
            "/api/tasks/3",
            Route::Task {
                number: "3".to_owned(),
                action: None,
            },
        ),
        (Method::GET, "/api/inbox", Route::Inbox),
        (
            Method::GET,
            "/api/inbox/9",
            Route::Message { id: "9".to_owned() },
        ),
        (Method::POST, "/api/questions", Route::AskQuestion),
        (Method::POST, "/api/notes", Route::Note),
    ] {
        let asked = request(method.clone(), path, Some(&scene.zeus), "");
        let (status, body) = said(dispatch(&scene.context, &caller, route, asked).await);
        assert_eq!(status, 404, "{method} {path}");
        assert_eq!(
            body,
            serde_json::json!({
                "error": "unknown-route",
                "message": format!("no such command: {method} {path}")
            })
        );
    }
    assert_eq!(scene.kicks.get(), 0);
}
