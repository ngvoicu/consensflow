//! P2 A2, smaller (T-2091): one update of a harness runs at a time, also across a daemon's
//! replacement, and no look returns a row older than an update that has ended.

use std::fs;
use std::rc::Rc;

use cf_proto::agents::Harness;

use super::{captured, says, Fixture};
use crate::admin::{record, Observed, Outcome};
use crate::testing::{finished, named, Driver, Response};

type Updated = Result<(Outcome, u64), String>;

fn updating_codex() -> Fixture {
    let fixture = Fixture::new();
    fixture.install("home/.codex/bin", "codex");
    fixture
        .capture
        .answer("codex --version", [says("1.0.0\n"), says("1.0.1\n")]);
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.capture.answer("codex update", [Response::Held]);
    fixture.capture.answer(
        "codex queue --help",
        [
            says(super::launch_check::FLAGS),
            says(super::launch_check::FLAGS),
        ],
    );
    fixture
}

fn begin_update(fixture: &Fixture, driver: &mut Driver<Updated>, work: usize) {
    let admin = Rc::clone(&fixture.admin);
    driver.begin(work, async move { admin.update_stamped("codex").await });
}

/// What the record says of the installer that is this process, as another daemon wrote it.
fn leave_record(fixture: &Fixture, began_offset: i64) {
    let path = record::path(&fixture.env, Harness::Codex).unwrap();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let pid = std::process::id();
    let began = cf_process::began_ms(pid).unwrap() + began_offset;
    fs::write(path, format!(r#"{{"pid":{pid},"began":{began}}}"#)).unwrap();
}

#[test]
fn a_record_of_a_running_installer_refuses_an_update_in_a_daemon_that_did_not_begin_it() {
    let fixture = updating_codex();
    leave_record(&fixture, 0);
    let observed = finished(Box::pin({
        let admin = Rc::clone(&fixture.admin);
        async move { admin.observe(Some("codex"), false).await.unwrap() }
    }));
    assert_eq!(
        observed.updating,
        vec![Harness::Codex],
        "the page is told it is running"
    );
    let mut driver = Driver::default();
    begin_update(&fixture, &mut driver, 0);
    let settled = driver.run();
    assert_eq!(settled.len(), 1, "the update is answered at once");
    assert_eq!(
        settled[0].1.as_ref().err().map(String::as_str),
        Some("an update of Codex is already running")
    );
    assert!(
        fixture
            .capture
            .take_ran()
            .iter()
            .all(|(program, _)| named(program) != "codex update"),
        "no second installer ran"
    );
}

#[test]
fn a_record_of_a_process_that_began_otherwise_is_stale_and_goes() {
    let fixture = updating_codex();
    // The number is alive, and another process's: it began a second after the record says.
    leave_record(&fixture, -1000);
    let mut driver = Driver::default();
    begin_update(&fixture, &mut driver, 0);
    assert!(driver.run().is_empty(), "the installer was started");
    assert!(fixture
        .capture
        .take_ran()
        .iter()
        .any(|(program, _)| named(program) == "codex update"));
    assert!(fixture
        .capture
        .release("codex update", Ok(captured("done\n"))));
    assert!(driver.run()[0].1.is_ok());
}

#[test]
fn the_installer_leaves_its_record_while_it_runs_and_takes_it_away_when_it_ends() {
    let fixture = updating_codex();
    fixture.capture.installers_are(std::process::id());
    let path = record::path(&fixture.env, Harness::Codex).unwrap();
    let mut driver = Driver::default();
    begin_update(&fixture, &mut driver, 0);
    assert!(driver.run().is_empty());
    assert!(
        record::running(&fixture.env, Harness::Codex),
        "the record of the running installer"
    );
    assert!(fixture
        .capture
        .release("codex update", Ok(captured("done\n"))));
    assert!(driver.run()[0].1.is_ok());
    assert!(!path.exists(), "the record went with the installer");
}

/// Pi's row is kept, its update runs, and a page's opening check of every harness waits for
/// Codex's answer: the check had Pi's old row at its start, and the update ends before it ends.
#[test]
fn a_check_that_waited_through_an_update_does_not_return_the_row_from_before_it() {
    let fixture = Fixture::new();
    fixture.install("home/.pi/bin", "pi");
    fixture.install("bin", "codex");
    fixture
        .capture
        .answer("pi --version", [says("1.0.0\n"), says("1.0.1\n")]);
    fixture.latest.says(Harness::Pi, "1.0.1");
    fixture.latest.says(Harness::Pi, "1.0.1");
    fixture.latest.says(Harness::Codex, "1.0.1");
    fixture.capture.says("codex --version", "1.0.1\n");
    fixture
        .capture
        .answer("codex queue --help", [Response::Held]);
    fixture.capture.answer("pi update --self", [Response::Held]);
    // Pi's row is kept, as an earlier check left it.
    let admin = Rc::clone(&fixture.admin);
    let kept = finished(Box::pin(async move {
        admin.observe(Some("pi"), false).await.unwrap()
    }));
    assert_eq!(version_of(&kept).as_deref(), Some("1.0.0"));
    let mut driver = Driver::default();
    let admin = Rc::clone(&fixture.admin);
    driver.begin(0, async move { admin.update_stamped("pi").await });
    assert!(driver.run().is_empty(), "the installer is running");
    // The page opens: Pi's row is the kept one, Codex's look waits.
    let admin = Rc::clone(&fixture.admin);
    let mut opened = Driver::default();
    opened.begin(1, async move { admin.observe(None, false).await.unwrap() });
    assert!(opened.run().is_empty());
    assert!(fixture
        .capture
        .release("pi update --self", Ok(captured("done\n"))));
    assert_eq!(driver.run().len(), 1, "the update ended");
    assert!(fixture.capture.release(
        "codex queue --help",
        Ok(captured(super::launch_check::FLAGS))
    ));
    let answered = opened.run();
    assert_eq!(answered.len(), 1);
    let observed = &answered[0].1;
    assert!(observed.updating.is_empty(), "the update has ended");
    let pi = observed
        .rows
        .iter()
        .find(|(_, row)| row.id == Harness::Pi)
        .unwrap();
    assert_eq!(
        pi.1.version.value(),
        Some("1.0.1"),
        "the row is not older than the update that ended"
    );
}

fn version_of(observed: &Observed) -> Option<String> {
    observed.rows[0].1.version.value().map(str::to_owned)
}
