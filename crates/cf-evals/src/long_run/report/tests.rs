//! What a long run keeps, and its verdict: a run fails for a break, for a
//! disruption not played, and for not getting to its end.

use std::time::Duration;

use serde_json::json;

use super::*;
use crate::long_run::disruptions::Disruption;
use crate::long_run::staff::{models_of, staff_of};

/// 2026-10-11T10:00:00Z, and the same an hour and a half on.
const STARTED: i64 = 1_791_712_800_000;
const ENDED: i64 = STARTED + 90 * 60_000;

fn options() -> Options {
    Options::of(&["--staff".to_owned(), "claude,devin,codex".to_owned()]).unwrap()
}

struct Scene {
    options: Options,
    found: Vec<Found>,
    left: Vec<LeftOut>,
    staff: Staff,
    schedule: Schedule,
    timeline: Timeline,
    stalls: Stalls,
}

fn scene() -> Scene {
    let options = options();
    let found = vec![
        Found {
            name: "claude".to_owned(),
            command: PathBuf::from("/bin/claude"),
            version: "2.1.90 (Claude Code)".to_owned(),
        },
        Found {
            name: "devin".to_owned(),
            command: PathBuf::from("/bin/devin"),
            version: "devin 3000.11.92".to_owned(),
        },
    ];
    let left = vec![LeftOut {
        name: "codex".to_owned(),
        why: "not on this machine: codex was not found".to_owned(),
    }];
    let names: Vec<String> = found.iter().map(|one| one.name.clone()).collect();
    let staff = staff_of(&names, &models_of(None).unwrap(), "medium").unwrap();
    let mut schedule = Schedule::new(
        &[Disruption::Pause, Disruption::DevinCut],
        Duration::from_secs(600),
        "claude",
        &names,
    );
    schedule.played(
        Disruption::Pause,
        Duration::from_secs(640),
        "strict: the owner paused T-3".to_owned(),
    );
    let mut timeline = Timeline::new(STARTED);
    timeline.add(STARTED + 12_000, "owner", "typed the ask");
    Scene {
        options,
        found,
        left,
        staff,
        schedule,
        timeline,
        stalls: Stalls::default(),
    }
}

fn kept<'a>(scene: &'a Scene, breaks: &'a [Break], error: Option<String>) -> Kept<'a> {
    Kept {
        options: &scene.options,
        chief: ChiefKept {
            harness: "claude".to_owned(),
            model: "claude-opus-5".to_owned(),
            effort: Some("high".to_owned()),
        },
        found: &scene.found,
        left: &scene.left,
        staff: &scene.staff,
        daemon: "native (rust 3.0.5)".to_owned(),
        home: Some(PathBuf::from("/tmp/home")),
        started: STARTED,
        ended: ENDED,
        schedule: &scene.schedule,
        breaks,
        unresolved: &[],
        filled: &[],
        timeline: &scene.timeline,
        stalls: &scene.stalls,
        ending: "the project is done".to_owned(),
        error,
        login: None,
        log: json!([]),
    }
}

#[test]
fn a_run_with_nothing_broken_and_something_unjudged_is_unresolved_never_a_pass() {
    // Result, send-back, a notice the log ties to no result: no break, and no pass.
    let mut scene = scene();
    scene.schedule.played(
        Disruption::DevinCut,
        Duration::from_secs(1300),
        "loose".to_owned(),
    );
    let mut notice = Unjudged::new(
        3,
        "3:m-101",
        "T-14's close note m-101 came while T-14 was working again, after it had ended and been sent back: whether it is the earlier attempt's window's, closed late, or the new attempt's own, the log does not say",
    );
    notice.at = STARTED + 30 * 60_000;
    let gap = Unjudged::new(
        6,
        "6:unnamed:p1-long-claude-1:2",
        "@long-claude-1's window p1-long-claude-1 runs 21 processes its turns started, and the pane host names 16: the rest are not followed, so whether one outlives the window is not known",
    );
    let unresolved = [notice, gap];
    let kept = Kept {
        unresolved: &unresolved,
        ..kept(&scene, &[], None)
    };
    assert!(!kept.held());
    assert_eq!(kept.verdict(), "UNRESOLVED");
    assert_eq!(
        kept.why(),
        [
            format!("unresolved (3): {}", unresolved[0].said),
            format!("unresolved (6): {}", unresolved[1].said),
        ]
    );
    let value = kept.value();
    assert_eq!(value["verdict"], "UNRESOLVED");
    assert_eq!(value["invariants"][2]["held"], false);
    assert_eq!(value["invariants"][2]["unresolved"], 1);
    assert_eq!(value["unresolved"][0]["key"], "3:m-101");
    let text = kept.markdown();
    assert!(text.starts_with("# Long run of 2026-10-11T10:00:00.000Z: UNRESOLVED\n"));
    assert!(text
        .contains("| 3 | no work is ended under a task at work | A3 | **unresolved** (1) | 0 |"));
    assert!(text.contains("## Unresolved\n\n- 3 · T-14's close note m-101"));
    // A break fails it, whatever is unjudged besides.
    let found = [Break::new(2, "2:m-131", "T-14 was done while its job ran")];
    let failed = Kept {
        unresolved: &unresolved,
        ..self::kept(&scene, &found, None)
    };
    assert_eq!(failed.verdict(), "FAIL");
}

#[test]
fn a_run_that_changed_the_owners_claude_setup_or_refreshed_its_login_fails_with_what_it_did_and_keeps_its_report(
) {
    let mut scene = scene();
    scene.schedule.played(
        Disruption::DevinCut,
        Duration::from_secs(1300),
        "loose".to_owned(),
    );
    let found = "~/.claude/settings.json is not as it was; \
                 the run's Claude Code wrote a different login in the folder's .credentials.json";
    let kept = Kept {
        login: Some(found.to_owned()),
        ..kept(&scene, &[], None)
    };
    assert!(!kept.held());
    assert_eq!(kept.verdict(), "FAIL");
    // The report keeps the words; the console, which a machine may send off it, the fact.
    assert_eq!(
        kept.why(),
        [format!(
            "the owner's Claude setup or login was not left as it was: {found}"
        )]
    );
    assert_eq!(
        kept.said_why(),
        ["the owner's Claude setup or login was not left as it was (what changed is in the report)"]
    );
    let value = kept.value();
    assert_eq!(value["verdict"], "FAIL");
    assert_eq!(value["login"], found);
    // The run's evidence is all there: the failure does not take the report from it.
    assert_eq!(value["disruptions"][0]["state"], "played");
    assert_eq!(value["seconds"], 5400);
    // One that left both be says nothing of it.
    assert!(self::kept(&scene, &[], None).value()["login"].is_null());
}

#[test]
fn a_run_with_every_disruption_played_and_nothing_broken_holds() {
    let mut scene = scene();
    scene.schedule.played(
        Disruption::DevinCut,
        Duration::from_secs(1300),
        "loose".to_owned(),
    );
    let kept = kept(&scene, &[], None);
    assert!(kept.held(), "{:?}", kept.why());
    let value = kept.value();
    assert_eq!(value["verdict"], "PASS");
    assert_eq!(value["seconds"], 5400);
    assert_eq!(value["harnesses"][0]["version"], "2.1.90 (Claude Code)");
    assert_eq!(value["harnesses"][0]["chief"], true);
    assert_eq!(value["harnesses"][1]["members"][0]["model"], "swe-2-medium");
    assert_eq!(value["harnesses"][1]["members"][0]["tier"], "light");
    assert_eq!(value["leftOut"][0]["harness"], "codex");
    assert_eq!(value["invariants"].as_array().unwrap().len(), 10);
    assert_eq!(value["disruptions"][0]["state"], "played");
    assert_eq!(value["disruptions"][0]["playedAt"], 640);
    assert_eq!(value["timeline"][0]["offset"], "+0m12s");
}

#[test]
fn a_run_whose_only_breaks_are_known_passes_with_them_and_any_other_break_fails_it() {
    use crate::long_run::invariants::CLOSE_NOTE;
    let mut scene = scene();
    scene.schedule.played(
        Disruption::DevinCut,
        Duration::from_secs(1300),
        "loose".to_owned(),
    );
    // Turn-end's A4: a result that holds its turn's progress is known.
    let progress = |key: &str| {
        let mut found = Break::new(4, key, format!("{key} holds 1 line of its turn's progress"))
            .with_shape("progress-lines");
        found.at = STARTED + 60_000;
        found
    };
    let found = [progress("4:m-6"), progress("4:m-9")];
    let kept = kept(&scene, &found, None);
    assert_eq!(kept.verdict(), "PASS with 2 known");
    assert!(kept.held());
    assert!(kept.why().is_empty(), "{:?}", kept.why());
    let value = kept.value();
    assert_eq!(value["verdict"], "PASS with 2 known");
    assert_eq!(value["invariants"][3]["held"], true);
    assert_eq!(value["invariants"][3]["breaks"], 0);
    assert_eq!(value["invariants"][3]["known"], 2);
    assert_eq!(value["breaks"][0]["shape"], "progress-lines");
    assert!(value["breaks"][0]["known"]
        .as_str()
        .unwrap()
        .contains("3.0.6"));
    let said = kept.say(Path::new("long.json"), Path::new("long.md"));
    assert!(said[0].ends_with(" · PASS with 2 known"), "{said:?}");
    assert!(
        said.iter().any(|line| line.starts_with("  known: 4:m-6 (")),
        "{said:?}"
    );
    let text = kept.markdown();
    assert!(text.starts_with("# Long run of 2026-10-11T10:00:00.000Z: PASS with 2 known\n"));
    assert!(text.contains(" · KNOWN: "), "{text}");
    assert!(text.contains("| known (2) | 2 |"), "{text}");
    // Any other break of the fourth fails the run, the known one beside it (one not the
    // turn's final answer, whole, and one of another shape).
    for other in [
        Break::new(4, "4:m-22", "m-22 is not its turn's final answer, whole"),
        Break::new(4, "4:m-22", "m-22 holds 1 line").with_shape(CLOSE_NOTE),
    ] {
        let found = [progress("4:m-6"), other];
        let failed = self::kept(&scene, &found, None);
        assert_eq!(failed.verdict(), "FAIL");
        assert_eq!(
            failed.why(),
            ["invariant 4 broke (A4, a result is the last turn's final answer): 1"]
        );
    }
    // The third has no known break (turn-end fixed it): a close note's, and one of no
    // shape, fail the run.
    for third in [
        Break::new(3, "3:m-20", "closed under work").with_shape(CLOSE_NOTE),
        Break::new(3, "3:m-22", "late"),
    ] {
        let found = [progress("4:m-6"), third];
        let failed = self::kept(&scene, &found, None);
        assert_eq!(failed.verdict(), "FAIL");
        assert!(!failed.held());
        assert_eq!(
            failed.why(),
            ["invariant 3 broke (A3, no work is ended under a task at work): 1"]
        );
    }
    // Known breaks and something unjudged: no pass.
    let found = [progress("4:m-6"), progress("4:m-9")];
    let open = [Unjudged::new(4, "4:m-131", "not judged")];
    let unresolved = Kept {
        unresolved: &open,
        ..self::kept(&scene, &found, None)
    };
    assert_eq!(unresolved.verdict(), "UNRESOLVED");
}

#[test]
fn a_break_a_disruption_not_played_or_a_run_cut_short_fails_it_and_says_which() {
    let mut scene = scene();
    scene
        .schedule
        .close("the project ended before its moment came");
    let mut found = Break::new(
        2,
        "2:m-131",
        "T-14 was done by m-131 while @zeus's background job bk2f9x still ran",
    );
    found.at = STARTED + 43 * 60_000;
    found.evidence.record = vec!["Command running in background with ID: bk2f9x".to_owned()];
    let breaks = [found];
    let kept = kept(
        &scene,
        &breaks,
        Some("the rig could not start again".to_owned()),
    );
    assert_eq!(
        kept.why(),
        [
            "invariant 2 broke (A2, no task done while its job runs): 1",
            "devin-cut was not played: the project ended before its moment came",
            "the run could not be run to its end: the rig could not start again",
        ]
    );
    let text = kept.markdown();
    for part in [
        "# Long run of 2026-10-11T10:00:00.000Z: FAIL",
        "| claude (chief) | 2.1.90 (Claude Code) | long-claude-1 (worker, claude-sonnet-5-5, complex)",
        "Left out: codex (not on this machine: codex was not found)",
        "| 2 | no task done while its job runs | A2 | **no** | 1 |",
        "| devin-cut | +20 min | not played: the project ended before its moment came |",
        "### 2 · no task done while its job runs (+43m00s)",
        "record:\n\n```\nCommand running in background with ID: bk2f9x\n```",
        "| +0m12s | owner | typed the ask |",
    ] {
        assert!(text.contains(part), "{part}\n\n{text}");
    }
    let said = kept.say(Path::new("/r/x.json"), Path::new("/r/x.md"));
    assert_eq!(
        said[0],
        "long run · chief claude (claude-opus-5) · staff claude,devin · 90 min · FAIL"
    );
    // Why a harness was left out, and what stopped the run, are the report's.
    assert_eq!(
        said[1..5],
        [
            "  left out: codex",
            "  why: invariant 2 broke (A2, no task done while its job runs): 1",
            "  why: devin-cut was not played",
            "  why: the run could not be run to its end (what stopped it is in the report)",
        ]
    );
    assert!(said.contains(&"  report: /r/x.json".to_owned()));
    assert_eq!(said.last().unwrap(), "  timeline: /r/x.md");
}

#[test]
fn a_disruption_without_its_effect_is_kept_failed_with_its_break_of_the_tenth() {
    let mut scene = scene();
    scene.schedule.failed(
        Disruption::DevinCut,
        Duration::from_secs(1300),
        "strict: the app quit; @long-devin-1's window did not come back (token=abcd1234efgh, said its screen)"
            .to_owned(),
    );
    let mut found = Break::new(
        10,
        "10:devin-cut",
        "devin-cut: @long-devin-1's window did not come back",
    );
    found.at = STARTED + 22 * 60_000;
    let breaks = [found];
    let kept = kept(&scene, &breaks, None);
    assert_eq!(
        kept.why(),
        ["invariant 10 broke (play, a disruption has its effect): 1"]
    );
    let value = kept.value();
    assert_eq!(value["disruptions"][1]["state"], "failed");
    assert_eq!(value["disruptions"][1]["playedAt"], 1300);
    assert_eq!(
        value["disruptions"][1]["said"],
        "strict: the app quit; @long-devin-1's window did not come back (token=[masked], said its screen)"
    );
    assert!(kept
        .markdown()
        .contains("| 10 | a disruption has its effect | play | **no** | 1 |"));
}

#[test]
fn the_runs_log_is_kept_with_a_key_or_a_token_hidden() {
    let scene = scene();
    let mut kept = kept(&scene, &[], None);
    kept.log = json!([
        { "at": 12, "line": "the owner's follow-up: use SECRET_KEY=s3cr3t-v4lue-123 for the deploy" },
        { "at": 30, "line": "chief idle; tasks T-1:working" },
    ]);
    assert_eq!(
        kept.value()["log"],
        json!([
            { "at": 12, "line": "the owner's follow-up: use SECRET_KEY=[masked] for the deploy" },
            { "at": 30, "line": "chief idle; tasks T-1:working" },
        ])
    );
}

#[test]
fn what_stopped_the_run_is_kept_and_said_with_a_key_or_a_token_hidden() {
    // A rig that timed out says what the daemon and the host wrote on their error output.
    let scene = scene();
    let kept = kept(
        &scene,
        &[],
        Some(
            "timed out waiting for the daemon; daemon stderr=refused token=abcd1234efgh, again"
                .to_owned(),
        ),
    );
    let hidden = "timed out waiting for the daemon; daemon stderr=refused token=[masked], again";
    assert_eq!(
        kept.why().last().unwrap(),
        &format!("the run could not be run to its end: {hidden}")
    );
    assert_eq!(kept.value()["error"], hidden);
    // The console says that something stopped it, and none of its words.
    let said = kept.say(Path::new("/r/x.json"), Path::new("/r/x.md"));
    assert!(
        said.iter().all(|line| !line.contains("daemon stderr")),
        "{said:?}"
    );
    assert!(!kept.markdown().contains("abcd1234efgh"));
}

#[test]
fn a_restart_played_without_a5s_shape_fails_the_run_and_is_said_so() {
    use crate::long_run::disruptions::NO_IDLE_COMPACTION;
    let mut scene = scene();
    scene.schedule.played(
        Disruption::DevinCut,
        Duration::from_secs(1300),
        "loose".to_owned(),
    );
    scene
        .schedule
        .lacked(Disruption::DevinCut, NO_IDLE_COMPACTION);
    let kept = kept(&scene, &[], None);
    assert_eq!(
        kept.why(),
        [format!("devin-cut was played without {NO_IDLE_COMPACTION}")]
    );
    let value = kept.value();
    assert_eq!(value["verdict"], "FAIL");
    assert_eq!(value["disruptions"][1]["state"], "played without its shape");
    assert!(value["disruptions"][1]["said"]
        .as_str()
        .unwrap()
        .ends_with(&format!("({NO_IDLE_COMPACTION})")));
}

#[test]
fn a_run_whose_time_ran_out_fails_whatever_it_found() {
    let mut scene = scene();
    scene.schedule.played(
        Disruption::DevinCut,
        Duration::from_secs(1300),
        "loose".to_owned(),
    );
    let mut kept = kept(&scene, &[], None);
    kept.ending = TIME_UP.to_owned();
    assert_eq!(
        kept.why(),
        ["time ran out: the run did not reach its end within its budget of 120 min"]
    );
    assert_eq!(kept.value()["verdict"], "FAIL");
}

#[test]
fn a_report_is_named_by_when_it_began_its_chief_and_its_staff() {
    assert_eq!(
        file_name(
            STARTED,
            "claude",
            &["claude".to_owned(), "devin".to_owned()]
        ),
        "2026-10-11T10-00-00-000Z-long-chief-claude-staff-claude-devin"
    );
}
