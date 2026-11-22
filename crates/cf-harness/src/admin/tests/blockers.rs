//! The review's blockers of A2 (T-2056, round 2), each a case of its own: the launch's probes
//! are told apart by the name a CLI is invoked by, a probe that did not end is retired and
//! nothing newer is, and a look of any harness is answered within 40 seconds.

use std::rc::Rc;

use cf_proto::agents::Harness;

use super::launch_check::{advance, begin_look, held_codex, no_queue, FLAGS, NO_FLAGS};
use super::{captured, Fixture};
use crate::admin::{Row, Setup, Update};
use crate::seams::processes::{probe, Processes};
use crate::seams::Time;
use crate::testing::{finished, named, CaptureAsProcesses, Driver, Response, EPOCH_MS};

fn ran(fixture: &Fixture, question: &str) -> usize {
    fixture
        .capture
        .take_ran()
        .iter()
        .filter(|(program, _)| named(program) == question)
        .count()
}

fn row_of(rows: &[Rc<Row>], id: Harness) -> &Row {
    rows.iter().find(|row| row.id == id).unwrap()
}

/// A Codex and a Devin that are one file under two names, as a version manager's launcher is.
#[cfg(unix)]
fn one_dispatcher() -> Fixture {
    let fixture = Fixture::new();
    let codex = fixture.install("bin", "codex");
    std::os::unix::fs::symlink(&codex, codex.with_file_name("devin")).unwrap();
    fixture.latest.says(Harness::Codex, "0.171.0");
    fixture.latest.says(Harness::Devin, "3000.13.4");
    fixture
}

#[cfg(unix)]
#[test]
fn two_command_names_for_one_dispatcher_each_keep_their_own_version() {
    let fixture = one_dispatcher();
    fixture
        .capture
        .answer("codex queue --help", [Response::Now(Ok(captured(FLAGS)))]);
    fixture
        .capture
        .says("codex --version", "codex-cli 0.171.0\n");
    fixture
        .capture
        .says("devin --version", "devin 3000.13.4 (053434014dbd)\n");
    let rows = fixture.check(None, false);
    assert_eq!(
        row_of(&rows, Harness::Codex).version.value(),
        Some("0.171.0")
    );
    assert_eq!(
        row_of(&rows, Harness::Devin).version.value(),
        Some("3000.13.4")
    );
    assert_eq!(row_of(&rows, Harness::Devin).setup, Some(Setup::Ready));
}

#[cfg(unix)]
#[test]
fn a_refresh_of_one_command_name_forgets_nothing_of_the_other() {
    let fixture = one_dispatcher();
    fixture
        .capture
        .says("devin --version", "devin 3000.13.4 (053434014dbd)\n");
    fixture
        .capture
        .answer("codex queue --help", [Response::Now(Ok(captured(FLAGS)))]);
    fixture
        .capture
        .says("codex --version", "codex-cli 0.171.0\n");
    fixture.row("devin", false);
    assert_eq!(ran(&fixture, "devin --version"), 1);
    fixture.latest.says(Harness::Codex, "0.171.0");
    fixture.row("codex", true);
    fixture.capture.take_ran();
    fixture.latest.says(Harness::Devin, "3000.13.4");
    let again = fixture.row("devin", false);
    assert_eq!(
        ran(&fixture, "devin --version"),
        0,
        "Devin's kept answer is not Codex's to forget"
    );
    assert_eq!(again.version.value(), Some("3000.13.4"));
}

/// Codex and Pi installed, Codex's queue question held first and answered the next time.
fn codex_and_pi() -> Fixture {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture.install("bin", "pi");
    for _ in 0..3 {
        fixture.latest.says(Harness::Codex, "0.171.0");
        fixture.latest.says(Harness::Pi, "0.9.0");
    }
    fixture.capture.answer(
        "codex queue --help",
        [Response::Held, Response::Now(Ok(captured(FLAGS)))],
    );
    fixture
        .capture
        .says("codex --version", "codex-cli 0.171.0\n");
    for _ in 0..3 {
        fixture.capture.says("pi --version", "0.9.0");
    }
    fixture
}

#[test]
fn a_look_that_began_pi_does_not_hold_back_codexs_retry_after_the_cut() {
    let fixture = codex_and_pi();
    let mut driver = Driver::default();
    let begin_all = |driver: &mut Driver<Vec<Rc<Row>>>, work| {
        let admin = Rc::clone(&fixture.admin);
        driver.begin(work, async move {
            admin
                .check(None, false)
                .await
                .unwrap()
                .into_iter()
                .collect::<Vec<_>>()
        });
    };
    begin_all(&mut driver, 0);
    let first = advance_all(&fixture, &mut driver, 40_000);
    let rows = &first[0].1;
    assert_eq!(
        row_of(rows, Harness::Codex).setup,
        Some(Setup::Unchecked {
            reason: "ConsensFlow could not finish checking codex in 40 seconds".to_owned()
        })
    );
    // The next plain look asks again, and is answered at once.
    begin_all(&mut driver, 1);
    let second = advance_all(&fixture, &mut driver, 0);
    assert_eq!(
        second.len(),
        1,
        "the retry is answered without waiting 40 s more"
    );
    assert_eq!(
        row_of(&second[0].1, Harness::Codex).setup,
        Some(Setup::Ready)
    );
    assert_eq!(ran(&fixture, "codex queue --help"), 2);
}

fn advance_all(
    fixture: &Fixture,
    driver: &mut Driver<Vec<Rc<Row>>>,
    ms: i64,
) -> Vec<(usize, Vec<Rc<Row>>)> {
    let until = fixture.time.wall_ms() + ms;
    let mut settled = driver.run();
    while fixture.time.fire_next(until) {
        settled.extend(driver.run());
    }
    fixture.time.settle_at(until);
    settled
}

#[test]
fn an_old_cut_keeps_what_a_newer_launch_was_answered() {
    let fixture = held_codex();
    fixture.capture.answer(
        "codex queue --help",
        [Response::Held, Response::Now(Ok(captured(FLAGS)))],
    );
    fixture
        .capture
        .says("codex --version", "codex-cli 0.171.0\n");
    let mut driver = Driver::default();
    begin_look(&mut driver, &fixture, 0, false);
    advance(&fixture, &mut driver, 1_000);
    // The executable is replaced, and a launch asks the file as it now is.
    let at =
        fixture
            .root
            .path()
            .join("bin")
            .join(if cfg!(windows) { "codex.exe" } else { "codex" });
    std::fs::write(&at, "a newer build").unwrap();
    let processes: Rc<dyn Processes> = Rc::new(CaptureAsProcesses(Rc::clone(&fixture.capture)));
    let asked = |label: &str| {
        let answer = finished(Box::pin(probe(
            &fixture.probes,
            &processes,
            &at,
            &["queue", "--help"],
            &fixture.env,
        )))
        .unwrap_or_else(|_| panic!("{label}"));
        assert!(answer.stdout.contains("--thread"), "{label}");
    };
    asked("the launch's question");
    // The first look's held question is cut at 40 s.
    let settled = advance(&fixture, &mut driver, 39_000);
    assert_eq!(settled.len(), 1);
    assert_eq!(fixture.time.wall_ms(), EPOCH_MS + 40_000);
    fixture.capture.take_ran();
    asked("the launch's next question");
    assert_eq!(
        ran(&fixture, "codex queue --help"),
        0,
        "the cut let go of the launch's newer answer"
    );
}

#[test]
fn a_version_cut_at_35_seconds_is_not_left_to_hold_the_next_look() {
    let fixture = held_codex();
    fixture
        .capture
        .answer("codex queue --help", [Response::Held]);
    fixture.capture.answer(
        "codex --version",
        [
            Response::Held,
            Response::Now(Ok(captured("codex-cli 0.150.0\n"))),
        ],
    );
    let mut driver = Driver::default();
    begin_look(&mut driver, &fixture, 0, false);
    advance(&fixture, &mut driver, 29_000);
    assert!(fixture
        .capture
        .release("codex queue --help", Ok(captured(NO_FLAGS))));
    let first = advance(&fixture, &mut driver, 6_000);
    assert_eq!(first.len(), 1);
    assert_eq!(
        first[0].1.setup,
        Some(Setup::UpdateRequired {
            reason: no_queue("This Codex")
        })
    );
    fixture.capture.take_ran();
    begin_look(&mut driver, &fixture, 1, false);
    let second = advance(&fixture, &mut driver, 0);
    assert_eq!(
        second.len(),
        1,
        "the next look is not held by the cut probe"
    );
    assert_eq!(second[0].1.version.value(), Some("0.150.0"));
    assert_eq!(ran(&fixture, "codex --version"), 1);
}

#[test]
fn a_held_claude_code_or_pi_version_does_not_keep_the_whole_page_open_past_40_seconds() {
    for held in ["claude", "pi"] {
        let fixture = Fixture::new();
        for command in ["claude", "codex", "pi"] {
            fixture.install("bin", command);
            fixture
                .latest
                .says(Harness::from_name(command).unwrap(), "5.0.1");
        }
        fixture
            .capture
            .answer("codex queue --help", [Response::Now(Ok(captured(FLAGS)))]);
        fixture.capture.says("codex --version", "codex-cli 5.0.0\n");
        for command in ["claude", "pi"] {
            if command == held {
                fixture
                    .capture
                    .answer(&format!("{command} --version"), [Response::Held]);
            } else {
                fixture
                    .capture
                    .says(&format!("{command} --version"), "5.0.0");
            }
        }
        let mut driver = Driver::default();
        let admin = Rc::clone(&fixture.admin);
        driver.begin(0, async move { admin.check(None, false).await.unwrap() });
        let until = EPOCH_MS + 40_000;
        let mut settled = driver.run();
        while fixture.time.fire_next(until) {
            settled.extend(driver.run());
        }
        assert_eq!(settled.len(), 1, "{held}: the page is answered by 40 s");
        assert!(fixture.time.wall_ms() <= until, "{held}");
        let rows = &settled[0].1;
        assert_eq!(
            row_of(rows, Harness::from_name(held).unwrap()).setup,
            Some(Setup::Unchecked {
                reason: format!("ConsensFlow could not finish checking {held} in 40 seconds")
            }),
            "{held}"
        );
        let other = if held == "pi" {
            Harness::Claude
        } else {
            Harness::Pi
        };
        assert!(matches!(row_of(rows, other).update, Update::Available(_)));
        assert_eq!(row_of(rows, Harness::Codex).setup, Some(Setup::Ready));
    }
}

#[test]
fn a_run_is_let_go_of_when_its_last_caller_stops_waiting_and_not_before() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let fixture = Fixture::new();
    let cli = fixture.install("bin", "codex");
    fixture.capture.answer(
        "codex queue --help",
        [
            Response::Held,
            Response::Held,
            Response::Now(Ok(captured(FLAGS))),
        ],
    );
    let processes: Rc<dyn Processes> = Rc::new(CaptureAsProcesses(Rc::clone(&fixture.capture)));
    let asking = || {
        Box::pin(probe(
            &fixture.probes,
            &processes,
            &cli,
            &["queue", "--help"],
            &fixture.env,
        ))
    };
    let mut context = Context::from_waker(Waker::noop());
    let (mut first, mut second) = (asking(), asking());
    assert!(first.as_mut().poll(&mut context).is_pending());
    assert!(second.as_mut().poll(&mut context).is_pending());
    // One of two callers stops: the run the other waits for is kept, and joined by a third.
    drop(first);
    let mut third = asking();
    assert!(third.as_mut().poll(&mut context).is_pending());
    assert_eq!(
        ran(&fixture, "codex queue --help"),
        1,
        "one run for the three"
    );
    assert!(fixture
        .capture
        .release("codex queue --help", Ok(captured(FLAGS))));
    assert!(matches!(
        second.as_mut().poll(&mut context),
        Poll::Ready(Ok(_))
    ));
    assert!(matches!(
        third.as_mut().poll(&mut context),
        Poll::Ready(Ok(_))
    ));
    // A new run held, its only caller stops: the next question runs afresh.
    fixture.probes.forget(&cli);
    let mut alone = asking();
    assert!(alone.as_mut().poll(&mut context).is_pending());
    drop(alone);
    let mut next = asking();
    assert!(matches!(
        next.as_mut().poll(&mut context),
        Poll::Ready(Ok(_))
    ));
    assert_eq!(ran(&fixture, "codex queue --help"), 2);
}
