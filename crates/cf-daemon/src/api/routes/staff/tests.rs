//! `GET /api/staff`: the project's members with the model and effort their
//! saved agents have; not the chief, not a member's sessions.

use std::rc::Rc;

use cf_base::refusal::Refusal;
use cf_catalog::{AgentRow, Catalog};
use hyper::Method;
use serde_json::json;

use crate::api::context::{AgentRows, Context};
use crate::api::routes::tests::support::{api, open_task};
use crate::roster::Agents;
use crate::testing::{scene, Scene};

/// The saved agents in a file of a home of their own, which the API reads as
/// the daemon reads them, and the agents it was asked for.
struct Asked {
    agents: Agents,
    asked: std::cell::RefCell<Vec<String>>,
}

impl AgentRows for Asked {
    fn row(&self, agent: &str) -> Result<Option<AgentRow>, Refusal> {
        self.asked.borrow_mut().push(agent.to_owned());
        self.agents.row(agent)
    }
}

/// `scene`, reading the saved agents out of `file` (what it says, as the
/// text of a file).
fn reading(scene: &Scene, file: &str) -> (Rc<Context>, Rc<Asked>, tempfile::TempDir) {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().join("agents.json");
    std::fs::write(&path, file).unwrap();
    let asked = Rc::new(Asked {
        agents: Agents::new(Catalog::bundled().unwrap(), path),
        asked: std::cell::RefCell::new(Vec::new()),
    });
    let context = Rc::new(Context {
        ledger: Rc::clone(&scene.context.ledger),
        credentials: Rc::clone(&scene.context.credentials),
        kick: Rc::clone(&scene.context.kick),
        closing: scene.context.closing.clone(),
        roster: Rc::clone(&asked) as Rc<dyn AgentRows>,
        log: Rc::clone(&scene.context.log),
        trace: Rc::clone(&scene.context.trace),
    });
    (context, asked, home)
}

async fn staff(scene: &Scene, context: &Rc<Context>) -> (u16, serde_json::Value) {
    let screens = crate::screens::testing::inert();
    crate::testing::said(
        crate::api::handle(
            context,
            &screens,
            crate::testing::request(Method::GET, "/api/staff", Some(&scene.chief), ""),
        )
        .await,
    )
}

const AGENTS: &str = r#"{"schemaVersion":1,"agents":[
  {"id":"zeus","kind":"claude-code","model":"claude-sonnet-5","effort":"high"}
]}"#;

#[tokio::test]
async fn a_member_is_said_with_its_roles_tier_harness_and_its_agent_s_model_and_effort() {
    let scene = scene();
    let (context, asked, _home) = reading(&scene, AGENTS);
    let (status, said) = staff(&scene, &context).await;
    assert_eq!(status, 200);
    assert_eq!(
        said.to_string(),
        r#"{"members":[{"handle":"zeus","role":"worker","roles":["worker"],"tier":"standard","harness":"claude-code","model":"claude-sonnet-5","effort":"high"}]}"#
    );
    assert_eq!(*asked.asked.borrow(), ["zeus"]);
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn an_agent_the_human_deleted_has_no_model_and_no_effort() {
    let scene = scene();
    // A member whose saved agent is gone: no catalog entry stands in for it.
    scene
        .context
        .ledger
        .borrow_mut()
        .add_member(
            scene.project.id,
            &cf_ledger::NewMember {
                agent: "gone-agent".to_owned(),
                harness: "claude-code".to_owned(),
                designer: false,
                roles: vec!["reviewer".to_owned()],
                tier: "light".to_owned(),
            },
        )
        .unwrap();
    let (context, _, _home) = reading(&scene, r#"{"schemaVersion":1,"agents":[]}"#);
    let (_, said) = staff(&scene, &context).await;
    let gone = &said["members"][1];
    assert_eq!(gone["handle"], "gone-agent");
    assert_eq!(gone["model"], json!(null));
    assert_eq!(gone["effort"], json!(null));
    assert_eq!(gone["roles"], json!(["reviewer"]));
    assert_eq!(gone["tier"], "light");
}

#[tokio::test]
async fn a_pi_agent_keeps_its_level_as_thinking_which_this_route_never_read() {
    let scene = scene();
    let (context, _, _home) = reading(
        &scene,
        r#"{"schemaVersion":1,"agents":[{"id":"zeus","kind":"pi","model":"m","thinking":"high"}]}"#,
    );
    let (_, said) = staff(&scene, &context).await;
    assert_eq!(said["members"][0]["model"], "m");
    assert_eq!(said["members"][0]["effort"], json!(null));
}

#[tokio::test]
async fn the_staff_is_the_members_and_not_the_chief_nor_the_windows_the_daemon_opened() {
    let scene = scene();
    // The daemon gives an open task to a new window of zeus: a session of it.
    let number = open_task(&scene);
    {
        let mut ledger = scene.context.ledger.borrow_mut();
        let project = ledger.project(scene.project.id).unwrap().unwrap();
        let zeus = project
            .participants
            .iter()
            .find(|p| p.handle == "zeus")
            .unwrap();
        ledger
            .assign_task(scene.project.id, number, zeus.id)
            .unwrap();
        assert!(ledger
            .project(scene.project.id)
            .unwrap()
            .unwrap()
            .participants
            .iter()
            .any(|p| p.member_id.is_some()));
    }
    let (context, asked, _home) = reading(&scene, AGENTS);
    let (_, said) = staff(&scene, &context).await;
    let handles: Vec<&str> = said["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|member| member["handle"].as_str().unwrap())
        .collect();
    assert_eq!(handles, ["zeus"]);
    assert_eq!(
        asked.asked.borrow().len(),
        1,
        "one member, one row asked for"
    );
}

#[tokio::test]
async fn a_saved_agents_file_that_cannot_be_read_fails_the_request_as_any_failure_does() {
    let scene = scene();
    let (context, _, _home) = reading(&scene, "this is not JSON");
    let (status, said) = staff(&scene, &context).await;
    assert_eq!(status, 500);
    assert_eq!(said["error"], "internal");
    assert!(said["message"].is_string());
    // A window that asks of another route is not told.
    let (status, _) = api(&scene, Method::GET, "/api/whoami", &scene.chief, "").await;
    assert_eq!(status, 200);
}
