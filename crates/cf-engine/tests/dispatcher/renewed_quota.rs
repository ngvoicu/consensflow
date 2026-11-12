//! A quota the person renewed by hand (T-1357, Gabriel, 2026-11-09: he reset his Codex quota to
//! let a waiting review start, and the board kept it waiting). The board learns of a quota from
//! a record of one of the member's windows; a member out until its reset, with no window
//! running, stays out until that time passes unless the human says it is back (Try again now,
//! `back_from_quota`). The human's word is for the account they renewed: every member on the
//! machine's one login for the harness is back, in every project, and a wait says what to
//! press. Held to no Node trace: Node's dispatcher had neither.

use cf_engine::testing::{Context, Made};
use cf_harness::records::Quota;
use cf_ledger::{NewMember, NewTask};
use serde_json::{json, Value};

use crate::fixtures::{assert_match, exhausted, now, participant, placed, soon, Tiers};

/// The staff of a project of `directory`, each member a worker of the standard tier on the
/// harness its pair names, and the chief on Claude Code.
fn project_of(context: &Context, directory: &str, members: &[(&str, &str)]) -> i64 {
    let staff: Vec<Value> = members
        .iter()
        .map(|(agent, harness)| {
            json!({ "agent": agent, "harness": harness, "role": "worker", "tier": "standard" })
        })
        .collect();
    context
        .open_project(json!({
            "directory": directory,
            "name": directory.trim_start_matches("/work/"),
            "chief": { "harness": "claude-code", "model": "claude-opus-5" },
            "staff": staff,
        }))
        .unwrap()
        .unwrap()
        .id
}

/// Marks `handle` of `project` out of quota until `until`, as a refusal of its harness does.
fn mark_out(context: &Context, project: i64, handle: &str, until: &str) {
    let id = context.id(project, handle);
    context
        .ledger
        .borrow_mut()
        .mark_out(id, until, "out of quota")
        .unwrap();
}

/// Until when `handle` of `project` is out, none when it is not.
fn out_until(context: &Context, project: i64, handle: &str) -> Option<String> {
    participant(context, project, handle)
        .unwrap_or_else(|| panic!("no @{handle}"))
        .out_until
}

#[test]
fn a_wait_for_a_member_out_of_quota_says_to_press_try_again_now_if_it_was_renewed_early() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let until = soon(&context, 3);
    mark_out(&context, tiers.project.id, "zeus", &until);
    mark_out(&context, tiers.project.id, "diana", &until);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "open");
    assert_eq!(
        tiers.notes("chief"),
        [format!(
            "T-1 waits for a free standard worker: @zeus is out of quota until {until}; @diana is out of quota until {until}. If the quota was renewed early, the human presses Try again now on @zeus or @diana."
        )]
    );
}

/// What a member is marked out until when its account refused for what no wait cures, and
/// when its harness moved its window's model on its own (`scheduler.rs`: `NEVER`, `MOVED`).
const ACCOUNT_HELD: &str = "9999-12-31T23:59:59.998Z";
const MODEL_MOVED: &str = "9999-12-31T23:59:59.999Z";

/// The note the chief was given that T-1 waits for a free standard worker, when `held` marks
/// `zeus`, the only one, out.
fn the_wait_for_a_member_held(held: &str) -> String {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    mark_out(&context, tiers.project.id, "zeus", held);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let notes = tiers.notes("chief");
    assert_eq!(notes.len(), 1, "{held}: {notes:?}");
    assert_match(&notes[0], "^T-1 waits for a free standard worker: @zeus's ");
    notes[0].clone()
}

#[test]
fn a_wait_for_a_member_held_for_its_account_says_to_press_try_again_now_if_the_login_or_billing_was_fixed(
) {
    // The human's Try again now means the login works again (Gabriel, 2026-11-10): it ends this
    // hold too, and the wait says so.
    let note = the_wait_for_a_member_held(ACCOUNT_HELD);
    assert!(
        note.ends_with(
            " If the login or the billing was fixed, the human presses Try again now on @zeus."
        ),
        "{note}"
    );
    assert!(!note.contains("quota was renewed"), "{note}");
}

#[test]
fn a_wait_for_a_member_whose_model_moved_names_no_button_for_the_human_has_one_for_that_member_alone(
) {
    // Only the human's word for that member ends it, and its own words say what to do.
    let note = the_wait_for_a_member_held(MODEL_MOVED);
    assert!(!note.contains("Try again now"), "{note}");
}

#[test]
fn a_wait_for_members_held_for_a_quota_and_for_an_account_says_which_button_is_for_which() {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus", "diana"]);
    let until = soon(&context, 3);
    mark_out(&context, tiers.project.id, "zeus", &until);
    mark_out(&context, tiers.project.id, "diana", ACCOUNT_HELD);
    tiers.open();
    context.pass().unwrap();
    context.pass().unwrap();
    let notes = tiers.notes("chief");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].ends_with(
            " If the quota was renewed early, the human presses Try again now on @zeus. If the login or the billing was fixed, the human presses Try again now on @diana."
        ),
        "{notes:?}"
    );
}

#[test]
fn a_member_the_human_says_is_back_is_given_the_next_task_at_once_and_a_refusal_after_marks_it_out_again(
) {
    let context = Context::new();
    let tiers = Tiers::new(&context, &["zeus"]);
    let until = soon(&context, 3);
    mark_out(&context, tiers.project.id, "zeus", &until);
    tiers.open();
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "open", "out until {until}");

    // The human renewed the quota by hand and presses Try again now: no window of zeus was
    // running, and nothing but their word could tell.
    context.back_from_quota(tiers.project.id, "zeus").unwrap();
    assert_eq!(out_until(&context, tiers.project.id, "zeus"), None);
    context.pass().unwrap();
    let session = tiers.assignee(1);
    assert_match(&session, "^zeus-");
    context.pass().unwrap();
    assert_eq!(tiers.task(1).task.state, "working", "given at once");

    // The quota was not renewed after all: its harness refuses, and zeus is out again. A
    // refusal is taken when it is dated strictly after the human's word (`refused_here`), so the
    // clock goes on by the smallest step, one millisecond.
    context.advance(1);
    let resets_at = soon(&context, 2);
    context
        .adapter
        .refuse(&session, exhausted(Some(&now(&context)), Some(&resets_at)));
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        placed(&tiers.task(1).task),
        ("paused", Some(session.as_str())),
        "the work waits with its window"
    );
    assert_eq!(
        out_until(&context, tiers.project.id, "zeus"),
        Some(resets_at)
    );
}

#[test]
fn the_humans_word_for_one_codex_member_brings_back_every_member_on_its_login_in_every_project() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    // Codex keeps the machine's one login; Pi logs into a provider for each model.
    context.codex.one_login.set(true);
    let first = project_of(
        &context,
        "/work/first",
        &[("zeus", "codex"), ("diana", "codex"), ("hera", "pi")],
    );
    let second = project_of(&context, "/work/second", &[("apollo", "codex")]);
    let until = soon(&context, 3);
    for (project, handle) in [
        (first, "zeus"),
        (first, "diana"),
        (first, "hera"),
        (first, "chief"),
        (second, "apollo"),
    ] {
        mark_out(&context, project, handle, &until);
    }

    context.back_from_quota(first, "zeus").unwrap();

    assert_eq!(out_until(&context, first, "zeus"), None);
    assert_eq!(
        out_until(&context, first, "diana"),
        None,
        "diana is on the same Codex login"
    );
    assert_eq!(
        out_until(&context, second, "apollo"),
        None,
        "and so is apollo, in another project"
    );
    assert_eq!(
        out_until(&context, first, "hera"),
        Some(until.clone()),
        "Pi's members may be on other accounts"
    );
    assert_eq!(
        out_until(&context, first, "chief"),
        Some(until),
        "the chief is on Claude Code, another login"
    );
}

#[test]
fn the_humans_word_for_one_pi_member_is_for_that_member_alone() {
    // Pi logs into a provider for each model: two of its members may be on two accounts.
    let context = Context::new();
    let project = project_of(&context, "/work/app", &[("hera", "pi"), ("apollo", "pi")]);
    let until = soon(&context, 3);
    mark_out(&context, project, "hera", &until);
    mark_out(&context, project, "apollo", &until);

    context.back_from_quota(project, "hera").unwrap();

    assert_eq!(out_until(&context, project, "hera"), None);
    assert_eq!(out_until(&context, project, "apollo"), Some(until));
}

#[test]
fn a_wait_names_for_a_renewal_only_the_members_out_now() {
    // hera is on a harness ConsensFlow cannot open and its old mark is past; zeus is out.
    let context = Context::made(Made {
        harnesses: vec!["claude-code"],
        ..Made::default()
    });
    let project = project_of(&context, "/work/app", &[("zeus", "claude-code")]);
    context
        .ledger
        .borrow_mut()
        .add_member(
            project,
            &NewMember {
                model: Some("muse-spark".to_owned()),
                effort: None,
                model_name: Some("hera".to_owned()),
                harness: "pi".to_owned(),
                designer: false,
                roles: vec!["worker".to_owned()],
                tier: "standard".to_owned(),
            },
        )
        .unwrap();
    mark_out(&context, project, "hera", &soon(&context, 1));
    context.advance(2 * 3_600_000);
    let until = soon(&context, 3);
    mark_out(&context, project, "zeus", &until);
    context.create_task(
        project,
        NewTask {
            from: "chief".to_owned(),
            pool: Some("worker".to_owned()),
            tier: Some("standard".to_owned()),
            body: "Write the parser".to_owned(),
            ..NewTask::default()
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let chief = context.id(project, "chief");
    let notes = crate::fixtures::notes(&context, chief);
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(
        notes[0].ends_with(
            " If the quota was renewed early, the human presses Try again now on @zeus."
        ),
        "{notes:?}"
    );
}

#[test]
fn the_humans_word_gives_every_member_it_brought_back_the_work_that_waited_for_it() {
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    context.codex.one_login.set(true);
    let project = project_of(
        &context,
        "/work/app",
        &[("zeus", "codex"), ("diana", "codex")],
    );
    let until = soon(&context, 3);
    mark_out(&context, project, "zeus", &until);
    mark_out(&context, project, "diana", &until);
    for body in ["Write the lexer", "Write the parser"] {
        context.create_task(
            project,
            NewTask {
                from: "chief".to_owned(),
                pool: Some("worker".to_owned()),
                tier: Some("standard".to_owned()),
                body: body.to_owned(),
                ..NewTask::default()
            },
        );
    }
    context.pass().unwrap();
    assert_eq!(context.task(project, 1).task.state, "open");
    assert_eq!(context.task(project, 2).task.state, "open");

    context.back_from_quota(project, "zeus").unwrap();
    context.pass().unwrap();
    context.pass().unwrap();

    let given = |number: i64| context.task(project, number).task.assignee;
    let (first, second) = (given(1), given(2));
    let by = |prefix: &str| {
        [&first, &second]
            .into_iter()
            .flatten()
            .filter(|assignee| assignee.starts_with(prefix))
            .count()
    };
    assert_eq!(
        (by("zeus-"), by("diana-")),
        (1, 1),
        "one task each, diana's quota being back as zeus's: {first:?} {second:?}"
    );
}

#[test]
fn a_refusal_stamped_at_the_very_moment_of_the_humans_word_is_history_and_one_a_millisecond_later_is_not(
) {
    // The boundary of `refused_here`: it takes a refusal only when it is strictly later than
    // the mark-back, so what the harness wrote at that instant (the refusal the human is
    // answering) is not held again. A real refusal of the next turn is dated after it.
    for (gap_ms, refused_again) in [(0, false), (1, true)] {
        let context = Context::new();
        let tiers = Tiers::new(&context, &["zeus"]);
        mark_out(&context, tiers.project.id, "zeus", &soon(&context, 3));
        tiers.open();
        context.pass().unwrap();
        context.back_from_quota(tiers.project.id, "zeus").unwrap();
        context.pass().unwrap();
        context.pass().unwrap();
        let session = tiers.assignee(1);
        let stamped = crate::fixtures::after(&context, gap_ms);
        context.advance(gap_ms);
        context.adapter.refuse(
            &session,
            exhausted(Some(&stamped), Some(&soon(&context, 2))),
        );
        context.pass().unwrap();
        context.pass().unwrap();
        assert_eq!(
            out_until(&context, tiers.project.id, "zeus").is_some(),
            refused_again,
            "a refusal {gap_ms} ms after the human's word"
        );
    }
}

#[test]
fn the_humans_word_clears_a_quota_and_an_account_on_the_login_and_leaves_a_busy_model_and_a_moved_model_held(
) {
    // The word means "this login works again" (quota renewed, logged in again, credit bought).
    // It is not about a model busy for a moment, whose back-off is its own, nor about a model
    // its harness moved, which only the human's word for that member ends.
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    context.codex.one_login.set(true);
    let first = project_of(&context, "/work/first", &[("zeus", "codex")]);
    let second = project_of(
        &context,
        "/work/second",
        &[
            ("apollo", "codex"),
            ("ares", "codex"),
            ("diana", "codex"),
            ("hermes", "codex"),
        ],
    );
    // apollo's model is busy for a moment, at work on T-1.
    context.give(second, "apollo", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context
        .task(second, 1)
        .task
        .assignee
        .expect("T-1 given to apollo");
    let tried_at = crate::fixtures::after(&context, 60_000);
    context.codex.refuse(
        &session,
        Quota::Busy {
            at: Some(now(&context)),
            because: "Selected model is at capacity.".to_owned(),
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    let held = context.task(second, 1).task;
    assert_eq!(
        (held.state.as_str(), held.held_until.as_deref()),
        ("paused", Some(tried_at.as_str())),
        "held a minute with its window"
    );
    // The others: out of quota, its account refused, its model moved; zeus is out of quota too.
    let until = soon(&context, 3);
    mark_out(&context, first, "zeus", &until);
    mark_out(&context, second, "diana", &until);
    mark_out(&context, second, "ares", ACCOUNT_HELD);
    mark_out(&context, second, "hermes", MODEL_MOVED);

    context.back_from_quota(first, "zeus").unwrap();

    assert_eq!(out_until(&context, first, "zeus"), None);
    assert_eq!(out_until(&context, second, "diana"), None, "quota");
    assert_eq!(out_until(&context, second, "ares"), None, "account");
    assert_eq!(
        out_until(&context, second, "hermes").as_deref(),
        Some(MODEL_MOVED),
        "a moved model waits for its own word"
    );
    assert_eq!(
        out_until(&context, second, "apollo").as_deref(),
        Some(tried_at.as_str()),
        "a busy model keeps its back-off"
    );
    let held = context.task(second, 1).task;
    assert_eq!(
        (held.state.as_str(), held.held_until.as_deref()),
        ("paused", Some(tried_at.as_str())),
        "its task is still held until the minute is out"
    );
    context.advance(30_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(second, 1).task.state, "paused", "30 s on");
    context.advance(31_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(
        context.task(second, 1).task.state,
        "working",
        "tried again when the minute is out"
    );
}

/// zeus at work on T-1 in a window the human has open, a note waiting for that window, and
/// zeus, the member, then marked out by `mark` (given the project and the member's id and
/// the time it is out until): the notes the human was given.
fn the_human_told_what_waits(mark: impl Fn(&Context, i64, i64, &str)) -> Vec<String> {
    let context = Context::new();
    let tiers = crate::background::working(&context);
    context
        .open_window(tiers.project.id, "zeus-amber-pine")
        .unwrap();
    context
        .ledger
        .borrow_mut()
        .note(
            tiers.project.id,
            &cf_ledger::NewNote {
                from: Some("chief".to_owned()),
                to: "zeus-amber-pine".to_owned(),
                task: Some(1),
                body: "Mind the lexer too".to_owned(),
            },
        )
        .unwrap();
    mark(
        &context,
        tiers.project.id,
        tiers.id("zeus"),
        &soon(&context, 3),
    );
    context.pass().unwrap();
    context.pass().unwrap();
    tiers.notes("human")
}

#[test]
fn a_message_waiting_for_a_window_of_a_member_out_of_quota_is_told_to_the_human_with_the_button() {
    let said = the_human_told_what_waits(|context, _, member, until| {
        context
            .ledger
            .borrow_mut()
            .mark_out(member, until, "out of quota")
            .unwrap();
    });
    assert_eq!(said.len(), 1, "{said:?}");
    assert_match(
        &said[0],
        r"^@zeus-amber-pine is out of quota until \S+, and m-\d+ \(a note on T-1\) waits for it, as does whatever is sent to it until then\. If its account has quota again \(another account, a bigger plan\), press Try again now on @zeus: what waits is typed in once it is back\.$",
    );
}

#[test]
fn a_message_waiting_for_a_window_of_a_member_whose_model_is_busy_is_told_to_the_human_as_busy_with_no_button(
) {
    // Not out of quota, and nothing for the human to press: ConsensFlow tries it again (T-1357).
    let said = the_human_told_what_waits(|context, _, member, until| {
        context.mark_out_busy(member, until);
    });
    assert_eq!(said.len(), 1, "{said:?}");
    assert_match(
        &said[0],
        r"^@zeus-amber-pine's model is busy, and m-\d+ \(a note on T-1\) waits for it, as does whatever is sent to it until ConsensFlow tries it again at \S+\.$",
    );
    assert!(!said[0].contains("quota"), "{said:?}");
    assert!(!said[0].contains("Try again now"), "{said:?}");
}

#[test]
fn the_humans_word_on_a_member_whose_model_is_busy_leaves_its_back_off_says_so_and_still_clears_the_others_on_its_login(
) {
    // A stale row's button, or a call of the API: the member was out of quota when the page
    // was drawn and is busy now. The press does not shorten its back-off (a try is spent for
    // nothing), and says why; the others on the login are cleared all the same.
    let context = Context::made(Made {
        codex: true,
        ..Made::default()
    });
    context.codex.one_login.set(true);
    let first = project_of(&context, "/work/first", &[("apollo", "codex")]);
    let second = project_of(&context, "/work/second", &[("diana", "codex")]);
    context.give(first, "apollo", "Write the parser");
    context.pass().unwrap();
    context.pass().unwrap();
    let session = context
        .task(first, 1)
        .task
        .assignee
        .expect("T-1 given to apollo");
    let tried_at = crate::fixtures::after(&context, 60_000);
    context.codex.refuse(
        &session,
        Quota::Busy {
            at: Some(now(&context)),
            because: "Selected model is at capacity.".to_owned(),
        },
    );
    context.pass().unwrap();
    context.pass().unwrap();
    mark_out(&context, second, "diana", &soon(&context, 3));

    let refused = context.back_from_quota(first, "apollo").unwrap_err();

    assert_eq!(
        refused.to_string(),
        format!("Its model is busy: ConsensFlow tries it again at {tried_at}")
    );
    assert_eq!(
        out_until(&context, first, "apollo").as_deref(),
        Some(tried_at.as_str()),
        "its back-off is its own"
    );
    let held = context.task(first, 1).task;
    assert_eq!(
        (held.state.as_str(), held.held_until.as_deref()),
        ("paused", Some(tried_at.as_str())),
        "its task is still held"
    );
    assert_eq!(
        out_until(&context, second, "diana"),
        None,
        "the others are cleared"
    );
    context.advance(30_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(first, 1).task.state, "paused", "30 s on");
    context.advance(31_000);
    context.pass().unwrap();
    context.pass().unwrap();
    assert_eq!(context.task(first, 1).task.state, "working", "a minute on");
}
