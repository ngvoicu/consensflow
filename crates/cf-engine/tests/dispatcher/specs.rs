//! A chief whose project has specs on is told where its Specmint skill is and
//! is given the skill at its launch, and only then: a project that has them
//! off, and every member, are asked about nothing and given nothing, and what
//! one project says is not another's. A change applies at the chief's next
//! launch (a Resume, a switch); what the human said as the project opened
//! applies at its first, since it is kept in the write that makes the project.
//! Node's dispatcher had no specs, so no trace holds these.

use cf_engine::testing::Context;
use cf_ledger::{ProjectView, SpecsChange};
use cf_proto::specs::Variant;
use serde_json::{json, Value};

/// The request that opens a project at `directory`, with `said` among its
/// words: apollo its chief, and zeus a worker.
fn request_at(directory: &str, said: &Value) -> Value {
    let mut request = json!({
        "directory": directory,
        "name": directory.trim_start_matches('/'),
        "chief": { "harness": "claude-code", "agent": "apollo" },
        "staff": [{ "agent": "zeus", "harness": "claude-code", "roles": ["worker"], "tier": "standard" }],
    });
    if let (Some(request), Some(said)) = (request.as_object_mut(), said.as_object()) {
        request.extend(said.clone());
    }
    request
}

/// A project at `directory`: apollo its chief, and zeus a worker.
fn project_at(context: &Context, directory: &str) -> ProjectView {
    context
        .open_project(request_at(directory, &json!({})))
        .unwrap()
        .expect("the project")
}

/// The project says this of its specs.
fn says(context: &Context, project: i64, on: Option<bool>, folder: &str, variant: Option<Variant>) {
    let change = SpecsChange {
        on,
        folder: (!folder.is_empty()).then(|| folder.to_owned()),
        variant,
    };
    context
        .ledger
        .borrow_mut()
        .set_specs(project, &change)
        .unwrap();
}

/// The human closes the project and resumes it: the chief's window opens again.
fn relaunch(context: &Context, project: i64) {
    context.close_project(project).unwrap();
    context.resume_project(project).unwrap();
}

/// The launches the adapter was asked for `role` in `project`, oldest first.
fn launches(context: &Context, project: i64, role: &str) -> Vec<Value> {
    context
        .adapter
        .prepared()
        .into_iter()
        .filter(|launch| launch["role"] == role && launch["project"]["id"] == project)
        .collect()
}

/// The last launch of the chief of `project`.
fn chief_launch(context: &Context, project: i64) -> Value {
    launches(context, project, "chief").pop().expect("a launch")
}

#[test]
fn a_chief_of_a_project_with_specs_off_is_told_nothing_of_them_and_no_adapter_is_asked_where_the_skill_is(
) {
    let context = Context::new();
    let project = project_at(&context, "/work/app");
    relaunch(&context, project.id);
    for launch in launches(&context, project.id, "chief") {
        assert_eq!(launch["instructions"], "instructions for chief");
        assert!(launch.get("specs").is_none(), "{launch}");
    }
    // The project says no, whatever the human turned on for every project.
    context.roster.specs_by_default.set(true);
    says(&context, project.id, Some(false), "", None);
    relaunch(&context, project.id);
    let launch = chief_launch(&context, project.id);
    assert_eq!(launch["instructions"], "instructions for chief");
    assert!(launch.get("specs").is_none());
    assert_eq!(
        context.adapter.specmint_asked.borrow().len(),
        0,
        "asked where a skill is only for a chief that is given one"
    );
}

#[test]
fn a_chief_of_a_project_with_specs_on_is_told_where_the_skill_is_and_given_it_at_its_next_launch_and_not_before(
) {
    let context = Context::new();
    let project = project_at(&context, "/work/app");
    let first = chief_launch(&context, project.id);
    assert!(first.get("specs").is_none());
    // The human says it while the chief runs: its window is as it was.
    says(
        &context,
        project.id,
        Some(true),
        "docs/specs",
        Some(Variant::TddHtml),
    );
    assert_eq!(launches(&context, project.id, "chief").len(), 1);
    assert_eq!(context.adapter.specmint_asked.borrow().len(), 0);
    // Its next launch has it.
    relaunch(&context, project.id);
    let launch = chief_launch(&context, project.id);
    let skill =
        "/consensflow/integrations/specmint/fake/tdd-html/.claude/skills/specmint-tdd-html/SKILL.md";
    assert_eq!(
        launch["specs"],
        json!({ "variant": "tdd-html", "skill": skill, "installed": false })
    );
    assert_eq!(
        launch["instructions"],
        format!("instructions for chief with specs: tdd-html in docs/specs at {skill}")
    );
    assert_eq!(*context.adapter.specmint_asked.borrow(), [Variant::TddHtml]);
}

#[test]
fn a_chief_is_given_the_specs_its_project_was_opened_with_at_its_very_first_launch() {
    let context = Context::new();
    let opened = context
        .open_project(request_at(
            "/work/app",
            &json!({ "specs": { "on": true, "folder": "docs/specs", "variant": "tdd-html" } }),
        ))
        .unwrap()
        .expect("the project");
    // The chief launched once, as the project opened: nothing relaunched it.
    assert_eq!(launches(&context, opened.id, "chief").len(), 1);
    let launch = chief_launch(&context, opened.id);
    let skill =
        "/consensflow/integrations/specmint/fake/tdd-html/.claude/skills/specmint-tdd-html/SKILL.md";
    assert_eq!(
        launch["specs"],
        json!({ "variant": "tdd-html", "skill": skill, "installed": false })
    );
    assert_eq!(
        launch["instructions"],
        format!("instructions for chief with specs: tdd-html in docs/specs at {skill}")
    );
    // The project says so, as it does when the human says it afterwards.
    assert_eq!(
        serde_json::to_value(&opened).unwrap()["specs"],
        json!({ "on": true, "folder": "docs/specs", "variant": "tdd-html" })
    );
}

#[test]
fn a_chief_of_a_project_opened_with_specs_is_told_and_given_what_one_is_whose_specs_were_said_after_a_resume(
) {
    let context = Context::new();
    let said = json!({ "on": true, "folder": "docs/specs", "variant": "tdd" });
    let opened = context
        .open_project(request_at("/work/opened", &json!({ "specs": said })))
        .unwrap()
        .expect("the project");
    let later = project_at(&context, "/work/later");
    says(
        &context,
        later.id,
        Some(true),
        "docs/specs",
        Some(Variant::Tdd),
    );
    relaunch(&context, later.id);
    // The same words, the same skill, whether the human said it as the project opened or after:
    // no agent is told anything of specs that an agent was not told before.
    let first = chief_launch(&context, opened.id);
    let after = chief_launch(&context, later.id);
    assert_eq!(first["specs"], after["specs"]);
    assert_eq!(first["instructions"], after["instructions"]);
    assert_eq!(first["role"], after["role"]);
}

#[test]
fn a_project_opened_with_specs_off_or_unsaid_launches_a_chief_told_nothing_of_them() {
    let context = Context::new();
    context.roster.specs_by_default.set(true);
    let off = context
        .open_project(request_at(
            "/work/off",
            &json!({ "specs": { "on": false } }),
        ))
        .unwrap()
        .expect("the project");
    let launch = chief_launch(&context, off.id);
    assert!(launch.get("specs").is_none(), "{launch}");
    // Unsaid, it follows the app's setting, which here is on.
    let follows = context
        .open_project(request_at("/work/follows", &json!({ "specs": null })))
        .unwrap()
        .expect("the project");
    assert!(chief_launch(&context, follows.id).get("specs").is_some());
}

#[test]
fn specs_the_ledger_refuses_make_no_project_and_launch_no_chief() {
    let context = Context::new();
    let refused = context
        .open_project(request_at(
            "/work/app",
            &json!({ "specs": { "on": true, "folder": "../elsewhere" } }),
        ))
        .expect_err("a refusal");
    assert_eq!(
        refused.to_string(),
        "the specs folder is a folder inside the project: it stays inside it, with no `..`"
    );
    assert!(context.ledger.borrow().projects().unwrap().is_empty());
    assert!(
        context.adapter.prepared().is_empty(),
        "no window was asked for"
    );
    assert!(context.adapter.specmint_asked.borrow().is_empty());
}

#[test]
fn a_member_is_never_given_specs_whatever_the_project_says() {
    let context = Context::new();
    let project = project_at(&context, "/work/app");
    says(&context, project.id, Some(true), "", Some(Variant::Core));
    context.roster.specs_by_default.set(true);
    context.pool_task(project.id, "worker", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let worker = launches(&context, project.id, "worker").pop().unwrap();
    assert_eq!(worker["instructions"], "instructions for worker");
    assert!(worker.get("specs").is_none(), "{worker}");
    relaunch(&context, project.id);
    assert!(chief_launch(&context, project.id).get("specs").is_some());
    assert_eq!(
        context.adapter.specmint_asked.borrow().len(),
        1,
        "the chief's launch alone asked"
    );
}

#[test]
fn the_app_s_default_applies_where_the_project_says_nothing_and_the_project_s_own_word_wins_either_way(
) {
    let context = Context::new();
    let project = project_at(&context, "/work/app");
    let on = |context: &Context| chief_launch(context, project.id).get("specs").is_some();
    // (what the project says, what the human turned on for every project, whether the chief is given specs)
    let table = [
        (None, false, false),
        (None, true, true),
        (Some(true), false, true),
        (Some(true), true, true),
        (Some(false), false, false),
        (Some(false), true, false),
    ];
    for (own, default, given) in table {
        context.roster.specs_by_default.set(default);
        says(&context, project.id, own, "", None);
        relaunch(&context, project.id);
        assert_eq!(
            on(&context),
            given,
            "the project says {own:?}, the app {default}"
        );
    }
    // Where the project leaves the folder and the variant unsaid they are the documented ones.
    context.roster.specs_by_default.set(true);
    says(&context, project.id, None, "", None);
    relaunch(&context, project.id);
    let launch = chief_launch(&context, project.id);
    assert_eq!(launch["specs"]["variant"], "core");
    assert!(
        launch["instructions"]
            .as_str()
            .unwrap()
            .contains(" in .specs at "),
        "{launch}"
    );
}

#[test]
fn two_projects_keep_their_own_settings_and_each_chief_is_given_its_own() {
    let context = Context::new();
    let first = project_at(&context, "/work/first");
    let second = project_at(&context, "/work/second");
    says(&context, first.id, Some(true), "plans", Some(Variant::Tdd));
    says(
        &context,
        second.id,
        Some(false),
        "elsewhere",
        Some(Variant::CoreHtml),
    );
    relaunch(&context, first.id);
    relaunch(&context, second.id);
    let one = chief_launch(&context, first.id);
    let two = chief_launch(&context, second.id);
    assert_eq!(one["specs"]["variant"], "tdd");
    assert!(
        one["instructions"]
            .as_str()
            .unwrap()
            .contains("tdd in plans at "),
        "{one}"
    );
    assert!(two.get("specs").is_none(), "{two}");
    assert_eq!(two["instructions"], "instructions for chief");
    // Turning the second on leaves the first as it was.
    says(&context, second.id, Some(true), "", Some(Variant::CoreHtml));
    relaunch(&context, second.id);
    assert_eq!(
        chief_launch(&context, second.id)["specs"]["variant"],
        "core-html"
    );
    assert_eq!(chief_launch(&context, first.id)["specs"]["variant"], "tdd");
}

#[test]
fn a_skill_the_harness_has_installed_is_the_one_the_window_is_given_and_its_text_names_that_file() {
    let context = Context::new();
    let project = project_at(&context, "/work/app");
    says(&context, project.id, Some(true), "", Some(Variant::Tdd));
    let installed = "/home/me/.claude/skills/specmint-tdd/SKILL.md";
    *context.adapter.specmint_installed.borrow_mut() = Some(installed.to_owned());
    relaunch(&context, project.id);
    let launch = chief_launch(&context, project.id);
    assert_eq!(
        launch["specs"],
        json!({ "variant": "tdd", "skill": installed, "installed": true })
    );
    assert_eq!(
        launch["instructions"],
        format!("instructions for chief with specs: tdd in .specs at {installed}")
    );
}
