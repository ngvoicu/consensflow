//! The project's rules for its staff (spec smaller-contexts, SCX-14): the human
//! writes them on the page, and every window of the project is given them at the
//! end of its text, whatever its harness: a member's at its launch, the chief's at
//! its next launch (a Resume), and a conversation whose harness keeps its first
//! instructions (Codex) by the note that says its role again, once, chief and
//! member alike, as for any change of a role. A change of the rules, or taking
//! them away, is a change of the role like any other.

use cf_engine::roles::with_staff_rules;
use cf_engine::testing::{Context, Made};
use serde_json::{json, Value};

use super::members::{finished_session, follow_up};
use super::{chief_launches, codex, recorded, relaunch, replacements};

/// Rules a human writes, in more than one line.
const RULES: &str = "- Push to origin only, never to upstream.\n- Tests use the .smoke bundle id.";

/// The rules after the human changed them.
const CHANGED: &str = "- Push to origin only.";

/// Every harness a window of a project runs on.
const HARNESSES: [&str; 5] = ["claude-code", "codex", "pi", "devin", "muse"];

/// The members' roles that take a task by tier, with the agent that holds each.
const MEMBERS: [(&str, &str); 4] = [
    ("worker", "zeus"),
    ("advisor", "athena"),
    ("reviewer", "argus"),
    ("tester", "tess"),
];

/// The fake role text of `role` with `rules` after it, as the engine gives it.
fn told(role: &str, rules: Option<&str>) -> String {
    with_staff_rules(format!("instructions for {role}"), role, rules)
}

/// The human writes the project's rules for its staff, or takes them away.
fn rules(context: &Context, project: i64, rules: Option<&str>) {
    context
        .ledger
        .borrow_mut()
        .set_staff_rules(project, rules)
        .unwrap();
}

/// A project on `harness`, its chief and one member of each role in [`MEMBERS`].
fn staffed_on(context: &Context, harness: &str) -> i64 {
    let staff: Vec<Value> = MEMBERS
        .iter()
        .map(|(role, agent)| {
            json!({ "agent": agent, "harness": harness, "roles": [role], "tier": "standard" })
        })
        .collect();
    context
        .open_project(json!({
            "directory": "/work/app",
            "name": "app",
            "chief": { "harness": harness, "model": "apollo" },
            "staff": staff,
        }))
        .unwrap()
        .expect("the project")
        .id
}

#[test]
fn every_members_window_and_the_chiefs_next_one_start_with_the_projects_rules_on_every_harness() {
    for harness in HARNESSES {
        let context = Context::made(Made {
            harnesses: HARNESSES.to_vec(),
            ..Made::default()
        });
        let project = staffed_on(&context, harness);
        assert_eq!(
            chief_launches(&context.adapter)[0]["instructions"],
            told("chief", None),
            "{harness}: a project with no rules starts as it always did"
        );
        rules(&context, project, Some(RULES));
        for (role, _) in MEMBERS {
            context.pool_task(project, role, &format!("A task for the {role}"));
        }
        context.pass().unwrap();
        context.pass().unwrap();
        for (role, _) in MEMBERS {
            let launches: Vec<Value> = context
                .adapter
                .prepared()
                .into_iter()
                .filter(|launch| launch["role"] == role)
                .collect();
            assert_eq!(launches.len(), 1, "{harness}: {role}");
            let given = launches[0]["instructions"].as_str().unwrap_or_default();
            assert_eq!(
                given,
                told(role, Some(RULES)),
                "{harness}: {role} at its launch"
            );
            assert!(
                given.ends_with(&format!("\n\n{RULES}\n")),
                "{harness}: {role}"
            );
        }
        // The chief's window has them at its next launch, a Resume.
        relaunch(&context, project);
        let chief = chief_launches(&context.adapter);
        assert_eq!(chief.len(), 2, "{harness}");
        assert!(chief[1]["resume"].is_string(), "{harness}: resumed");
        assert_eq!(
            chief[1]["instructions"],
            told("chief", Some(RULES)),
            "{harness}: the chief on resume"
        );
        // Taken away, the next launch is as it was before them.
        rules(&context, project, None);
        relaunch(&context, project);
        assert_eq!(
            chief_launches(&context.adapter)[2]["instructions"],
            told("chief", None),
            "{harness}"
        );
        context.close();
    }
}

#[test]
fn a_codex_chief_resumed_after_the_rules_changed_is_told_its_role_with_them_once_and_again_at_each_change(
) {
    let context = codex();
    let project = super::project_of(&context, "codex");
    let chief = context.id(project.id, "chief");
    let mut said = 0;
    for now in [Some(RULES), Some(CHANGED), None] {
        rules(&context, project.id, now);
        relaunch(&context, project.id);
        let text = told("chief", now);
        let launch = chief_launches(&context.codex).pop().expect("a launch");
        assert!(launch["resume"].is_string(), "{now:?}");
        assert_eq!(launch["instructions"], text, "{now:?}");
        assert_eq!(
            launch["message"],
            Value::Null,
            "{now:?}: the role goes first"
        );
        let notes = replacements(&context, chief);
        said += 1;
        assert_eq!(
            notes.len(),
            said,
            "{now:?}: one replacement for each change"
        );
        let note = notes.last().expect("the replacement");
        assert!(
            note.body.ends_with(&format!("\n\n{text}")),
            "{now:?}: the role with the rules as they are now, last: {}",
            note.body
        );
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(context.message(note.id).state, "delivered", "{now:?}");
        assert_eq!(recorded(&context, chief).as_deref(), Some(text.as_str()));
        context.codex.answer("chief", "Understood.");
        context.pass().unwrap();
        // Rules that did not change since say nothing at the next Resume.
        relaunch(&context, project.id);
        assert_eq!(replacements(&context, chief).len(), said, "{now:?}");
    }
    context.close();
}

#[test]
fn a_codex_members_session_resumed_after_the_rules_changed_is_told_its_role_with_them_before_its_follow_up(
) {
    let context = codex();
    let (project, _session, member) = finished_session(&context);
    assert_eq!(
        recorded(&context, member).as_deref(),
        Some("instructions for worker")
    );
    rules(&context, project.id, Some(RULES));
    follow_up(&context, &project, "Now the lexer");
    context.pass().unwrap();
    context.pass().unwrap();
    let launch = context.codex.prepared().pop().expect("a launch");
    let text = told("worker", Some(RULES));
    assert_eq!(launch["instructions"], text);
    assert_eq!(launch["message"], Value::Null, "the role goes first");
    let notes = replacements(&context, member);
    assert_eq!(notes.len(), 1);
    assert!(
        notes[0].body.ends_with(&format!("\n\n{text}")),
        "{}",
        notes[0].body
    );
    context.pass().unwrap();
    assert_eq!(context.message(notes[0].id).state, "delivered");
    assert_eq!(recorded(&context, member).as_deref(), Some(text.as_str()));
    context.close();
}
