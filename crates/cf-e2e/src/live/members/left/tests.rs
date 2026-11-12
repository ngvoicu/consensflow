use std::cell::RefCell;

use super::*;

/// A process `pid`, begun at `started`, started by `ppid`.
fn running(pid: u32, started: u64, ppid: u32, command: &str) -> Running {
    Running {
        identity: Identity { pid, started },
        ppid,
        command: command.to_owned(),
        exe: None,
    }
}

fn pids(found: &[Running]) -> Vec<u32> {
    let mut pids: Vec<u32> = found.iter().map(|one| one.identity.pid).collect();
    pids.sort_unstable();
    pids
}

const MONITOR: &str = "zsh -c tail -f out | grep --line-buffered tick";

/// The window `100`, the shell its Monitor ran, and the pipeline's two programs: a Monitor armed
/// on `tail -f … | grep tick`.
fn while_the_window_runs() -> Vec<Running> {
    vec![
        running(100, 10, 1, "claude --session-id s"),
        running(300, 20, 100, MONITOR),
        running(301, 21, 300, "tail -f out"),
        running(302, 22, 300, "grep --line-buffered tick"),
        running(103, 12, 100, "node server.js"),
        // Another tree's window and its Monitor, the same command lines.
        running(200, 9, 1, "claude --session-id s"),
        running(400, 19, 200, MONITOR),
    ]
}

#[test]
fn what_a_window_ran_that_still_runs_once_it_is_closed_is_left_by_which_process_it_is() {
    let window = Identity {
        pid: 100,
        started: 10,
    };
    let mut watch = Watch::new();
    watch.add(window, Path::new("/runs/monitor-wait"));
    watch.look(&while_the_window_runs());
    // Closed: the window, its shell and its server have gone; the pipeline's programs run on
    // with no parent, and another process has the number of the shell now.
    let after = vec![
        running(301, 21, 1, "tail -f out"),
        running(302, 22, 1, "grep --line-buffered tick"),
        running(300, 90, 1, "an editor that took the shell's number"),
        running(200, 9, 1, "claude --session-id s"),
        running(400, 19, 1, MONITOR),
    ];
    let left = watch.left(&after, |_| None);
    assert_eq!(pids(&left), [301, 302], "{left:#?}");
}

#[test]
fn a_monitor_whose_claude_was_killed_is_found_as_an_orphan_of_the_windows_folder_where_no_look_saw_it(
) {
    // No look fell while the window ran (a case whose window had closed before its close): what is
    // left is an orphan of the folder the window ran in, begun after the window, with what it
    // started; never one of another folder, one begun before the window, or one that has a parent.
    let folder = Path::new("/runs/monitor-wait");
    let mut watch = Watch::new();
    watch.add(
        Identity {
            pid: 100,
            started: 10,
        },
        folder,
    );
    let after = vec![
        running(300, 20, 1, MONITOR),
        running(301, 21, 300, "tail -f out"),
        running(302, 22, 300, "grep --line-buffered tick"),
        // The same Monitor of another tree's run: another folder.
        running(400, 23, 1, MONITOR),
        // The same in this folder, begun before the window did: an earlier run's.
        running(500, 5, 1, MONITOR),
        // A process of a shell that still runs, in this folder.
        running(600, 24, 77, MONITOR),
    ];
    let folder_of = |pid: u32| match pid {
        300 | 500 | 600 => Some(folder.to_path_buf()),
        400 => Some(PathBuf::from("/runs/another-tree")),
        _ => None,
    };
    let left = watch.left(&after, folder_of);
    assert_eq!(pids(&left), [300, 301, 302], "{left:#?}");
}

/// A machine that lists each of `lists` in turn, the last again and again, and ends nothing it is
/// not asked to.
struct Lists(RefCell<Vec<Vec<Running>>>);

impl Machine for Lists {
    fn processes(&self) -> crate::Result<Vec<Running>> {
        let mut lists = self.0.borrow_mut();
        Ok(if lists.len() > 1 {
            lists.remove(0)
        } else {
            lists[0].clone()
        })
    }

    fn identity(&self, _pid: u32) -> crate::Result<Option<Identity>> {
        Ok(None)
    }
}

#[test]
fn a_process_on_its_way_out_when_the_windows_are_closed_is_no_leftover_and_one_that_stays_is_ended_and_said(
) {
    let mut watch = Watch::new();
    watch.add(
        Identity {
            pid: 100,
            started: 10,
        },
        Path::new("/runs/monitor-wait"),
    );
    watch.look(&while_the_window_runs());
    let on_its_way = running(301, 21, 1, "tail -f out");
    let stays = running(302, 22, 1, "grep --line-buffered tick");
    // The first listing still has both, the second only the one that stays.
    let machine = Lists(RefCell::new(vec![
        vec![on_its_way.clone(), stays.clone()],
        vec![stays.clone()],
        vec![stays.clone()],
    ]));
    let asked = RefCell::new(Vec::new());
    let swept = watch.sweep_by(
        &machine,
        |_| None,
        |chosen| {
            asked
                .borrow_mut()
                .extend(chosen.iter().map(|one| one.identity));
            Ended {
                ended: chosen.to_vec(),
                ..Ended::default()
            }
        },
        // Looks again until the settle time is out: the first look found one more than the rest.
        Duration::from_millis(600),
    );
    assert_eq!(pids(&swept.found), [302], "{swept:#?}");
    // Ended by which process it is, once.
    assert_eq!(asked.into_inner(), [stays.identity]);
    assert!(!swept.is_clean());
    let said = swept.said();
    assert!(
        said.starts_with("the windows left 1 process(es) running after they were closed: 302 grep --line-buffered tick"),
        "{said}"
    );
    assert!(said.contains("1 ended, 0 gone already"), "{said}");
    assert!(swept.problems().is_empty());
}

#[test]
fn windows_that_left_nothing_say_so_and_what_could_not_be_ended_or_read_is_a_problem() {
    let mut watch = Watch::new();
    watch.add(
        Identity {
            pid: 100,
            started: 10,
        },
        Path::new("/runs/monitor-wait"),
    );
    let machine = Lists(RefCell::new(vec![vec![running(1, 1, 0, "launchd")]]));
    let swept = watch.sweep_by(&machine, |_| None, |_| Ended::default(), Duration::ZERO);
    assert!(swept.is_clean());
    assert_eq!(
        swept.said(),
        "the windows left nothing running (1 window(s) watched, 0 process(es) seen under them while they ran)"
    );
    assert!(swept.problems().is_empty());
    // A process that could not be ended may still run.
    let stuck = running(301, 21, 1, "tail -f out");
    let swept = Swept {
        found: vec![stuck.clone()],
        ended: Ended {
            failed: vec![(stuck, "its end failed: refused".to_owned())],
            ..Ended::default()
        },
        unread: None,
        watched: (1, 3),
    };
    assert_eq!(
        swept.problems(),
        ["process 301 (tail -f out) may still run: its end failed: refused"]
    );
    // A machine that could not be read says so and is not clean.
    let unread = Swept {
        unread: Some("ps: killed".to_owned()),
        ..Swept::default()
    };
    assert!(!unread.is_clean());
    assert!(
        unread.said().contains("could not be read"),
        "{}",
        unread.said()
    );
}

/// A real Monitor's shape, on a Mac or Linux: a window (a shell) that runs `tail -f … | grep` in a
/// folder made for the case, killed, which leaves the pipeline with no parent. Found and ended by
/// which process each is, and what runs in another folder, begun at the same time, is left alone.
#[cfg(unix)]
#[test]
fn a_real_monitor_pipeline_left_by_a_killed_window_is_ended_and_what_runs_elsewhere_is_not() {
    use crate::process::{is_alive, Run};

    let folder = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    // Another run's orphan: parent init's, in a folder of its own, begun after this window.
    let ran = Run::new("/bin/sh")
        .args(["-c", "sleep 31 > /dev/null 2>&1 &"])
        .cwd(elsewhere.path())
        .inheriting_env()
        .run()
        .unwrap();
    assert_eq!(ran.code, Some(0));
    let mut window = Run::new("/bin/sh")
        .args(["-c", "tail -f /dev/null | grep --line-buffered tick & wait"])
        .cwd(folder.path())
        .inheriting_env()
        .spawn()
        .unwrap();
    let root = ThisMachine
        .identity(window.id())
        .unwrap()
        .expect("the window is there");
    let mut watch = Watch::new();
    watch.add(root, folder.path());
    // The pipeline's programs, as they run under the window.
    let mut tree = Vec::new();
    for _ in 0..100 {
        watch.look_now();
        tree = watch.windows[0].tree.clone();
        let has = |word: &str| tree.iter().any(|one| one.command == word);
        if has("tail -f /dev/null") && has("grep --line-buffered tick") {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let tail = tree
        .iter()
        .find(|one| one.command == "tail -f /dev/null")
        .cloned()
        .expect("the window ran its pipeline");
    let grep = tree
        .iter()
        .find(|one| one.command == "grep --line-buffered tick")
        .cloned()
        .expect("the window ran its pipeline");
    // Killed, as a window that is closed is, and not its children: they are left with no parent.
    window.kill();
    let swept = watch.sweep();
    let found = pids(&swept.found);
    // Read what is alive before anything is ended by this test.
    let decoy = ThisMachine
        .processes()
        .unwrap()
        .into_iter()
        .find(|one| one.command == "sleep 31" && one.ppid == 1)
        .filter(|one| working_folder(one.identity.pid) == elsewhere.path().canonicalize().ok());
    // Gone a moment after the signal, which is asked of the system and not waited for.
    let gone = (0..100).any(|_| {
        std::thread::sleep(Duration::from_millis(100));
        !is_alive(tail.identity.pid) && !is_alive(grep.identity.pid)
    });
    if let Some(decoy) = &decoy {
        let _ = end_chosen(std::slice::from_ref(decoy));
    }
    assert!(
        found.contains(&tail.identity.pid) && found.contains(&grep.identity.pid),
        "the pipeline was not found: {found:?} {swept:#?}"
    );
    assert_eq!(swept.ended.ended.len(), found.len(), "{swept:#?}");
    assert!(swept.problems().is_empty(), "{swept:#?}");
    assert!(gone, "the pipeline still runs: {tail:?} {grep:?}");
    let decoy = decoy.expect("the other run's orphan was never listed: the test proves nothing");
    assert!(
        !found.contains(&decoy.identity.pid),
        "another folder's orphan is taken for the window's: {found:?}"
    );
}
