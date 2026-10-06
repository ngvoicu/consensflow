//! The harness diagnostics' routes, over scripted feeds and programs: a harness
//! that is none of the five is refused before the machine is asked anything, a
//! check is kept five minutes, and an update says what became of it.

use cf_harness::testing::EPOCH_MS;
use cf_proto::agents::Harness;

use super::*;

#[tokio::test]
async fn a_harness_that_is_none_of_the_five_is_refused_before_the_machine_is_asked_anything() {
    let rig = Rig::new(&["claude"]);
    for id in [
        "\"nope\"",
        "5",
        "\"\"",
        r#"["claude"]"#,
        "null",
        "\"Claude\"",
        "\"claude-code\"",
    ] {
        for route in ["check", "update"] {
            let body = format!(r#"{{"id":{id}}}"#);
            assert_eq!(
                the_app_asks(
                    &rig,
                    Method::POST,
                    &format!("/api/harnesses/{route}"),
                    &body
                )
                .await,
                (400, error("Unknown harness")),
                "{route} {body}"
            );
        }
    }
    // An update names one; a check may name none.
    for body in ["{}", r#"{"refresh":true}"#] {
        assert_eq!(
            the_app_asks(&rig, Method::POST, "/api/harnesses/update", body).await,
            (400, error("Unknown harness")),
            "{body}"
        );
    }
    assert!(rig.capture.take_ran().is_empty(), "no program was run");
    assert!(rig.latest.take_asked().is_empty(), "no feed was asked");
}

#[tokio::test]
async fn every_harness_is_checked_in_the_order_the_page_lists_them() {
    let rig = Rig::new(&[]);
    let (status, body) = the_app_asks(&rig, Method::POST, "/api/harnesses/check", "{}").await;
    assert_eq!(status, 200);
    let rows: Vec<&Value> = body["harnesses"].as_array().unwrap().iter().collect();
    let ids: Vec<&str> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["devin", "claude", "codex", "opencode", "pi"]);
    assert_eq!(
        rows[1].to_string(),
        format!(
            r#"{{"id":"claude","path":null,"installed":false,"checkedAt":{EPOCH_MS},"version":{{"state":"not-installed"}},"update":{{"state":"not-checked"}}}}"#
        )
    );
}

#[tokio::test]
async fn a_check_of_one_harness_is_kept_five_minutes_and_looked_at_again_only_when_asked_to() {
    let rig = Rig::new(&["claude"]);
    let (installed, ran) = (&rig.installed[0].1, || rig.capture.take_ran().len());
    let script = |version: &str, release: &str| {
        rig.capture.says("claude --version", version);
        rig.latest.says(Harness::Claude, release);
    };
    script("claude 2.1.0\n", "2.2.0");
    let (status, body) = the_app_asks(
        &rig,
        Method::POST,
        "/api/harnesses/check",
        r#"{"id":"claude"}"#,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        body["harnesses"].to_string(),
        format!(
            r#"[{{"id":"claude","path":{},"installed":true,"checkedAt":{EPOCH_MS},"version":{{"state":"checked","value":"2.1.0"}},"update":{{"state":"available","value":"2.2.0","source":"https://registry.npmjs.org/@anthropic-ai/claude-code/latest","command":null}},"distribution":null}}]"#,
            Value::from(installed.to_string_lossy().as_ref())
        )
    );
    assert_eq!(ran(), 1);
    // Kept: asked again, it is not looked at again, `refresh` being anything but `true` itself.
    for body in [
        r#"{"id":"claude"}"#,
        r#"{"id":"claude","refresh":"true"}"#,
        r#"{"id":"claude","refresh":1}"#,
    ] {
        let (status, _) = the_app_asks(&rig, Method::POST, "/api/harnesses/check", body).await;
        assert_eq!(status, 200, "{body}");
    }
    assert_eq!(ran(), 0, "kept");
    script("claude 2.2.0\n", "2.2.0");
    let (_, body) = the_app_asks(
        &rig,
        Method::POST,
        "/api/harnesses/check",
        r#"{"id":"claude","refresh":true}"#,
    )
    .await;
    assert_eq!(ran(), 1, "looked at again");
    assert_eq!(body["harnesses"][0]["update"]["state"], "current");
}

#[tokio::test]
async fn an_update_of_a_harness_that_is_not_installed_says_so_by_its_name() {
    let rig = Rig::new(&[]);
    for (id, name) in [
        ("claude", "Claude"),
        ("codex", "Codex"),
        ("opencode", "OpenCode"),
        ("pi", "Pi"),
        ("devin", "Devin"),
    ] {
        let body = format!(r#"{{"id":"{id}"}}"#);
        assert_eq!(
            the_app_asks(&rig, Method::POST, "/api/harnesses/update", &body).await,
            (400, error(&format!("{name} is not installed"))),
            "{id}"
        );
    }
}

#[tokio::test]
async fn an_update_of_a_harness_installed_in_a_way_nothing_recognizes_runs_nothing() {
    let rig = Rig::new(&["claude"]);
    rig.capture.says("claude --version", "claude 2.1.0\n");
    rig.latest.says(Harness::Claude, "2.2.0");
    let (status, body) = the_app_asks(
        &rig,
        Method::POST,
        "/api/harnesses/update",
        r#"{"id":"claude"}"#,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let result = &body["result"];
    assert_eq!(
        result["reason"],
        "ConsensFlow does not recognize how Claude was installed here: update it the way you installed it."
    );
    let said: Vec<&str> = result
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(said, ["id", "state", "reason", "harness"]);
    assert_eq!(result["state"], "unsupported");
    assert_eq!(result["harness"]["version"]["value"], "2.1.0");
    assert_eq!(
        rig.capture.take_ran().len(),
        1,
        "the version only: the update was not run"
    );
}
