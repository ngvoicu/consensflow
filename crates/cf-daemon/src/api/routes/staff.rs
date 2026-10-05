//! `GET /api/staff` (`api.js:123-143`): the members of the staff, each with
//! its roles, tier and harness, and the model and effort its saved agent has.
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
