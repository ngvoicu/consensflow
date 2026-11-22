//! P2 A2, the daemon decides which look is newest (T-2074): each look takes a number from one
//! counter per harness when it begins, every answer to the page carries it, and one update of a
//! harness runs at a time, whichever page asks.

use std::rc::Rc;

use cf_proto::agents::Harness;

use super::launch_check::{held_codex, FLAGS};
use super::{captured, says, Fixture};
use crate::admin::{Observed, Outcome, Setup};
use crate::testing::{finished, named, Driver, Response};

fn observe_codex(fixture: &Fixture, refresh: bool) -> impl std::future::Future<Output = Observed> {
    let admin = Rc::clone(&fixture.admin);
    async move { admin.observe(Some("codex"), refresh).await.unwrap() }
}

#[test]
fn a_look_begun_later_has_the_higher_stamp_whatever_order_the_answers_arrive_in() {
    let fixture = held_codex();
    fixture
        .capture
        .answer("codex queue --help", [Response::Held, Response::Held]);
    fixture
        .capture
        .says("codex --version", "codex-cli 0.150.0\n");
    fixture
        .capture
        .says("codex --version", "codex-cli 0.150.0\n");
    let mut driver = Driver::default();
    driver.begin(0, observe_codex(&fixture, false));
    assert!(driver.run().is_empty());
    driver.begin(1, observe_codex(&fixture, true));
    assert!(driver.run().is_empty());
    // The later look is answered first, and the earlier last.
    assert!(fixture
        .capture
        .release_newest("codex queue --help", Ok(captured(FLAGS))));
    let later = driver.run();
    assert_eq!(later.len(), 1);
    assert_eq!(later[0].0, 1);
    assert!(fixture.capture.release(
        "codex queue --help",
        Ok(captured(super::launch_check::NO_FLAGS))
    ));
    let earlier = driver.run();
    assert_eq!(earlier.len(), 1);
    assert_eq!(earlier[0].0, 0);
    let (older, newer) = (earlier[0].1.rows[0].0, later[0].1.rows[0].0);
    assert!(
        newer > older,
        "the look begun later is stamped higher: {newer} against {older}"
    );
    assert_eq!(later[0].1.rows[0].1.setup, Some(Setup::Ready));
}

#[test]
fn each_harness_counts_its_looks_on_its_own() {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture.install("bin", "pi");
    for _ in 0..3 {
        fixture.latest.says(Harness::Codex, "0.171.0");
        fixture.latest.says(Harness::Pi, "0.9.0");
    }
    for _ in 0..3 {
        fixture
            .capture
            .answer("codex queue --help", [Response::Now(Ok(captured(FLAGS)))]);
        fixture
            .capture
            .says("codex --version", "codex-cli 0.171.0\n");
        fixture.capture.says("pi --version", "0.9.0");
    }
    let stamp = |id: &str, refresh: bool| {
        let admin = Rc::clone(&fixture.admin);
        let id = id.to_owned();
        finished(Box::pin(async move {
            admin.observe(Some(&id), refresh).await.unwrap().rows[0].0
        }))
    };
    assert_eq!(stamp("codex", false), 1);
    assert_eq!(stamp("codex", true), 2);
    assert_eq!(stamp("pi", false), 1, "Pi's counter is its own");
    assert_eq!(stamp("codex", true), 3);
}

/// A Codex with its own installer, whose update is held, and the answers its looks need.
fn updating_codex() -> Fixture {
    let fixture = Fixture::new();
    fixture.install("home/.codex/bin", "codex");
    fixture
        .capture
        .answer("codex --version", [says("1.0.0\n"), says("1.0.1\n")]);
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.capture.answer("codex update", [Response::Held]);
    fixture
}

type Updated = Result<(Outcome, u64), String>;

fn begin_update(fixture: &Fixture, driver: &mut Driver<Updated>, work: usize) {
    let admin = Rc::clone(&fixture.admin);
    driver.begin(work, async move { admin.update_stamped("codex").await });
}

#[test]
fn a_second_update_of_a_harness_is_refused_in_words_while_the_first_runs() {
    let fixture = updating_codex();
    let mut driver = Driver::default();
    begin_update(&fixture, &mut driver, 0);
    assert!(driver.run().is_empty(), "the installer is running");
    let running = finished(Box::pin(observe_codex(&fixture, false)));
    assert_eq!(running.updating, vec![Harness::Codex]);
    // Another page, a reload, or another tab asks for it again.
    begin_update(&fixture, &mut driver, 1);
    let settled = driver.run();
    assert_eq!(settled.len(), 1);
    assert_eq!(settled[0].0, 1);
    assert_eq!(
        settled[0].1.as_ref().err().map(String::as_str),
        Some("an update of Codex is already running")
    );
    let installs = fixture
        .capture
        .take_ran()
        .iter()
        .filter(|(program, _)| named(program) == "codex update")
        .count();
    assert_eq!(installs, 1, "one installer ran");
    // The first ends: nothing runs, and the harness may be updated again.
    assert!(fixture
        .capture
        .release("codex update", Ok(captured("done\n"))));
    let ended = driver.run();
    assert_eq!(ended.len(), 1);
    assert!(ended[0].1.is_ok());
    let after = finished(Box::pin(observe_codex(&fixture, false)));
    assert!(after.updating.is_empty());
}
