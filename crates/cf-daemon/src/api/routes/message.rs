//! `GET /api/inbox/<id>` (`api.js:208-221`): one message, to its recipient or
//! its sender, unless it still waits for the human. The id is the digits as
//! the path had them: the 404 quotes them as typed.
//!
//! Its landing is the API's. Until then it answers as Node answers a route it
//! has none for.

use super::{Answer, Caller, Context, Failure, Request};

pub(super) async fn handle(
    _context: &Context,
    _caller: &Caller,
    request: Request,
    _id: &str,
) -> Result<Answer, Failure> {
    Err(request.unknown_route())
}
