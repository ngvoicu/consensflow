//! One task (`taskRoute`, `api.js:312-386`): `GET /api/tasks/<n>`, its
//! `/transcript`, and the `POST`s that move it (`done`, `accept`, `reopen`,
//! `cancel`, `pause`, `resume`, `tell`). It reads the task first, so an
//! unknown one is 404 before anything else is asked; and it takes the task as
//! it was then, awaiting the body after. The number is the digits as the path
//! had them.
//!
//! Its landing is the API's. Until then it answers as Node answers a route it
//! has none for.

use super::{Answer, Caller, Context, Failure, Request, TaskAction};

pub(super) async fn handle(
    _context: &Context,
    _caller: &Caller,
    request: Request,
    _number: &str,
    _action: Option<TaskAction>,
) -> Result<Answer, Failure> {
    Err(request.unknown_route())
}
