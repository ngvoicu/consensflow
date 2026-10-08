//! An update as Node's admin suite held it: run the way the CLI was installed,
//! checked again, and said as it came out; the recorded goldens
//! (`tests/goldens/admin`) hold the rest to Node's answer.

use std::rc::Rc;
use std::time::Duration;

use cf_process::{CaptureFailed, Captured, Limits};
use cf_proto::agents::Harness;
use serde_json::{json, Value};

use super::*;
use crate::admin::tests::{says, Fixture};
use crate::admin::Row;
use crate::testing::{finished, Driver, Response};

/// A codex installed by its own installer, whose version file the update
/// rewrites: an admin ready to update it.
fn codex_with_its_installer() -> (Fixture, String) {
    let fixture = Fixture::new();
    let path = fixture
        .install("home/.codex/bin", "codex")
        .to_string_lossy()
        .into_owned();
    (fixture, path)
}

fn update(fixture: &Fixture, id: &str) -> Result<Outcome, String> {
    finished(Box::pin(fixture.admin.update(id)))
}

/// What the update says, as the page reads it.
fn said(outcome: &Outcome) -> Value {
    serde_json::to_value(outcome).unwrap()
}

#[test]
fn updates_a_harness_with_its_own_tool_looks_again_and_says_what_happened() {
    let (fixture, path) = codex_with_its_installer();
    fixture
        .capture
        .answer("codex --version", [says("1.0.0\n"), says("1.0.1\n")]);
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.capture.answer(
        "codex update",
        [Response::Now(Ok(Captured {
            stdout: "Updated to 1.0.1\n".to_owned(),
            stderr: String::new(),
        }))],
    );
    let done = said(&update(&fixture, "codex").unwrap());
    assert_eq!(done["state"], "updated");
    assert_eq!(done["before"], "1.0.0");
    assert_eq!(done["after"], "1.0.1");
    assert_eq!(done["command"], format!("{path} update"));
    assert_eq!(done["output"], "Updated to 1.0.1");
    assert_eq!(done["harness"]["version"]["value"], "1.0.1");
    assert_eq!(done["harness"]["update"]["state"], "current");
    let ran = fixture.capture.take_ran();
    let updating: Vec<_> = ran
        .iter()
        .filter(|(program, _)| program.args == ["update"])
        .collect();
    let [(program, limits)] = &updating[..] else {
        panic!("one update: {ran:?}")
    };
    assert_eq!(program.executable.to_string_lossy(), path);
    assert_eq!(program.cwd.as_deref(), fixture.env.path("HOME"));
    assert_eq!(
        *limits,
        Limits {
            timeout: Duration::from_secs(600),
            max_buffer: 1_000_000
        }
    );
}

#[test]
fn an_update_that_fails_says_why_and_what_the_tool_wrote_to_either_stream() {
    let (fixture, _) = codex_with_its_installer();
    fixture
        .capture
        .answer("codex --version", [says("1.0.0"), says("1.0.0")]);
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.capture.answer(
        "codex update",
        [Response::Now(Err(CaptureFailed {
            message: "Command failed: codex update\nno network\n".to_owned(),
            code: Some(1),
            killed: false,
            stdout: "trying\n".to_owned(),
            stderr: "no network\n".to_owned(),
        }))],
    );
    let failed = said(&update(&fixture, "codex").unwrap());
    assert_eq!(failed["state"], "failed");
    assert_eq!(
        failed["reason"],
        "Command failed: codex update\nno network\n"
    );
    assert_eq!(
        failed["output"], "trying\nno network",
        "stdout, then stderr"
    );
    assert_eq!(failed["after"], "1.0.0");
}

#[test]
fn an_update_that_outlived_its_ten_minutes_says_it_was_stopped() {
    let (fixture, _) = codex_with_its_installer();
    fixture
        .capture
        .answer("codex --version", [says("1.0.0"), says("1.0.0")]);
    fixture.latest.says(Harness::Codex, "1.0.0");
    fixture.latest.says(Harness::Codex, "1.0.0");
    fixture.capture.answer(
        "codex update",
        [Response::Now(Err(CaptureFailed {
            message: "Command failed: codex update\n".to_owned(),
            code: None,
            killed: true,
            stdout: "half".to_owned(),
            stderr: String::new(),
        }))],
    );
    let stopped = said(&update(&fixture, "codex").unwrap());
    assert_eq!(stopped["state"], "failed");
    assert_eq!(
        stopped["reason"],
        "the update ran for ten minutes and was stopped"
    );
    assert_eq!(stopped["output"], "half");
}

#[test]
fn an_update_is_unchanged_only_where_the_same_version_was_read_twice() {
    for (before, after, state) in [
        ("1.0.0", "1.0.0", "unchanged"),
        ("1.0.0", "1.0.1", "updated"),
        ("1.0.0", "unusual", "updated"),
        ("unusual", "1.0.0", "updated"),
        // `undefined !== null`: no version read either time is not the same.
        ("unusual", "unusual", "updated"),
    ] {
        let (fixture, _) = codex_with_its_installer();
        fixture
            .capture
            .answer("codex --version", [says(before), says(after)]);
        fixture.latest.says(Harness::Codex, "1.0.1");
        fixture.latest.says(Harness::Codex, "1.0.1");
        fixture.capture.says("codex update", "");
        let done = said(&update(&fixture, "codex").unwrap());
        assert_eq!(done["state"], state, "{before} then {after}");
        let version = |text: &str| {
            if text == "unusual" {
                Value::Null
            } else {
                json!(text)
            }
        };
        assert_eq!(
            (&done["before"], &done["after"]),
            (&version(before), &version(after))
        );
    }
}

#[test]
fn a_cli_found_where_nothing_is_recognized_is_not_updated_and_nothing_is_run() {
    let fixture = Fixture::new();
    fixture.install("bin", "pi");
    fixture.capture.says("pi --version", "0.1.0");
    fixture.latest.says(Harness::Pi, "0.1.1");
    let outcome = update(&fixture, "pi").unwrap();
    let json = said(&outcome);
    assert_eq!(json["state"], "unsupported");
    assert_eq!(
        json["reason"],
        "ConsensFlow does not recognize how Pi was installed here: update it the way you installed it."
    );
    assert_eq!(json["harness"]["version"]["value"], "0.1.0");
    assert_eq!(
        fixture.capture.take_ran().len(),
        1,
        "only the version asked"
    );
}

#[test]
fn a_harness_that_is_not_installed_or_not_one_is_an_error() {
    let fixture = Fixture::new();
    for (id, words) in [
        ("devin", "Devin is not installed"),
        ("opencode", "OpenCode is not installed"),
        ("claude", "Claude is not installed"),
        ("kimi", "Unknown harness"),
        ("", "Unknown harness"),
    ] {
        assert_eq!(update(&fixture, id).unwrap_err(), words, "{id:?}");
    }
    assert!(fixture.capture.take_ran().is_empty());
}

#[test]
fn the_look_after_an_update_is_a_fresh_one_not_the_row_kept() {
    let (fixture, _) = codex_with_its_installer();
    fixture
        .capture
        .answer("codex --version", [says("1.0.0"), says("1.0.1")]);
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.capture.says("codex update", "");
    let before: Rc<Row> = fixture.row("codex", false);
    let Outcome::Ran { harness, .. } = update(&fixture, "codex").unwrap() else {
        panic!("it ran")
    };
    assert!(!Rc::ptr_eq(&before, &harness));
    assert!(
        Rc::ptr_eq(&harness, &fixture.row("codex", false)),
        "and kept"
    );
}

#[test]
fn what_an_update_wrote_is_cut_to_its_last_twenty_lines_and_two_thousand_units() {
    let lines: Vec<String> = (1..=30).map(|number| format!("line {number}")).collect();
    let cut = last_of(&format!("\n  {}  \n\n", lines.join("\n")));
    assert_eq!(cut.split('\n').count(), 20);
    assert!(cut.starts_with("line 11\n") && cut.ends_with("line 30"));
    // Lines end at `\n` alone: a carriage return stays with its line.
    assert_eq!(last_of("a\r\nb\r\n"), "a\r\nb");
    // The last 2000 units of the lines, counted as JavaScript counts them.
    let long = "x".repeat(3000);
    assert_eq!(last_of(&long), "x".repeat(2000));
    let accents = "é".repeat(2500);
    assert_eq!(last_of(&accents), "é".repeat(2000));
    let emoji = "😀".repeat(1500);
    assert_eq!(last_of(&emoji).chars().count(), 1000);
    // JavaScript's white space, not Rust's, is taken off.
    assert_eq!(last_of("\u{FEFF}\u{a0}out\u{2028}"), "out");
    assert_eq!(last_of("\u{85}out\u{85}"), "\u{85}out\u{85}");
    assert_eq!(last_of(""), "");
}

#[test]
fn a_cut_through_half_a_character_is_marked_where_javascript_left_a_lone_surrogate() {
    // An emoji is two units: the last three of `😀ab` are the low half of
    // the pair and the two letters, which JavaScript leaves as a lone
    // surrogate and no Rust text can hold.
    assert_eq!(last_units("😀ab", 3), "\u{FFFD}ab");
    assert_eq!(last_units("x😀ab", 3), "\u{FFFD}ab");
    assert_eq!(last_units("x😀ab", 4), "😀ab");
    assert_eq!(last_units("😀ab", 4), "😀ab");
    assert_eq!(last_units("ab", 5), "ab");
    assert_eq!(last_units("ab", 0), "");
    assert_eq!(last_units("😀", 1), "\u{FFFD}");
    assert_eq!(last_units("😀", 2), "😀");
}

#[test]
fn an_update_given_up_while_it_looks_again_leaves_no_look_for_the_next_call_to_wait_on() {
    let (fixture, _) = codex_with_its_installer();
    fixture.capture.answer(
        "codex --version",
        [says("1.0.0"), Response::Held, says("1.0.1")],
    );
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.capture.says("codex update", "");
    let mut driver = Driver::default();
    let admin = Rc::clone(&fixture.admin);
    driver.begin(0, async move { admin.update("codex").await });
    assert!(
        driver.run().is_empty(),
        "it waits for the look after the update"
    );
    assert_eq!(fixture.admin.inner.pending.borrow().len(), 1);
    drop(driver);
    assert!(fixture.admin.inner.pending.borrow().is_empty());
    let row = fixture.row("codex", true);
    assert_eq!(row.version.value(), Some("1.0.1"), "looked at afresh");
}
