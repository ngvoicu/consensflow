//! `GET /api/guide`: the chief's, with the staff and rules as they are now.

use std::rc::Rc;

use hyper::Method;

use crate::api::context::Context;
use crate::testing::{scene, Scene};

async fn guide(context: &Rc<Context>, token: &str) -> (u16, serde_json::Value) {
    let screens = crate::screens::testing::inert();
    crate::testing::said(
        crate::api::handle(
            context,
            &screens,
            crate::testing::request(Method::GET, "/api/guide", Some(token), ""),
        )
        .await,
    )
}

fn write_rules(scene: &Scene, rules: Option<&str>) {
    scene
        .context
        .ledger
        .borrow_mut()
        .set_staff_rules(scene.project.id, rules)
        .unwrap();
}

#[tokio::test]
async fn the_guide_follows_the_project_s_own_hands_switch() {
    let scene = scene();
    assert!(!scene.project.own_hands);
    write_rules(&scene, Some("- A current staff rule."));
    *scene.specs_folder.borrow_mut() = Some("docs/specs".to_owned());
    let specs = format!(
        "\n## This project's specs\n\nSpecs are on; the skill is /skills/specmint/SKILL.md.\n{}",
        cf_engine::roles::ProjectSpecs::board_for("docs/specs")
    );
    *scene.ordinary_specs.borrow_mut() = Some(specs.clone());
    let text = guide(&scene.context, &scene.chief).await.1["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        text.contains("You run this project for the human as its Chief of Staff"),
        "an ordinary chief must get the ordinary opening: {text}"
    );
    assert!(text.contains("You do not change the project yourself"));
    assert!(!text.contains("Use the board only when the\nhuman explicitly asks you to"));
    assert!(text.contains(&specs));
    assert!(text.ends_with("- A current staff rule.\n"));

    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, true)
        .unwrap();
    let text = guide(&scene.context, &scene.chief).await.1["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(text.contains("Use the board only when the\nhuman explicitly asks you to"));
    assert!(!text.contains("You do not change the project yourself"));
    assert!(text.contains(&cf_engine::roles::ProjectSpecs::board_for("docs/specs")));
    assert!(!text.contains("the skill is /skills/specmint/SKILL.md"));
    assert!(text.ends_with("- A current staff rule.\n"));

    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, false)
        .unwrap();
    let text = guide(&scene.context, &scene.chief).await.1["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(text.contains("You do not change the project yourself"));
    assert!(!text.contains("Use the board only when the\nhuman explicitly asks you to"));
    assert!(text.contains(&specs));
    assert!(text.ends_with("- A current staff rule.\n"));
}

#[tokio::test]
async fn the_chief_is_given_the_guide_with_the_staff_and_no_frontmatter() {
    let scene = scene();
    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, true)
        .unwrap();
    let (status, said) = guide(&scene.context, &scene.chief).await;
    assert_eq!(status, 200);
    let text = said["text"].as_str().unwrap();
    assert!(text.starts_with("# ConsensFlow Chief of Staff\n\nThis is the guide to the board"));
    assert!(text.contains("zeus"), "the staff as it is: {text}");
    assert!(text.contains("task add --tier"));
    assert!(!text.contains("{{") && !text.contains("---\nname:"));
    assert!(!text.contains("The project's rules for the staff"));
    assert_eq!(scene.kicks.get(), 0);
}

#[tokio::test]
async fn the_project_s_rules_for_its_staff_are_in_the_guide_as_they_are_now() {
    let scene = scene();
    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, true)
        .unwrap();
    write_rules(&scene, Some("- Push to origin only."));
    let text = guide(&scene.context, &scene.chief).await.1["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(text.ends_with("- Push to origin only.\n"), "{text}");
    write_rules(&scene, Some("- Another rule."));
    let text = guide(&scene.context, &scene.chief).await.1["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(text.ends_with("- Another rule.\n") && !text.contains("Push to origin"));
}

#[tokio::test]
async fn where_the_project_has_specs_on_the_guide_tells_the_chief_what_an_ordinary_chief_is_told_of_them(
) {
    let scene = scene();
    scene
        .context
        .ledger
        .borrow_mut()
        .set_own_hands(scene.project.id, true)
        .unwrap();
    let text = guide(&scene.context, &scene.chief).await.1["text"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(
        !text.contains("Use a spec when the human asks"),
        "off: {text}"
    );
    *scene.specs_folder.borrow_mut() = Some("docs/specs".to_owned());
    write_rules(&scene, Some("- Push to origin only."));
    let text = guide(&scene.context, &scene.chief).await.1["text"]
        .as_str()
        .unwrap()
        .to_owned();
    let board = cf_engine::roles::ProjectSpecs::board_for("docs/specs");
    let at = text.find(&board).expect("the specs and the board");
    assert!(
        text[at + board.len()..].contains("- Push to origin only."),
        "the rules come after"
    );
    assert!(
        !text.contains("Specs are on in this project"),
        "where the skill is stays in the short text: {text}"
    );
}

#[tokio::test]
async fn an_ordinary_guide_is_refused_when_its_specs_cannot_be_resolved() {
    let mut scene = scene();
    Rc::get_mut(&mut scene.context).unwrap().ordinary_specs = Rc::new(|_, _| {
        Err(cf_base::refusal::Refusal::with_status(
            "starting",
            "try again",
            503,
        ))
    });
    let (status, said) = guide(&scene.context, &scene.chief).await;
    assert_eq!(status, 503);
    assert_eq!(said["error"], "starting");
}

#[tokio::test]
async fn a_member_is_told_the_guide_is_the_chief_s() {
    let scene = scene();
    let (status, said) = guide(&scene.context, &scene.zeus).await;
    assert_eq!(status, 403);
    assert_eq!(said["error"], "not-the-chief");
}
