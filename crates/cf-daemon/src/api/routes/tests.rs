//! The route table, held to Node's: which request is which route, in what
//! order they are matched, and that nothing Node routes is left out.

use super::*;
use crate::testing::{request, said, scene, Scene};

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

/// A route Node has no twin of: a door says it handed its answer over.
#[test]
fn the_receipt_of_an_answer_is_a_post_to_its_number_and_nothing_else_is() {
    assert_eq!(
        route(Method::POST, "/api/answers/12/receipt"),
        Some(Route::AnswerReceipt {
            id: "12".to_owned()
        })
    );
    for (method, path) in [
        (Method::GET, "/api/answers/12/receipt"),
        (Method::PUT, "/api/answers/12/receipt"),
        (Method::POST, "/api/answers/12"),
        (Method::POST, "/api/answers/12/"),
        (Method::POST, "/api/answers//receipt"),
        (Method::POST, "/api/answers/x/receipt"),
        (Method::POST, "/api/answers/12x/receipt"),
        (Method::POST, "/api/answers/12/receipt/"),
        (Method::POST, "/api/answers/12/Receipt"),
        (Method::POST, "/api/answers/12/receipts"),
    ] {
        assert_eq!(route(method.clone(), path), None, "{method} {path}");
    }
}

/// The other: `cf` says which answers it wrote whole.
#[test]
fn the_read_of_answers_is_a_post_to_read_and_nothing_else_is() {
    assert_eq!(
        route(Method::POST, "/api/answers/read"),
        Some(Route::AnswersRead)
    );
    for (method, path) in [
        (Method::GET, "/api/answers/read"),
        (Method::PUT, "/api/answers/read"),
        (Method::POST, "/api/answers/read/"),
        (Method::POST, "/api/answers/Read"),
        (Method::POST, "/api/answers/reads"),
        (Method::POST, "/api/answers/read/receipt"),
    ] {
        assert_eq!(route(method.clone(), path), None, "{method} {path}");
    }
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

/// Every route has a handler that answers it, as its own and not as a route
/// there is none of: what a window with nothing to its name is told is the
/// route's own (a refusal, an empty answer, an unknown task), never
/// `unknown-route`.
#[tokio::test]
async fn every_route_is_answered_by_a_handler_of_its_own() {
    let scene = scene();
    let caller = crate::api::callers::caller_of(
        &scene.context,
        &request(Method::GET, "/api/whoami", Some(&scene.zeus), ""),
    )
    .unwrap();
    for (method, path, route, status) in [
        (Method::GET, "/api/whoami", Route::Whoami, 200),
        (Method::GET, "/api/history", Route::History, 403),
        (Method::GET, "/api/staff", Route::Staff, 200),
        (Method::GET, "/api/tasks", Route::Tasks, 200),
        (Method::POST, "/api/tasks", Route::CreateTask, 403),
        (
            Method::GET,
            "/api/tasks/3",
            Route::Task {
                number: "3".to_owned(),
                action: None,
            },
            404,
        ),
        (Method::GET, "/api/inbox", Route::Inbox, 200),
        (
            Method::GET,
            "/api/inbox/9",
            Route::Message { id: "9".to_owned() },
            404,
        ),
        (Method::POST, "/api/questions", Route::AskQuestion, 400),
        (Method::POST, "/api/notes", Route::Note, 400),
        (
            Method::GET,
            "/api/questions/9",
            Route::Question { id: "9".to_owned() },
            404,
        ),
        (Method::POST, "/api/answers", Route::Answers, 404),
        (
            Method::POST,
            "/api/answers/9/receipt",
            Route::AnswerReceipt { id: "9".to_owned() },
            400,
        ),
        (Method::POST, "/api/answers/read", Route::AnswersRead, 400),
    ] {
        let asked = request(method.clone(), path, Some(&scene.zeus), "");
        let (got, body) = said(dispatch(&scene.context, &caller, route, asked).await);
        assert_eq!(got, status, "{method} {path}: {body}");
        assert_ne!(body["error"], "unknown-route", "{method} {path}");
    }
}

/// What the route tests stand on: the API as a client reaches it, and the
/// board's state to start from.
pub(super) mod support {

    use bytes::Bytes;
    use cf_ledger::{MessageView, NewNote, NewQuestion, NewTask};
    use futures_util::stream;
    use serde_json::{json, Value};

    use super::{request, said, Method, Scene};
    use crate::api::body::Body;
    use crate::api::request::Request;

    /// `method target` from the window of `token`, with `body` as its text:
    /// through the API's own checks, as a client sends it. The answer's
    /// status and JSON.
    pub(in crate::api::routes) async fn api(
        scene: &Scene,
        method: Method,
        target: &str,
        token: &str,
        body: &str,
    ) -> (u16, Value) {
        through(scene, request(method, target, Some(token), body)).await
    }

    /// A request of this shape, whose body is `body`: for a body that does
    /// something as it is read.
    pub(in crate::api::routes) fn with_body(
        method: Method,
        target: &str,
        token: &str,
        body: Body,
    ) -> Request {
        Request::new(method, target, Some(format!("Bearer {token}")), body).unwrap()
    }

    /// `text`, which says `meanwhile` is done as it is read: what Node did
    /// between its view of the board and its use of it, with the board.
    pub(in crate::api::routes) fn read_while(
        text: &str,
        meanwhile: impl FnOnce() + 'static,
    ) -> Body {
        let text = text.to_owned();
        Body::new(stream::once(async move {
            meanwhile();
            Ok(Bytes::from(text))
        }))
    }

    pub(in crate::api::routes) async fn through(scene: &Scene, asked: Request) -> (u16, Value) {
        let screens = crate::screens::testing::inert();
        said(crate::api::handle(&scene.context, &screens, asked).await)
    }

    /// A task the chief gave `zeus` by name and its window took: working.
    pub(in crate::api::routes) fn working_task(scene: &Scene) -> i64 {
        let mut ledger = scene.context.ledger.borrow_mut();
        let created = ledger
            .create_task(
                scene.project.id,
                &NewTask {
                    from: "chief".to_owned(),
                    to: Some("zeus".to_owned()),
                    body: "Parser".to_owned(),
                    ..NewTask::default()
                },
            )
            .unwrap();
        let brief = created.message.unwrap().id;
        ledger.begin_delivery(brief).unwrap();
        ledger
            .confirm_delivery(brief, Some(&json!({ "item": "test" })))
            .unwrap();
        created.task.number
    }

    /// [`working_task`], and a question `zeus` asked the chief through its
    /// harness's tool on it, which waits for a choice: the task works, and the
    /// question's door is open.
    pub(in crate::api::routes) fn working_question(scene: &Scene) -> MessageView {
        let number = working_task(scene);
        question_on(scene, number)
    }

    /// [`working_question`], and the chief's answer to it in `words`: the task
    /// waits, and the answer is queued for `zeus`.
    pub(in crate::api::routes) fn answered_question(scene: &Scene, words: &str) -> MessageView {
        let question = working_question(scene);
        answer_in_words(scene, question.id, words)
    }

    /// A question in `words` `zeus` asked the chief about task `number`, with
    /// no options to choose from: it is answered in words.
    pub(in crate::api::routes) fn plain_question_on(
        scene: &Scene,
        number: i64,
        words: &str,
    ) -> MessageView {
        let asked = NewQuestion {
            from: Some("zeus".to_owned()),
            to: "chief".to_owned(),
            body: Some(words.to_owned()),
            task: Some(number),
            ..NewQuestion::default()
        };
        scene
            .context
            .ledger
            .borrow_mut()
            .ask(scene.project.id, &asked)
            .unwrap()
    }

    /// The chief's note to `zeus` about task `number`.
    pub(in crate::api::routes) fn note_for_zeus(
        scene: &Scene,
        number: i64,
        words: &str,
    ) -> MessageView {
        let note = NewNote {
            from: Some("chief".to_owned()),
            to: "zeus".to_owned(),
            body: words.to_owned(),
            task: Some(number),
        };
        scene
            .context
            .ledger
            .borrow_mut()
            .note(scene.project.id, &note)
            .unwrap()
    }

    /// The chief's answer to question `question` in `words`.
    pub(in crate::api::routes) fn answer_in_words(
        scene: &Scene,
        question: i64,
        words: &str,
    ) -> MessageView {
        let chief = scene.id("chief");
        scene
            .context
            .ledger
            .borrow_mut()
            .answer(question, chief, Some(&json!(words)), None)
            .unwrap()
    }

    /// A question `zeus` asked the chief through its harness's tool about
    /// task `number`, which waits for a choice.
    pub(in crate::api::routes) fn question_on(scene: &Scene, number: i64) -> MessageView {
        scene
            .context
            .ledger
            .borrow_mut()
            .ask(
                scene.project.id,
                &NewQuestion {
                    from: Some("zeus".to_owned()),
                    to: "chief".to_owned(),
                    task: Some(number),
                    questions: Some(json!([{
                        "question": "Which?", "header": "H",
                        "options": [{ "label": "red" }, { "label": "blue" }], "multiple": false,
                    }])),
                    ..NewQuestion::default()
                },
            )
            .unwrap()
    }

    /// A task the chief put on the board for a standard worker, which waits.
    pub(in crate::api::routes) fn open_task(scene: &Scene) -> i64 {
        open_task_of(scene, "Lexer")
    }

    /// [`open_task`], with `brief` as its words.
    pub(in crate::api::routes) fn open_task_of(scene: &Scene, brief: &str) -> i64 {
        let created = scene
            .context
            .ledger
            .borrow_mut()
            .create_task(
                scene.project.id,
                &NewTask {
                    from: "chief".to_owned(),
                    pool: Some("worker".to_owned()),
                    tier: Some("standard".to_owned()),
                    body: brief.to_owned(),
                    ..NewTask::default()
                },
            )
            .unwrap();
        created.task.number
    }

    /// A task the daemon gave a window of `zeus` of its own (a session), which
    /// has finished it: the one a follow-up goes back to.
    pub(in crate::api::routes) fn finished_session_task(scene: &Scene) -> i64 {
        let number = open_task(scene);
        let mut ledger = scene.context.ledger.borrow_mut();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let zeus = project
            .participants
            .iter()
            .find(|participant| participant.handle == "zeus")
            .unwrap()
            .id;
        let moved = ledger.assign_task(scene.project.id, number, zeus).unwrap();
        let brief = moved.message.unwrap().id;
        ledger.begin_delivery(brief).unwrap();
        ledger
            .confirm_delivery(brief, Some(&json!({ "item": "test" })))
            .unwrap();
        ledger
            .record_result(scene.project.id, number, "Done.")
            .unwrap();
        number
    }

    /// With the human's approval required, a task opened for a standard
    /// worker and given to `zeus` by the daemon: its brief waits at the gate.
    /// The token of the session the daemon opened, and the brief's number.
    pub(in crate::api::routes) fn gated_brief(scene: &Scene) -> (String, i64) {
        gated_brief_of(scene, "Lexer")
    }

    /// [`gated_brief`], with `brief` as its words.
    pub(in crate::api::routes) fn gated_brief_of(scene: &Scene, brief: &str) -> (String, i64) {
        scene
            .context
            .ledger
            .borrow_mut()
            .set_gate(scene.project.id, true)
            .unwrap();
        let number = open_task_of(scene, brief);
        let mut ledger = scene.context.ledger.borrow_mut();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let zeus = project
            .participants
            .iter()
            .find(|participant| participant.handle == "zeus")
            .unwrap();
        let moved = ledger
            .assign_task(scene.project.id, number, zeus.id)
            .unwrap();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let session = project
            .participants
            .iter()
            .find(|participant| Some(&participant.handle) == moved.task.assignee.as_ref())
            .unwrap();
        let token = scene.context.credentials.issue(project.id, session.id);
        (token, moved.message.unwrap().id)
    }

    /// The state task `number` is in now.
    pub(in crate::api::routes) fn state_of(scene: &Scene, number: i64) -> String {
        scene
            .context
            .ledger
            .borrow()
            .task(scene.project.id, number)
            .unwrap()
            .unwrap()
            .task
            .state
    }
}
