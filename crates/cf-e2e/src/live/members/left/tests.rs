use std::cell::RefCell;

use super::*;
#[cfg(unix)]
use crate::live::members::descent::end_chosen;

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
    watch.add(
        window,
        Some(RunFolder::assumed(Path::new("/runs/monitor-wait"))),
    );
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
        Some(RunFolder::assumed(folder)),
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
fn a_process_on_its_way_out_when_the_windows_are_closed_is_no_leftover_and_one_that_stays_is_said_and_not_ended(
) {
    let mut watch = Watch::new();
    watch.add(
        Identity {
            pid: 100,
            started: 10,
        },
        Some(RunFolder::assumed(Path::new("/runs/monitor-wait"))),
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
    let swept = watch.sweep_by(
        &machine,
        |_| None,
        // Looks again until the settle time is out: the first look found one more than the rest.
        Duration::from_millis(600),
    );
    assert_eq!(pids(&swept.found), [302], "{swept:#?}");
    assert!(!swept.is_clean());
    let said = swept.said();
    assert_eq!(
        said,
        "the windows left 1 process(es) running after they were closed: 302 grep --line-buffered tick; the kit ended none of them (it ends a process only by descent from the run's window, the stand-in's jobs, the run's own word or an orphan of a folder under the run's temp root)"
    );
    assert!(swept.problems().is_empty());
}

#[test]
fn windows_that_left_nothing_say_so_and_a_machine_that_could_not_be_read_is_a_problem() {
    let mut watch = Watch::new();
    watch.add(
        Identity {
            pid: 100,
            started: 10,
        },
        Some(RunFolder::assumed(Path::new("/runs/monitor-wait"))),
    );
    let machine = Lists(RefCell::new(vec![vec![running(1, 1, 0, "launchd")]]));
    let swept = watch.sweep_by(&machine, |_| None, Duration::ZERO);
    assert!(swept.is_clean());
    assert_eq!(
        swept.said(),
        "the windows left nothing running (1 window(s) watched, 0 process(es) seen under them while they ran)"
    );
    assert!(swept.problems().is_empty());
    // What was found and left as it is is no problem of the close: it is said.
    let left = Swept {
        found: vec![running(301, 21, 1, "tail -f out")],
        unread: None,
        watched: (1, 3),
    };
    assert!(left.problems().is_empty());
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
    assert_eq!(unread.problems().len(), 1);
}

/// Ends what it holds, each by which process it is, when the test ends however it does.
#[cfg(unix)]
struct Reaper(Vec<Running>);

#[cfg(unix)]
impl Drop for Reaper {
    fn drop(&mut self) {
        let _ = end_chosen(&self.0);
    }
}

/// A process the test's own shell started and printed the number of (`pid`, running `command`),
/// ended when the test ends however it does: as the process it was read as, or, where no read
/// came first, the process of that number and that command line begun no earlier than the test.
#[cfg(unix)]
struct Decoy {
    pid: u32,
    command: &'static str,
    read: Option<Running>,
}

#[cfg(unix)]
impl Drop for Decoy {
    fn drop(&mut self) {
        let unread = || {
            let since = ThisMachine.identity(std::process::id()).ok()??.started;
            ThisMachine.processes().ok()?.into_iter().find(|one| {
                one.identity.pid == self.pid
                    && one.command == self.command
                    && one.identity.started >= since
            })
        };
        if let Some(decoy) = self.read.take().or_else(unread) {
            let _ = end_chosen(&[decoy]);
        }
    }
}

/// A real Monitor's shape, on a Mac or Linux: a window (a shell) that runs `tail -f … | grep` in a
/// folder made for the case, killed, which leaves the pipeline with no parent. Found and said, and
/// not ended (the test ends it, by which process each is), and what runs in another folder, begun
/// at the same time, is left alone: another tree running this test at the same time has its own,
/// which this test never takes for its own, for each is told by the number the test's own shell
/// printed.
#[cfg(unix)]
#[test]
fn a_real_monitor_pipeline_left_by_a_killed_window_is_found_and_not_ended_and_what_runs_elsewhere_is_not_found(
) {
    use crate::process::{is_alive, Run};

    let folder = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    // Another run's orphan: parent init's, in a folder of its own, begun after this window. Its
    // number is the one this test's shell printed; it is ended however the test ends.
    let ran = Run::new("/bin/sh")
        .args(["-c", "sleep 31 > /dev/null 2>&1 & echo $!"])
        .cwd(elsewhere.path())
        .inheriting_env()
        .run()
        .unwrap();
    assert_eq!(ran.code, Some(0));
    let mut guard = Decoy {
        pid: ran.stdout.trim().parse().unwrap(),
        command: "sleep 31",
        read: None,
    };
    let mut reaper = Reaper(Vec::new());
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
    watch.add(root, RunFolder::under(folder.path(), folder.path()));
    // The pipeline's programs, as they run under the window.
    let mut tree = Vec::new();
    for _ in 0..100 {
        watch.look_now();
        tree = watch.windows[0].tree.clone();
        // What the window runs is ended however the test ends, from the first look that saw it.
        reaper.0.clone_from(&tree);
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
    // The decoy, by the number the shell printed: the same process, as it runs.
    let decoy = ThisMachine
        .processes()
        .unwrap()
        .into_iter()
        .find(|one| one.identity.pid == guard.pid)
        .expect("the other run's orphan was never listed: the test proves nothing");
    guard.read = Some(decoy.clone());
    // Killed, as a window that is closed is, and not its children: they are left with no parent.
    window.kill();
    let swept = watch.sweep();
    let found = pids(&swept.found);
    std::thread::sleep(Duration::from_secs(1));
    let runs = |one: &Running| {
        ThisMachine
            .processes()
            .unwrap_or_default()
            .iter()
            .any(|now| now.identity == one.identity)
    };
    assert!(
        found.contains(&tail.identity.pid) && found.contains(&grep.identity.pid),
        "the pipeline was not found: {found:?} {swept:#?}"
    );
    assert!(swept.problems().is_empty(), "{swept:#?}");
    assert!(
        runs(&tail) && runs(&grep),
        "the sweep ended the pipeline: {}",
        swept.said()
    );
    // The test's own, ended by which process each is.
    let ended = end_chosen(&[tail.clone(), grep.clone()]);
    assert!(ended.problems().is_empty(), "{ended:#?}");
    assert!(
        (0..100).any(|_| {
            std::thread::sleep(Duration::from_millis(100));
            !is_alive(tail.identity.pid) && !is_alive(grep.identity.pid)
        }),
        "the pipeline still runs: {tail:?} {grep:?}"
    );
    assert!(
        !found.contains(&decoy.identity.pid),
        "another folder's orphan is taken for the window's: {found:?}"
    );
}

/// What the kit asked a window to run carries a word of the run's own (a folder of its, the word
/// made for the run), which stays in the command line of what a monitor ran: on Windows, where the
/// dead parent's number stays in the child's row and nothing reads a process's folder, it is what
/// finds the pipeline of a window that closed before the look, or was never looked at.
#[test]
fn a_process_whose_parent_has_gone_and_whose_command_line_holds_a_word_of_the_run_is_left_on_every_platform(
) {
    let mut watch = Watch::new();
    watch.add(
        Identity {
            pid: 100,
            started: 10,
        },
        Some(RunFolder::assumed(Path::new("/runs/monitor-wait"))),
    );
    watch.add_word("cf-run-77-1700000000000");
    watch.add_word("short");
    // A path is a word of a folder of the run's alone ([`RunFolder`]).
    watch.add_word("/Users/someone/projects");
    watch.add_word(r"C:\Users\someone");
    assert_eq!(
        watch.words,
        ["cf-run-77-1700000000000"],
        "a short word is nobody's own, and a path no word"
    );
    let word = "zsh -c : cf-mark-ticks cf-run-77-1700000000000; tail -f out | grep tick";
    let after = vec![
        // Windows' table: the parent's number (90) is no process now, and nothing reads a folder.
        running(300, 20, 90, word),
        running(301, 21, 300, "tail -f out"),
        // The parent's number is another process's, begun after the child.
        running(310, 22, 91, word),
        running(91, 50, 1, "an editor"),
        // A shell that still runs the command: its parent runs, and the window's tree sees it.
        running(320, 23, 92, word),
        running(92, 5, 1, "a shell"),
        // The same command of another run: another word.
        running(
            400,
            24,
            90,
            "zsh -c : cf-mark-ticks cf-run-88-1700000000001; tail -f out",
        ),
        // An orphan that holds no word and has no folder to read.
        running(500, 25, 1, "tail -f out"),
        // One that holds the word and began before the run's window: none of the run's.
        running(600, 5, 90, word),
    ];
    let left = watch.left(&after, |_| None);
    assert_eq!(pids(&left), [300, 301, 310], "{left:#?}");
}

#[test]
fn a_command_left_by_a_window_closed_and_reopened_under_the_same_pane_id_is_found_by_the_final_sweep(
) {
    // The engine opens the reopened member's window under the old pane id with a new generation:
    // two windows of the run, the old one's pipeline left running, in the folder, begun after the
    // old window and before the new one.
    let folder = Path::new("/runs/reopened");
    let mut watch = Watch::new();
    for root in [
        Identity {
            pid: 100,
            started: 10,
        },
        Identity {
            pid: 200,
            started: 50,
        },
    ] {
        watch.add(root, Some(RunFolder::assumed(folder)));
    }
    let while_it_runs = vec![
        running(100, 10, 1, "claude"),
        running(300, 20, 100, "sh -c node -e long"),
    ];
    watch.look(&while_it_runs);
    let after = vec![
        // The old window and its shell have gone; the pipeline is an orphan of the folder.
        running(301, 21, 1, "node -e long"),
        // Orphaned and begun after the old window, before the new: the folder rule's bound is the
        // earliest window of the run.
        running(302, 30, 1, "node -e other"),
        running(200, 50, 1, "claude"),
    ];
    let folder_of = |pid: u32| (pid == 301 || pid == 302).then(|| folder.to_path_buf());
    let left = watch.left(&after, folder_of);
    assert_eq!(pids(&left), [301, 302], "{left:#?}");
}

#[test]
fn a_sweep_that_watched_no_window_could_not_look_and_never_says_nothing() {
    let watch = Watch::new();
    let machine = Lists(RefCell::new(vec![vec![running(1, 1, 0, "launchd")]]));
    let swept = watch.sweep_by(&machine, |_| None, Duration::ZERO);
    assert!(!swept.looked());
    assert!(!swept.is_clean());
    assert!(
        swept.said().starts_with("the sweep could not look"),
        "{}",
        swept.said()
    );
    // A window watched and a machine read is a look.
    let mut watch = Watch::new();
    watch.add(
        Identity {
            pid: 100,
            started: 10,
        },
        Some(RunFolder::assumed(Path::new("/runs/x"))),
    );
    let swept = watch.sweep_by(&machine, |_| None, Duration::ZERO);
    assert!(swept.looked() && swept.is_clean());
}

/// A live rig's home and project folder are the person's own, outside the rig's temp root: at
/// 18:58 on 2026-11-12 the close's sweep took `/Users/…` for a word of the run. Each ground alone:
/// a path outside the root is no word and no folder of the run's, whenever what holds it began; a
/// path under the root is the run's only for what began no earlier than the run's first window.
/// The window is watched all the same, by descent.
#[test]
fn a_path_is_a_ground_of_the_runs_only_under_its_temp_root_and_only_for_what_began_after_its_window(
) {
    let root = tempfile::tempdir().unwrap();
    let inside = root.path().join("home");
    let live = inside.join("live");
    std::fs::create_dir_all(&live).unwrap();
    let person = tempfile::tempdir().unwrap();
    let theirs = person.path().join("live");
    std::fs::create_dir_all(&theirs).unwrap();
    let run_folder = |path: &Path| RunFolder::under(root.path(), path);
    let window = Identity {
        pid: 100,
        started: 10,
    };
    let text = |path: &Path| path.display().to_string();

    // The live shape: the home and the window's folder are the person's.
    let mut watch = Watch::of_windows(
        &run_folder,
        &[(window, theirs.clone())],
        Some(person.path()),
    );
    assert_eq!(watch.windows(), 1, "the window is watched, by descent");
    assert!(watch.windows[0].folder.is_none(), "{:?}", watch.windows[0]);
    assert_eq!(
        watch.words,
        [super::super::run_word()],
        "no path of the person's is a word"
    );
    watch.look(&[
        running(100, 10, 1, "claude"),
        running(101, 11, 100, "sh -c sleep 4711"),
    ]);
    let after = vec![
        // What the window ran, which descends from it: the run's, by descent.
        running(101, 11, 1, "sh -c sleep 4711"),
        // The person's and other tasks', begun after the window, holding the home's path or
        // running in the window's folder.
        running(
            300,
            20,
            1,
            &format!("tail -f {}", text(&person.path().join("log"))),
        ),
        running(301, 21, 1, "a program of the person's"),
    ];
    let folder_of = |pid: u32| (pid == 301).then(|| theirs.canonicalize().unwrap());
    assert_eq!(pids(&watch.left(&after, folder_of)), [101]);

    // The stand-in's shape: the home and the window's folder are under the run's root.
    let watch = Watch::of_windows(&run_folder, &[(window, live.clone())], Some(&inside));
    assert!(watch.windows[0].folder.is_some());
    assert!(watch.words.contains(&text(&inside)), "{:?}", watch.words);
    let after = vec![
        // Holds the home's path: after the window, the run's; before it, none of the run's.
        running(
            300,
            20,
            1,
            &format!("tail -f {}", text(&inside.join("log"))),
        ),
        running(302, 5, 1, &format!("tail -f {}", text(&inside.join("log")))),
        // Runs in the window's folder: after the window, the run's; before it, none of the run's.
        running(301, 21, 1, "a job"),
        running(303, 6, 1, "a job"),
    ];
    let folder_of = |pid: u32| (pid == 301 || pid == 303).then(|| live.canonicalize().unwrap());
    assert_eq!(pids(&watch.left(&after, folder_of)), [300, 301]);
}
