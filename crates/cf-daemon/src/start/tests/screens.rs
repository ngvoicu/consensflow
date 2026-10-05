//! The screens as a started daemon serves them: on its own address under the
//! token its handle line gave, over its own environment, and a change to the
//! agents reaching the members and the page as the start wired it. (What each
//! screen answers is held to Node's recordings in `tests/screens`.)

use cf_ledger::{NewChief, NewMember, NewProject};
use tokio::io::AsyncReadExt;

use super::*;

/// One request to the daemon's own address: its status and its body.
async fn call(
    url: &str,
    method: &str,
    target: &str,
    token: Option<&str>,
    body: &str,
) -> (u16, String) {
    let address = url.strip_prefix("http://").unwrap().trim_end_matches('/');
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    let auth = token.map_or_else(String::new, |token| {
        format!("Authorization: Bearer {token}\r\n")
    });
    let request = format!(
        "{method} {target} HTTP/1.1\r\nHost: t\r\n{auth}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answered = Vec::new();
    stream.read_to_end(&mut answered).await.unwrap();
    let text = String::from_utf8_lossy(&answered).into_owned();
    let (head, body) = text.split_once("\r\n\r\n").unwrap();
    (
        head.split(' ').nth(1).unwrap().parse().unwrap(),
        body.to_owned(),
    )
}

#[tokio::test]
async fn the_daemon_serves_the_screens_under_its_token_over_its_own_environment() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            let handle = rig.daemon.handle().clone();

            let (status, page) =
                call(&handle.url, "GET", "/harnesses", Some(&handle.token), "").await;
            assert_eq!(status, 200);
            assert!(page.contains(&format!("\nconst TOKEN = \"{}\";\n", handle.token)));
            let (status, page) = call(
                &handle.url,
                "GET",
                &format!("/?token={}", handle.token),
                None,
                "",
            )
            .await;
            assert_eq!(status, 200);
            assert!(page.contains(&format!("<span>v{VERSION}</span>")));

            // The agents offered are those of the harnesses the daemon's PATH has: `claude`, and no other.
            let (status, agents) =
                call(&handle.url, "GET", "/api/agents", Some(&handle.token), "").await;
            assert_eq!(status, 200);
            let agents: Value = serde_json::from_str(&agents).unwrap();
            assert_eq!(agents["harnesses"], json!(["claude"]));
            assert_eq!(agents["preferences"], json!({ "ownHarnessOnly": false }));

            // Nobody else: no token, and none of the agents' windows' (which the API checks).
            let (status, body) = call(&handle.url, "GET", "/api/agents", None, "").await;
            assert_eq!(
                (status, body.as_str()),
                (401, r#"{"error":"unauthorized"}"#)
            );
            let (status, body) = call(
                &handle.url,
                "GET",
                "/api/agents",
                Some("a-window-s-token"),
                "",
            )
            .await;
            assert_eq!(
                (status, body.as_str()),
                (401, r#"{"error":"unauthorized"}"#)
            );
        })
        .await;
}

#[tokio::test]
async fn a_change_to_the_agents_moves_the_members_tiers_and_tells_the_page() {
    LocalSet::new()
        .run_until(async {
            let root = tempfile::tempdir().unwrap();
            let rig = started(root.path()).await;
            let handle = rig.daemon.handle().clone();
            let events: Rc<RefCell<Vec<Value>>> = Rc::default();
            let told = Rc::clone(&events);
            rig.app.on_event("state.changed", move |body| {
                told.borrow_mut().push(body.clone());
            });

            // An agent of the human's own, through the screen, and a member that runs it.
            let (status, body) = call(
                &handle.url,
                "POST",
                "/api/agents",
                Some(&handle.token),
                r#"{"name":"mine","harness":"claude","model":"claude-opus-5","workTier":"standard"}"#,
            )
            .await;
            assert_eq!(status, 201, "{body}");
            let (project, tier) = {
                let mut ledger = rig.daemon.parts.ledger.borrow_mut();
                let project = ledger
                    .create_project(&NewProject {
                        directory: root.path().to_string_lossy().into_owned(),
                        name: "app".to_owned(),
                        chief: NewChief {
                            harness: "claude-code".to_owned(),
                            agent: Some("mine".to_owned()),
                        },
                        staff: Vec::new(),
                        gate: false,
                    })
                    .unwrap();
                ledger
                    .add_member(
                        project.id,
                        &NewMember {
                            agent: "mine".to_owned(),
                            harness: "claude-code".to_owned(),
                            designer: false,
                            roles: vec!["worker".to_owned()],
                            tier: "standard".to_owned(),
                        },
                    )
                    .unwrap();
                (project.id, "standard")
            };
            let member_tier = || {
                let ledger = rig.daemon.parts.ledger.borrow();
                let project = ledger.project(project).unwrap().unwrap();
                project
                    .participants
                    .iter()
                    .find(|member| member.handle == "mine")
                    .and_then(|member| member.tier.clone())
            };
            assert_eq!(member_tier().as_deref(), Some(tier));

            let (status, body) = call(
                &handle.url,
                "PATCH",
                "/api/agents/mine",
                Some(&handle.token),
                r#"{"workTier":"critical"}"#,
            )
            .await;
            assert_eq!(status, 200, "{body}");
            assert_eq!(
                member_tier().as_deref(),
                Some("critical"),
                "the member follows its agent's tier before the answer is out"
            );
            let until = Instant::now() + Duration::from_secs(10);
            while !events.borrow().contains(&json!({ "reason": "roster" })) {
                assert!(Instant::now() < until, "the page was told: {:?}", events.borrow());
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await;
}
