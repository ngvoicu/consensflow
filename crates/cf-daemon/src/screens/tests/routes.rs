//! The agents' routes, over a home of their own: the list the screen offers and
//! the roster's writes (each followed by the roster's change being told, a
//! refusal by none). What each route reads of a body that is no object is here
//! too, as JavaScript read it.

use cf_proto::agents::{AgentView, Harness, Profile, WorkTier};

use super::*;

fn roster_text(rig: &Rig) -> Option<String> {
    std::fs::read_to_string(rig.roster_file()).ok()
}

#[tokio::test]
async fn the_agents_are_the_catalog_s_as_the_pickers_offer_them() {
    let rig = Rig::new(&["claude", "pi"]);
    let (status, body) = the_app_asks(&rig, Method::GET, "/api/agents", "").await;
    assert_eq!(status, 200);
    let keys: Vec<&str> = body
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["agents", "harnesses", "efforts", "preferences"]);
    assert_eq!(
        body["harnesses"],
        json!(["claude", "pi"]),
        "installed, in the roster's order"
    );
    assert_eq!(body["preferences"], json!({ "ownHarnessOnly": false }));
    let efforts = body["efforts"].as_object().unwrap();
    let harnesses: Vec<&str> = efforts.keys().map(String::as_str).collect();
    assert_eq!(harnesses, ["claude", "codex", "pi", "opencode", "devin"]);
    assert_eq!(
        efforts["claude"],
        json!(["low", "medium", "high", "xhigh", "max"])
    );
    let agents = body["agents"].as_array().unwrap();
    let (mut offered, mut hidden) = (0, 0);
    for agent in agents {
        let installed = matches!(agent["harness"].as_str(), Some("claude" | "pi"));
        // The two keys come last and together, for an agent of a harness not installed here.
        let said: Vec<&str> = agent
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            said.ends_with(&["hidden", "notInstalled"]),
            !installed,
            "{agent}"
        );
        assert_eq!(agent.get("notInstalled").is_some(), !installed, "{agent}");
        if installed {
            offered += 1;
        } else {
            hidden += 1;
        }
    }
    assert!(offered > 0 && hidden > 0, "{offered} offered, {hidden} not");
}

fn view(name: &str, harness: Option<&str>) -> AgentView {
    AgentView {
        name: Some(name.to_owned()),
        harness: harness.map(str::to_owned),
        designer: false,
        model: None,
        work_tier: None,
        effort: None,
        description: None,
        preset: None,
        custom: false,
        profile: Profile {
            model_key: "m".to_owned(),
            model_label: "M".to_owned(),
            route_label: "R".to_owned(),
            route_note: None,
            work_tier: WorkTier::Light,
        },
        unsupported: false,
        hidden: false,
    }
}

#[test]
fn an_agent_that_cannot_be_offered_is_said_hidden_and_not_installed_in_that_order() {
    let kept_out = AgentView {
        hidden: true,
        ..view("kept-out", Some("codex"))
    };
    let kimi = AgentView {
        unsupported: true,
        ..view("kimi", Some("kimi"))
    };
    let offered = offerable(
        vec![
            view("here", Some("claude")),
            view("away", Some("codex")),
            kept_out,
            kimi,
            view("nameless", None),
        ],
        &[Harness::Codex],
    )
    .unwrap();
    let written: Vec<String> = offered.iter().map(Value::to_string).collect();
    assert!(!written[0].contains("hidden") && !written[0].contains("notInstalled"));
    assert!(
        written[1].ends_with(r#""workTier":"light"},"hidden":true,"notInstalled":true}"#),
        "{}",
        written[1]
    );
    // A `hidden` the view has stays where it was, the other follows it.
    assert!(
        written[2].ends_with(r#""hidden":true,"notInstalled":true}"#),
        "{}",
        written[2]
    );
    assert_eq!(written[2].matches("hidden").count(), 1);
    // A row for a harness this build does not run is out of the picker, installed or not.
    assert!(
        written[3].contains(r#""hidden":true,"notInstalled":true"#),
        "{}",
        written[3]
    );
    // One with no harness at all is no harness missing.
    assert!(!written[4].contains("notInstalled"), "{}", written[4]);
}

#[tokio::test]
async fn an_agent_is_added_edited_and_removed_and_each_is_told() {
    let rig = Rig::new(&["claude"]);
    let (status, body) = the_app_asks(
        &rig,
        Method::POST,
        "/api/agents",
        r#"{"name":"mine","harness":"claude","model":"claude-opus-5"}"#,
    )
    .await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body["agent"]["name"], "mine");
    assert_eq!(body["agent"]["custom"], true);
    assert_eq!(rig.changes.get(), 1);
    assert!(roster_text(&rig).unwrap().contains(r#""id": "mine""#));

    let (status, body) = the_app_asks(
        &rig,
        Method::PATCH,
        "/api/agents/mine",
        r#"{"model":"claude-fable-5-1","workTier":"complex"}"#,
    )
    .await;
    assert_eq!(
        (status, body["agent"]["workTier"].as_str()),
        (200, Some("complex"))
    );
    assert_eq!(rig.changes.get(), 2);

    let answered = ask_as_the_app(&rig.screens, Method::DELETE, "/api/agents/mine", "").await;
    assert_eq!(answered, Some((204, Content::Nothing)));
    assert_eq!(rig.changes.get(), 3);
    assert!(!roster_text(&rig).unwrap().contains("mine"));
}

#[tokio::test]
async fn a_write_the_roster_refuses_is_a_400_in_its_words_and_is_not_told() {
    let rig = Rig::new(&[]);
    for (method, target, body, words) in [
        (
            Method::POST,
            "/api/agents",
            r#"{"name":"Bad Name","harness":"claude","model":"m"}"#,
            "agent names are lowercase [a-z0-9-] starting with a letter; got \"Bad Name\"",
        ),
        (
            Method::PATCH,
            "/api/agents/nobody",
            r#"{"model":"x"}"#,
            "no agent named nobody",
        ),
        (
            Method::DELETE,
            "/api/agents/nobody",
            "",
            "no agent named nobody",
        ),
        (
            Method::POST,
            "/api/preferences",
            r#"{"ownHarnessOnly":"yes"}"#,
            "ownHarnessOnly is on or off",
        ),
        (
            Method::POST,
            "/api/preferences",
            r#"{"x":true}"#,
            "no preference named x",
        ),
    ] {
        assert_eq!(
            the_app_asks(&rig, method.clone(), target, body).await,
            (400, error(words)),
            "{method} {target}"
        );
    }
    assert_eq!(rig.changes.get(), 0);
    assert_eq!(
        roster_text(&rig),
        None,
        "nothing was written for any of them"
    );
}

#[tokio::test]
async fn a_change_that_could_not_be_told_fails_the_request_with_its_words_the_change_made() {
    let rig = Rig::new(&[]);
    *rig.failure.borrow_mut() = Some("the tiers could not follow".to_owned());
    let (status, body) = the_app_asks(
        &rig,
        Method::POST,
        "/api/agents",
        r#"{"name":"mine","harness":"claude","model":"m"}"#,
    )
    .await;
    assert_eq!((status, body), (400, error("the tiers could not follow")));
    assert_eq!(rig.changes.get(), 1);
    assert!(
        roster_text(&rig).unwrap().contains(r#""id": "mine""#),
        "the agent was added all the same"
    );
}

#[tokio::test]
async fn the_preferences_are_set_kept_and_told() {
    let rig = Rig::new(&[]);
    for (body, expected) in [
        (r#"{"ownHarnessOnly":true}"#, true),
        (r#"{}"#, true),
        (r#"{"ownHarnessOnly":false}"#, false),
    ] {
        let (status, answered) = the_app_asks(&rig, Method::POST, "/api/preferences", body).await;
        assert_eq!(
            (status, answered),
            (
                200,
                json!({ "preferences": { "ownHarnessOnly": expected } })
            ),
            "{body}"
        );
    }
    assert_eq!(
        rig.changes.get(),
        3,
        "each is a write, told, whether or not it changed anything"
    );
}

#[tokio::test]
async fn a_body_that_is_no_object_is_read_as_javascript_read_it() {
    let rig = Rig::new(&[]);
    let null_read = |property: &str| {
        error(&format!(
            "Cannot read properties of null (reading '{property}')"
        ))
    };
    // `null` has no properties; any other value has none of the ones a route reads.
    assert_eq!(
        the_app_asks(&rig, Method::POST, "/api/agents", "null").await,
        (400, null_read("name"))
    );
    assert_eq!(
        the_app_asks(&rig, Method::PATCH, "/api/agents/mine", "null").await,
        (400, null_read("workTier"))
    );
    for body in ["[]", r#""x""#, "5", "true"] {
        assert_eq!(
            the_app_asks(&rig, Method::POST, "/api/agents", body).await,
            (
                400,
                error("agent names are lowercase [a-z0-9-] starting with a letter; got undefined")
            ),
            "{body}"
        );
    }
    // The preferences read `patch ?? {}`: `null` is no patch, and a number or a flag too.
    for body in ["null", "5", "true", "[]"] {
        assert_eq!(
            the_app_asks(&rig, Method::POST, "/api/preferences", body).await,
            (200, json!({ "preferences": { "ownHarnessOnly": false } })),
            "{body}"
        );
    }
    assert_eq!(
        the_app_asks(&rig, Method::POST, "/api/preferences", r#""x""#).await,
        (400, error("no preference named 0"))
    );
    for (route, id) in [("check", "id"), ("update", "id")] {
        assert_eq!(
            the_app_asks(
                &rig,
                Method::POST,
                &format!("/api/harnesses/{route}"),
                "null"
            )
            .await,
            (400, null_read(id)),
            "{route}"
        );
    }
}

#[tokio::test]
async fn a_roster_that_cannot_be_read_is_said_with_its_path_by_every_route_that_reads_it() {
    let rig = Rig::new(&[]);
    std::fs::create_dir_all(rig.roster_file().parent().unwrap()).unwrap();
    std::fs::write(rig.roster_file(), r#"{ "agents": [,] }"#).unwrap();
    let words = format!(
        "Your agents file {} is not valid JSON: fix it or move it away. ConsensFlow left it as it is.",
        rig.roster_file().display()
    );
    for (method, target, body) in [
        (Method::GET, "/api/agents", ""),
        (
            Method::POST,
            "/api/agents",
            r#"{"name":"zed","harness":"claude","model":"m"}"#,
        ),
        (Method::PATCH, "/api/agents/zed", r#"{"model":"m"}"#),
        (Method::DELETE, "/api/agents/zed", ""),
        (Method::POST, "/api/preferences", "{}"),
    ] {
        assert_eq!(
            the_app_asks(&rig, method.clone(), target, body).await,
            (400, error(&words)),
            "{method} {target}"
        );
    }
    assert_eq!(rig.changes.get(), 0);
    assert_eq!(
        roster_text(&rig).as_deref(),
        Some(r#"{ "agents": [,] }"#),
        "left as it was"
    );
}
