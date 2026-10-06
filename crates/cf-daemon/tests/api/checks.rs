//! What the player compares, once a step has been made: the answer to an
//! exchange of the test's own, as bytes; what the API did on the way (the
//! wake-ups, the events, the clock, the roster); and the request `cf` made and
//! how it was answered.

use serde_json::{json, Value};

use crate::replay::{compare, differs};
use crate::rig::Rig;
use crate::trace::Names;
use crate::wire::Reply;

/// The answer to an exchange of the test's own against Node's: its status, the
/// type it carried and its bytes; and then what the API did to give it.
pub fn exchange(
    rig: &Rig,
    step: &Value,
    reply: &Reply,
    events: &[Value],
    kicks: usize,
) -> Result<(), String> {
    let response = &step["response"];
    let status = response["status"].as_u64().expect("a status");
    if u64::from(reply.status) != status {
        return Err(format!(
            "answered {}, Node {status}: {}",
            reply.status,
            String::from_utf8_lossy(&reply.body)
        ));
    }
    let kind = response["contentType"].as_str();
    if reply.content_type.as_deref() != kind {
        return Err(format!(
            "answered as {:?}, Node as {kind:?}",
            reply.content_type
        ));
    }
    let expected = response["body"].as_str().unwrap_or_default();
    if reply.body != expected.as_bytes() {
        return Err(differs(
            "the answer",
            &String::from_utf8_lossy(&reply.body),
            expected,
        ));
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
    if let Some(why) = rig.queues.settle(steps) {
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

/// An exchange `cf` made: the request the API took (what it was, whose window
/// it came from), and how it was answered.
pub fn made(rig: &Rig, names: &Names, step: &Value) -> Result<(), String> {
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
    let response = &step["response"];
    let kind = response["contentType"].as_str();
    let body = response["body"].as_str().unwrap_or_default();
    let answered = seen
        .answered
        .as_ref()
        .ok_or(format!("exchange {id}: never answered"))?;
    if u64::from(answered.status) != response["status"].as_u64().unwrap_or(0)
        || answered.content_type != kind
    {
        return Err(format!(
            "exchange {id}: answered {} as {:?}, Node {} as {kind:?}",
            answered.status, answered.content_type, response["status"]
        ));
    }
    if answered.body != body {
        return Err(differs(
            &format!("exchange {id}'s answer"),
            &answered.body,
            body,
        ));
    }
    Ok(())
}
