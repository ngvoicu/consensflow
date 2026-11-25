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

fn unproved_of(rows: &[Running], root: Identity) -> Vec<Running> {
    unproved_under(rows, root)
        .into_iter()
        .map(|(process, _)| process)
        .collect()
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

/// The printed second of a start, for a table of the first minute of 1970.
fn second_of(micros: u64) -> Option<u64> {
    Some(19_700_101_000_000 + micros / 1_000_000)
}

/// What the system says of one process a Mac's reader asks about: its number, its parent's, when
/// it began, and the unique identifiers the pane host reads, its own and its forker's.
#[derive(Clone, Copy)]
struct Said {
    pid: u32,
    ppid: u32,
    started: u64,
    own: u64,
    forker: u64,
}

fn said(pid: u32, ppid: u32, started: u64, own: u64, forker: u64) -> Said {
    Said {
        pid,
        ppid,
        started,
        own,
        forker,
    }
}

/// The Mac's reader over `listing` (`ps`) and the `answers` the system gives, in order, for the
/// numbers asked, each second printed as `second_of` says.
fn mac_rows_in(
    listing: &str,
    answers: &[Said],
    second_of: impl Fn(u64) -> Option<u64>,
) -> Vec<Running> {
    let answers: std::cell::RefCell<std::collections::VecDeque<_>> =
        std::cell::RefCell::new(answers.iter().copied().collect());
    refined_by_bsd(
        ps_lines(listing),
        |pid| {
            let said = answers.borrow_mut().pop_front().expect("an answer");
            assert_eq!(pid, said.pid);
            Ok(Some(Bsd {
                ppid: said.ppid,
                started: said.started,
                link: Link::Forked {
                    own: said.own,
                    forker: said.forker,
                },
            }))
        },
        second_of,
    )
}

fn mac_rows(listing: &str, answers: &[Said]) -> Vec<Running> {
    mac_rows_in(listing, answers, second_of)
}

/// Identifiers: the window 100, its owned launcher 200 and the owned leaf 300; a stranger's
/// process that took the launcher's number 900, and its own child 950.
const WINDOW_ID: u64 = 100;
const PARENT_ID: u64 = 200;
const LEAF_ID: u64 = 300;
const STRANGER_ID: u64 = 900;
const STRANGER_CHILD_ID: u64 = 950;

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
        &[
            said(W, 1, 1_000_000, WINDOW_ID, 1),
            said(P, W, 2_000_000, PARENT_ID, WINDOW_ID),
            said(L, P, 4_800_000, STRANGER_CHILD_ID, STRANGER_ID),
        ],
    );
    let ended = ended_of(&rows, w, &[(P, 4_500_000), (L, 4_800_000)]);
    assert!(
        !ended.contains(&L),
        "the stranger's child 5080 was ended: {ended:?}"
    );
    assert_eq!(numbers(&descending_from(&rows, w)), [P]);
    // Another process's child, as its forker says, is no one of the window's and is not named.
    assert!(unproved_under(&rows, w).is_empty());
}

#[test]
fn a_mac_leaf_born_an_hour_after_the_one_listed_in_the_same_printed_second_is_left_running() {
    // The clock hour that repeats when the clocks go back: births 3600.7 s apart print the same
    // second, so the printed second tells nothing between them.
    let w = id(W, 1_000_000);
    let rows = mac_rows_in(
        LISTING,
        &[
            said(W, 1, 1_000_000, WINDOW_ID, 1),
            said(P, W, 2_000_000, PARENT_ID, WINDOW_ID),
            said(L, P, 3_604_800_000, STRANGER_CHILD_ID, STRANGER_ID),
        ],
        // Both births print as the same second of the repeated hour.
        |micros| {
            Some(if micros > 3_000_000_000 { 4 } else { micros / 1_000_000 } + 19_700_101_000_000)
        },
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

/// Windows' reader over a table, each handle saying the birth the table says and the creator the
/// table's parent says (`creator` says another for a number), Git Bash's `ps -W` saying `git`, and
/// `held` the creation time a handle held since before the reading says after it.
fn win_rows_by(
    table: &[serde_json::Value],
    creator: impl Fn(u32, u32) -> u32,
    git: &str,
    held: impl Fn(u32) -> Option<u64>,
) -> Vec<Running> {
    let listed = table_of(&serde_json::Value::Array(table.to_vec()).to_string()).unwrap();
    let opened = refined_by_handle(listed, |pid| {
        Ok(table.iter().find(|row| row["pid"] == pid).map(|row| Opened {
            created: row["started"].as_u64().unwrap(),
            creator: Some(creator(pid, row["ppid"].as_u64().unwrap() as u32)),
        }))
    });
    with_cygwin_parents(opened, |_| Some(git.to_owned()), held)
}

/// As [`win_rows_by`], every handle held and saying after the reading what it said before.
fn win_rows(table: &[serde_json::Value], git: &str) -> Vec<Running> {
    win_rows_by(table, |_, ppid| ppid, git, |pid| {
        table
            .iter()
            .find(|row| row["pid"] == pid)
            .map(|row| row["started"].as_u64().unwrap())
    })
}

/// As [`win_rows`], the handles of the numbers in `changed` saying another time (or none: not held,
/// or exited) after the reading.
fn win_rows_changed(
    table: &[serde_json::Value],
    git: &str,
    changed: &[(u32, Option<u64>)],
) -> Vec<Running> {
    win_rows_by(table, |_, ppid| ppid, git, |pid| {
        match changed.iter().find(|(number, _)| *number == pid) {
            Some((_, now)) => *now,
            None => table
                .iter()
                .find(|row| row["pid"] == pid)
                .map(|row| row["started"].as_u64().unwrap()),
        }
    })
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
    let rows = win_rows_changed(&table, git, &[(P, Some(at(400)))]);
    let ended = ended_of(&rows, w, &[(S, at(100)), (P, at(400)), (L, at(300))]);
    assert!(
        !ended.contains(&L),
        "the foreign process's child 5080 was ended: {ended:?}"
    );
    // The shell is the window's; the foreign process and its child are named as left running.
    assert_eq!(numbers(&descending_from(&rows, w)), [S]);
    assert_eq!(numbers(&unproved_of(&rows, w)), [P, L]);
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
    let rows = win_rows_changed(&table, git, &[(P, Some(at(250))), (M, Some(at(300)))]);
    let ended = ended_of(&rows, w, &[(P, at(250)), (M, at(300)), (L, at(150))]);
    assert!(
        !ended.contains(&L),
        "the foreign process's descendant 5080 was ended: {ended:?}"
    );
    assert_eq!(numbers(&descending_from(&rows, w)), [P]);
    assert_eq!(numbers(&unproved_of(&rows, w)), [M, L]);
}

#[test]
fn the_genuine_chains_are_still_chosen_and_ended() {
    let w = id(W, 1_000_000);
    let rows = mac_rows(
        LISTING,
        &[
            said(W, 1, 1_000_000, WINDOW_ID, 1),
            said(P, W, 2_000_000, PARENT_ID, WINDOW_ID),
            said(L, P, 4_100_000, LEAF_ID, PARENT_ID),
        ],
    );
    assert_eq!(numbers(&descending_from(&rows, w)), [P, L]);
    assert!(unproved_under(&rows, w).is_empty());
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

/// The Mac's genuine chain, W, P, L, read with their identifiers.
fn genuine() -> Vec<Running> {
    mac_rows(
        LISTING,
        &[
            said(W, 1, 1_000_000, WINDOW_ID, 1),
            said(P, W, 2_000_000, PARENT_ID, WINDOW_ID),
            said(L, P, 4_100_000, LEAF_ID, PARENT_ID),
        ],
    )
}

#[test]
fn a_chosen_leaf_whose_unique_identifier_is_another_processs_at_the_send_is_gone_and_not_ended() {
    // The leaf's own check at the send: its birth is the one chosen, as a process whose number
    // another took in the same microsecond would also show, but the identifier it was proved by
    // is not.
    let w = id(W, 1_000_000);
    let rows = genuine();
    let chosen = descending_from(&rows, w);
    let recorder = Listed::new(rows.clone()).with_unique(L, STRANGER_CHILD_ID);
    let said = end_chosen_by(&recorder, &chosen);
    assert_eq!(recorder.ended(), [P]);
    assert_eq!(numbers(&said.gone), [L]);
    assert!(said.failed.is_empty(), "{said:?}");
}

#[test]
fn a_child_the_system_would_not_read_is_named_and_left_running_with_what_is_under_it() {
    let w = id(W, 1_000_000);
    let mut rows = genuine();
    rows.iter_mut().find(|row| row.identity.pid == P).unwrap().link = Link::Unread;
    assert!(descending_from(&rows, w).is_empty());
    let named = unproved_under(&rows, w);
    assert_eq!(numbers(&named.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>()), [P, L]);
    assert!(named[0].1.contains("said nothing"), "{}", named[0].1);
    assert!(named[1].1.contains("under process 5040"), "{}", named[1].1);
}

#[test]
fn two_processes_of_one_reading_with_one_identifier_prove_no_edge_and_are_named() {
    let w = id(W, 1_000_000);
    let mut rows = genuine();
    // A second row (a number another process holds in the same reading) with the parent's identifier.
    let mut twin = rows[1].clone();
    twin.identity = id(7777, 2_500_000);
    twin.ppid = 1;
    rows.push(twin);
    // The window made P; what P made cannot be told from what its twin's identifier names.
    assert_eq!(numbers(&descending_from(&rows, w)), [P]);
    assert_eq!(numbers(&unproved_of(&rows, w)), [L]);
}

#[test]
fn a_windows_child_whose_handle_names_another_creator_than_the_table_is_named_and_left_running() {
    let w = id(W, at(90));
    let table = [
        win_row(W, 0, w.started, None),
        win_row(P, W, at(100), None),
        win_row(L, P, at(150), None),
    ];
    // The handle of the leaf says a creator that is not the one the table names.
    let rows = win_rows_by(&table, |pid, ppid| if pid == L { 77 } else { ppid }, "PID PPID PGID WINPID\n", |_| None);
    assert_eq!(numbers(&descending_from(&rows, w)), [P]);
    let named = unproved_under(&rows, w);
    assert_eq!(named.len(), 1);
    assert_eq!(named[0].0.identity.pid, L);
}

#[test]
fn a_windows_child_born_before_the_process_that_holds_its_creators_number_is_no_one_of_the_windows() {
    // The creator of L ended, and its number went to P, born after L: L is another's.
    let w = id(W, at(90));
    let table = [
        win_row(W, 0, w.started, None),
        win_row(P, W, at(250), None),
        win_row(L, P, at(150), None),
    ];
    let rows = win_rows(&table, "PID PPID PGID WINPID\n");
    assert_eq!(numbers(&descending_from(&rows, w)), [P]);
    assert!(unproved_under(&rows, w).is_empty());
}

#[test]
fn the_stuck_scripts_sleep_whose_creator_has_gone_is_the_windows_when_both_ends_were_held() {
    // `sh ./script` forks a copy of itself that starts the sleep and is gone: the sleep's creator
    // (9080) is no process. The shell S is the window's; Git Bash says S is the sleep's parent.
    let w = id(W, at(90));
    let table = [
        win_row(W, 0, w.started, None),
        win_row(S, W, at(100), None),
        win_row(P, 9080, at(250), Some(SH)),
    ];
    let git = "PID PPID PGID WINPID\n101 1 101 5020\n303 101 101 5040\n";
    let rows = win_rows(&table, git);
    assert_eq!(numbers(&descending_from(&rows, w)), [S, P]);
}

#[test]
fn the_stuck_scripts_sleep_is_left_running_and_named_where_either_end_is_not_held_or_a_time_differs() {
    let w = id(W, at(90));
    let table = [
        win_row(W, 0, w.started, None),
        win_row(S, W, at(100), None),
        win_row(P, 9080, at(250), Some(SH)),
    ];
    let git = "PID PPID PGID WINPID\n101 1 101 5020\n303 101 101 5040\n";
    for changed in [
        // The sleep is not held, or its number has another process by the end of the reading.
        [(P, None)],
        [(P, Some(at(400)))],
        // The parent is not held, or says another time.
        [(S, None)],
        [(S, Some(at(101)))],
    ] {
        let rows = win_rows_changed(&table, git, &changed);
        assert_eq!(numbers(&descending_from(&rows, w)), [S], "{changed:?}");
        assert_eq!(numbers(&unproved_of(&rows, w)), [P], "{changed:?}");
        assert!(!ended_of(&rows, w, &[]).contains(&P), "{changed:?}");
    }
    // A parent that began after the sleep is no parent of it.
    let late = [
        win_row(W, 0, w.started, None),
        win_row(S, W, at(300), None),
        win_row(P, 9080, at(250), Some(SH)),
    ];
    let rows = win_rows(&late, git);
    assert_eq!(numbers(&descending_from(&rows, w)), [S]);
}

#[test]
fn a_leaf_proved_by_its_identifier_whose_identifier_cannot_be_read_at_the_send_is_not_ended() {
    let w = id(W, 1_000_000);
    let rows = genuine();
    let chosen = descending_from(&rows, w);
    let recorder = Listed::new(rows.clone()).without_unique(L);
    let said = end_chosen_by(&recorder, &chosen);
    assert_eq!(recorder.ended(), [P]);
    assert_eq!(said.failed.len(), 1, "{said:?}");
    assert_eq!(said.failed[0].0.identity.pid, L);
    assert!(said.failed[0].1.contains("could not be read again"), "{said:?}");
}

#[test]
fn one_number_twice_in_a_windows_reading_proves_no_edge_and_is_named() {
    let w = id(W, at(90));
    // Two rows with the leaf's parent's number (a reading that is not one reading).
    let table = [
        win_row(W, 0, w.started, None),
        win_row(P, W, at(100), None),
        win_row(P, W, at(100), None),
        win_row(L, P, at(150), None),
    ];
    let rows = win_rows(&table, "PID PPID PGID WINPID\n");
    assert!(!ended_of(&rows, w, &[]).contains(&L));
    assert!(numbers(&unproved_of(&rows, w)).contains(&L));
}
