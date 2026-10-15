//! The sections of Project settings, saved for a project that is closed: one the
//! human closed, or that a start left so (every project is, after each start,
//! until its Resume). Each save is accepted as it is for an open one, and none
//! starts a window or delivers anything while the project waits for its Resume,
//! a pass or the interval of a wake-up gone by notwithstanding: what was set is
//! what the chief has when it starts, which the Resume does, and specs reach it
//! only then, so before the Resume is the moment to set them. Not held to a Node
//! recording: Node's page had none of these settings. The real dispatcher stands
//! behind the page.

use super::reading::home_with;
use super::*;

const MINUTE: i64 = 60_000;

/// The saved agents of the home: zeus, who is on the staff, and hera, who joins it.
const AGENTS: &str = r#"[
    {"id":"zeus","kind":"claude-code","model":"m","workTier":"standard"},
    {"id":"hera","kind":"claude-code","model":"m","workTier":"light"}
]"#;

/// The page over a project the human closed, with a task waiting for zeus.
struct Closed {
    _home: tempfile::TempDir,
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    project: i64,
    /// The brief of zeus's task, which waits for the Resume.
    brief: i64,
}

fn closed() -> Closed {
    let (home, env) = home_with(AGENTS);
    let rig = rig_over(env, None);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let context = &rig.kit;
    let project = context.with_staff(&["zeus"]).id;
    context.close_project(project).unwrap();
    let brief = context.give(project, "zeus", "Parser").message.unwrap().id;
    assert_eq!(context.message(brief).state, "queued");
    Closed {
        _home: home,
        _daemon: daemon,
        app,
        rig,
        project,
        brief,
    }
}

/// What the app asks and gets back, the engine's own work run meanwhile as the
/// daemon's executor runs it (the kit's is run by whoever drives the test): a
/// member's removal waits for its turn on it.
async fn asked(held: &Closed, operation: &str, body: Value) -> Value {
    let answer = ask(&held.app, operation, body);
    tokio::pin!(answer);
    loop {
        tokio::select! {
            answer = &mut answer => return answer,
            () = tokio::task::yield_now() => held.rig.kit.settle(),
        }
    }
}

/// What the engine has done with windows so far: the panes it opened and killed,
/// the words it typed into panes, and the launches it prepared.
fn windows(context: &Context) -> [usize; 4] {
    [
        context.host.opened().len(),
        context.host.killed().len(),
        context.host.inputs().len(),
        context.adapter.prepared().len(),
    ]
}

/// The kinds of the events the project's log holds, in order.
fn kinds(context: &Context, project: i64) -> Vec<String> {
    context
        .ledger
        .borrow()
        .events(project, 0, 5000)
        .unwrap()
        .into_iter()
        .map(|event| event.kind)
        .collect()
}

/// A few passes, for what a pass starts to be started and handed over.
fn passes(context: &Context) {
    for _ in 0..3 {
        context.pass().unwrap();
    }
}

#[tokio::test]
async fn each_section_saves_for_a_closed_project_and_nothing_starts_or_goes_until_it_is_resumed() {
    LocalSet::new()
        .run_until(async {
            let held = closed();
            let (project, context) = (held.project, &held.rig.kit);
            let (before, earlier) = (windows(context), kinds(context, project).len());
            let saves = [
                ("project.gate", json!({ "project": project, "gate": true })),
                (
                    "wakeup.set",
                    json!({ "project": project, "on": true, "every": 1, "unit": "minutes" }),
                ),
                (
                    "project.specs",
                    json!({ "project": project, "on": true, "folder": "plans", "variant": "tdd" }),
                ),
                (
                    "member.add",
                    json!({ "project": project, "agent": "hera", "roles": ["worker"] }),
                ),
                (
                    "member.roles",
                    json!({ "project": project, "agent": "hera", "roles": ["worker", "reviewer"] }),
                ),
                (
                    "member.remove",
                    json!({ "project": project, "agent": "hera" }),
                ),
            ];
            for (operation, body) in saves {
                let answer = asked(&held, operation, body).await;
                assert_eq!(answer["ok"], true, "{operation}: {answer}");
                // The dispatcher is woken, and a pass runs with a wake-up's interval gone by.
                context.advance(2 * MINUTE);
                context.pass().unwrap();
            }
            assert_eq!(held.rig.kicks.get(), 6, "each of them woke the dispatcher");
            passes(context);
            assert_eq!(
                windows(context),
                before,
                "no window was opened, closed or typed into, and none was prepared"
            );
            assert_eq!(
                context.message(held.brief).state,
                "queued",
                "what waits for zeus waits for the Resume"
            );
            // The ledger has each save, in the order the human made it, and nothing else of
            // the project's was logged since it closed: no wake-up, no message passed on.
            assert_eq!(
                kinds(context, project)[earlier..],
                [
                    "project.gate",
                    "project.wake_up_setting",
                    "project.specs",
                    "member.added",
                    "member.roles",
                    "member.left"
                ]
            );
            let calls = context.ledger.borrow().wake_up(project).unwrap();
            assert_eq!((calls.on, calls.every_ms), (true, MINUTE));
            let seen = serde_json::to_value(context.project(project)).unwrap();
            assert_eq!(seen["state"], "suspended");
            assert_eq!(seen["gate"], true);
            assert_eq!(
                seen["specs"],
                json!({ "on": true, "folder": "plans", "variant": "tdd" })
            );

            // The Resume starts what waits, and the chief starts with what was set.
            context.resume_project(project).unwrap();
            passes(context);
            let launched = context.adapter.prepared();
            let chief = launched
                .iter()
                .rev()
                .find(|launch| launch["role"] == "chief")
                .expect("the chief's window came up");
            assert!(
                chief["instructions"]
                    .as_str()
                    .unwrap()
                    .contains("tdd in plans at "),
                "{chief}"
            );
            assert!(
                context.host.last("zeus").is_some(),
                "zeus's window opened for its task"
            );
        })
        .await;
}

#[tokio::test]
async fn a_member_removed_from_a_closed_project_has_its_tasks_cancelled_and_the_chief_is_told_at_the_resume(
) {
    LocalSet::new()
        .run_until(async {
            let held = closed();
            let (project, context) = (held.project, &held.rig.kit);
            let before = windows(context);
            let answer = asked(
                &held,
                "member.remove",
                json!({ "project": project, "agent": "zeus" }),
            )
            .await;
            assert_eq!(answer["ok"], true, "{answer}");
            assert_eq!(answer["cancelled"], json!([1]));
            passes(context);
            assert_eq!(windows(context), before, "no window, and nothing typed");
            assert_eq!(context.message(held.brief).state, "cancelled");
            // The chief hears of it in a note that waits, as it does of anything while it is closed.
            let chief = context.id(project, "chief");
            let note = context
                .inbox(chief)
                .into_iter()
                .find(|message| message.body.contains("@zeus left the staff"))
                .expect("a note for the chief");
            assert_eq!(note.state, "queued");

            context.resume_project(project).unwrap();
            passes(context);
            assert_eq!(context.message(note.id).state, "delivered");
        })
        .await;
}
