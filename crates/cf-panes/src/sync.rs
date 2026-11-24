//! Where the pane host takes the locks the model of N17's checks interleaves (`model`): the
//! screen's, the pane's writer's and its input state's, and the activity time's. In a build that
//! is no test this is `Mutex::lock`; in one, a lock taken inside a schedule is taken through the
//! scheduler, which chooses who runs and tells an actor that waits for a held lock from one that
//! is merely not chosen, and a point where it may be told to stop is named where the host reads or
//! moves a count (`point`).

use std::sync::{LockResult, Mutex, MutexGuard};

/// `mutex.lock()`, or the scheduler's lock inside a schedule.
#[cfg(not(test))]
#[inline]
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> LockResult<MutexGuard<'_, T>> {
    mutex.lock()
}

#[cfg(test)]
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> LockResult<MutexGuard<'_, T>> {
    crate::model::sched::lock(mutex)
}

/// A place where a schedule of the model may choose another actor to run: named after what the
/// host is about to do or has done. It compiles to nothing outside tests, and does nothing outside
/// a schedule.
#[cfg(not(test))]
#[inline(always)]
pub(crate) fn point(_name: &'static str) {}

#[cfg(test)]
pub(crate) fn point(name: &'static str) {
    crate::model::sched::at(name);
}
