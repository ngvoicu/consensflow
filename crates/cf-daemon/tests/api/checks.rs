//! What the player compares, once a step has been made: the answer to an
//! exchange of the test's own, as bytes; what the API did on the way (the
//! wake-ups, the events, the clock, the roster); and the requests `cf` made,
//! each whole, and how they were answered: as the answer passed the relay, its
//! status, its type and its bytes, as the test's own exchanges' are held.

use serde_json::{json, Value};

use crate::frames::Sent;
use crate::front::Reply;
use crate::names::Names;
use crate::relay::Relay;
use crate::rig::Rig;
use crate::support::compare::{compare, differs};

/// The answer to an exchange of the test's own against `response`, which is
/// the `response` Node recorded, or the screens' where the exchange is one of
/// theirs: its status, the type it carried and its bytes; and then what the API
/// did to give it, against what `step` says it did.
pub fn exchange(
    rig: &Rig,
    step: &Value,
    response: &Value,
    reply: &Reply,
    events: &[Value],
    kicks: usize,
) -> Result<(), String> {
    if let Some(why) = reply.head_differs(response) {
        return Err(why);
    }
    let expected = response["body"].as_str().unwrap_or_default();
    if let Some(why) = differs("the answer", &reply.body, expected) {
        return Err(why);
    }
    effects(rig, &[step], events, kicks)
}

/// What the API did on the way, against what Node's did: the wake-ups, the
/// events its ledger logged, the clock and the names it drew, the agents it
/// asked the roster for.
pub fn effects(rig: &Rig, steps: &[&Value], events: &[Value], kicks: usize) -> Result<(), String> {
    let expected: usize = steps
        .iter()
        .map(|step| usize::try_from(step["kicks"].as_u64().unwrap_or(0)).unwrap_or(usize::MAX))
        .sum();
    if kicks != expected {
        return Err(format!(
            "woke the dispatcher {kicks} times, Node {expected}"
        ));
    }
    let recorded: Vec<Value> = steps
        .iter()
        .flat_map(|step| step["events"].as_array().cloned().unwrap_or_default())
        .collect();
    if let Some(why) = compare("the events it logged", &json!(events), &json!(recorded)) {
        return Err(why);
    }
    if let Some(why) = rig.ledger.settle(steps) {
        return Err(why);
    }
    let agents: Vec<Value> = steps
        .iter()
        .flat_map(|step| step["seams"].as_array().cloned().unwrap_or_default())
        .map(|call| call["args"][0].clone())
        .collect();
    let asked = json!(rig.take_asked());
    compare("the agents it asked for", &asked, &json!(agents)).map_or(Ok(()), Err)
}

/// The requests a run of `cf` wrote, as the relay saw them pass, against the
/// exchanges Node recorded for it: no more and no fewer, and each whole: its
/// method, its target with its query, its authorization, its content type and
/// its body, byte for byte.
pub fn sent(relay: &Relay, names: &Names, run: &[Value]) -> Result<(), String> {
    let (written, unread) = relay.taken();
    if let Some(why) = unread.first() {
        return Err(format!(
            "the relay could not read what the run sent or was given: {why}"
        ));
    }
    if written.len() != run.len() {
        let targets: Vec<&str> = written.iter().map(|sent| sent.target.as_str()).collect();
        return Err(format!(
            "made {} requests, {targets:?}, Node's run made {}",
            written.len(),
            run.len()
        ));
    }
    for (written, step) in written.iter().zip(run) {
        let request = &step["request"];
        let recorded = Sent {
            method: request["method"].as_str().unwrap_or_default().to_owned(),
            target: names.put(request["target"].as_str().unwrap_or_default()),
            authorization: request["authorization"]
                .as_str()
                .map(|header| names.put(header)),
            content_type: request["contentType"].as_str().map(str::to_owned),
            body: request["body"]
                .as_str()
                .map(|body| body.as_bytes().to_vec())
                .unwrap_or_default(),
        };
        let id = &step["id"];
        let problems: Vec<String> = [
            (written.method != recorded.method)
                .then(|| format!("its method {}, Node {}", written.method, recorded.method)),
            differs("its target", &written.target, &recorded.target),
            (written.authorization != recorded.authorization).then(|| {
                format!(
                    "its authorization {:?}, Node {:?}",
                    written.authorization, recorded.authorization
                )
            }),
            (written.content_type != recorded.content_type).then(|| {
                format!(
                    "its content type {:?}, Node {:?}",
                    written.content_type, recorded.content_type
                )
            }),
            differs("its body", &written.body, &recorded.body),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !problems.is_empty() {
            return Err(format!(
                "exchange {id}, as `cf` wrote it: {}",
                problems.join("; ")
            ));
        }
    }
    Ok(())
}

/// An exchange `cf` made: the request the API took (what it was, whose window
/// it came from), and how it was answered, against `response`: the one Node
/// recorded, or the screens' where the exchange is theirs. `reply` is the
/// answer as it passed the relay, which is what `cf` was given: none if no
/// whole answer passed.
pub fn made(
    rig: &Rig,
    names: &Names,
    step: &Value,
    response: &Value,
    reply: Option<&Reply>,
) -> Result<(), String> {
    let id = usize::try_from(step["id"].as_u64().expect("an id")).expect("a small id");
    let seen = rig.seen.borrow();
    let Some(seen) = seen.get(id - 1) else {
        return Err(format!("exchange {id}: the API took no such request"));
    };
    let request = &step["request"];
    let target = request["target"].as_str().unwrap_or_default();
    let path = target.split('?').next().unwrap_or_default();
    let bearer = request["authorization"]
        .as_str()
        .and_then(|header| header.strip_prefix("Bearer "))
        .map(|token| names.put(token));
    let wanted = (request["method"].as_str().unwrap_or_default(), path);
    if (seen.method.as_str(), seen.path.as_str()) != wanted || seen.bearer != bearer {
        return Err(format!(
            "exchange {id}: took {} {} as {:?}, Node {} {} as {:?}",
            seen.method, seen.path, seen.bearer, wanted.0, wanted.1, bearer
        ));
    }
    answered(id, reply, response)
}

/// Why the answer `cf` was given to exchange `id` is not `response`: its
/// status, the type it carried and its bytes, held as the test's own exchanges
/// are (`exchange`). `cf` takes any 2xx and reads any type as JSON, so a
/// status or a type the server got wrong is seen here and nowhere else.
fn answered(id: usize, reply: Option<&Reply>, response: &Value) -> Result<(), String> {
    let reply = reply.ok_or_else(|| format!("exchange {id}: no whole answer passed the relay"))?;
    if let Some(why) = reply.head_differs(response) {
        return Err(format!("exchange {id}: {why}"));
    }
    let body = response["body"].as_str().unwrap_or_default();
    differs(&format!("exchange {id}'s answer"), &reply.body, body).map_or(Ok(()), Err)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::support::trace::locally;

    /// The exchange Node recorded of a `cf` request.
    fn recorded(
        target: &str,
        authorization: Option<&str>,
        kind: Option<&str>,
        body: Option<&str>,
    ) -> Value {
        json!({ "id": 1, "client": "cf", "run": 1, "request": {
            "method": if body.is_some() { "POST" } else { "GET" },
            "target": target, "authorization": authorization, "contentType": kind, "body": body,
        }})
    }

    /// The bytes of a request as `ureq` writes one.
    fn written(
        target: &str,
        authorization: &str,
        kind: Option<&str>,
        body: Option<&str>,
    ) -> String {
        let method = if body.is_some() { "POST" } else { "GET" };
        let kind = kind.map_or(String::new(), |kind| format!("Content-Type: {kind}\r\n"));
        let length = body.map_or(String::new(), |body| {
            format!("Content-Length: {}\r\n", body.len())
        });
        format!(
            "{method} {target} HTTP/1.1\r\nHost: x\r\nAuthorization: {authorization}\r\n{kind}{length}\r\n{}",
            body.unwrap_or_default()
        )
    }

    /// What `sent` says of a relay that was written `requests`, one after the
    /// other on one connection, against the exchanges `run`.
    fn said(requests: &[String], run: &[Value]) -> Result<(), String> {
        locally(async {
            let api = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let address = api.local_addr().unwrap().to_string();
            // An API that takes what it is sent and says nothing.
            tokio::task::spawn_local(async move {
                let _kept = api.accept().await;
                std::future::pending::<()>().await;
            });
            let relay = Relay::start(&address).await.unwrap();
            let mut client = TcpStream::connect(relay.address()).await.unwrap();
            for request in requests {
                client.write_all(request.as_bytes()).await.unwrap();
            }
            // The relay has read them when it has written them down.
            for _ in 0..500 {
                if relay.taken().0.len() == requests.len() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let mut names = Names::new("127.0.0.1:1");
            names.issued("T1", "tok-1".to_owned());
            sent(&relay, &names, run)
        })
    }

    const TRANSCRIPT: &str = "/api/tasks/1/transcript?last=10";
    const JSON: Option<&str> = Some("application/json");

    fn transcript() -> String {
        written(TRANSCRIPT, "Bearer tok-1", JSON, None)
    }

    #[test]
    fn a_request_is_as_the_trace_has_it_when_every_part_of_it_is() {
        let post = recorded(
            "/api/notes",
            Some("Bearer «token:T1»"),
            JSON,
            Some("{\"body\":\"x\"}"),
        );
        let run = [
            recorded(TRANSCRIPT, Some("Bearer «token:T1»"), JSON, None),
            post,
        ];
        let requests = [
            transcript(),
            written("/api/notes", "Bearer tok-1", JSON, Some("{\"body\":\"x\"}")),
        ];
        assert_eq!(said(&requests, &run), Ok(()));
    }

    #[test]
    fn a_part_that_is_not_the_traces_fails_the_run_though_the_api_would_answer_alike() {
        let get = |target, authorization, kind| recorded(target, authorization, kind, None);
        let bearer = Some("Bearer «token:T1»");
        let cases = [
            (
                "its target",
                written(
                    "/api/tasks/1/transcript?last=11",
                    "Bearer tok-1",
                    JSON,
                    None,
                ),
                get(TRANSCRIPT, bearer, JSON),
            ),
            (
                "its target",
                written("/api/tasks/1/transcript", "Bearer tok-1", JSON, None),
                get(TRANSCRIPT, bearer, JSON),
            ),
            (
                "its authorization",
                written(TRANSCRIPT, "Bearer other", JSON, None),
                get(TRANSCRIPT, bearer, JSON),
            ),
            (
                "its content type",
                written(TRANSCRIPT, "Bearer tok-1", None, None),
                get(TRANSCRIPT, bearer, JSON),
            ),
            (
                "its body",
                written(
                    "/api/notes",
                    "Bearer tok-1",
                    JSON,
                    Some("{\"a\":1,\"b\":2}"),
                ),
                recorded("/api/notes", bearer, JSON, Some("{\"b\":2,\"a\":1}")),
            ),
            (
                "its body",
                written("/api/notes", "Bearer tok-1", JSON, Some("{\"a\": 1}")),
                recorded("/api/notes", bearer, JSON, Some("{\"a\":1}")),
            ),
        ];
        for (part, request, step) in cases {
            let why = said(&[request], &[step]).unwrap_err();
            assert!(
                why.starts_with("exchange 1, as `cf` wrote it") && why.contains(part),
                "{why}"
            );
        }
    }

    /// What Node recorded of an answer to a run of `cf`.
    fn recorded_answer() -> Value {
        json!({ "status": 200, "contentType": "application/json", "body": "{\"a\":1}" })
    }

    /// An answer as it passed the relay.
    fn passed(status: u16, kind: Option<&str>, body: &str) -> Reply {
        Reply {
            status,
            content_type: kind.map(str::to_owned),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn an_answer_is_nodes_when_its_status_its_type_and_its_bytes_are() {
        let same = passed(200, JSON, "{\"a\":1}");
        assert_eq!(answered(3, Some(&same), &recorded_answer()), Ok(()));
        // An answer with no body and no type, as a 204 is.
        let nothing = json!({ "status": 204, "contentType": null, "body": null });
        assert_eq!(answered(3, Some(&passed(204, None, "")), &nothing), Ok(()));
    }

    #[test]
    fn a_status_a_type_or_a_byte_that_is_not_nodes_fails_the_exchange_though_cf_would_take_it() {
        // `cf` takes any 2xx, and reads any type as JSON: none of these would
        // change what it printed.
        let cases = [
            ("answered 201, Node 200", passed(201, JSON, "{\"a\":1}")),
            (
                "answered as Some(\"text/plain\"), Node as Some(\"application/json\")",
                passed(200, Some("text/plain"), "{\"a\":1}"),
            ),
            (
                "answered as None, Node as Some(\"application/json\")",
                passed(200, None, "{\"a\":1}"),
            ),
            (
                "exchange 3's answer differs at byte 5",
                passed(200, JSON, "{\"a\": 1}"),
            ),
        ];
        for (says, reply) in cases {
            let why = answered(3, Some(&reply), &recorded_answer()).unwrap_err();
            assert!(
                why.starts_with("exchange 3") && why.contains(says),
                "{says}: {why}"
            );
        }
    }

    #[test]
    fn an_answer_that_never_passed_the_relay_fails_the_exchange() {
        let why = answered(3, None, &recorded_answer()).unwrap_err();
        assert!(why.starts_with("exchange 3: no whole answer"), "{why}");
    }

    #[test]
    fn a_request_the_trace_did_not_record_and_one_it_did_fail_the_run() {
        let bearer = Some("Bearer «token:T1»");
        let step = recorded(TRANSCRIPT, bearer, JSON, None);
        let twice = said(&[transcript(), transcript()], std::slice::from_ref(&step)).unwrap_err();
        assert!(twice.starts_with("made 2 requests"), "{twice}");
        let never = said(&[], std::slice::from_ref(&step)).unwrap_err();
        assert!(never.starts_with("made 0 requests"), "{never}");
    }
}
