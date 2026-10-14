//! What the pane host ends, when a rig ends it, is the windows' own and nothing of the
//! machine's: the rig keeps a process that is none of any window's beside them, and a rig
//! that closes with it gone fails (Unix's). Every case that ends a window holds the pane
//! host to that; this holds the check itself.

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
