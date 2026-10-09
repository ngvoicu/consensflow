//! `cf ui --json --no-open speaks the bridge after its handle line`: the bridge
//! as the app meets it. The daemon's end of it is held by
//! `crates/cf-bridge/src/local/tests/`, and this is the process, `cf ui`, with
//! the handle line the app reads before it speaks.

use std::sync::mpsc;

use cf_e2e::daemon::Home;
use cf_e2e::daemon_log::assert_started;
use cf_e2e::wire;
use regex::Regex;
use serde_json::json;

use crate::{secs, Outcome};

#[test]
fn keeps_the_handle_line_first_then_answers_a_ping_frame_then_exits_on_eof() -> Outcome {
    // `cf ui` as the app runs it: the native `cf`, and the daemon it starts is
    // the native one: the start line in its log says so.
    let home = Home::new()?;
    let mut daemon = home.daemon()?;
    let handle = daemon.handle(secs(10))?;
    assert_started(&home.log(), daemon.id())?;
    let url = handle["url"].as_str().unwrap_or_default();
    assert!(!url.is_empty());
    assert!(
        Regex::new(r"^http://127\.0\.0\.1:\d+/$")?.is_match(url),
        "{url}"
    );
    assert!(handle["token"].is_string(), "{handle}");

    let (lines, said) = mpsc::channel();
    daemon.read_each(
        move |line| {
            let _ = lines.send(line);
            true
        },
        || {},
    );
    daemon.send(&wire::request("r-1", "ping", &json!({})));
    let answer = said.recv_timeout(secs(10))?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&answer)?,
        json!({ "v": 1, "id": "r-1", "kind": "res", "op": "ping", "body": { "ok": true } })
    );

    daemon.end_input();
    let code = daemon.exit_code(secs(10))?;
    assert!(code.is_some(), "the daemon kept serving");
    assert_eq!(code, Some(0));
    Ok(())
}
