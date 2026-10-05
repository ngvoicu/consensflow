//! `POST /api/questions` (`api.js:222-243`): a member's question for the chief,
//! which the member's task waits on. The chief asks the human in its own
//! terminal, and is refused here before the body is read. The door waiting for
//! the answer is [`super::door`].
//!
//! Its landing is the API's. Until then it answers as Node answers a route it
//! has none for.

use super::{Answer, Caller, Context, Failure, Request};

pub(super) async fn ask(
    _context: &Context,
    _caller: &Caller,
    request: Request,
) -> Result<Answer, Failure> {
    Err(request.unknown_route())
}
