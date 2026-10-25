//! What the pane host ends, when a rig ends it, is the windows' own and nothing of the
//! machine's: the rig keeps a process that is none of any window's beside them, and a rig
//! that closes with it ended by something else fails. Every case that ends a window holds
//! the pane host to that; this holds the check itself: an end given is told, and an end
//! of the stranger's own (which a rig that lives long must not take for one given) is not.

use cf_e2e::process::{is_alive, signal, Signal};

use crate::{secs, Outcome};

#[test]
fn a_rig_closes_with_the_process_that_is_none_of_any_windows_still_running() -> Outcome {
    let rig = crate::rig()?;
    assert!(is_alive(rig.stranger_pid()), "it runs beside the pane host");

    rig.close()?;
    Ok(())
}

#[test]
fn a_rig_that_finds_the_process_that_is_none_of_any_windows_ended_says_so_when_it_closes() -> Outcome
{
    let rig = crate::rig()?;
    rig.expect_the_close_to_fail();
    signal(rig.stranger_pid(), Signal::Kill)?;
    // The daemon and the pane host end first, which takes longer than the signal does.
    std::thread::sleep(secs(1));

    let closed = rig.close();

    let told = closed.err().map(|failed| failed.to_string());
    assert!(
        told.as_deref()
            .is_some_and(|said| said.contains("none of any window's was ended")),
        "{told:?}"
    );
    Ok(())
}

/// A stranger that slept ten minutes ran out by itself under a live suite, and the rig
/// took it for one the pane host ended (2026-10-15): one that left of itself is no end
/// the pane host gave it.
#[test]
fn a_stranger_that_left_of_itself_is_not_taken_for_one_ended_with_the_windows() -> Outcome {
    let mut rig = crate::rig()?;
    assert!(rig.stranger_leaves(secs(10))?, "it left of itself");

    rig.close()?;
    Ok(())
}
