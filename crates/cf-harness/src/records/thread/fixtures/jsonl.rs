//! The JSONL harnesses' fixtures (Claude, Codex, Pi): each written a piece at
//! a time into the place its harness keeps a transcript.

use std::fs;
use std::path::{Path, PathBuf};

use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::rig::{append, fixture_lines, pieces, stamp, Rig, LATE_MS, PI_QUIET_MS, STEP_MS};
use crate::records::{Options, PiSettlement};

/// How a Pi record is read: just written, quiet past its window, or with the
/// evidence its extension keeps naming the last record.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Way {
    Fresh,
    Quiet,
    Evidence,
}

impl Way {
    /// The ways a harness's record is read: Pi's three, and the others' one.
    pub(super) fn of(harness: Harness) -> &'static [Way] {
        if harness == Harness::Pi {
            &[Way::Fresh, Way::Quiet, Way::Evidence]
        } else {
            &[Way::Fresh]
        }
    }

    fn name(self) -> &'static str {
        match self {
            Way::Fresh => "fresh",
            Way::Quiet => "quiet",
            Way::Evidence => "evidence",
        }
    }
}

/// The environment a JSONL harness finds its record by, over a root.
pub(super) fn jsonl_vars(harness: Harness) -> fn(&Path) -> Vec<(&'static str, PathBuf)> {
    match harness {
        Harness::Codex => |root| vec![("CODEX_HOME", root.to_path_buf())],
        Harness::Claude => |root| vec![("CLAUDE_CONFIG_DIR", root.to_path_buf())],
        _ => |root| vec![("HOME", root.to_path_buf())],
    }
}

/// Where a JSONL harness keeps the record of `session`, under a root.
pub(super) fn jsonl_file(harness: Harness, session: &str) -> PathBuf {
    match harness {
        Harness::Codex => [
            "sessions",
            "2026",
            "09",
            "06",
            &format!("rollout-2026-09-06T00-00-00-{session}.jsonl"),
        ]
        .iter()
        .collect(),
        Harness::Claude => ["projects", "-work-app", &format!("{session}.jsonl")]
            .iter()
            .collect(),
        _ => [
            ".pi",
            "agent",
            "sessions",
            "--work-app--",
            &format!("2026-09-06T00-00-00-000Z_{session}.jsonl"),
        ]
        .iter()
        .collect(),
    }
}

/// A JSONL fixture written a piece at a time, and a look after each, and one
/// more with the clock moved past Pi's quiet window: how many looks.
pub(super) async fn jsonl(harness: Harness, name: &str, session: &str, way: Way) -> usize {
    let mut rig = Rig::new(&format!("{name} ({})", way.name()), jsonl_vars(harness));
    let file = rig.root.path().join(jsonl_file(harness, session));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    let lines = fixture_lines(name);
    let mut options = Options::default();
    if way == Way::Evidence {
        let settled = rig.path(&["settled"]);
        fs::create_dir_all(&settled).unwrap();
        let frontier = serde_json::from_str::<Value>(lines.last().unwrap()).unwrap()["id"].clone();
        let evidence =
            json!({ "launchId": "launch-1", "sessionId": session, "frontier": { "id": frontier } });
        fs::write(settled.join("launch-1.json"), evidence.to_string()).unwrap();
        options.pi_settlement = Some(PiSettlement {
            directory: Some(settled.to_string_lossy().into_owned()),
            launch_id: Some("launch-1".to_owned()),
        });
    }
    for piece in pieces(&lines) {
        rig.tick(STEP_MS);
        append(&file, &piece);
        let written = if way == Way::Quiet {
            rig.now() - PI_QUIET_MS - 1_000
        } else {
            rig.now()
        };
        stamp(&file, written);
        rig.look(harness, session, &options).await;
    }
    rig.tick(LATE_MS);
    rig.look(harness, session, &options).await;
    rig.finish()
}
