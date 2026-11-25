//! What an ordinary stop leaves in `daemon.log` while another process held the log's file: the
//! lines the writer still holds, then the count of what it had to drop, then the stop's line and
//! the exit's (T-2263, finding 11).

use std::fs::OpenOptions;
use std::time::Instant;

use super::*;

/// How many lines the daemon says while the file is held: more than the writer's queue takes.
const BURST: usize = 1600;

/// Runs a daemon, holds its log's file for `held`, says [`BURST`] lines, and stops it: what the
/// log holds at the moment the process would exit, and how long the stop took.
async fn stopped_while_held(held: Duration) -> (String, Duration) {
    let root = Rc::new(tempfile::tempdir().unwrap());
    let (said, exited, handles) = (Said::default(), Rc::default(), Rc::default());
    let (mut options, _app) = options(root.path(), &said, &exited, &handles);
    let file = root.path().join("consensflow").join("daemon.log");
    let at_exit = Rc::new(RefCell::new(None));
    let (seen, read) = (Rc::clone(&at_exit), file.clone());
    options.exit = Rc::new(move |_| {
        *seen.borrow_mut() = Some(std::fs::read_to_string(&read).unwrap_or_default());
    });
    let daemon = start(environment(root.path()), options)
        .await
        .expect("it starts");
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cf_base::file::lock_beside(&file))
        .unwrap();
    lock.lock().unwrap();
    for line in 0..BURST {
        daemon.parts.log.info(&format!("end-burst {line}"));
    }
    let release = std::thread::spawn(move || {
        std::thread::sleep(held);
        drop(lock);
    });
    let began = Instant::now();
    daemon.stop("test stop");
    daemon.finished().await;
    let took = began.elapsed();
    cf_ledger::Ledger::finish_test_fixture(&daemon.parts.ledger);
    release.join().unwrap();
    let text = at_exit.borrow().clone().expect("it exited");
    (text, took)
}

/// The count a line of dropped lines gives, where there is one.
fn dropped_in(text: &str) -> Option<usize> {
    text.lines()
        .find(|line| line.contains("lines of the log were dropped"))?
        .split(" warn ")
        .nth(1)?
        .split(' ')
        .next()?
        .parse()
        .ok()
}

#[tokio::test]
async fn a_stop_writes_what_it_holds_then_the_count_of_what_it_dropped_then_the_stop_and_the_exit()
{
    LocalSet::new()
        .run_until(async {
            for held in [60, 600] {
                let (text, _) = stopped_while_held(Duration::from_millis(held)).await;
                let written = text.lines().filter(|l| l.contains(" info end-burst ")).count();
                assert!(written > 0 && written < BURST, "{held} ms: {written} written");
                let count = dropped_in(&text)
                    .unwrap_or_else(|| panic!("{held} ms: no count of the dropped lines"));
                assert!(
                    (BURST - written..=BURST - written + 3).contains(&count),
                    "{held} ms: {count} counted, {written} of {BURST} written"
                );
                let lines: Vec<&str> = text.lines().map(said_of).collect();
                let ends = &lines[lines.len() - 3..];
                assert!(ends[0].contains("lines of the log were dropped"), "{ends:?}");
                assert!(ends[1].starts_with("info stop: test stop; rss "), "{ends:?}");
                assert_eq!(ends[2], "info exit 0", "{held} ms");
            }
        })
        .await;
}

#[tokio::test]
async fn a_stop_with_the_log_held_past_its_bound_ends_anyway() {
    LocalSet::new()
        .run_until(async {
            let (_, took) = stopped_while_held(Duration::from_secs(5)).await;
            assert!(took < Duration::from_millis(1900), "the stop took {took:?}");
        })
        .await;
}
