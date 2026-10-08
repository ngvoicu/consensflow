//! Real hyper requests (`api::Api`, on sockets) whose handler does real
//! engine work: the human's Close and Resume of a project, as the page's
//! operations do them.
//!
//! A request's handler is begun where the request is and the rest is the
//! executor's, so the engine's work does not depend on its client: a body that
//! comes with the head or in pieces, two requests one after the other on one
//! connection, and a client that leaves once the work began are all the same
//! work, and the events it logs are the kit's run of the same operations
//! ([`super::reference`]). Two requests whose bodies end at once are two
//! callbacks, each of whose chains is whole; and a body that ends is a callback
//! of its own, which its handler goes on from before a window's exit that is
//! runnable already (an agent's `done` as its pane exits).
//!
//! hyper serves the requests of one connection one at a time, so requests
//! written together are answered in order, the second's work begun once the
//! first is answered; Node's handlers began as each head came. No client the
//! daemon has pipelines (`cf` and the pages wait for each answer), and the
//! order the work runs in is the order the requests came.

use std::rc::Rc;
use std::time::Duration;

use cf_engine::runtime::begin;
use cf_engine::Dispatcher;
use hyper::Method;
use serde_json::{json, Value};

use super::client::{head, request, Client};
use super::host::{answer, pane_of};
use super::reference::{closed_then_resumed, exit_then_result, result_then_exit};
use super::rig::{Pieces, Rig, Transport};
use super::{assert_whole, chain, scene, settle, Order, TURNS};
use crate::api::answer::{Answer, Failure};
use crate::api::body::Body;
use crate::api::callers::caller_of;
use crate::api::context::{Closing, Context};
use crate::api::credentials::Credentials;
use crate::api::request::Request;
use crate::api::{routes, Api, Handler};
use crate::files::{Log, Trace};
use crate::testing::{sent_body, NoRows};

/// A handler that closes or resumes the project its body names: the engine's
/// work, begun in the request's first part.
fn handler(dispatcher: &Rc<Dispatcher>) -> Rc<Handler> {
    let dispatcher = Rc::clone(dispatcher);
    Rc::new(move |mut request: Request| {
        let dispatcher = Rc::clone(&dispatcher);
        Box::pin(async move {
            let body = request.json().await?;
            let project = body["project"].as_i64().ok_or_else(|| {
                Failure::refuse(400, "bad-request", "a project is named by its number")
            })?;
            let project = match request.at().as_str() {
                "POST /close" => dispatcher.close_project(project).await?,
                _ => dispatcher.resume_project(project).await?,
            };
            Ok(Answer::ok(
                json!({ "state": project.map(|project| project.state) }),
            ))
        })
    })
}

/// A project whose chief and worker have windows open, the worker at work on
/// a task, and the API in front of its engine.
async fn serving() -> (Rig, Api, i64) {
    let mut rig = Rig::new().await;
    let project = rig.open_project(&["zeus"]).await;
    rig.give(project.id, "zeus", "Parser");
    rig.passes(2).await;
    assert_eq!(rig.task_state(project.id, 1), "working");
    rig.mark();
    let api = Api::start(
        handler(&rig.dispatcher),
        Closing::new(),
        Rc::clone(&rig.pieces.spawn),
    )
    .await
    .expect("the front listens");
    (rig, api, project.id)
}

/// The reply to the request a client sent, while the pane host answers what
/// the engine asks of it meanwhile.
async fn answered(rig: &mut Rig, mut client: Client) -> (Client, (u16, String)) {
    let reading = tokio::task::spawn_local(async move {
        let reply = client.reply().await;
        (client, reply)
    });
    rig.serve_until(|| reading.is_finished()).await;
    reading.await.expect("the reply was read")
}

/// What the daemon wrote of what it did, which no request may have failed.
fn log_of(rig: &Rig) -> String {
    std::fs::read_to_string(rig.pieces.home.path().join("daemon.log")).unwrap_or_default()
}

#[tokio::test]
async fn a_body_that_comes_with_its_head_is_the_work_the_kit_did() {
    scene(async {
        let (mut rig, api, project) = serving().await;
        let mut client = Client::connect(&api).await;
        client.send(request("/close", project).as_bytes()).await;
        let (client, (status, body)) = answered(&mut rig, client).await;
        assert_eq!((status, body.as_str()), (200, r#"{"state":"suspended"}"#));
        rig.quiet().await;
        assert_eq!(rig.events(), closed_then_resumed().0);
        assert_eq!(log_of(&rig), "", "nothing failed");
        drop(client);
        api.close().await;
    })
    .await;
}

#[tokio::test]
async fn a_body_that_comes_in_pieces_is_the_same_work_begun_when_its_head_came() {
    scene(async {
        let (mut rig, api, project) = serving().await;
        let mut client = Client::connect(&api).await;
        let body = json!({ "project": project }).to_string();
        let (first, rest) = body.split_at(4);
        let (second, last) = rest.split_at(5);
        client.send(head("/close", body.len()).as_bytes()).await;
        settle().await;
        client.send(first.as_bytes()).await;
        settle().await;
        client.send(second.as_bytes()).await;
        settle().await;
        // Nothing is done until the body is whole.
        assert_eq!(rig.events(), []);
        client.send(last.as_bytes()).await;
        let (client, (status, body)) = answered(&mut rig, client).await;
        assert_eq!((status, body.as_str()), (200, r#"{"state":"suspended"}"#));
        rig.quiet().await;
        assert_eq!(rig.events(), closed_then_resumed().0);
        drop(client);
        api.close().await;
    })
    .await;
}

#[tokio::test]
async fn two_requests_on_one_connection_are_two_operations_the_second_after_the_first() {
    scene(async {
        let (mut rig, api, project) = serving().await;
        let (closed, resumed) = closed_then_resumed();
        let mut client = Client::connect(&api).await;
        client.send(request("/close", project).as_bytes()).await;
        let (mut client, (status, body)) = answered(&mut rig, client).await;
        assert_eq!((status, body.as_str()), (200, r#"{"state":"suspended"}"#));
        rig.quiet().await;
        assert_eq!(rig.events(), closed);
        client.send(request("/resume", project).as_bytes()).await;
        let (client, (status, body)) = answered(&mut rig, client).await;
        assert_eq!((status, body.as_str()), (200, r#"{"state":"open"}"#));
        rig.quiet().await;
        assert_eq!(rig.events(), [closed, resumed].concat());
        drop(client);
        api.close().await;
    })
    .await;
}

#[tokio::test]
async fn two_requests_written_together_on_one_connection_are_answered_in_order_each_whole() {
    scene(async {
        let (mut rig, api, project) = serving().await;
        let (closed, resumed) = closed_then_resumed();
        let mut client = Client::connect(&api).await;
        client
            .send(
                [request("/close", project), request("/resume", project)]
                    .concat()
                    .as_bytes(),
            )
            .await;
        let (mut client, (status, body)) = answered(&mut rig, client).await;
        assert_eq!((status, body.as_str()), (200, r#"{"state":"suspended"}"#));
        let (status, body) = {
            let reading = tokio::task::spawn_local(async move { client.reply().await });
            rig.serve_until(|| reading.is_finished()).await;
            reading.await.expect("the second reply")
        };
        assert_eq!((status, body.as_str()), (200, r#"{"state":"open"}"#));
        rig.quiet().await;
        assert_eq!(rig.events(), [closed, resumed].concat());
        api.close().await;
    })
    .await;
}

#[tokio::test]
async fn a_client_that_leaves_once_the_work_began_leaves_the_work_to_finish() {
    scene(async {
        let (mut rig, api, project) = serving().await;
        let mut client = Client::connect(&api).await;
        client.send(request("/close", project).as_bytes()).await;
        // The work has begun: the engine has asked the host to close the
        // windows, and the host has not answered.
        let mut kills = Vec::new();
        for _ in 0..4 {
            kills.extend(rig.pieces.host.serve(|frame| frame.op == "pane.kill").await);
        }
        assert!(!kills.is_empty(), "the windows are being closed");
        drop(client);
        settle().await;
        for kill in &kills {
            rig.pieces.host.write(&super::host::answer(kill)).await;
        }
        rig.quiet().await;
        assert_eq!(rig.events(), closed_then_resumed().0, "the work finished");
        assert_eq!(rig.task_state(project, 1), "paused");
        assert_eq!(log_of(&rig), "", "nothing failed");
        api.close().await;
    })
    .await;
}

#[tokio::test]
async fn two_requests_whose_bodies_end_at_once_are_two_callbacks_each_chain_whole() {
    scene(async {
        let pieces = Pieces::new(Transport::Memory).await;
        let order = Order::default();
        let ran = Rc::clone(&order);
        let handler: Rc<Handler> = Rc::new(move |mut request| {
            let order = Rc::clone(&ran);
            Box::pin(async move {
                let body = request.json().await?;
                let name = match body["name"].as_str() {
                    Some("a") => "a",
                    _ => "b",
                };
                chain(order, name, TURNS).await;
                Ok(Answer::ok(Value::Null))
            })
        });
        let api = Api::start(handler, Closing::new(), Rc::clone(&pieces.spawn))
            .await
            .expect("the front listens");
        // Two connections, each with its request's head and half its body.
        let mut clients = Vec::new();
        for name in ["a", "b"] {
            let body = json!({ "name": name }).to_string();
            let (first, rest) = body.split_at(body.len() / 2);
            let mut client = Client::connect(&api).await;
            client.send(head("/x", body.len()).as_bytes()).await;
            client.send(first.as_bytes()).await;
            clients.push((client, rest.to_owned()));
        }
        settle().await;
        // The rest of both bodies, written together: both sockets are ready
        // the next time tokio looks, and both connections are polled in it.
        for (client, rest) in &mut clients {
            client.send(rest.as_bytes()).await;
        }
        std::thread::sleep(Duration::from_millis(50));
        settle().await;
        assert_whole(&order, [("a", TURNS), ("b", TURNS)]);
        for (mut client, _) in clients {
            assert_eq!(client.reply().await.0, 200);
        }
        api.close().await;
    })
    .await;
}

#[test]
fn the_kit_tells_a_result_before_an_exit_from_an_exit_before_a_result_by_what_the_ledger_took() {
    let (recorded, before) = result_then_exit();
    let (refused, after) = exit_then_result();
    assert!(recorded, "the result came while the task worked");
    assert!(!refused, "the result came to a paused task");
    assert_eq!(before.task, "done");
    assert_eq!(after.task, "paused");
}

/// Zeus is at work on a task, in a window whose pane the host opened; and what
/// a handler of the agents' API is given is over his rig's ledger, with his
/// window's token in it.
async fn zeus_at_work() -> (Rig, Rc<Context>, String, Value) {
    let mut rig = Rig::new().await;
    let project = rig.open_project(&["zeus"]).await;
    rig.give(project.id, "zeus", "Parser");
    // Everything the pass asks of the host is answered but zeus's open, whose
    // pane is what his exit will name.
    let pass = rig.pass().await;
    let mut opens = Vec::new();
    for _ in 0..4 {
        opens.extend(rig.pieces.host.serve(|frame| frame.op == "pane.open").await);
    }
    let [open] = &opens[..] else {
        panic!("zeus's window is asked for: {opens:?}");
    };
    let pane = pane_of(open);
    rig.pieces.host.write(&answer(open)).await;
    rig.serve_until(|| pass.ended()).await;
    pass.await.expect("a pass");
    rig.quiet().await;
    rig.passes(1).await;
    assert_eq!(rig.task_state(project.id, 1), "working");
    rig.mark();
    let zeus = rig
        .ledger
        .borrow()
        .project(project.id)
        .expect("the ledger read")
        .expect("the project")
        .participants
        .into_iter()
        .find(|participant| participant.handle == "zeus")
        .expect("zeus")
        .id;
    let credentials = Credentials::new();
    let token = credentials.issue(project.id, zeus);
    let home = rig.pieces.home.path();
    let context = Rc::new(Context {
        ledger: Rc::clone(&rig.ledger),
        credentials: Rc::new(credentials),
        kick: Rc::new(|| {}),
        closing: Closing::new(),
        roster: Rc::new(NoRows),
        log: Rc::new(Log::new(home)),
        trace: Rc::new(Trace::new(home)),
    });
    (rig, context, token, pane)
}

#[tokio::test]
async fn a_result_whose_body_ends_as_its_window_exits_is_recorded_before_the_exit() {
    let (recorded, reference) = result_then_exit();
    assert!(recorded);
    scene(async {
        let (mut rig, context, token, pane) = zeus_at_work().await;
        // `POST /api/tasks/1/done` as zeus's window sends it: the caller by its
        // token, then the route, whose body comes through the daemon's own pump
        // as the test sends it.
        let (sending, incoming) = sent_body();
        let draining = Rc::clone(&rig.pieces.spawn);
        let (body, pump) = Body::pumped(incoming, move || draining.drain());
        tokio::task::spawn_local(pump);
        let request = Request::new(
            Method::POST,
            "/api/tasks/1/done",
            Some(format!("Bearer {token}")),
            body,
        )
        .expect("a request");
        let handling = Rc::clone(&context);
        let begun = begin(&*rig.pieces.spawn, async move {
            let caller = caller_of(&handling, &request)?;
            let route = routes::recognize(&request.method, &request.path)
                .ok_or_else(|| request.unknown_route())?;
            routes::dispatch(&handling, &caller, route, request).await
        })
        .await;
        rig.pieces.spawn.drain();
        settle().await;
        assert!(!begun.ended(), "the handler waits for its body");
        // The last of the body and its end come in one wake of the pump, and
        // zeus's window exits right behind them: the bridge's reader is
        // runnable when the body ends.
        sending.chunk(br#"{"body":"Parser done"}"#);
        drop(sending);
        let exit = rig.pieces.host.exit(&pane);
        rig.pieces.host.write(&exit).await;
        rig.quiet().await;
        let answered = begun.await.expect("the result was recorded");
        assert_eq!(answered.status, 200);
        assert_eq!(rig.task_state(1, 1), reference.task);
        assert_eq!(rig.events(), reference.events);
        assert_eq!(rig.calls("started"), reference.started);
    })
    .await;
}
