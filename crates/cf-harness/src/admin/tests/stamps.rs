//! P2 A2, the daemon decides which look is newest (T-2074): each look takes a number from one
//! counter per harness when it begins, and every answer to the page carries it.

use std::rc::Rc;

use cf_proto::agents::Harness;

use super::launch_check::{held_codex, FLAGS};
use super::{captured, Fixture};
use crate::admin::{Setup, Stamped};
use crate::testing::{finished, Driver, Response};

fn observe_codex(
    fixture: &Fixture,
    refresh: bool,
) -> impl std::future::Future<Output = Vec<Stamped>> {
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
    let (older, newer) = (earlier[0].1[0].0, later[0].1[0].0);
    assert!(
        newer > older,
        "the look begun later is stamped higher: {newer} against {older}"
    );
    assert_eq!(later[0].1[0].1.setup, Some(Setup::Ready));
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
            admin.observe(Some(&id), refresh).await.unwrap()[0].0
        }))
    };
    assert_eq!(stamp("codex", false), 1);
    assert_eq!(stamp("codex", true), 2);
    assert_eq!(stamp("pi", false), 1, "Pi's counter is its own");
    assert_eq!(stamp("codex", true), 3);
}
