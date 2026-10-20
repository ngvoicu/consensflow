//! A window's exit, as the host told it (`pane.exit`, with the time the rig saw it):
//! whose window it was, who closed it, how it ended, and what its close ended of
//! what its turn left running. Pure, so that a test holds it with frames written by
//! hand.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::long_run::look::{ClosedBy, Exit, Process};

/// What tells an exit's window and its closer: the mark every window of the
/// project's id begins with (`p1-`), whose each window is by its id, the windows the
/// run killed and those the daemon asked the host to close (`pane.kill`), by id and
/// generation.
pub struct Known<'a> {
    pub prefix: &'a str,
    pub panes: &'a BTreeMap<String, String>,
    pub killed: &'a [(String, i64)],
    pub kills: &'a [Value],
}

/// The exit `body` the host told at `at`, of a window of the project: none for a
/// window that is none of its (the rig's own, Claude's trust of the workspace). A
/// window the board never showed (one that died as it opened) is named by its id,
/// which is its participant's handle after the project's mark.
pub fn exit_of(body: &Value, at: i64, known: &Known<'_>) -> Option<Exit> {
    let pane = body["id"].as_str()?.to_owned();
    let own = pane.strip_prefix(known.prefix)?;
    let handle = known
        .panes
        .get(&pane)
        .cloned()
        .unwrap_or_else(|| own.to_owned());
    let generation = body["generation"].as_i64().unwrap_or_default();
    let of = |id: &str, gen: Option<i64>| id == pane && gen == Some(generation);
    let closed_by = if known
        .killed
        .iter()
        .any(|(killed, at_gen)| of(killed, Some(*at_gen)))
    {
        ClosedBy::Run
    } else if known.kills.iter().any(|kill| {
        kill["id"]
            .as_str()
            .is_some_and(|id| of(id, kill["generation"].as_i64()))
    }) {
        ClosedBy::Daemon
    } else {
        ClosedBy::Itself
    };
    Some(Exit {
        handle,
        generation,
        at,
        code: body["exitCode"].as_i64(),
        signal: body["signal"].as_str().map(str::to_owned),
        closed_by,
        tail: lines(&body["tail"]),
        ended: processes(&body["background"]),
        pane,
    })
}

/// The processes a list of the host's names: `{ pid, command }` each.
pub fn processes(list: &Value) -> Vec<Process> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|process| {
            Some(Process {
                pid: u32::try_from(process["pid"].as_u64()?).ok()?,
                command: process["command"].as_str().unwrap_or_default().to_owned(),
            })
        })
        .collect()
}

/// The lines of a list of text.
pub fn lines(list: &Value) -> Vec<String> {
    list.as_array()
        .into_iter()
        .flatten()
        .filter_map(|line| line.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn known<'a>(
        panes: &'a BTreeMap<String, String>,
        killed: &'a [(String, i64)],
        kills: &'a [Value],
    ) -> Known<'a> {
        Known {
            prefix: "p1-",
            panes,
            killed,
            kills,
        }
    }

    #[test]
    fn an_exit_is_the_windows_at_the_time_the_host_told_it_with_what_its_close_ended() {
        let panes = BTreeMap::from([(
            "p1-long-devin-1-rusty".to_owned(),
            "long-devin-1-rusty".to_owned(),
        )]);
        let body = json!({
            "id": "p1-long-devin-1-rusty", "generation": 3, "signal": "SIGKILL",
            "tail": ["Running npm test…"],
            "background": [{ "pid": 4300, "command": "node scripts/longrun-dev-server.mjs" }],
        });
        let exit = exit_of(&body, 1_234_567, &known(&panes, &[], &[])).unwrap();
        assert_eq!(
            exit.at, 1_234_567,
            "when the host told it, not the next look"
        );
        assert_eq!(exit.handle, "long-devin-1-rusty");
        assert_eq!(exit.closed_by, ClosedBy::Itself);
        assert_eq!(exit.signal.as_deref(), Some("SIGKILL"));
        assert_eq!(
            exit.ended,
            [Process {
                pid: 4300,
                command: "node scripts/longrun-dev-server.mjs".to_owned()
            }]
        );
        assert_eq!(exit.tail, ["Running npm test…"]);
    }

    #[test]
    fn an_exit_is_closed_by_the_run_or_the_daemon_of_its_own_generation_else_by_itself() {
        let panes = BTreeMap::new();
        let body = json!({ "id": "p1-long-claude-1-x", "generation": 2, "exitCode": 0 });
        let killed = [("p1-long-claude-1-x".to_owned(), 2)];
        assert_eq!(
            exit_of(&body, 0, &known(&panes, &killed, &[]))
                .unwrap()
                .closed_by,
            ClosedBy::Run
        );
        let kills = [json!({ "id": "p1-long-claude-1-x", "generation": 2 })];
        assert_eq!(
            exit_of(&body, 0, &known(&panes, &[], &kills))
                .unwrap()
                .closed_by,
            ClosedBy::Daemon
        );
        // A close the daemon asked of the window's last generation is none of this one's.
        let earlier = [json!({ "id": "p1-long-claude-1-x", "generation": 1 })];
        let exit = exit_of(&body, 0, &known(&panes, &[], &earlier)).unwrap();
        assert_eq!(exit.closed_by, ClosedBy::Itself);
        assert_eq!(exit.handle, "long-claude-1-x", "named by its id");
        // The rig's own window is none of the project's.
        assert!(exit_of(
            &json!({ "id": "trust-1", "generation": 1 }),
            0,
            &known(&panes, &[], &[])
        )
        .is_none());
    }
}
