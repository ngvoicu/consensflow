//! A window's command that waits on a macOS privacy dialog drawn in the app's name (T-1194).
//! On a Mac every window's process has the app as its responsible process, so a folder or a
//! library that a command touches (`ls ~/Music`, `find ~`, a project on the home folder) is
//! asked of the system in the app's name; the dialog is on the Mac's screen, the command waits
//! until someone answers it, and the board said nothing (field, alpha.1, 2026-11-03: "Allow
//! ConsensFlow to access your music, video activity, and media library in Apple Music?" with a
//! Claude Code window working behind it). The pane host names the dialog from the system's log;
//! a dialog is told once for each window and service, to whoever gave the window's task and to
//! the human, and holds the window until its record moves, no alert is on screen any more, or
//! ten minutes have gone. A process of the app's that no window holds is told to the human alone.

use cf_engine::testing::Context;
use cf_engine::ActivityState;
use cf_proto::panes::{TccPrompt, TccUnreadable};

use crate::chiefs::to_human;
use crate::fixtures::Tiers;

/// T-1 given to zeus and at work in its window.
fn at_work(context: &Context) -> Tiers<'_> {
    let tiers = Tiers::new(context, &["zeus"]);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working");
    tiers
}

/// The dialog the system's log named: `program`, process `pid`, asked for `service`, in zeus's
/// window.
fn prompt(
    context: &Context,
    tiers: &Tiers<'_>,
    service: &str,
    program: &str,
    pid: u32,
) -> TccPrompt {
    let pane = context
        .dispatcher
        .pane(tiers.id(&tiers.assignee(1)))
        .expect("its pane");
    TccPrompt {
        id: Some(pane.id),
        generation: Some(pane.generation),
        service: service.to_owned(),
        pid,
        program: program.to_owned(),
    }
}

/// The same dialog for a process no window holds.
fn loose(service: &str, program: &str, pid: u32) -> TccPrompt {
    TccPrompt {
        id: None,
        generation: None,
        service: service.to_owned(),
        pid,
        program: program.to_owned(),
    }
}

/// `count` passes, `ms` apart.
fn passes(context: &Context, count: usize, ms: i64) {
    for _ in 0..count {
        context.advance(ms);
        context.pass().unwrap();
        context.settle();
    }
}

/// The notes among `bodies` that tell of a dialog some command waits on.
fn of_dialogs(bodies: &[String]) -> Vec<String> {
    bodies
        .iter()
        .filter(|body| body.contains("macOS permission dialog"))
        .cloned()
        .collect()
}

fn waits_on_the_dialog(context: &Context, tiers: &Tiers<'_>) -> bool {
    let activity = context.dispatcher.activity(tiers.id(&tiers.assignee(1)));
    activity.state == ActivityState::Waiting
        && activity
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("macOS permission dialog"))
}

#[test]
fn a_dialog_for_a_process_of_a_window_is_told_once_to_the_tasks_giver_and_the_human() {
    let context = Context::new();
    let tiers = at_work(&context);
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "ls", 22913));
    context.settle();

    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 1, "{told:?}");
    let note = &told[0];
    // A member's handle carries its session's suffix: zeus-amber-pine.
    assert!(note.starts_with("@zeus"), "{note}");
    for words in [
        "'s window waits on a macOS permission dialog: `ls` (process 22913)",
        "your music and media library (Apple Music)",
        "on the Mac's screen, not in the window",
        "waits until someone answers there",
        "nothing is typed into the window meanwhile",
        "\u{201c}Don\u{2019}t Allow\u{201d} is the safe answer unless the task needs your music and media library (Apple Music)",
    ] {
        assert!(note.contains(words), "{words:?} is not in {note}");
    }
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)), told);

    // The same dialog again, and any other of that window's, are not told twice for a service.
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "find", 22950));
    context.settle();
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "Photos", "find", 22950));
    context.settle();
    let told = of_dialogs(&tiers.notes("chief"));
    assert_eq!(told.len(), 2, "{told:?}");
    assert!(told[1].contains("your Photos library"), "{}", told[1]);
}

#[test]
fn the_window_waits_on_the_dialog_in_its_lane_until_its_record_moves() {
    let context = Context::new();
    let tiers = at_work(&context);
    assert!(!waits_on_the_dialog(&context, &tiers));
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "ls", 22913));
    passes(&context, 1, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    // Nothing else changed: a few seconds on, with the record as it was, it waits still.
    passes(&context, 3, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    // The command's answer comes, and the harness writes it: the record moved.
    let answered = context
        .adapter
        .item(cf_harness::records::Role::Assistant, "Listed the folder");
    context
        .adapter
        .with("zeus", |agent| agent.items.push(answered));
    passes(&context, 1, 1_000);
    assert!(!waits_on_the_dialog(&context, &tiers));
}

#[test]
fn a_look_that_cannot_read_the_record_does_not_end_the_wait() {
    let context = Context::new();
    let tiers = at_work(&context);
    // The record holds something, so that a record that cannot be read (no items) is not the same.
    let said = context.adapter.item(
        cf_harness::records::Role::Assistant,
        "Looking in the folder",
    );
    context.adapter.with("zeus", |agent| agent.items.push(said));
    context.host.set_alerts(Some(1));
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "ls", 22913));
    passes(&context, 2, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    // The record cannot be read for a few looks: it shows no item, which is no movement.
    context.adapter.unread("zeus", true);
    passes(&context, 4, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    // Readable again and as it was: still waiting. Then it moves, and the wait ends.
    context.adapter.unread("zeus", false);
    passes(&context, 2, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    let answered = context
        .adapter
        .item(cf_harness::records::Role::Assistant, "Listed the folder");
    context
        .adapter
        .with("zeus", |agent| agent.items.push(answered));
    passes(&context, 1, 1_000);
    assert!(!waits_on_the_dialog(&context, &tiers));
}

#[test]
fn the_window_stops_waiting_when_no_alert_is_on_screen_any_more() {
    let context = Context::new();
    let tiers = at_work(&context);
    context.host.set_alerts(Some(1));
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "ls", 22913));
    // An alert is up: the window waits, and the host is not asked at once, nor at every look.
    passes(&context, 2, 1_000);
    assert_eq!(context.host.alert_looks(), 0, "too soon to look");
    passes(&context, 10, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    let looks = context.host.alert_looks();
    assert!((1..=3).contains(&looks), "{looks} looks in twelve seconds");
    // The host cannot say how many: the window waits on.
    context.host.set_alerts(None);
    passes(&context, 10, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    // The dialog is answered: none is listed.
    context.host.set_alerts(Some(0));
    passes(&context, 6, 1_000);
    assert!(!waits_on_the_dialog(&context, &tiers));
}

#[test]
fn the_window_stops_waiting_after_ten_minutes_whatever_is_on_screen() {
    let context = Context::new();
    let tiers = at_work(&context);
    context.host.set_alerts(Some(1));
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "ls", 22913));
    passes(&context, 1, 1_000);
    // Just short of the bound it waits; just past it, it does not.
    passes(&context, 1, 9 * 60_000 + 50_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    passes(&context, 1, 20_000);
    assert!(!waits_on_the_dialog(&context, &tiers));
    // Released, it is not told of again: the services told of are the window's for good.
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "ls", 23000));
    context.settle();
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);
}

#[test]
fn a_window_that_closed_forgets_what_it_waited_on_and_what_was_told_of_it() {
    let context = Context::new();
    let tiers = at_work(&context);
    context.host.set_alerts(Some(1));
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "ls", 22913));
    passes(&context, 1, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 1);

    // The human closes the project and resumes it: zeus's window is a new one, in a new pane.
    context.close_project(tiers.project.id).unwrap();
    context.settle();
    context.resume_project(tiers.project.id).unwrap();
    passes(&context, 4, 1_000);
    assert!(!waits_on_the_dialog(&context, &tiers));
    // The same service, asked in the new window, is told of again: the first window's is gone.
    context
        .dispatcher
        .privacy_prompt(prompt(&context, &tiers, "MediaLibrary", "ls", 24000));
    passes(&context, 1, 1_000);
    assert!(waits_on_the_dialog(&context, &tiers));
    assert_eq!(of_dialogs(&tiers.notes("chief")).len(), 2);
}

#[test]
fn a_dialog_for_the_chiefs_window_is_told_to_the_human_alone() {
    let context = Context::new();
    let tiers = at_work(&context);
    let pane = context
        .dispatcher
        .pane(tiers.id("chief"))
        .expect("the chief's pane");
    context.dispatcher.privacy_prompt(TccPrompt {
        id: Some(pane.id),
        generation: Some(pane.generation),
        service: "SystemPolicyDocumentsFolder".to_owned(),
        pid: 777,
        program: "git".to_owned(),
    });
    context.settle();

    assert_eq!(of_dialogs(&tiers.notes("chief")), Vec::<String>::new());
    let told = of_dialogs(&to_human(&context, tiers.project.id));
    assert_eq!(told.len(), 1, "{told:?}");
    assert!(told[0].contains("the Documents folder"), "{}", told[0]);
}

#[test]
fn a_process_no_window_holds_is_told_to_the_human_alone_once_for_each_service() {
    let context = Context::new();
    let tiers = at_work(&context);
    context
        .dispatcher
        .privacy_prompt(loose("MediaLibrary", "find", 31000));
    context.settle();
    let told = of_dialogs(&to_human(&context, tiers.project.id));
    assert_eq!(told.len(), 1, "{told:?}");
    for words in [
        "A process of ConsensFlow's that no window holds (`find`, process 31000) waits on a macOS permission dialog",
        "your music and media library (Apple Music)",
        "waits until someone answers there",
    ] {
        assert!(told[0].contains(words), "{words:?} is not in {}", told[0]);
    }
    // Nobody else hears of it: no window waits.
    assert_eq!(of_dialogs(&tiers.notes("chief")), Vec::<String>::new());
    assert!(!waits_on_the_dialog(&context, &tiers));
    context
        .dispatcher
        .privacy_prompt(loose("MediaLibrary", "du", 31001));
    context.settle();
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)).len(), 1);
    context
        .dispatcher
        .privacy_prompt(loose("Photos", "du", 31001));
    context.settle();
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)).len(), 2);
    // A window the engine does not know is no window either.
    context.dispatcher.privacy_prompt(TccPrompt {
        id: Some("p9-nobody".to_owned()),
        generation: Some(1),
        service: "SystemPolicyDownloadsFolder".to_owned(),
        pid: 5,
        program: "ls".to_owned(),
    });
    context.settle();
    assert_eq!(of_dialogs(&to_human(&context, tiers.project.id)).len(), 3);
}

#[test]
fn a_log_the_host_cannot_read_is_told_once_to_the_human_and_written_to_the_daemons_log() {
    let context = Context::new();
    let tiers = at_work(&context);
    let refused = || TccUnreadable {
        id: None,
        generation: None,
        said: "log: Must be admin to run 'stream' command".to_owned(),
    };
    context.dispatcher.privacy_unreadable(refused());
    context.settle();

    let told: Vec<String> = to_human(&context, tiers.project.id)
        .into_iter()
        .filter(|body| body.contains("cannot read the Mac's log of privacy dialogs"))
        .collect();
    assert_eq!(told.len(), 1, "{told:?}");
    for words in [
        "log: Must be admin to run 'stream' command",
        "will not tell you when a window's command waits on one",
        "on the Mac's screen",
        "needs a Mac user in the admin group",
    ] {
        assert!(told[0].contains(words), "{words:?} is not in {}", told[0]);
    }
    let logged: Vec<String> = context
        .log
        .warnings
        .borrow()
        .iter()
        .filter(|line| line.contains("privacy dialogs cannot be read"))
        .cloned()
        .collect();
    assert_eq!(logged.len(), 1, "{logged:?}");
    assert!(logged[0].contains("Must be admin"), "{}", logged[0]);

    // Said again, it is not told again.
    context.dispatcher.privacy_unreadable(refused());
    context.settle();
    let again = to_human(&context, tiers.project.id)
        .into_iter()
        .filter(|body| body.contains("cannot read the Mac's log of privacy dialogs"))
        .count();
    assert_eq!(again, 1);
    assert_eq!(
        context
            .log
            .warnings
            .borrow()
            .iter()
            .filter(|line| line.contains("privacy dialogs cannot be read"))
            .count(),
        1
    );
}

#[cfg(target_os = "macos")]
mod home_folder {
    use super::*;

    fn home_notes(context: &Context, project: i64) -> Vec<String> {
        to_human(context, project)
            .into_iter()
            .filter(|body| body.contains("opened on your home folder"))
            .collect()
    }

    #[test]
    fn a_window_opened_on_the_home_folder_is_told_to_the_human_once() {
        let context = Context::new();
        context.pane_env.home_is_the_project.set(true);
        let tiers = at_work(&context);

        let told = home_notes(&context, tiers.project.id);
        // The chief's window and zeus's both opened on it; each is told of once.
        assert!(
            !told.is_empty(),
            "{:?}",
            to_human(&context, tiers.project.id)
        );
        for words in [
            "opened on your home folder",
            "Music, Photos, Desktop, Documents and Downloads",
            "find",
            "waits for your answer on the Mac's screen",
            "a subfolder",
        ] {
            assert!(told[0].contains(words), "{words:?} is not in {}", told[0]);
        }
        let handles: std::collections::HashSet<&str> = told
            .iter()
            .filter_map(|body| body.split("'s window").next())
            .collect();
        assert_eq!(
            handles.len(),
            told.len(),
            "one note for each window: {told:?}"
        );
        // A window opened again on it is not told again: the human closes the project and resumes
        // it, and the windows that come up are new ones, on the same folder.
        let before = told.len();
        context.close_project(tiers.project.id).unwrap();
        context.settle();
        context.resume_project(tiers.project.id).unwrap();
        passes(&context, 5, 1_000);
        assert_eq!(home_notes(&context, tiers.project.id).len(), before);
    }

    #[test]
    fn a_window_opened_on_any_other_folder_is_told_nothing() {
        let context = Context::new();
        let tiers = at_work(&context);
        assert_eq!(home_notes(&context, tiers.project.id), Vec::<String>::new());
    }
}
