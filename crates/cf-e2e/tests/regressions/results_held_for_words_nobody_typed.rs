//! A chief's results held for almost two hours as "you have typed in this window and
//! not sent it", when nobody had typed.
//!
//! Seen on 2026-10-27, on 3.0.6-alpha.2 (Claude Code 2.1.310), on both of the owner's
//! Macs: an idle chief's turn ended at 17:59, Claude Code wrote its recap at 18:02, and
//! from 18:24 three workers' results queued behind the hold, told by the chief's lane
//! alone, until the owner typed into the window at 20:08 (T-839). Alpha.2 held every
//! paste while the pane host's count of the human's keys was up; what raised it is not
//! in any record (the page's input is traced nowhere). One way it rises with nobody
//! typing: the count read the body of a string the terminal reports in (an OSC colour,
//! a DCS name) as characters typed, so one such report on the page's human path held
//! every delivery to a window whose box no reader reads. The report is
//! `issues/results-held-for-words-nobody-typed.md` in consensflow-sme.
//!
//! What should have been seen: the worker's result handed to the chief; and, where a
//! result is held for words typed, the human told once the hold outlasts its bound (the
//! engine's case, `dispatcher::unsent`).
//!
//! The case is that shape on the rig: the chief's stand-in draws no box any reader
//! reads (the count is the only word), the chief hands a worker a turn, and the
//! terminal's reports arrive on the human's path as the page's keys do (`pane.typed`).

use cf_e2e::rig::{Project, Rig};
use serde_json::json;

use crate::keys_cleared_with_escape_held_every_delivery::{a_worker_at_work, result};
use crate::{config, secs, Outcome};

/// What a terminal reports of itself when a program asks: its background colour (OSC
/// 11, ended by ST), its foreground (OSC 10, ended by BEL) and its name (DCS).
const REPORTS: [&[u8]; 3] = [
    b"\x1b]11;rgb:1e1e/1e1e/1e1e\x1b\\",
    b"\x1b]10;rgb:cccc/cccc/cccc\x07",
    b"\x1bP>|xterm.js(6.0.0)\x1b\\",
];

#[test]
fn a_terminals_reports_on_the_humans_path_hold_no_result_from_the_chief() -> Outcome {
    let rig = Rig::start(config())?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let session = a_worker_at_work(&rig, &project, 5)?;
    let pane = project.lane("chief")?["pane"].clone();
    for report in REPORTS {
        rig.types(&pane, report)?;
    }
    let counted = rig.host(
        "pane.snapshot",
        json!({ "id": pane["id"], "generation": pane["generation"] }),
    )?;
    assert_eq!(counted["unsent"], false, "a report is no key: {counted}");
    rig.wait_for("T-1 to be done", secs(90), || {
        Ok(project.task(1)?["state"] == "done")
    })?;
    rig.wait_for(
        "the worker's result to be handed to the chief",
        secs(60),
        || {
            Ok(result(&project)?.is_some_and(|(id, delivered)| {
                delivered
                    && rig
                        .transcript(&session)
                        .contains(&format!("[ConsensFlow m-{id} "))
            }))
        },
    )?;
    assert_eq!(project.lane("chief")?["holding"], false);
    rig.close()?;
    Ok(())
}
