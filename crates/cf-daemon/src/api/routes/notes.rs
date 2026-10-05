//! `POST /api/notes` (`api.js:244-266`): a note, from the chief to the human or
//! from a member to whoever gave it its task. It reads its body first, and
//! wakes the dispatcher once it is written.
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
