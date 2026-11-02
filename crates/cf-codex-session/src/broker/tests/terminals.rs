//! `GET /terminals`: the commands Codex keeps running for the shown thread, from Codex's
//! server (`thread/backgroundTerminals/list`), which no process tree names (T-1137).

use serde_json::{json, Value};

use super::fixture::{http, run, Fixture, A, TOKEN};

/// `GET /terminals`: its status and its body.
async fn terminals(f: &Fixture) -> (u16, Value) {
    let reply = http(
        f.address(),
        "GET",
        "/terminals",
        &[("authorization", &format!("Bearer {TOKEN}"))],
        b"",
    )
    .await;
    (reply.status, reply.json())
}

#[test]
fn the_commands_codex_keeps_running_for_the_shown_thread_are_its_own_list_of_them() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        // No thread shown: nothing to ask, and nothing runs for it.
        let (status, body) = terminals(&f).await;
        assert_eq!(status, 200);
        assert_eq!(body["commands"], json!([]));
        f.start_thread(&tui, 1, json!({ "id": A, "status": { "type": "idle" } }))
            .await;
        let ((status, body), asked) = tokio::join!(
            terminals(&f),
            f.respond(
                "thread/backgroundTerminals/list",
                json!({ "data": [
                    { "command": "sh ./ignore-signals-cf-stops-live.sh", "cwd": "/w", "itemId": "i", "processId": "94610", "osPid": 94611 },
                    { "command": "ping -n 361 127.0.0.1", "cwd": "/w", "itemId": "j", "processId": "7" },
                ], "nextCursor": null }),
            ),
        );
        assert_eq!(asked["params"]["threadId"], A);
        assert_eq!(status, 200);
        assert_eq!(
            body["commands"],
            json!([
                "sh ./ignore-signals-cf-stops-live.sh",
                "ping -n 361 127.0.0.1"
            ])
        );
        assert!(body["launchId"].is_string());
    });
}

#[test]
fn a_list_codex_refuses_or_answers_with_no_list_is_no_word_of_what_runs() {
    run(async {
        let f = Fixture::start().await;
        let tui = f.connect().await;
        f.start_thread(&tui, 1, json!({ "id": A, "status": { "type": "idle" } }))
            .await;
        let ((status, _), _) = tokio::join!(
            terminals(&f),
            f.respond_error(
                "thread/backgroundTerminals/list",
                json!({ "message": "unknown method" })
            ),
        );
        assert_eq!(status, 503, "a refusal is no list of none");
        let ((status, _), _) = tokio::join!(
            terminals(&f),
            f.respond(
                "thread/backgroundTerminals/list",
                json!({ "data": [{ "cwd": "/w" }] })
            ),
        );
        assert_eq!(status, 503, "a terminal with no command is no answer");
    });
}
