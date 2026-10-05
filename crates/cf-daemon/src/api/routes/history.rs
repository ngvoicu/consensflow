//! `GET /api/history` (`api.js:96-122`): a page of the chief's history, which
//! only a chief reads, and which writes down that it was read.
//!
//! Its landing is the API's. Until then it answers as Node answers a route it
//! has none for.

use super::{Answer, Caller, Context, Failure, Request};

pub(super) async fn handle(
    _context: &Context,
    _caller: &Caller,
    request: Request,
) -> Result<Answer, Failure> {
    Err(request.unknown_route())
}
