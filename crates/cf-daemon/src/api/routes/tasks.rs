//! `GET /api/tasks` (`api.js:144-154`), the open tasks and each lane's, and
//! `POST /api/tasks` (`api.js:155-202`), a task given: the chief's alone,
//! refused before its body is read. One task is [`super::task`].
//!
//! Their landing is the API's. Until then they answer as Node answers a route
//! it has none for.

use super::{Answer, Caller, Context, Failure, Request};

/// `GET /api/tasks`.
pub(super) async fn list(
    _context: &Context,
    _caller: &Caller,
    request: Request,
) -> Result<Answer, Failure> {
    Err(request.unknown_route())
}

/// `POST /api/tasks`.
pub(super) async fn create(
    _context: &Context,
    _caller: &Caller,
    request: Request,
) -> Result<Answer, Failure> {
    Err(request.unknown_route())
}
