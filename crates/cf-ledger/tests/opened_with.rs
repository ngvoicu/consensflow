//! A project opened with its wake-up calls and its specs said: the ledger keeps
//! them in the write that makes the project, so the project is never seen
//! without them (the chief's first launch is given the project as the ledger
//! has it, and reads its specs from it), and logs them as it logs the same
//! words said right after. What the ledger refuses is refused before the write
//! begins, in the words the Project settings dialog is refused in, and a write
//! that fails after the project's row is made leaves no project.

#![allow(clippy::expect_used)]

use cf_base::time::Clock;
use cf_ledger::{
    open_ledger, parse_compaction, parse_staff_rules, Ledger, LedgerError, NewChief, NewProject,
    NewWakeUp, Options, ProjectView, SpecsChange, WakeUpSetting, WakeUpUnit, DEFAULT_WAKE_UP_TEXT,
};
use rusqlite::Connection;
use serde_json::{json, Value};

/// The ledger's clock here: 2026-10-15 at 10:00, a second later at each reading, so that
/// a write that read the clock again shows in the times it left.
struct Ticks(i64);

impl Clock for Ticks {
    fn now_ms(&mut self) -> i64 {
        self.0 += 1000;
        self.0
    }
}

fn options() -> Options {
    Options {
        clock: Box::new(Ticks(1_792_058_400_000)),
        ..Options::default()
    }
}

fn ledger_in(dir: &tempfile::TempDir) -> Ledger {
    open_ledger(&dir.path().join("consensflow.db"), options()).expect("a ledger")
}

/// A project that says nothing of its calls and specs.
fn project(name: &str) -> NewProject {
    NewProject {
        directory: format!("/work/{name}"),
        name: name.into(),
        chief: NewChief {
            harness: "claude-code".into(),
            model: Some("apollo".into()),
            effort: None,
        },
        staff: Vec::new(),
        gate: false,
        wake_up: None,
        specs: None,
        staff_rules: None,
        compaction: None,
    }
}

fn calls(on: bool, every: i64, unit: WakeUpUnit, text: Option<&str>) -> NewWakeUp {
    NewWakeUp {
        on,
        every,
        unit,
        text: text.map(str::to_owned),
    }
}

fn said(request: Value) -> SpecsChange {
    SpecsChange::from_json(&request).expect("a request the ledger takes")
}

/// What the project's log says of `kinds`, oldest first: the kind and its data.
fn logged(ledger: &Ledger, project: i64, kinds: &[&str]) -> Vec<(String, Value)> {
    ledger
        .events(project, 0, 500)
        .expect("the events")
        .into_iter()
        .filter(|event| kinds.contains(&event.kind.as_str()))
        .map(|event| (event.kind, event.data))
        .collect()
}

fn refused(opened: Result<ProjectView, LedgerError>) -> (Option<&'static str>, String) {
    let cause = opened.expect_err("a refusal");
    (cause.code(), cause.to_string())
}

const SETTINGS: [&str; 2] = ["project.specs", "project.wake_up_setting"];

#[test]
fn a_project_opened_with_calls_and_specs_has_them_when_it_is_first_seen() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let opened = ledger
        .create_project(&NewProject {
            wake_up: Some(calls(true, 2, WakeUpUnit::Hours, Some("Anything left?"))),
            specs: Some(said(
                json!({ "on": true, "folder": "./docs//specs/", "variant": "tdd" }),
            )),
            ..project("app")
        })
        .expect("a project");
    // The view the chief's launch is given, which is what the specs section of its text reads.
    assert_eq!(
        serde_json::to_value(&opened).expect("the view")["specs"],
        json!({ "on": true, "folder": "docs/specs", "variant": "tdd" })
    );
    assert_eq!(
        ledger.wake_up(opened.id).expect("the calls"),
        WakeUpSetting {
            on: true,
            every_ms: 7_200_000,
            text: Some("Anything left?".into()),
        }
    );
    let read = ledger
        .project(opened.id)
        .expect("read")
        .expect("it is there");
    assert_eq!(read.specs, opened.specs);
}

#[test]
fn what_is_said_as_a_project_opens_is_logged_as_the_same_words_said_right_after() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let (calling, specs) = (
        calls(true, 90, WakeUpUnit::Minutes, None),
        said(json!({ "on": false, "folder": "plans" })),
    );
    let opened = ledger
        .create_project(&NewProject {
            wake_up: Some(calling.clone()),
            specs: Some(specs.clone()),
            ..project("opened")
        })
        .expect("a project");
    let later = ledger.create_project(&project("later")).expect("a project");
    ledger.set_specs(later.id, &specs).expect("specs");
    ledger.set_wake_up(later.id, &calling).expect("calls");
    // Each is the setting's own event, from what a project has by default to what was said.
    for (kind, to) in [
        (
            "project.specs",
            json!({ "on": false, "folder": "plans", "variant": null }),
        ),
        (
            "project.wake_up_setting",
            json!({ "on": true, "everyMs": 5_400_000, "text": null }),
        ),
    ] {
        let own = logged(&ledger, opened.id, &[kind]);
        assert_eq!(own.len(), 1, "{kind}: {own:?}");
        assert_eq!(own[0].1["to"], to, "{kind}");
        assert_eq!(own, logged(&ledger, later.id, &[kind]), "{kind}");
    }
    // The project was made before either was said.
    let events: Vec<String> = ledger
        .events(opened.id, 0, 500)
        .expect("the events")
        .into_iter()
        .map(|event| event.kind)
        .collect();
    let at = |kind: &str| events.iter().position(|logged| logged == kind).expect(kind);
    assert!(at("project.created") < at("project.specs"), "{events:?}");
    assert!(
        at("project.created") < at("project.wake_up_setting"),
        "{events:?}"
    );
}

#[test]
fn a_project_that_says_nothing_is_as_it_always_was_and_so_is_one_that_says_what_a_project_has() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let plain = ledger.create_project(&project("plain")).expect("a project");
    // The calls a project has when the human never set them: off, an hour apart, the default words.
    let default = calls(false, 1, WakeUpUnit::Hours, Some(DEFAULT_WAKE_UP_TEXT));
    let same = ledger
        .create_project(&NewProject {
            wake_up: Some(default),
            ..project("same")
        })
        .expect("a project");
    for opened in [&plain, &same] {
        assert_eq!(opened.specs, None);
        assert_eq!(
            opened.created_at, opened.updated_at,
            "nothing was written after the row"
        );
        assert_eq!(logged(&ledger, opened.id, &SETTINGS), []);
        let setting = ledger.wake_up(opened.id).expect("the calls");
        assert_eq!(
            (setting.on, setting.every_ms, setting.text),
            (false, 3_600_000, None)
        );
    }
}

#[test]
fn calls_the_ledger_refuses_refuse_the_opening_before_a_write_and_no_project_is_made() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let probe = ledger.create_project(&project("probe")).expect("a project");
    for (asked, code, words) in [
        (
            calls(true, 0, WakeUpUnit::Minutes, None),
            "invalid-interval",
            "a wake-up comes once a minute at the most, not every 0 minutes",
        ),
        (
            calls(true, i64::MAX, WakeUpUnit::Days, None),
            "invalid-interval",
            "9223372036854775807 days is longer than a wake-up can wait",
        ),
        (
            calls(true, 1, WakeUpUnit::Hours, Some("  ")),
            "invalid-text",
            "the wake-up text must be text, not empty, at most 1000000 characters",
        ),
    ] {
        let opening = NewProject {
            wake_up: Some(asked.clone()),
            ..project("app")
        };
        assert_eq!(
            refused(ledger.create_project(&opening)),
            (Some(code), words.to_owned())
        );
        // The words `wakeup.set` refuses the same calls in.
        let saving = ledger
            .set_wake_up(probe.id, &asked)
            .expect_err("the same refusal");
        assert_eq!(
            (saving.code(), saving.to_string()),
            (Some(code), words.to_owned())
        );
    }
    assert_eq!(
        ledger.projects().expect("the projects").len(),
        1,
        "only the probe"
    );
    // Not even a row was tried: the next project follows the probe.
    let next = ledger.create_project(&project("next")).expect("a project");
    assert_eq!(next.id, probe.id + 1);
}

#[test]
fn a_request_in_json_says_its_calls_and_specs_after_the_rest_and_is_refused_in_the_dialogs_words() {
    let asked = json!({
        "directory": "/work/app",
        "name": "app",
        "chief": { "harness": "claude-code", "model": "claude-opus-5" },
        "staff": [],
        "wakeUp": { "on": true, "every": 3, "unit": "days", "text": "Still there?" },
        "specs": { "on": true, "folder": "docs/specs", "variant": "core-html" },
    });
    let request = NewProject::from_json(&asked).expect("a request");
    assert_eq!(
        request.wake_up,
        Some(calls(true, 3, WakeUpUnit::Days, Some("Still there?")))
    );
    assert_eq!(
        request.specs,
        Some(said(
            json!({ "on": true, "folder": "docs/specs", "variant": "core-html" })
        ))
    );
    // Left out, or null, a project says nothing of them.
    for left_out in [json!({}), json!({ "wakeUp": null, "specs": null })] {
        let mut bare = asked.clone();
        bare.as_object_mut()
            .expect("an object")
            .retain(|key, _| key != "wakeUp" && key != "specs");
        bare.as_object_mut()
            .expect("an object")
            .extend(left_out.as_object().cloned().expect("an object"));
        let request = NewProject::from_json(&bare).expect("a request");
        assert_eq!((request.wake_up, request.specs), (None, None), "{bare}");
    }
    let wrong = |field: &str, value: Value| {
        let mut request = asked.clone();
        request[field] = value;
        let refusal = NewProject::from_json(&request).expect_err("a refusal");
        (refusal.code(), refusal.to_string())
    };
    let inside = "the specs folder is a folder inside the project: ";
    for (field, value, code, words) in [
        (
            "specs",
            json!({ "folder": "../elsewhere" }),
            "invalid-specs-folder",
            format!("{inside}it stays inside it, with no `..`"),
        ),
        (
            "specs",
            json!({ "variant": "full" }),
            "invalid-specs-variant",
            "the specs variant is one of core, core-html, tdd, tdd-html, not \"full\"".to_owned(),
        ),
        (
            "specs",
            json!({ "on": "yes" }),
            "invalid-specs",
            "specs are on (true), off (false) or follow the app's setting (null)".to_owned(),
        ),
        (
            "specs",
            json!("on"),
            "invalid-specs",
            "specs are said as on, folder and variant, not \"on\"".to_owned(),
        ),
        (
            "wakeUp",
            json!({ "on": true, "every": 0.5, "unit": "minutes" }),
            "invalid-interval",
            "a wake-up comes every whole number of minutes, hours or days, not 0.5 \"minutes\""
                .to_owned(),
        ),
        (
            "wakeUp",
            json!({ "on": "yes", "every": 1, "unit": "hours" }),
            "invalid-wake-up",
            "wake-up calls are on (true) or off (false)".to_owned(),
        ),
        (
            "wakeUp",
            json!("hourly"),
            "invalid-wake-up",
            "wake-up calls are on (true) or off (false)".to_owned(),
        ),
    ] {
        assert_eq!(
            wrong(field, value.clone()),
            (Some(code), words),
            "{field}: {value}"
        );
    }
}

/// What New project says of the rules for the staff and of the compaction, as the page asks
/// for a project to start with them (`rules`, as `project.rules` takes them, and `compaction`,
/// as `project.compaction` does).
fn asked_with(said: Value) -> Value {
    let mut asked = json!({
        "directory": "/work/app",
        "name": "app",
        "chief": { "harness": "claude-code", "model": "claude-opus-5" },
        "staff": [],
    });
    asked
        .as_object_mut()
        .expect("an object")
        .extend(said.as_object().cloned().expect("an object"));
    asked
}

const RULES: &str = "- Push to origin only.\n- Tests use the .smoke bundle id.";

#[test]
fn a_project_opened_with_rules_and_a_compaction_has_them_when_it_is_first_seen_and_logs_them_as_if_said_right_after(
) {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let request = NewProject::from_json(&asked_with(
        json!({ "rules": RULES, "compaction": 250_000 }),
    ))
    .expect("a request");
    let opened = ledger.create_project(&request).expect("a project");
    // The view the chief's launch is given, which is what the rules in its text are read from.
    let view = serde_json::to_value(&opened).expect("the view");
    assert_eq!(view["staffRules"], RULES);
    assert_eq!(view["compaction"], 250_000);
    let read = ledger
        .project(opened.id)
        .expect("read")
        .expect("it is there");
    assert_eq!(read.staff_rules.as_deref(), Some(RULES));
    assert_eq!(read.compaction, Some(250_000));
    // Each is its own event, from none to what was said, as when the human says it right after.
    let later = ledger.create_project(&project("later")).expect("a project");
    ledger
        .set_staff_rules(later.id, Some(RULES))
        .expect("rules");
    ledger
        .set_compaction(later.id, Some(250_000))
        .expect("a size");
    let kinds = ["project.staff_rules", "project.compaction"];
    let own = logged(&ledger, opened.id, &kinds);
    assert_eq!(own.len(), 2, "{own:?}");
    assert_eq!(own, logged(&ledger, later.id, &kinds));
    assert_eq!(own[0].1, json!({ "from": null, "to": RULES }));
    assert_eq!(own[1].1, json!({ "from": null, "to": 250_000 }));
    // The project was made before either was said.
    let events: Vec<String> = ledger
        .events(opened.id, 0, 500)
        .expect("the events")
        .into_iter()
        .map(|event| event.kind)
        .collect();
    let at = |kind: &str| events.iter().position(|logged| logged == kind).expect(kind);
    for kind in kinds {
        assert!(at("project.created") < at(kind), "{events:?}");
    }
}

#[test]
fn a_project_that_says_no_rules_and_no_size_has_none_and_nothing_is_logged() {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    for said in [
        json!({}),
        json!({ "rules": null, "compaction": null }),
        json!({ "rules": "  \n ", "compaction": null }),
    ] {
        let request = NewProject::from_json(&asked_with(said.clone())).expect("a request");
        let opened = ledger.create_project(&request).expect("a project");
        assert_eq!(opened.staff_rules, None, "{said}");
        assert_eq!(opened.compaction, None, "{said}");
        assert_eq!(
            opened.created_at, opened.updated_at,
            "{said}: nothing was written after the row"
        );
        assert_eq!(
            logged(
                &ledger,
                opened.id,
                &["project.staff_rules", "project.compaction"]
            ),
            [],
            "{said}"
        );
    }
}

/// A request in JSON is read by `NewProject::from_json` and refused there, before any write: it
/// shows the words and the code, not that a write that fails leaves nothing (the next test does).
#[test]
fn a_request_in_json_with_rules_or_a_size_the_ledger_refuses_is_refused_in_the_words_the_settings_are_refused_in(
) {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    ledger.create_project(&project("there")).expect("a project");
    let before = ledger.projects().expect("projects").len();
    for (field, value) in [
        ("rules", json!("x".repeat(8_001))),
        ("rules", json!("a control character \u{7}")),
        ("rules", json!(42)),
        ("compaction", json!(99_999)),
        ("compaction", json!(1_000_001)),
        ("compaction", json!("400000")),
        ("compaction", json!(250_000.5)),
    ] {
        let refusal = NewProject::from_json(&asked_with(json!({ field: value.clone() })))
            .expect_err("a refusal");
        // The words and the code the setting's own operation refuses the same value with.
        let own = if field == "rules" {
            parse_staff_rules(Some(&value)).expect_err("a refusal")
        } else {
            parse_compaction(Some(&value)).expect_err("a refusal")
        };
        assert_eq!(
            (refusal.code(), refusal.to_string()),
            (own.code(), own.to_string()),
            "{field}: {value}"
        );
        assert_eq!(
            ledger.projects().expect("projects").len(),
            before,
            "{field}: {value}"
        );
    }
}

/// A project asked for by the typed request the daemon makes (`typed`), which reaches
/// `create_project`: rules and a size it checks inside the write that makes the project.
fn typed(name: &str, rules: Option<&str>, size: Option<u32>) -> NewProject {
    NewProject {
        staff_rules: rules.map(str::to_owned),
        compaction: size,
        ..project(name)
    }
}

#[test]
fn rules_or_a_size_that_fail_while_the_project_is_being_written_leave_no_project_and_none_of_what_was_written_before_them(
) {
    let dir = tempfile::tempdir().expect("a home");
    let mut ledger = ledger_in(&dir);
    let there = ledger.create_project(&project("there")).expect("a project");
    ledger
        .set_staff_rules(there.id, Some("Its own rules."))
        .expect("rules");
    let before = ledger.projects().expect("projects").len();
    let too_long = "x".repeat(8_001);
    // Each is refused by the write that makes the project, not before it: the rules are written
    // first, the size after them, so the last two have the rules written when the size fails.
    for (what, request, own) in [
        (
            "rules over a bound",
            typed("a", Some(&too_long), None),
            parse_staff_rules(Some(&json!(too_long))).expect_err("a refusal"),
        ),
        (
            "a control character in the rules",
            typed("b", Some("a bell \u{7}"), Some(250_000)),
            parse_staff_rules(Some(&json!("a bell \u{7}"))).expect_err("a refusal"),
        ),
        (
            "a size under the range, after good rules",
            typed("c", Some(RULES), Some(99_999)),
            parse_compaction(Some(&json!(99_999))).expect_err("a refusal"),
        ),
        (
            "a size over the range, after good rules",
            typed("d", Some(RULES), Some(1_000_001)),
            parse_compaction(Some(&json!(1_000_001))).expect_err("a refusal"),
        ),
    ] {
        let refusal = ledger.create_project(&request).expect_err(what);
        // The words and the code the setting's own operation refuses the same value with.
        assert_eq!(
            (refusal.code(), refusal.to_string()),
            (own.code(), own.to_string()),
            "{what}"
        );
        // No project, and nothing of one: the count is the same, and the ids go on from the one
        // that was there. That shows no row was kept; it does not show one was tried, for SQLite
        // gives a rolled-back row's id again (1, a rolled-back 2, then 2), AUTOINCREMENT or not.
        assert_eq!(ledger.projects().expect("projects").len(), before, "{what}");
    }
    // Nothing was written for any of them, and what the project that was there has is its own.
    let next = ledger.create_project(&project("next")).expect("a project");
    assert_eq!(next.id, there.id + 1);
    assert_eq!(
        ledger
            .project(there.id)
            .expect("read")
            .expect("it is there")
            .staff_rules
            .as_deref(),
        Some("Its own rules.")
    );
    // Each good request after them is made whole: the rules and the size together.
    let whole = ledger
        .create_project(&typed("whole", Some(RULES), Some(250_000)))
        .expect("a project");
    assert_eq!(
        (whole.staff_rules.as_deref(), whole.compaction),
        (Some(RULES), Some(250_000))
    );
}

/// The file at `dir`, closed, with `trigger` made in it, and the ledger opened again on it.
fn ledger_with(dir: &tempfile::TempDir, trigger: &str) -> Ledger {
    let file = dir.path().join("consensflow.db");
    open_ledger(&file, options())
        .expect("a ledger")
        .close()
        .expect("closed");
    Connection::open(&file)
        .expect("the file")
        .execute_batch(trigger)
        .expect("a trigger");
    open_ledger(&file, options()).expect("a ledger")
}

#[test]
fn a_write_the_file_refuses_after_the_projects_row_is_made_leaves_no_project() {
    for (trigger, said_by) in [
        (
            "CREATE TRIGGER no_specs BEFORE UPDATE OF specs ON project
             BEGIN SELECT RAISE(ABORT, 'the file keeps no specs'); END;",
            "the file keeps no specs",
        ),
        (
            "CREATE TRIGGER no_calls BEFORE UPDATE OF wake_up_on ON project
             BEGIN SELECT RAISE(ABORT, 'the file keeps no calls'); END;",
            "the file keeps no calls",
        ),
    ] {
        let dir = tempfile::tempdir().expect("a home");
        let mut ledger = ledger_with(&dir, trigger);
        let opening = NewProject {
            wake_up: Some(calls(true, 1, WakeUpUnit::Hours, None)),
            specs: Some(said(json!({ "on": true }))),
            ..project("app")
        };
        let refusal = ledger
            .create_project(&opening)
            .expect_err("the file refuses");
        assert!(refusal.to_string().contains(said_by), "{refusal}");
        assert_eq!(ledger.projects().expect("the projects"), [], "{said_by}");
        // Nothing of it is left, not even the number its row took: the next project is the first.
        let first = ledger.create_project(&project("first")).expect("a project");
        assert_eq!(first.id, 1);
        assert_eq!(logged(&ledger, first.id, &["project.created"]).len(), 1);
        assert_eq!(first.participants.len(), 2, "the human and the chief");
    }
}
