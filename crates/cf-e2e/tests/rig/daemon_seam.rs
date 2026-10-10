//! `the rig starts the daemon it is told to`: the rig starts the native daemon
//! and connects it to the real headless pane host. What else the daemon is asked
//! is the other suites' to show.

use cf_e2e::daemon_log::{start_line, Kind};
use cf_e2e::rig::Rig;
use serde_json::json;

use crate::{config, rig, Outcome};

#[test]
fn answers_the_page_over_the_headless_bridge() -> Outcome {
    let rig = rig()?;
    assert_eq!(rig.page("ping", json!({}))?, json!({ "ok": true }));
    rig.close()?;
    Ok(())
}

#[test]
fn starts_the_native_daemon_and_says_so_in_its_logs_start_line() -> Outcome {
    let rig = rig()?;
    let log = rig.log();
    let start = start_line(&log, Some(rig.daemon_pid()))
        .unwrap_or_else(|| panic!("no start line of pid {} in {log}", rig.daemon_pid()));
    assert_eq!(start.kind, Kind::Native, "{}", start.line);
    assert_eq!(rig.start_line().kind, start.kind);
    assert_eq!(rig.start_line().line, start.line);
    rig.close()?;
    Ok(())
}

#[test]
fn refuses_a_daemon_whose_start_line_says_it_is_not_the_native_one_and_ends_it() -> Outcome {
    // Asked for a command that is not the native daemon: the stand-in writes
    // the start line of a Node daemon, as the releases before the deletion ran.
    let Err(refused) = Rig::start(config().daemon(env!("CARGO_BIN_EXE_liar-daemon"))) else {
        panic!("a rig was started on a daemon that is not the native one");
    };
    let said = refused.to_string();
    assert!(
        said.contains(
            "the native daemon was asked for, but the start line in its log says node v0.0.0"
        ),
        "{said}"
    );
    // The stand-in and the rig's home went with the refusal: a rig that took it
    // would hold them, and the next case's turn would wait for ever.
    Ok(())
}
