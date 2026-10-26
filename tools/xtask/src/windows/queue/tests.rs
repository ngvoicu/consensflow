use std::time::Duration;

use super::*;

/// A queue in a folder of its own, whose tickets are not beaten (an hour) so that the times a
/// case names are the only times there are.
fn queues() -> (tempfile::TempDir, Queues) {
    let dir = tempfile::tempdir().unwrap();
    let queues = Queues::at(dir.path(), Duration::from_secs(3600));
    (dir, queues)
}

fn who(tree: &str) -> Who {
    Who {
        tree: tree.to_owned(),
        command: format!("cargo test in {tree}"),
    }
}

/// Midnight of a day, in milliseconds: a time a case counts from.
const DAY: i64 = 1_800_000_000_000;

#[test]
fn runs_are_ahead_of_each_other_in_the_order_they_came() {
    let (_dir, queues) = queues();
    let first = queues.join("zeewin2", &who("a"), DAY).unwrap();
    let second = queues.join("zeewin2", &who("b"), DAY + 5_000).unwrap();
    let third = queues.join("zeewin2", &who("c"), DAY + 6_000).unwrap();
    let now = DAY + 10_000;
    assert_eq!(first.ahead(now), []);
    assert_eq!(
        second.ahead(now),
        [Ahead {
            who: who("a"),
            since_ms: 10_000
        }]
    );
    let behind: Vec<String> = third
        .ahead(now)
        .into_iter()
        .map(|run| run.who.tree)
        .collect();
    assert_eq!(behind, ["a", "b"]);
    // Another machine's queue is its own.
    let other = queues.join("zeewin", &who("d"), DAY + 1).unwrap();
    assert_eq!(other.ahead(now), []);
    assert_eq!(queues.depth("zeewin2", now), 3);
    assert_eq!(queues.depth("zeewin", now), 1);
    assert_eq!(queues.depth("nowhere", now), 0);
}

#[test]
fn a_ticket_goes_with_its_run_however_the_run_ends() {
    let (dir, queues) = queues();
    let ticket = queues.join("zeewin2", &who("a"), DAY).unwrap();
    assert_eq!(fs::read_dir(dir.path().join("zeewin2")).unwrap().count(), 1);
    drop(ticket);
    assert_eq!(fs::read_dir(dir.path().join("zeewin2")).unwrap().count(), 0);
}

#[test]
fn a_ticket_nobody_touches_is_a_run_that_was_killed_and_holds_no_one_back() {
    let (dir, queues) = queues();
    let killed = queues.join("zeewin2", &who("killed"), DAY).unwrap();
    let waiting = queues
        .join("zeewin2", &who("waiting"), DAY + 1_000)
        .unwrap();
    // The first was touched at DAY and the next run looks 31 s later: six beats gone.
    let later = DAY + 31_000;
    assert!(waiting.ahead(later).is_empty());
    // The killed run's ticket was taken away by the look that found it dead; the waiting run's
    // own, touched a second later, is within its time.
    assert_eq!(fs::read_dir(dir.path().join("zeewin2")).unwrap().count(), 1);
    assert_eq!(queues.depth("zeewin2", later), 1);
    drop(killed);
    assert_eq!(queues.depth("zeewin2", later), 1);
}

#[test]
fn a_ticket_taken_away_while_its_run_lives_is_made_again_in_its_place() {
    let (dir, queues) = queues();
    let first = queues.join("zeewin2", &who("a"), DAY).unwrap();
    let second = queues.join("zeewin2", &who("b"), DAY + 1_000).unwrap();
    let mine = fs::read_dir(dir.path().join("zeewin2"))
        .unwrap()
        .map(|file| file.unwrap().path())
        .max()
        .unwrap();
    fs::remove_file(&mine).unwrap();
    let ahead = second.ahead(DAY + 2_000);
    assert_eq!(ahead.len(), 1, "a still goes first");
    assert!(mine.exists(), "the second's ticket is back under its name");
    drop(first);
}

#[test]
fn a_run_keeps_its_ticket_alive_by_touching_it() {
    let dir = tempfile::tempdir().unwrap();
    let queues = Queues::at(dir.path(), Duration::from_millis(20));
    let now = SystemClock.now_ms();
    let ticket = queues.join("zeewin2", &who("a"), now - 3_600_000).unwrap();
    let file = fs::read_dir(dir.path().join("zeewin2"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    // Written an hour ago as far as its name and its touch say; the thread has touched it since.
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let touched = fs::metadata(&file).unwrap().modified().unwrap();
        let touched_ms =
            i64::try_from(touched.duration_since(UNIX_EPOCH).unwrap().as_millis()).unwrap();
        let age = SystemClock.now_ms() - touched_ms;
        if age < 5_000 {
            break;
        }
        assert!(std::time::Instant::now() < until, "never touched: {age} ms");
        std::thread::sleep(Duration::from_millis(20));
    }
    drop(ticket);
}

/// A pace whose time moves only as the wait pauses, and that hears a stop after `stop_after`
/// pauses if it is given one.
struct Slow {
    now: i64,
    pauses: Vec<u64>,
    stop_after: Option<usize>,
}

impl Slow {
    fn at(now: i64) -> Self {
        Self {
            now,
            pauses: Vec::new(),
            stop_after: None,
        }
    }
}

impl Pace for Slow {
    fn now_ms(&mut self) -> i64 {
        self.now
    }

    fn pause(&mut self, ms: u64) -> bool {
        self.pauses.push(ms);
        self.now += i64::try_from(ms).unwrap();
        self.stop_after.is_none_or(|n| self.pauses.len() < n)
    }

    fn stopped_after_failure(&mut self) -> bool {
        false
    }
}

/// What a wait said, and how it ended: `Go(lines)`, `Refused(why)` or `Stopped`.
fn waited(
    queues: Option<&Queues>,
    minutes: u32,
    answers: &mut dyn Iterator<Item = Result<Ready, String>>,
    pace: &mut Slow,
) -> (String, Vec<String>) {
    let mut said = Vec::new();
    let came = wait(
        queues,
        "zeewin2",
        &who("mine"),
        minutes,
        &mut || {
            answers
                .next()
                .expect("the machine was asked more often than the case planned")
        },
        pace,
        &mut |line| {
            said.push(line.to_owned());
            Ok(())
        },
    );
    let ended = match came {
        Ok(Waited::Go(lines, ticket)) => format!("Go({lines:?}, ticket {})", ticket.is_some()),
        Ok(Waited::Refused(why)) => format!("Refused({why})"),
        Ok(Waited::Stopped) => "Stopped".to_owned(),
        Err(error) => format!("Error({error})"),
    };
    (ended, said)
}

fn busy(why: &str) -> Result<Ready, String> {
    Ok(Ready::Refused(why.to_owned()))
}

fn go() -> Result<Ready, String> {
    Ok(Ready::Go(vec!["on it".to_owned()]))
}

#[test]
fn a_run_with_no_one_ahead_and_a_machine_that_is_ready_goes_at_once_holding_its_place() {
    let (dir, queues) = queues();
    let mut pace = Slow::at(DAY);
    let (ended, said) = waited(Some(&queues), 60, &mut [go()].into_iter(), &mut pace);
    assert_eq!(ended, r#"Go(["on it"], ticket true)"#);
    assert!(said.is_empty() && pace.pauses.is_empty());
    // The ticket was dropped with the result above: nothing is left.
    assert_eq!(fs::read_dir(dir.path().join("zeewin2")).unwrap().count(), 0);
}

#[test]
fn a_run_behind_another_waits_for_it_and_says_where_it_stands_and_goes_when_it_has_gone() {
    let (_dir, queues) = queues();
    // The elder came two minutes ago and has been beaten since: alive for three pauses, then gone.
    let elder = queues
        .join("zeewin2", &who("elder"), DAY - 120_000)
        .unwrap();
    touch(&elder.path, &elder.text, DAY).unwrap();
    let (elder_path, elder_text) = (elder.path.clone(), elder.text.clone());
    let mut pace = Slow::at(DAY);
    let mut elder = Some(elder);
    let mut answers = std::iter::from_fn(|| Some(go()));
    let mut said = Vec::new();
    let mut looks = 0;
    let came = {
        let mut pace_with_exit = Exiting {
            slow: &mut pace,
            on_pause: &mut |n| {
                looks = n;
                if n == 3 {
                    drop(elder.take());
                } else {
                    touch(
                        &elder_path,
                        &elder_text,
                        DAY + 10_000 * i64::try_from(n).unwrap(),
                    )
                    .unwrap();
                }
            },
        };
        wait(
            Some(&queues),
            "zeewin2",
            &who("mine"),
            60,
            &mut || answers.next().unwrap(),
            &mut pace_with_exit,
            &mut |line| {
                said.push(line.to_owned());
                Ok(())
            },
        )
    };
    assert!(matches!(came, Ok(Waited::Go(_, Some(_)))));
    assert_eq!(looks, 3);
    // It said where it stood when it came and again each minute: 0 s, then at 30 s it was
    // still the same, so the next line is the minute's.
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("waiting for zeewin2, 0 s of up to 60 min: 2nd in the queue for zeewin2; the run ahead is elder (cargo test in elder), here for 2 min"),
        "{said:?}"
    );
}

/// A pace that tells what it waited on.
struct Exiting<'a, 'b> {
    slow: &'a mut Slow,
    on_pause: &'b mut dyn FnMut(usize),
}

impl Pace for Exiting<'_, '_> {
    fn now_ms(&mut self) -> i64 {
        self.slow.now_ms()
    }

    fn pause(&mut self, ms: u64) -> bool {
        let kept = self.slow.pause(ms);
        (self.on_pause)(self.slow.pauses.len());
        kept
    }

    fn stopped_after_failure(&mut self) -> bool {
        false
    }
}

#[test]
fn a_run_whose_machine_stays_busy_ends_when_its_time_is_up_saying_how_long_it_waited_and_why() {
    let (dir, queues) = queues();
    let mut pace = Slow::at(DAY);
    let (ended, said) = waited(
        Some(&queues),
        3,
        &mut std::iter::from_fn(|| Some(busy("zeewin2 is busy: poker-lab's job holds it")))
            .take(400),
        &mut pace,
    );
    assert_eq!(
        ended,
        "Refused(zeewin2 is still busy after waiting 3 min (as far as --wait 3 goes): zeewin2 is busy: poker-lab's job holds it)"
    );
    // It looked every 10 s for the three minutes, and said so at the start and each minute.
    assert_eq!(pace.pauses.len(), 18);
    assert_eq!(said.len(), 3, "{said:?}");
    assert!(
        said[1].starts_with("waiting for zeewin2, 1 min of up to 3 min: "),
        "{said:?}"
    );
    assert_eq!(fs::read_dir(dir.path().join("zeewin2")).unwrap().count(), 0);
}

#[test]
fn a_run_told_not_to_wait_asks_once_takes_no_ticket_and_refuses_as_it_did() {
    let (dir, queues) = queues();
    let mut pace = Slow::at(DAY);
    let (ended, said) = waited(
        Some(&queues),
        0,
        &mut [busy("zeewin2 is short of disk")].into_iter(),
        &mut pace,
    );
    assert_eq!(ended, "Refused(zeewin2 is short of disk)");
    assert!(said.is_empty() && pace.pauses.is_empty());
    assert!(!dir.path().join("zeewin2").exists(), "no ticket was taken");
    let (ended, _) = waited(Some(&queues), 0, &mut [go()].into_iter(), &mut pace);
    assert_eq!(ended, r#"Go(["on it"], ticket false)"#);
}

#[test]
fn a_run_with_no_queue_to_keep_waits_for_the_machine_alone_as_long_as_it_is_told() {
    let mut pace = Slow::at(DAY);
    let (ended, _) = waited(
        None,
        60,
        &mut std::iter::from_fn(|| Some(busy("zeewin2 is busy"))).take(400),
        &mut pace,
    );
    assert!(
        ended.starts_with("Refused(zeewin2 is still busy after waiting 60 min"),
        "{ended}"
    );
    assert_eq!(pace.pauses.len(), 360);
}

#[test]
fn a_stop_ends_the_wait_and_the_ticket_goes() {
    let (dir, queues) = queues();
    let mut pace = Slow::at(DAY);
    pace.stop_after = Some(2);
    let (ended, _) = waited(
        Some(&queues),
        60,
        &mut std::iter::from_fn(|| Some(busy("busy"))).take(400),
        &mut pace,
    );
    assert_eq!(ended, "Stopped");
    assert_eq!(fs::read_dir(dir.path().join("zeewin2")).unwrap().count(), 0);
}

#[test]
fn a_machine_that_cannot_be_asked_is_the_error_it_was() {
    let (_dir, queues) = queues();
    let mut pace = Slow::at(DAY);
    let (ended, _) = waited(
        Some(&queues),
        60,
        &mut [Err("zeewin2 did not say what it has free".to_owned())].into_iter(),
        &mut pace,
    );
    assert_eq!(ended, "Error(zeewin2 did not say what it has free)");
}

#[test]
fn the_queue_is_in_the_folder_the_environment_names_else_in_the_home_folder() {
    let named = Env::from_vars([("CF_WINDOWS_QUEUES", "/q"), ("HOME", "/home/me")]);
    assert_eq!(Queues::of(&named).unwrap().root, Path::new("/q"));
    let home = Env::from_vars([("HOME", "/home/me")]);
    assert_eq!(
        Queues::of(&home).unwrap().root,
        Path::new("/home/me/.cf-windows-queues")
    );
    assert!(Queues::of(&Env::default()).is_none());
}

#[test]
fn what_a_ticket_says_of_its_run_is_its_tree_and_its_command_cut_short() {
    let long: Vec<String> = (0..60).map(|n| format!("word{n}")).collect();
    let said = Who::of(Path::new("/trees/windows-machines-3"), &long);
    assert_eq!(said.tree, "windows-machines-3");
    assert_eq!(said.command.chars().count(), 80);
    assert!(said.command.ends_with('…'));
    let short = Who::of(Path::new("/trees/t"), &["cargo".into(), "test".into()]);
    assert_eq!(short.command, "cargo test");
}

#[test]
fn a_queue_that_cannot_be_kept_is_said_and_the_run_waits_for_the_machine_alone() {
    // A folder that is a file: no ticket can be made in it. The run is not stopped for that.
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("not-a-folder");
    fs::write(&file, "").unwrap();
    let queues = Queues::at(&file, Duration::from_secs(3600));
    let mut pace = Slow::at(DAY);
    let (ended, said) = waited(Some(&queues), 60, &mut [go()].into_iter(), &mut pace);
    assert_eq!(ended, r#"Go(["on it"], ticket false)"#);
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(
        said[0].starts_with("no place could be kept in the queue for zeewin2 ("),
        "{said:?}"
    );
    assert!(
        said[0].ends_with("): waiting for the machine alone"),
        "{said:?}"
    );
}
