//! An answer that lacks the list a command is built around fails, as Node's
//! `cf` did when it threw reading it, rather than reading as an empty board,
//! inbox or thread, which would tell the agent something untrue.

mod common;

use cf_board::scripted::{reply, scripted};
use common::cf;
use serde_json::json;

#[test]
fn an_answer_missing_the_list_a_command_reads_fails_instead_of_reading_as_empty() {
    let cases = [
        (vec!["task", "list"], json!({}), "/api/tasks has no open"),
        (
            vec!["task", "list"],
            json!({ "open": [], "lanes": [{ "handle": "zeus" }] }),
            "/api/tasks has no tasks",
        ),
        (
            vec!["inbox"],
            json!({ "messages": null }),
            "/api/inbox has no messages",
        ),
        (
            vec!["staff"],
            json!({ "members": 3 }),
            "/api/staff has no members",
        ),
        (
            vec!["task", "get", "T-3"],
            json!({ "task": { "number": 3 } }),
            "/api/tasks/3 has no messages",
        ),
    ];
    for (args, answer, said) in cases {
        let api = scripted(vec![reply(200, answer)]);
        let ran = cf(
            &args,
            &[("CONSENSFLOW_URL", &api.url), ("CONSENSFLOW_TOKEN", "tok")],
            "",
        );
        assert_eq!(ran.status.code(), Some(1), "{args:?}");
        assert!(ran.stdout.is_empty(), "{args:?}");
        assert_eq!(
            String::from_utf8_lossy(&ran.stderr),
            format!("cf: ConsensFlow's answer to {said}\n"),
            "{args:?}"
        );
    }
}
