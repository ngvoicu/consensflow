//! The end (9): a project reaches it within the run's budget when every task is in
//! an end state (accepted, cancelled or failed; a task deleted from the board is
//! none of the run's) and the chief has reported to the owner after the last of
//! them (a note to the human, or its own words in its terminal), both by the
//! budget's deadline: a task accepted or a report written after it is late, however
//! soon after, and a run whose time ran out did not reach its end in time. A run
//! whose project does not says where it stood: the tasks still open or ended late,
//! and who holds them.

use super::{iso, Break, Evidence};
use crate::clock;
use crate::long_run::look::Look;
use crate::long_run::mask::{cut, Names};

/// The states a task ends in.
pub const ENDED: [&str; 3] = ["accepted", "cancelled", "failed"];

/// Whether the project of `look` reached its end by `deadline` (in milliseconds),
/// and if it did not, the break that says where it stood; `why` is how the run ended
/// (`time ran out`, `it went quiet`).
pub fn end(look: &Look, why: &str, deadline: i64) -> Option<Break> {
    let tasks: Vec<_> = look
        .ledger
        .tasks
        .iter()
        .filter(|task| !task.deleted)
        .collect();
    let ended_at = |task: &&&crate::long_run::ledger::Task| {
        ENDED
            .contains(&task.state.as_str())
            .then(|| clock::millis(&task.updated_at))
            .flatten()
    };
    let open: Vec<_> = tasks
        .iter()
        .filter(|task| ended_at(task).is_none_or(|at| at > deadline))
        .collect();
    let last_end = tasks
        .iter()
        .filter_map(|task| clock::millis(&task.updated_at))
        .max();
    let reported = last_end.and_then(|after| reported_after(look, after, deadline));
    if !tasks.is_empty() && open.is_empty() && reported.is_some() {
        return None;
    }
    let names = Names::of(look);
    let mut rows: Vec<String> = open
        .iter()
        .map(|task| {
            let late = ended_at(task).map_or_else(String::new, |at| {
                format!(" at {}, after the deadline", iso(at))
            });
            format!(
                "T-{} {}{late} @{}: {}",
                task.number,
                task.state,
                task.assignee.map_or("-", |id| look.ledger.handle(id)),
                cut(&task.title, 80, &names)
            )
        })
        .collect();
    if tasks.is_empty() {
        rows.push("nothing was put on the board".to_owned());
    }
    if reported.is_none() {
        rows.push(match last_end {
            Some(at) => format!(
                "no report to the owner between {} and the deadline",
                iso(at)
            ),
            None => "no report to the owner".to_owned(),
        });
    }
    rows.push(format!("the budget's deadline: {}", iso(deadline)));
    let said = format!(
        "the project did not reach its end within the budget ({why}): {} of {} tasks open{}",
        open.len(),
        tasks.len(),
        if reported.is_none() {
            ", no report"
        } else {
            ""
        }
    );
    Some(Break::new(9, "9:end", said).with(Evidence {
        rows,
        ..Evidence::default()
    }))
}

/// When the chief reported to the owner after `after` and by `deadline`: its note to
/// the human, or the end of a turn of its own.
fn reported_after(look: &Look, after: i64, deadline: i64) -> Option<i64> {
    let ledger = &look.ledger;
    let chief = ledger
        .participants
        .iter()
        .find(|someone| someone.role == "chief")?;
    let human = ledger
        .participants
        .iter()
        .find(|someone| someone.role == "human");
    let noted = ledger
        .messages
        .iter()
        .filter(|message| {
            message.kind == "note"
                && message.sender == Some(chief.id)
                && human.is_some_and(|human| message.recipient == human.id)
        })
        .filter_map(|message| clock::millis(&message.created_at));
    let said = ledger
        .items_of(chief.id)
        .into_iter()
        .filter(|item| item.role == "assistant" && item.complete)
        .filter_map(|item| item.millis());
    noted
        .chain(said)
        .filter(|at| *at > after && *at <= deadline)
        .max()
}
