//! The Close of a project whose Codex chief never had a turn went unanswered.
//!
//! Seen by the tester on 2026-10-25, on ConsensFlow 3.0.6-alpha.2 with codex-cli
//! 0.164.0 (T-534, bug 3; fixed in T-542): the live resume-fresh suite opened a
//! project with a Codex chief, whose window came up idle, and closed it. The
//! project went to suspended, and the Close was not answered for the 10 s the rig
//! waits, 3 times of 9. The pane host closes a window with SIGTERM and then SIGHUP
//! at once. The window's supervisor (`cf codex-session`) took SIGTERM, but SIGHUP
//! was left to its default, and ended it before it had ended Codex's server.
//! Codex's server, closed soon after it started, stalled in its own shutdown, and
//! lived on with no parent until the pane host's grace (10 s) was over and it was
//! killed. The Close waited for the window all that time. The chief that had had a
//! turn was closed later, once Codex was done starting, and its server went at
//! once.
//!
//! What the tester should have seen: the Close answered at once, the chief's
//! window gone, and the chief opened again afresh on the Resume. A supervisor that
//! lives through the SIGHUP ends a server that does not end when asked within its
//! own grace (1.5 s).
//!
//! The stand-in Codex's server here takes neither signal (`CF_TEST_CODEX_BACKEND=deaf`):
//! a server that does not end when asked, as Codex's did not in time. The case is
//! the tester's own otherwise: a Codex chief that was opened, idle, closed and
//! resumed, through the real daemon and pane host. Unix's: Windows asks no program
//! to end, and the pane's job object ends the window whole at once.

use std::time::{Duration, Instant};

use cf_e2e::files;
use cf_e2e::rig::{Project, Rig};
use serde_json::json;

use crate::config_with_codex;
use crate::resume_of_a_chief_with_no_turn::{
    resumed_on, stays_open, wait_for_the_chief, windows_of,
};
use crate::{secs, Outcome};

/// How long the Close may take: the supervisor's grace for a server that does
/// not end when asked (1.5 s), and a margin for a loaded machine, well short of
/// the pane host's (10 s).
const AT_ONCE: Duration = Duration::from_secs(5);

#[test]
fn the_close_of_a_codex_chief_whose_server_does_not_end_when_asked_is_answered_at_once_and_the_chief_comes_back_afresh(
) -> Outcome {
    let rig = Rig::start(config_with_codex().var("CF_TEST_CODEX_BACKEND", "deaf"))?;
    let roster = json!({
        "schemaVersion": 1,
        "agents": [{ "id": "codex-chief", "kind": "codex", "model": "fake" }],
    });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    let project = Project::open(&rig, "codex-chief", json!({}))?;
    wait_for_the_chief(&rig, &project)?;

    let asked = Instant::now();
    let closed = rig.page("project.close", json!({ "project": project.id() }))?;
    let took = asked.elapsed();
    assert_eq!(closed["ok"], true, "{closed}");
    assert!(
        took < AT_ONCE,
        "the Close was answered {took:?} after it was asked: {:?}",
        rig.exits()
    );
    assert!(
        project.lane("chief")?["pane"].is_null(),
        "the chief's window went with the Close"
    );
    assert_eq!(project.board()?["project"]["state"], "suspended");

    let resumed = rig.page("project.resume", json!({ "project": project.id() }))?;
    assert_eq!(resumed["ok"], true, "{resumed}");
    rig.wait_for("the chief's window to be opened again", secs(60), || {
        Ok(windows_of(&rig, &project).len() >= 2)
    })?;
    let again = windows_of(&rig, &project).remove(1);
    assert_eq!(
        resumed_on(&again),
        None,
        "the window was opened on a thread nothing was said in"
    );
    stays_open(&rig, &project, &again)?;
    wait_for_the_chief(&rig, &project)?;
    rig.close()?;
    Ok(())
}
