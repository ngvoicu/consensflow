//! The agents' routes, in the order Node matched them. **Frozen**: the table is
//! [`recognize`], the order of the checks around it is [`super::handle`], and a
//! route's place is its own module, which a landing fills in without touching
//! this file. Its two routes that Node's has no twin of are the door's receipt
//! (`POST /api/answers/<n>/receipt`) and `cf`'s (`POST /api/answers/read`),
//! which the receipt and stop redesign added after the answers' own.
//!
//! The order of a request's checks, as Node has it:
//!
//! 1. the human's screens, under the UI token, before any window's: their
//!    paths are the screens', and a path that is not one falls through;
//! 2. the window's token ([`super::callers::caller_of`]): so a path that is
//!    no route at all, from a caller with no valid token, is 401, not 404;
//! 3. the routes below, in this order, a match deciding which handler runs;
//! 4. none: 404 `unknown-route` (`no such command: GET /api/nothing`).
//!
//! A handler is given the context, the caller and the request. Where Node
//! checks who may ask before it reads a body, the handler does, and reads the
//! body ([`Request::json`]) only where Node did.

mod answers;
mod door;
mod history;
mod inbox;
mod message;
mod notes;
mod questions;
mod staff;
mod task;
mod tasks;
mod whoami;

use hyper::Method;

use super::answer::{Answer, Failure};
use super::callers::Caller;
use super::context::Context;
use super::request::Request;

/// What a task route does to its task (the word after the number).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskAction {
    Done,
    Accept,
    Reopen,
    Cancel,
    Pause,
    Resume,
    Tell,
    Transcript,
}

impl TaskAction {
    fn named(word: &str) -> Option<Self> {
        Some(match word {
            "done" => Self::Done,
            "accept" => Self::Accept,
            "reopen" => Self::Reopen,
            "cancel" => Self::Cancel,
            "pause" => Self::Pause,
            "resume" => Self::Resume,
            "tell" => Self::Tell,
            "transcript" => Self::Transcript,
            _ => return None,
        })
    }
}

/// A route of the agents' API. A number is the digits as the path had them,
/// which a handler reads as `Number(...)` reads them and quotes as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// `GET /api/whoami`.
    Whoami,
    /// `GET /api/history`.
    History,
    /// `GET /api/staff`.
    Staff,
    /// `GET /api/tasks`.
    Tasks,
    /// `POST /api/tasks`.
    CreateTask,
    /// `/api/tasks/<n>` and `/api/tasks/<n>/<action>`, whatever the method:
    /// the task route decides which methods it has.
    Task {
        number: String,
        action: Option<TaskAction>,
    },
    /// `GET /api/inbox`.
    Inbox,
    /// `GET /api/inbox/<id>`.
    Message { id: String },
    /// `POST /api/questions`.
    AskQuestion,
    /// `POST /api/notes`.
    Note,
    /// `GET /api/questions/<id>`: a door waiting for the answer.
    Question { id: String },
    /// `POST /api/answers`.
    Answers,
    /// `POST /api/answers/<id>/receipt`: a door says it handed its answer over.
    AnswerReceipt { id: String },
    /// `POST /api/answers/read`: `cf` says which answers it wrote whole.
    AnswersRead,
}

/// The route a request is for, in Node's order; none for the rest.
pub fn recognize(method: &Method, path: &str) -> Option<Route> {
    let get = method == Method::GET;
    let post = method == Method::POST;
    if get && path == "/api/whoami" {
        return Some(Route::Whoami);
    }
    if get && path == "/api/history" {
        return Some(Route::History);
    }
    if get && path == "/api/staff" {
        return Some(Route::Staff);
    }
    if get && path == "/api/tasks" {
        return Some(Route::Tasks);
    }
    if post && path == "/api/tasks" {
        return Some(Route::CreateTask);
    }
    if let Some((number, action)) = task_path(path) {
        return Some(Route::Task { number, action });
    }
    if get && path == "/api/inbox" {
        return Some(Route::Inbox);
    }
    if let (true, Some(id)) = (get, numbered(path, "/api/inbox/")) {
        return Some(Route::Message { id });
    }
    if post && path == "/api/questions" {
        return Some(Route::AskQuestion);
    }
    if post && path == "/api/notes" {
        return Some(Route::Note);
    }
    if let (true, Some(id)) = (get, numbered(path, "/api/questions/")) {
        return Some(Route::Question { id });
    }
    if post && path == "/api/answers" {
        return Some(Route::Answers);
    }
    if post && path == "/api/answers/read" {
        return Some(Route::AnswersRead);
    }
    if let (true, Some(id)) = (post, answer_receipt(path)) {
        return Some(Route::AnswerReceipt { id });
    }
    None
}

/// `/api/answers/(\d+)/receipt` whole: the digits as the path had them.
fn answer_receipt(path: &str) -> Option<String> {
    let id = path
        .strip_prefix("/api/answers/")?
        .strip_suffix("/receipt")?;
    is_digits(id).then(|| id.to_owned())
}

/// `/api/tasks/(\d+)(?:/(done|accept|reopen|cancel|pause|resume|tell|transcript))?`
/// whole.
fn task_path(path: &str) -> Option<(String, Option<TaskAction>)> {
    let rest = path.strip_prefix("/api/tasks/")?;
    let (number, action) = match rest.split_once('/') {
        Some((number, word)) => (number, Some(TaskAction::named(word)?)),
        None => (rest, None),
    };
    is_digits(number).then(|| (number.to_owned(), action))
}

/// What follows `prefix` when it is digits and nothing else (`\d+`).
fn numbered(path: &str, prefix: &str) -> Option<String> {
    let digits = path.strip_prefix(prefix)?;
    is_digits(digits).then(|| digits.to_owned())
}

/// `\d+`: ASCII digits only, at least one.
fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
}

/// Runs the handler of `route`.
pub async fn dispatch(
    context: &Context,
    caller: &Caller,
    route: Route,
    request: Request,
) -> Result<Answer, Failure> {
    match route {
        Route::Whoami => whoami::handle(context, caller, request).await,
        Route::History => history::handle(context, caller, request).await,
        Route::Staff => staff::handle(context, caller, request).await,
        Route::Tasks => tasks::list(context, caller, request).await,
        Route::CreateTask => tasks::create(context, caller, request).await,
        Route::Task { number, action } => {
            task::handle(context, caller, request, &number, action).await
        }
        Route::Inbox => inbox::handle(context, caller, request).await,
        Route::Message { id } => message::handle(context, caller, request, &id).await,
        Route::AskQuestion => questions::ask(context, caller, request).await,
        Route::Note => notes::handle(context, caller, request).await,
        Route::Question { id } => door::handle(context, caller, request, &id).await,
        Route::Answers => answers::handle(context, caller, request).await,
        Route::AnswerReceipt { id } => answers::receipt(context, caller, request, &id).await,
        Route::AnswersRead => answers::read(context, caller, request).await,
    }
}

#[cfg(test)]
mod tests;
