//! The daemon's ledger tells the home's trace only about committed events.

use cf_ledger::{NewProject, NewQuestion, NewTask};

use super::*;

#[tokio::test]
async fn a_refused_ask_leaves_no_task_state_line_in_the_trace() {
    LocalSet::new()
        .run_until(async {
            let root = Rc::new(tempfile::tempdir().unwrap());
            let rig = started(&root).await;
            let mut ledger = rig.daemon.parts.ledger.borrow_mut();
            let project = ledger.create_project(&NewProject::from_json(&json!({
                "directory": root.path().join("workspace").to_string_lossy(),
                "name": "app",
                "chief": {"harness": "claude-code"},
                "staff": [{"harness": "claude-code", "roles": ["worker"], "tier": "standard"}]
            })).unwrap()).unwrap();
            let worker = project
                .participants
                .iter()
                .find(|p| p.role == "worker")
                .unwrap()
                .handle
                .clone();
            let created = ledger
                .create_task(
                    project.id,
                    &NewTask {
                        from: "chief".into(),
                        to: Some(worker.clone()),
                        body: "Parser".into(),
                        ..NewTask::default()
                    },
                )
                .unwrap();
            let brief = created.message.unwrap().id;
            ledger.begin_delivery(brief).unwrap();
            ledger
                .confirm_delivery(brief, Some(&json!({"item": "test"})))
                .unwrap();
            let number = created.task.number;
            let before = crate::files::settled_trace(&rig.home).unwrap();
            assert!(
                before.contains(r#""kind":"task.state""#),
                "committed moves are traced"
            );

            let refused = ledger.ask(
                project.id,
                &NewQuestion {
                    from: Some("chief".into()),
                    to: worker,
                    body: Some("  ".into()),
                    task: Some(number),
                    questions: None,
                    urgent: true,
                },
            );
            assert_eq!(refused.unwrap_err().code(), Some("invalid-text"));
            assert_eq!(
                ledger.task(project.id, number).unwrap().unwrap().task.state,
                "working"
            );
            assert_eq!(
                crate::files::settled_trace(&rig.home).unwrap(),
                before,
                "the rolled-back pause has no trace line"
            );
        })
        .await;
}
