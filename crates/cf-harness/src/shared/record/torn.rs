//! What a reader says of a torn line it passed over: a line of a record that is
//! not JSON and never will be, as a short write leaves one (a full disk, a
//! process killed in a large write) once the next append completes it. The
//! harness itself goes on past such a line, and so do the readers; the
//! daemon's log is told of it, once a line, by its file and the record it
//! stood where, never by its text.
//!
//! The records thread tells its worker where to say it ([`say_with`]): the
//! readers are on that thread. A reader on any other thread says nothing.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where a thread's readers say what they passed over, in a line each.
pub type Say = Box<dyn Fn(&str) + Send>;

/// Where this thread says it, and the lines it said, by their file and byte.
struct Saying {
    say: Say,
    said: HashSet<(PathBuf, u64)>,
}

thread_local! {
    static SAYING: RefCell<Option<Saying>> = const { RefCell::new(None) };
}

/// The readers on this thread say what they pass over with `say`.
pub(crate) fn say_with(say: Say) {
    SAYING.with(|saying| {
        *saying.borrow_mut() = Some(Saying {
            say,
            said: HashSet::new(),
        });
    });
}

/// The torn line at the byte `at` of `file`, which stood where the record
/// `index` is, was passed over: said the first time a reader on this thread
/// passes over it, and not when one reads the file again.
pub(crate) fn passed_over(file: &Path, index: usize, at: u64) {
    SAYING.with(|saying| {
        if let Some(saying) = saying.borrow_mut().as_mut() {
            if saying.said.insert((file.to_owned(), at)) {
                (saying.say)(&format!(
                    "passed over a torn line of {}: not JSON, at record {index} (byte {at})",
                    file.display()
                ));
            }
        }
    });
}
