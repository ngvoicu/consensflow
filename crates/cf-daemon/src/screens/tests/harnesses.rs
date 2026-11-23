//! The harness diagnostics' routes, over scripted feeds and programs: a harness
//! that is none of the five is refused before the machine is asked anything, a
//! check is kept five minutes, and there is no route that installs anything.

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
        let body = format!(r#"{{"id":{id}}}"#);
        assert_eq!(
            the_app_asks(&rig, Method::POST, "/api/harnesses/check", &body).await,
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
    assert_eq!(ids, ["devin", "claude", "codex", "pi", "muse"]);
    assert!(rows
        .iter()
        .all(|row| row["tested"]["newest"].as_str().is_some()));
    assert_eq!(rows[1]["tested"]["release"], Value::Null);
    assert_eq!(rows[1]["tested"]["beyond"], false);
    assert_eq!(
        rows[1].to_string(),
        format!(
            r#"{{"id":"claude","path":null,"installed":false,"checkedAt":{EPOCH_MS},"version":{{"state":"not-installed"}},"update":{{"state":"not-checked"}},"tested":{},"stamp":1}}"#,
            rows[1]["tested"]
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
    script("2.1.0 (Claude Code)\n", "2.2.0");
    let (status, body) = the_app_asks(
        &rig,
        Method::POST,
        "/api/harnesses/check",
        r#"{"id":"claude"}"#,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body["harnesses"][0]["tested"]["release"], "2.1.0");
    assert_eq!(body["harnesses"][0]["tested"]["beyond"], false);
    assert_eq!(
        body["harnesses"].to_string(),
        format!(
            r#"[{{"id":"claude","path":{},"installed":true,"checkedAt":{EPOCH_MS},"version":{{"state":"checked","value":"2.1.0"}},"update":{{"state":"available","value":"2.2.0","source":"https://registry.npmjs.org/@anthropic-ai/claude-code/latest","command":null}},"tested":{},"distribution":null,"stamp":1}}]"#,
            Value::from(installed.to_string_lossy().as_ref()),
            body["harnesses"][0]["tested"]
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
    script("2.2.0 (Claude Code)\n", "2.2.0");
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

/// T-2091: the page is read-only. Nothing in the daemon installs a harness: no route for it,
/// and no code behind one.
#[tokio::test]
async fn the_daemon_has_no_route_that_installs_a_harness() {
    let rig = Rig::new(&["codex"]);
    let path = ["/api/harnesses/", "update"].concat();
    assert_eq!(recognize(&path), None, "no screen is the route");
    let answered = ask_as_the_app(&rig.screens, Method::POST, &path, r#"{"id":"codex"}"#).await;
    assert!(answered.is_none(), "the screens answer nothing at it");
    assert!(rig.capture.take_ran().is_empty(), "nothing was run");
}

#[test]
fn no_source_of_the_daemon_names_an_install_route() {
    let sources = [
        ("screens/mod.rs", include_str!("../mod.rs")),
        ("screens/harnesses.rs", include_str!("../harnesses.rs")),
        (
            "screens/pages/harnesses.html",
            include_str!("../pages/harnesses.html"),
        ),
    ];
    let route = ["harnesses/", "update"].concat();
    for (file, text) in sources {
        assert!(!text.contains(&route), "{file} names {route}");
        assert!(
            !text.contains("HarnessUpdate"),
            "{file} has an install screen"
        );
        assert!(
            !text.contains("update_harness"),
            "{file} has an install handler"
        );
    }
}
