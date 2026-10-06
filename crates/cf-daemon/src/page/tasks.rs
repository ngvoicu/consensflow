//! A task as the human sees and changes it: opened in the drawer, its
//! transcript read, cancelled, paused, given to another member, resumed, and
//! the finished ones taken off the board.

use cf_base::js;
use cf_ledger::RESUME_WORDS;
use serde_json::Value;

use super::body::{merged, one, Body, Fields, Said};
use super::Page;

/// The human, as the ledger is told who acted.
const HUMAN: &str = "human";

/// `task.get`: as much of the task as one frame carries, what was cut marked.
pub(super) async fn get(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let number = body.whole("task")?;
    let found = page
        .ledger
        .borrow()
        .task_that_fits(body.whole("project")?, number)?
        .ok_or_else(|| Said(format!("no task T-{number} in this project")))?;
    one("task", found)
}

/// `task.transcript`: what the windows that had the task wrote, the last that
/// fit in one frame, at most `limit` of them.
pub(super) async fn transcript(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let read = page.ledger.borrow().latest_transcript(
        body.whole("project")?,
        body.whole("task")?,
        limit_of(body.get("limit"))?,
    )?;
    merged(read)
}

/// `task.cancel`.
pub(super) async fn cancel(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let task =
        page.ledger
            .borrow_mut()
            .cancel_task(body.whole("project")?, body.whole("task")?, HUMAN)?;
    one("task", task)
}

/// `task.pause`.
pub(super) async fn pause(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let task = page.ledger.borrow_mut().pause_task(
        body.whole("project")?,
        body.whole("task")?,
        Some(HUMAN),
        None,
    )?;
    one("task", task)
}

/// `task.reassign`: back to the board for another member of its tier, once its
/// window is stopped.
pub(super) async fn reassign(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let released = page
        .engine
        .reassign_task(body.whole("project")?, body.whole("task")?)
        .await?;
    merged(released)
}

/// `task.resume`: the human resumes without writing to the agent: the words
/// are always these.
pub(super) async fn resume(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let moved = page.ledger.borrow_mut().resume_task(
        body.whole("project")?,
        body.whole("task")?,
        Some(HUMAN),
        RESUME_WORDS,
    )?;
    merged(moved)
}

/// `tasks.delete`: finished tasks off the board for good, the ones the human
/// confirmed or none; nobody is told, and `cf task get` still reads each.
pub(super) async fn delete(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let project = body.whole("project")?;
    let numbers = match body.get("tasks") {
        // `new Set(undefined)` is a set of nothing.
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(numbers)) => numbers
            .iter()
            .map(|number| {
                number
                    .as_f64()
                    .filter(|number| {
                        number.fract() == 0.0 && number.abs() <= 9_007_199_254_740_991.0
                    })
                    .map(|number| number as i64)
                    .ok_or_else(|| {
                        Said(format!(
                            "no task T-{} in project {project}",
                            js::text(Some(number))
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(other) => {
            return Err(Said(format!(
                "tasks is a list of task numbers, not {}",
                js::text(Some(other))
            )))
        }
    };
    let deleted = page.ledger.borrow_mut().delete_tasks(project, &numbers)?;
    one("tasks", deleted)
}

/// How many of a transcript's last items the page asked for, as the ledger
/// reads `rows.slice(Math.max(0, rows.length - limit))`: every item when it
/// named none (or a number that is none), the last `ceil(limit)` for a
/// positive one and none for any other, which is how the page counts a
/// transcript without reading it (`limit: 0`).
fn limit_of(limit: Option<&Value>) -> Result<Option<usize>, Said> {
    let Some(limit) = limit else {
        return Ok(None);
    };
    let limit = js::to_number(Some(limit)).map_err(Said)?;
    Ok(if limit.is_nan() || limit >= 9_007_199_254_740_991.0 {
        None
    } else if limit <= 0.0 {
        Some(0)
    } else {
        Some(limit.ceil() as usize)
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_limit_is_read_as_the_ledger_s_slice_reads_it() {
        assert_eq!(limit_of(None).unwrap(), None, "none named: every item");
        for (limit, last) in [
            (json!(0), Some(0)),
            (json!(-1), Some(0)),
            (json!(1), Some(1)),
            (json!(2), Some(2)),
            (json!(1.5), Some(2)),
            (json!(0.5), Some(1)),
            (json!(99), Some(99)),
            (json!(null), Some(0)),
            (json!("2"), Some(2)),
            (json!("lots"), None),
            (json!(true), Some(1)),
        ] {
            assert_eq!(limit_of(Some(&limit)).unwrap(), last, "{limit}");
        }
    }
}
