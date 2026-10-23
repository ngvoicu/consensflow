//! Breakaway, on Windows: a program that asks to start a process outside its job may (Codex
//! does, to detach its daemon: T-363), and one that does not stays in the job and is ended
//! with it; a program held whole may not leave its job at all.
//!
//! Each test runs `starts_with_and_without_breakaway`, which starts a ping that asks for
//! nothing and a ping that asks to be outside the job, and says what came of each.

use std::pin::pin;

use super::tree::tree_env;
use super::*;
use crate::testing::{
    gone_within, pids_so_far, starts_with_and_without_breakaway, Recorded, STARTS_WITHIN,
};

/// What the program wrote of its two pings, once it has written both lines: `stays`, and
/// `breaks` or `refused …`.
async fn outcome(dir: &Path) -> Vec<String> {
    let until = Instant::now() + STARTS_WITHIN;
    loop {
        let said = std::fs::read_to_string(dir.join("outcome")).unwrap_or_default();
        let lines: Vec<String> = said.lines().map(str::to_owned).collect();
        if lines.len() >= 2 {
            return lines;
        }
        assert!(Instant::now() < until, "the program said {lines:?}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[test]
fn a_program_that_asks_to_break_away_may_and_the_one_that_does_not_ask_is_ended_with_its_job() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let (run, env) = (starts_with_and_without_breakaway(dir.path()), tree_env());
    let runtime = runtime();
    let _left = Recorded(&file);
    let kept: Rc<RefCell<Option<Ender>>> = Rc::default();
    let said = runtime.block_on(async {
        let handed = Rc::clone(&kept);
        let capturing = capture(&run, None, &env, ROOMY, move |ender| {
            *handed.borrow_mut() = Some(ender);
        });
        let mut capturing = pin!(capturing);
        let said = tokio::select! {
            said = outcome(dir.path()) => said,
            captured = &mut capturing => panic!("it ended before it said: {captured:?}"),
        };
        kept.borrow().as_ref().expect("handed over").force();
        let _ = tokio::time::timeout(Duration::from_secs(10), &mut capturing).await;
        said
    });
    runtime.shutdown_timeout(Duration::from_secs(10));
    assert_eq!(
        said,
        ["stays", "breaks"],
        "the program that asked to leave started"
    );
    let pids = pids_so_far(&file);
    assert_eq!(pids.len(), 3, "{pids:?}");
    assert!(
        gone_within(pids[1], Duration::from_secs(5)),
        "the program that asked for nothing is in the job, and ended with it"
    );
    assert!(
        crate::alive(pids[2]),
        "the program that asked to be outside the job is outside it: the end left it"
    );
}

#[test]
fn a_program_held_whole_is_refused_the_breakaway_and_all_it_starts_is_ended() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("pids");
    let (run, env) = (starts_with_and_without_breakaway(dir.path()), tree_env());
    let runtime = runtime();
    let _left = Recorded(&file);
    let kept: Rc<RefCell<Option<Ender>>> = Rc::default();
    let said = runtime.block_on(async {
        let handed = Rc::clone(&kept);
        let capturing = capture_whole(&run, None, &env, ROOMY, move |ender| {
            *handed.borrow_mut() = Some(ender);
        });
        let mut capturing = pin!(capturing);
        let said = tokio::select! {
            said = outcome(dir.path()) => said,
            captured = &mut capturing => panic!("it ended before it said: {captured:?}"),
        };
        kept.borrow().as_ref().expect("handed over").force();
        let _ = tokio::time::timeout(Duration::from_secs(10), &mut capturing).await;
        said
    });
    runtime.shutdown_timeout(Duration::from_secs(10));
    assert_eq!(said[0], "stays");
    assert!(
        said[1].starts_with("refused"),
        "nothing leaves the job of a program held whole: {said:?}"
    );
    let pids = pids_so_far(&file);
    assert!(
        gone_within(pids[1], Duration::from_secs(5)),
        "the program held whole has all it started ended"
    );
}
