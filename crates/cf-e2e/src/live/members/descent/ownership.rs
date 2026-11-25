//! The review T-2228's four cases, on recorded tables: a stranger's process the kit took for the
//! window's, and then ended (the recorder says which numbers it was asked to end). Each is a
//! table of fictional numbers and times. W is the window, P the intermediate number that was
//! reused, L the leaf, X a process outside the window. Nothing is signalled.

use super::testing::{Listed, Now};
use super::*;

const W: u32 = 5000;
const S: u32 = 5020;
const P: u32 = 5040;
const M: u32 = 5060;
const L: u32 = 5080;
const X: u32 = 9000;

fn id(pid: u32, started: u64) -> Identity {
    Identity { pid, started }
}

fn numbers(items: &[Running]) -> Vec<u32> {
    items.iter().map(|one| one.identity.pid).collect()
}

/// What the recorder was asked to end when the kit chose among `rows` the processes that descend
/// from `root`, each of which `now` says began when it says.
fn ended_of(rows: &[Running], root: Identity, now: &[(u32, u64)]) -> Vec<u32> {
    let chosen = descending_from(rows, root);
    let mut recorder = Listed::new(rows.to_vec());
    for &(pid, started) in now {
        recorder = recorder.with(pid, Now::Began(started));
    }
    end_chosen_by(&recorder, &chosen);
    recorder.ended()
}

/// What the system answers of a process, as the reader takes it.
fn bsd_of(ppid: u32, started: u64) -> Bsd {
    Bsd { ppid, started }
}

/// The printed second of a start, for a table of the first minute of 1970.
fn second_of(micros: u64) -> Option<u64> {
    Some(19_700_101_000_000 + micros / 1_000_000)
}

/// The Mac's reader over `listing` (`ps`) and the `answers` the system gives, in order, for the
/// numbers asked.
fn mac_rows(listing: &str, answers: &[(u32, u32, u64)]) -> Vec<Running> {
    let answers: std::cell::RefCell<std::collections::VecDeque<_>> =
        std::cell::RefCell::new(answers.iter().copied().collect());
    refined_by_bsd(
        ps_lines(listing),
        |pid| {
            let (expected, ppid, started) = answers.borrow_mut().pop_front().expect("an answer");
            assert_eq!(pid, expected);
            Ok(Some(bsd_of(ppid, started)))
        },
        second_of,
    )
}

const LISTING: &str = "5000 1 Thu Jan 1 00:00:01 1970 window\n5040 5000 Thu Jan 1 00:00:02 1970 owned launcher\n5080 5040 Thu Jan 1 00:00:04 1970 owned worker\n";

#[test]
fn a_mac_leaf_whose_number_and_parent_number_a_stranger_took_in_the_same_printed_second_is_left_running()
{
    // ps saw the window's parent 5040 (2.000 s) and its leaf 5080 (4.100 s). The parent's
    // answer came before the turnover. Then a stranger took 5040 (4.500 s) and its own child took
    // 5080 (4.800 s), in the second `ps` printed for the leaf: the last answer names parent 5040.
    let w = id(W, 1_000_000);
    let rows = mac_rows(
        LISTING,
        &[(W, 1, 1_000_000), (P, W, 2_000_000), (L, P, 4_800_000)],
    );
    let ended = ended_of(&rows, w, &[(P, 4_500_000), (L, 4_800_000)]);
    assert!(
        !ended.contains(&L),
        "the stranger's child 5080 was ended: {ended:?}"
    );
}

#[test]
fn a_mac_leaf_born_an_hour_after_the_one_listed_in_the_same_printed_second_is_left_running() {
    // The clock hour that repeats when the clocks go back: births 3600.7 s apart print the same
    // second, so the printed second tells nothing between them.
    let w = id(W, 1_000_000);
    let answers: std::cell::RefCell<std::collections::VecDeque<_>> =
        std::cell::RefCell::new(
            [(W, 1, 1_000_000), (P, W, 2_000_000), (L, P, 3_604_800_000u64)]
                .into_iter()
                .collect(),
        );
    let rows = refined_by_bsd(
        ps_lines(LISTING),
        |pid| {
            let (expected, ppid, started) = answers.borrow_mut().pop_front().expect("an answer");
            assert_eq!(pid, expected);
            Ok(Some(bsd_of(ppid, started)))
        },
        // Both births print as the same second of the repeated hour.
        |micros| Some(if micros > 3_000_000_000 { 4 } else { micros / 1_000_000 } + 19_700_101_000_000),
    );
    let ended = ended_of(&rows, w, &[(P, 3_600_000_000), (L, 3_604_800_000)]);
    assert!(
        !ended.contains(&L),
        "the stranger's child 5080 was ended: {ended:?}"
    );
}

const BASE: u64 = 133_000_000_000_000_000;

fn at(seconds: u64) -> u64 {
    BASE + seconds * 10_000_000
}

fn win_row(pid: u32, ppid: u32, born: u64, exe: Option<&str>) -> serde_json::Value {
    serde_json::json!({"pid": pid, "ppid": ppid, "started": born, "exe": exe, "command": "row"})
}

/// Windows' reader over a table, the handles each saying the birth the table says, and Git Bash's
/// `ps -W` of `folder` saying `git`.
fn win_rows(table: &[serde_json::Value], git: &str) -> Vec<Running> {
    let listed = table_of(&serde_json::Value::Array(table.to_vec()).to_string()).unwrap();
    let held = refined_by_handle(listed, |pid| {
        Ok(table
            .iter()
            .find(|row| row["pid"] == pid)
            .map(|row| row["started"].as_u64().unwrap()))
    });
    with_cygwin_parents(held, |_| Some(git.to_owned()))
}

const SH: &str = r"C:\fixture\usr\bin\sh.exe";

#[test]
fn a_windows_leaf_of_a_foreign_process_is_left_running_when_a_later_git_bash_listing_names_its_number_under_the_window()
{
    // Table: the window's own shell S, a foreign P (its starter gone, 9080) and P's child L, all
    // read exactly. Later a different process took P's number under S; Git Bash says so. Its
    // parent is that other process's, not the foreign P's.
    let w = id(W, at(90));
    let table = [
        win_row(W, 0, w.started, None),
        win_row(S, W, at(100), None),
        win_row(P, 9080, at(250), Some(SH)),
        win_row(L, P, at(300), None),
    ];
    let git = "PID PPID PGID WINPID\n101 1 101 5020\n303 101 101 5040\n";
    let rows = win_rows(&table, git);
    let ended = ended_of(&rows, w, &[(S, at(100)), (P, at(400)), (L, at(300))]);
    assert!(
        !ended.contains(&L),
        "the foreign process's child 5080 was ended: {ended:?}"
    );
}

#[test]
fn a_windows_foreign_descendant_is_left_running_when_a_later_git_bash_listing_gives_its_parents_numbers_to_the_window()
{
    // The table: the window's P (100 s), a foreign M (130 s, its starter gone) and M's child L
    // (150 s). After the handles were closed an outside process took P's number and its own child
    // took M's; Git Bash describes that pair, and the kit gave its edge to the two older rows.
    let w = id(W, at(90));
    let table = [
        win_row(W, 0, w.started, None),
        win_row(P, W, at(100), None),
        win_row(M, 9080, at(130), Some(SH)),
        win_row(L, M, at(150), None),
    ];
    let git = "PID PPID PGID WINPID\n700 1 700 9000\n701 700 700 5040\n702 701 700 5060\n";
    let rows = win_rows(&table, git);
    let ended = ended_of(&rows, w, &[(P, at(250)), (M, at(300)), (L, at(150))]);
    assert!(
        !ended.contains(&L),
        "the foreign process's descendant 5080 was ended: {ended:?}"
    );
}

#[test]
fn the_genuine_chains_are_still_chosen_and_ended() {
    let w = id(W, 1_000_000);
    let rows = mac_rows(
        LISTING,
        &[(W, 1, 1_000_000), (P, W, 2_000_000), (L, P, 4_100_000)],
    );
    assert_eq!(numbers(&descending_from(&rows, w)), [P, L]);
    assert_eq!(ended_of(&rows, w, &[]), [P, L]);

    let w = id(W, at(90));
    let table = [
        win_row(W, 0, w.started, None),
        win_row(P, W, at(100), None),
        win_row(L, P, at(150), None),
    ];
    let rows = win_rows(&table, "PID PPID PGID WINPID\n");
    assert_eq!(ended_of(&rows, w, &[]), [P, L]);
    let _ = (S, X);
}
