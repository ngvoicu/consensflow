//! What the board page asks of the daemon, over the bridge (`pageOperations`,
//! `src/core/page.js`, and `daemon.js:165-170`): the 28 operations of
//! [`PageOperation`](cf_proto::page::PageOperation), and `ping`, which says
//! the bridge is up.
//!
//! Every operation is registered on the bridge here ([`register`]), and
//! [`operations::serve`] is the table of which function serves which; each
//! concern has a module of its own: [`projects`], [`staff`] (the agents and
//! the staff), [`sessions`], [`board`] (and the inbox), [`tasks`] and
//! [`messages`]. What they stand on is [`Page`] and the engine they call
//! ([`Engine`]), a small trait of what `page.js` calls on the dispatcher and
//! its five projections, which the dispatcher is; the saved agents are read
//! from the file at each use ([`agents`]), and the body is read as JavaScript
//! read it ([`body`]).
//!
//! How an operation answers is the bridge's: `{ok: true, ...fields}` for the
//! fields it serves, `{ok: false, error}` with the words of its failure. One
//! that changes something wakes the dispatcher once it has succeeded
//! (`PageOperation::kicks`), after the work and before the answer is queued,
//! and none does for a failure. The bridge starts each handler where its frame
//! is read and polls it once, so an operation reads and begins its engine work
//! in its first poll, in the order the frames came: an operation is begun with
//! [`cf_engine::runtime::begin`], which does what comes before its first wait
//! there and leaves the rest to the executor, where the engine's work runs
//! (the handler only waits for its answer, and queues it). No operation may
//! panic in its first poll: that would end the bridge, so every operation runs
//! contained, whatever poll it panics at, and answers `{ok: false}` with what
//! it said.

mod agents;
mod board;
mod body;
mod engine;
mod messages;
mod operations;
mod projects;
mod sessions;
mod staff;
mod tasks;

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::env::Env;
use cf_bridge::local::Bridge;
use cf_engine::runtime::begin;
use cf_ledger::Ledger;
use cf_proto::page::PageOperation;
use futures_util::future::LocalBoxFuture;
use serde_json::{json, Map, Value};

pub use engine::Engine;

use crate::errors::contain;
use crate::seams::DaemonSpawn;

/// What the page's operations are given: the ledger (borrowed for one call
/// at a time, never across a wait), the engine, the daemon's environment
/// (which finds the saved agents and the harnesses), and the kick.
pub struct Page {
    pub ledger: Rc<RefCell<Ledger>>,
    pub engine: Rc<dyn Engine>,
    pub env: Env,
    /// Wakes the dispatcher: what the loop kick does.
    pub kick: Rc<dyn Fn()>,
}

/// What an operation serves: the fields of its answer, which follow `ok: true`,
/// or the words of why it did not.
pub type Served = Result<Map<String, Value>, String>;

/// What serves an operation: given the page, the operation and its body.
pub type Serve = dyn Fn(Rc<Page>, PageOperation, Value) -> LocalBoxFuture<'static, Served>;

/// Registers the page's 28 operations and `ping` on `bridge`, to be answered
/// as the page expects (`daemon.js:165-170`), the engine's work run by `spawn`.
/// Called before the connection is first polled, so no frame finds an
/// operation missing.
pub fn register(bridge: &Bridge, page: &Rc<Page>, spawn: &Rc<DaemonSpawn>) {
    let serve: Rc<Serve> = Rc::new(|page, operation, body| {
        Box::pin(async move { operations::serve(&page, operation, body).await })
    });
    register_with(bridge, page, spawn, &serve);
}

/// [`register`], with what serves the operations given.
fn register_with(bridge: &Bridge, page: &Rc<Page>, spawn: &Rc<DaemonSpawn>, serve: &Rc<Serve>) {
    for operation in PageOperation::ALL {
        let (page, spawn, serve) = (Rc::clone(page), Rc::clone(spawn), Rc::clone(serve));
        bridge.on(operation.as_str(), move |_, body| {
            let (page, spawn, serve) = (Rc::clone(&page), Rc::clone(&spawn), Rc::clone(&serve));
            async move { answer(page, &spawn, serve, operation, body).await }
        });
    }
    bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
}

/// One operation: `{ok: true, ...fields}`, or the words it failed with. A
/// body that is null is the empty one (`body ?? {}`).
async fn answer(
    page: Rc<Page>,
    spawn: &DaemonSpawn,
    serve: Rc<Serve>,
    operation: PageOperation,
    body: Value,
) -> Result<Value, String> {
    let body = if body.is_null() { json!({}) } else { body };
    let kick = Rc::clone(&page.kick);
    // Begun where its frame is read, and contained from the first poll: what
    // the operation does before its first wait is done now, in the order the
    // frames came, and is what a panic there would end the bridge over. The
    // rest is the executor's; this waits for the answer and queues it.
    let begun = begin(
        spawn,
        contain(async move { serve(page, operation, body).await }),
    )
    .await;
    match begun.await {
        Ok(Ok(fields)) => {
            if operation.kicks() {
                kick();
            }
            let mut answered = Map::new();
            answered.insert("ok".to_owned(), Value::Bool(true));
            answered.extend(fields);
            Ok(Value::Object(answered))
        }
        Ok(Err(words)) => Err(words),
        Err(panicked) => {
            spawn.errors().caught(
                &format!("page operation {} failed", operation.as_str()),
                &panicked,
            );
            Err(panicked.message)
        }
    }
}

#[cfg(test)]
mod tests;
