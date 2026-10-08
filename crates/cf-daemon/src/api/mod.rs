//! The daemon's HTTP front: the agents' API, which `cf` calls from inside a
//! window, and the human's screens, which the app frames, on one loopback
//! address.
//!
//! **Frozen** for the three landings that follow (the API's routes, the
//! screens, the page operations): [`routes`] is the route table,
//! [`handle`] the order of its checks, [`body`] the two body readers,
//! [`answer`] the answer helpers and the error shape, [`context`] what a
//! handler is given, and [`credentials`] and [`callers`] who is asking. A
//! landing fills in handlers, which are files of their own, and changes none
//! of these.
//!
//! The server ([`Api`]) is hyper's `http1` on the engine's `LocalSet`, a task
//! to each connection, and each request's handler is begun where the request
//! is and run on the engine's executor, so a client that leaves does not drop
//! its handler: Node ran it to the end, and an update would die halfway.

pub mod answer;
pub mod body;
pub mod callers;
pub mod context;
pub mod credentials;
pub mod request;
pub mod routes;
mod server;
pub mod views;

use std::io;
use std::rc::Rc;

use answer::{Answer, Failure};
use context::Context;
use request::Request;

use crate::screens::Screens;
use crate::seams::DaemonSpawn;

pub use server::{Api, Handler};

/// The front the daemon runs: `context` and `screens` answer each request in
/// [`handle`]'s order, and the context's `closing` is set when the front
/// closes. Its requests run on `spawn`'s executor.
pub async fn serve(
    context: Rc<Context>,
    screens: Rc<Screens>,
    spawn: Rc<DaemonSpawn>,
) -> io::Result<Api> {
    let closing = context.closing.clone();
    let handler: Rc<Handler> = Rc::new(move |request| {
        let (context, screens) = (Rc::clone(&context), Rc::clone(&screens));
        Box::pin(async move { handle(&context, &screens, request).await })
    });
    Api::start(handler, closing, spawn).await
}

/// One request, in Node's order:
///
/// 1. the human's screens, under the UI token, before any window's: one that
///    is theirs is answered here, and the rest fall through;
/// 2. the window's token: so a path that is no route at all, from a caller
///    with no valid token, is 401, not 404;
/// 3. the route the method and the path name ([`routes::recognize`]);
/// 4. none: 404 `unknown-route`.
pub async fn handle(
    context: &Context,
    screens: &Screens,
    mut request: Request,
) -> Result<Answer, Failure> {
    if let Some(answer) = screens.handle(&mut request).await {
        return Ok(answer);
    }
    let caller = callers::caller_of(context, &request)?;
    match routes::recognize(&request.method, &request.path) {
        Some(route) => routes::dispatch(context, &caller, route, request).await,
        None => Err(request.unknown_route()),
    }
}

#[cfg(test)]
mod tests;
