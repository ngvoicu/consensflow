//! What the board page asks of the daemon, over the bridge (`pageOperations`,
//! `src/core/page.js`, and `daemon.js:165-170`): the 28 operations of
//! [`PageOperation`](cf_proto::page::PageOperation), and `ping`, which says
//! the bridge is up.
//!
//! **Frozen** for the page operations' landing: every operation is registered
//! on the bridge here ([`register`]), and [`operations::serve`] is the table
//! of which function serves which. Until an operation lands it answers an
//! error that names it. What the operations stand on is [`Page`] and the
//! engine they call ([`Engine`]), a small trait of what `page.js` calls on the
//! dispatcher and its five projections, which the dispatcher is.
//!
//! How an operation answers is the bridge's: `{ok: true, ...fields}` for the
//! fields it serves, `{ok: false, error}` with the words of its failure. One
//! that changes something wakes the dispatcher once it has succeeded
//! (`PageOperation::kicks`), after the work and before the answer is queued,
//! and none does for a failure. The bridge starts each handler where its frame
//! is read and polls it once, so an operation reads and begins its engine work
//! in its first poll, in the order the frames came, and no operation may
//! panic there: a panic at the first poll would end the bridge, so every
//! operation runs contained, whatever poll it panics at, and answers
//! `{ok: false}` with what it said.

mod engine;
mod operations;

use std::cell::RefCell;
use std::rc::Rc;

use cf_base::env::Env;
use cf_bridge::local::Bridge;
use cf_ledger::Ledger;
use cf_proto::page::PageOperation;
use futures_util::future::LocalBoxFuture;
use serde_json::{json, Map, Value};

pub use engine::Engine;

use crate::errors::{contain, Errors};

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
/// as the page expects (`daemon.js:165-170`). Called before the connection is
/// first polled, so no frame finds an operation missing.
pub fn register(bridge: &Bridge, page: &Rc<Page>, errors: &Rc<Errors>) {
    let serve: Rc<Serve> = Rc::new(|page, operation, body| {
        Box::pin(async move { operations::serve(&page, operation, body).await })
    });
    register_with(bridge, page, errors, &serve);
}

/// [`register`], with what serves the operations given.
fn register_with(bridge: &Bridge, page: &Rc<Page>, errors: &Rc<Errors>, serve: &Rc<Serve>) {
    for operation in PageOperation::ALL {
        let (page, errors, serve) = (Rc::clone(page), Rc::clone(errors), Rc::clone(serve));
        bridge.on(operation.as_str(), move |_, body| {
            let (page, errors, serve) = (Rc::clone(&page), Rc::clone(&errors), Rc::clone(&serve));
            async move { answer(page, &errors, &serve, operation, body).await }
        });
    }
    bridge.on("ping", |_, _| async { Ok(json!({ "ok": true })) });
}

/// One operation: `{ok: true, ...fields}`, or the words it failed with. A
/// body that is null is the empty one (`body ?? {}`).
async fn answer(
    page: Rc<Page>,
    errors: &Errors,
    serve: &Rc<Serve>,
    operation: PageOperation,
    body: Value,
) -> Result<Value, String> {
    let body = if body.is_null() { json!({}) } else { body };
    let kick = Rc::clone(&page.kick);
    // Contained from the first poll: what the operation does before its first
    // wait is what a panic there would end the bridge over.
    match contain(async move { serve(page, operation, body).await }).await {
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
            errors.caught(
                &format!("page operation {} failed", operation.as_str()),
                &panicked,
            );
            Err(panicked.message)
        }
    }
}

#[cfg(test)]
mod tests;
