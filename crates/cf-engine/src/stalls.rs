//! A task whose window went away mid-work is paused, not given up: the chief
//! resumes it into the same window, with its memory (`stall` in Node's
//! dispatcher). Its requester is told in a pause note, one note for the tasks
//! that stall together. A restart that finds a dozen windows gone, or a project
//! that closes with a dozen open, is one message in the chief's window and one
//! turn, not a dozen.
//!
//! The tasks that stall together are the stalls of one burst: a pass, or the
//! closing of a project's windows. The first stall of a burst for a requester
//! queues its note, as a lone stall always did, and each later one joins that
//! note while it is queued. A note already pasted into the window is read, so
//! the stall after it queues a note of its own, and the next ones join that.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use cf_ledger::{ProjectView, TaskView};

use crate::dispatcher::Dispatcher;
use crate::seams::EngineError;

/// What the dispatcher keeps of the bursts under way.
#[derive(Debug, Default)]
pub(crate) struct StallsState {
    /// How many bursts are under way; stalls are one while any is.
    bursts: Cell<usize>,
    /// The note that told each requester of a project since its burst began,
    /// by project and requester: its message.
    told: RefCell<HashMap<(i64, String), i64>>,
}

/// A burst under way. Dropping it ends it, however its work ends: what the
/// bursts told is forgotten with the last of them.
pub(crate) struct Burst<'a>(&'a StallsState);

impl StallsState {
    /// A pass, or the closing of windows, begins: the stalls it makes are told together.
    pub(crate) fn begin_burst(&self) -> Burst<'_> {
        self.bursts.set(self.bursts.get() + 1);
        Burst(self)
    }
}

impl Drop for Burst<'_> {
    fn drop(&mut self) {
        let left = self.0.bursts.get().saturating_sub(1);
        self.0.bursts.set(left);
        if left == 0 {
            self.0.told.borrow_mut().clear();
        }
    }
}

impl Dispatcher {
    /// A task whose window went away is paused, and its requester told
    /// ([`Dispatcher::tell_stall`]).
    pub(crate) fn stall(
        &self,
        project: &ProjectView,
        task: &TaskView,
        because: &str,
        why: &str,
        except: Option<i64>,
    ) -> Result<(), EngineError> {
        self.seams
            .ledger
            .borrow_mut()
            .pause_task(project.id, task.number, None, Some(because))?;
        self.tell_stall(project, task, because, why, except)
    }

    /// A task ConsensFlow paused as its window went away (`because`), its requester
    /// told: in a note of its own, or in the one that told it of the stalls before
    /// this one in the same burst, if its reader has not been given it yet. Words told
    /// to the window that wait for it still are named in it, as waiting for the
    /// resume (`why`), but `except`, the message a give-up that closed the window
    /// settles and names itself, and the human hears of each once ([`Dispatcher::tell_kept`]),
    /// however the window went (T-414 P3-2: a window that closed by itself left them
    /// unnamed). A resume whose words never reached the window tells of its pause here
    /// too, with the stalls it comes with (P3-5).
    pub(crate) fn tell_stall(
        &self,
        project: &ProjectView,
        task: &TaskView,
        because: &str,
        why: &str,
        except: Option<i64>,
    ) -> Result<(), EngineError> {
        let key = (project.id, task.requester.clone());
        let earlier = self.stalls.told.borrow().get(&key).copied();
        let (note, kept) = self.seams.ledger.borrow_mut().note_stall(
            project.id,
            task.number,
            because,
            why,
            earlier,
            except,
        )?;
        // A stall that comes alone leaves nothing to join: its note is its own.
        if self.stalls.bursts.get() > 0 {
            self.stalls.told.borrow_mut().insert(key, note.id);
        }
        for message in &kept {
            self.tell_kept(message, why)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use crate::testing::Context;

    /// Zeus's T-1 and diana's T-2, both paused as their windows closed and both resumed,
    /// and neither resume's window comes up, while a burst of stalls is under way: both
    /// pauses are told in one note, the burst's, as the stalls of one burst are (T-414
    /// P3-5: the ledger wrote each resume's note itself, past the joining). In the
    /// product a launch given up runs apart from any pass, so no burst is under way then,
    /// for a window's exit or for a resume alike; the burst is held here to show the
    /// resume's note goes the way a stall's does.
    #[test]
    fn resumes_whose_windows_never_come_up_in_one_burst_are_told_in_one_note() {
        let context = Context::new();
        let project = context.with_staff(&["zeus", "diana"]);
        context.give(project.id, "zeus", "Parser");
        context.give(project.id, "diana", "Lexer");
        context.pass().unwrap();
        context.pass().unwrap();
        for member in ["zeus", "diana"] {
            context.exit(member);
        }
        context.pass().unwrap();
        for number in [1, 2] {
            assert_eq!(context.task(project.id, number).task.state, "paused");
        }

        *context.adapter.started.borrow_mut() = Some(Rc::new(|| {
            Err("the broker never named its thread".to_owned())
        }));
        context.resume_task(project.id, 1, "Carry on");
        context.resume_task(project.id, 2, "Carry on too");
        let burst = context.dispatcher.stalls.begin_burst();
        context.pass().unwrap();
        context.pass().unwrap();
        drop(burst);

        for number in [1, 2] {
            assert_eq!(context.task(project.id, number).task.state, "paused");
        }
        let chief = context.id(project.id, "chief");
        let again: Vec<String> = context
            .inbox(chief)
            .into_iter()
            .map(|message| message.body)
            .filter(|said| said.contains("the words that resumed it"))
            .collect();
        assert_eq!(again.len(), 1, "{again:#?}");
        assert!(
            again[0].starts_with("T-1 is paused: the words that resumed it")
                && again[0].contains("\nT-2 is paused: the words that resumed it"),
            "{}",
            again[0]
        );
    }
}
