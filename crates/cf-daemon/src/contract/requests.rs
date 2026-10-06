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
//! callbacks, each of whose chains is whole.
//!
//! hyper serves the requests of one connection one at a time, so requests
//! written together are answered in order, the second's work begun once the
//! first is answered; Node's handlers began as each head came. No client the
//! daemon has pipelines (`cf` and the pages wait for each answer), and the
//! order the work runs in is the order the requests came.

use std::rc::Rc;
use std::time::Duration;

use cf_engine::Dispatcher;
use serde_json::{json, Value};

use super::client::{head, request, Client};
use super::reference::closed_then_resumed;
use super::rig::{Pieces, Rig, Transport};
use super::{assert_whole, chain, scene, settle, Order, TURNS};
use crate::api::answer::{Answer, Failure};
use crate::api::context::Closing;
use crate::api::request::Request;
use crate::api::{Api, Handler};

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
