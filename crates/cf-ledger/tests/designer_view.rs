//! An image designer draws with Codex's image tool, and its window is launched on Codex's own
//! default model at no effort (the Codex adapter drops the model and effort a designer's row
//! holds). So a designer row from before that (Add member asked for a model, an effort and a
//! tier of it) is read as running on the default, not on the model it holds: the line that
//! says what a member runs on must be true (designer P2, T-1280).

#![allow(clippy::expect_used)]

use cf_ledger::{
    open_ledger, Ledger, NewChief, NewMember, NewProject, Options, ParticipantView, ProjectView,
};

fn options() -> Options {
    let mut names = ["amber-pine", "brisk-birch"].into_iter().cycle();
    Options {
        names: Box::new(move || names.next().unwrap_or_default().to_owned()),
        ..Options::default()
    }
}

/// A member on Codex's `gpt-6.1-sol` at `high`, as an older Add member stored it.
fn on_sol(designer: bool) -> NewMember {
    NewMember {
        harness: "codex".into(),
        model: Some("gpt-6.1-sol".into()),
        effort: Some("high".into()),
        model_name: Some("gpt-6.1-sol".into()),
        designer,
        roles: vec![if designer { "designer" } else { "worker" }.into()],
        tier: "light".into(),
    }
}

fn project(ledger: &mut Ledger, staff: Vec<NewMember>) -> ProjectView {
    ledger
        .create_project(&NewProject {
            directory: "/work/app".into(),
            name: "app".into(),
            chief: NewChief {
                harness: "claude-code".into(),
                model: Some("claude-opus-5-5".into()),
                effort: None,
            },
            staff,
            gate: false,
            wake_up: None,
            specs: None,
            staff_rules: None,
            compaction: None,
        })
        .expect("a project")
}

fn participant<'a>(project: &'a ProjectView, designer: bool) -> &'a ParticipantView {
    project
        .participants
        .iter()
        .find(|participant| {
            participant.designer == designer
                && participant.role != "chief"
                && participant.role != "human"
        })
        .expect("the member")
}

#[test]
fn a_designer_that_holds_a_model_is_read_as_running_on_codex_s_default_and_a_worker_is_not() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = open_ledger(&dir.path().join("consensflow.db"), options()).expect("a ledger");
    // Two members that differ by the designer flag, so neither merges into the other.
    let made = project(&mut ledger, vec![on_sol(true), on_sol(false)]);
    for project in [
        made.clone(),
        ledger
            .project(made.id)
            .expect("a read")
            .expect("the project"),
    ] {
        let designer = participant(&project, true);
        assert_eq!(
            (&designer.model, &designer.effort, &designer.model_name),
            (&None, &None, &None),
            "a designer's window opens on no model and no effort: {designer:?}"
        );
        assert!(
            designer.name_apart,
            "its name holds no model, so the line says what it runs on"
        );
        let worker = participant(&project, false);
        assert_eq!(worker.model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(worker.effort.as_deref(), Some("high"));
        assert!(!worker.name_apart, "a worker's name is its model");
    }
}
