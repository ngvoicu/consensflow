//! The Admin page and a launch share one answer (P2, A2): a stand-in Codex that counts
//! its `queue --help` runs is looked at through `/api/harnesses/check`, and then a project
//! is seated on it. The daemon's own map of kept answers serves both: one run.
//!
//! A Unix case: the stand-in counts by a script, and Windows opens a window on the shape
//! of an npm shim alone, which cannot count.

#![cfg(unix)]

use cf_e2e::daemon_log::lines_of;
use cf_e2e::files;
use cf_e2e::http::Http;
use serde_json::json;

use crate::over_bridge::OverBridge;
use crate::Outcome;

/// The stand-in Codex the rig has.
const FAKE_CODEX: &str = env!("CARGO_BIN_EXE_fake-codex");

#[test]
fn harness_check_asks_a_codex_its_queue_once_for_a_look_and_the_seat_that_follows() -> Outcome {
    let d = OverBridge::start()?;
    // The rig's own Codex, behind a script that writes a line for each `queue --help`.
    let counts = d.home.root().join("queue-runs");
    let script = d.home.path_dir().join("codex");
    files::write_executable(
        &script,
        format!(
            "#!/bin/sh\ncase \"$*\" in \"queue --help\") echo run >> '{}';; esac\nexec '{FAKE_CODEX}' \"$@\"\n",
            counts.display()
        ),
    )?;
    let runs = || {
        files::read_string(&counts)
            .map(|text| text.lines().count())
            .unwrap_or(0)
    };

    // The page looks at it.
    let url = d.handle["url"].as_str().unwrap_or_default().to_owned();
    let token = d.handle["token"].as_str().unwrap_or_default().to_owned();
    let looked = Http::new().send(
        "POST",
        &format!("{url}api/harnesses/check"),
        Some(&token),
        Some(&json!({ "id": "codex" })),
    )?;
    assert_eq!(looked.status, 200, "{}", looked.text);
    let row = &looked.json()?["harnesses"][0];
    assert_eq!(row["setup"]["state"], "ready", "{row}");
    assert_eq!(runs(), 1, "the look asked the queue once");

    // A chief is seated on it: its launch checks the same Codex.
    let opened = d.request(
        "project.open",
        json!({
            "directory": d.home.workspace(),
            "harness": "codex",
            "model": "fake",
        }),
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    d.until("opened the chief", || {
        d.frames()
            .into_iter()
            .find(|frame| frame["kind"] == "req" && frame["op"] == "pane.open")
    })?;
    assert_eq!(
        runs(),
        1,
        "the seat read what the look kept: one `queue --help` in all\n{}",
        lines_of(&d.home.log(), d.daemon.id()).join("\n")
    );
    Ok(())
}
