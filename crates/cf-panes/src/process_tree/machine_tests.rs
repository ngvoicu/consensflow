//! What the tree signals, on a machine made up (`testing`): a table of the window among
//! strangers, and a record of the signals the tree sends it, which reach no process. A
//! selection gone wrong and a close, and what the bound refuses of them.

use super::testing::{
    a_window_among_strangers, identity_of, listed, the_tests_own_parent, the_windows, Machine,
    BUILD, DEEP, DETACHED, JOB, LEFT, PROGRAM, SHELL, USERS_JOB, USERS_SHELL,
};
use super::*;

/// The tree of the window of [`a_window_among_strangers`]: its program leads its session.
fn tree_of(table: &[Process]) -> ProcessTree {
    let mut tree = ProcessTree::new();
    tree.root = Some(identity_of(table, PROGRAM));
    tree.session = Some(PROGRAM);
    tree.bound = Bound::new(tree.root, tree.session);
    tree
}

/// A session no process leads, which the strangers of a table are put in: its number is above
/// any a Mac gives, so that no process there is has it.
pub(super) const STRANGERS: i32 = 4_000_099;

/// [`a_window_among_strangers`], where every process that is none of the window's (the test's
/// own parent, the build, the user's shell and its job, the job that left the window's
/// session) is in [`STRANGERS`].
fn strangers_in_a_session_nobody_leads() -> Vec<Process> {
    let windows = the_windows();
    a_window_among_strangers()
        .into_iter()
        .map(|process| {
            if windows.contains(&process.identity.pid) {
                process
            } else {
                Process {
                    session: Some(STRANGERS),
                    ..process
                }
            }
        })
        .collect()
}

/// Every process of `table`: what a force of a close is given as found before, by a
/// selection gone wrong, which then takes them all and what descends from them.
pub(super) fn every_process(table: &[Process]) -> Vec<Identity> {
    table.iter().map(|process| process.identity).collect()
}

#[test]
fn a_selection_gone_wrong_asks_only_the_windows_own_to_end() {
    let machine = Machine::new(strangers_in_a_session_nobody_leads());
    let table = machine.processes().expect("the table");
    // The selection takes the strangers' session for the window's, which the bound,
    // which shares none of its code, knows is not.
    let mut tree = tree_of(&table);
    tree.session = Some(STRANGERS);
    let chosen = tree.select(&table, &HashMap::new());
    for stranger in [the_tests_own_parent(), BUILD, USERS_SHELL, USERS_JOB, LEFT] {
        assert!(
            chosen.contains_key(&stranger),
            "the selection names {stranger}: {chosen:?}"
        );
    }

    let asked = tree.ask_by(&machine, None).expect("asked");

    let windows = [PROGRAM, SHELL, JOB, DEEP];
    assert_eq!(machine.sent(libc::SIGTERM), windows);
    assert_eq!(machine.sent(libc::SIGHUP), windows);
    let waited_for: Vec<i32> = asked.identities.iter().map(|asked| asked.pid).collect();
    assert_eq!(waited_for, windows, "no stranger is waited for");
    assert!(!asked.failed());
}

#[test]
fn a_selection_of_every_process_of_the_machine_stops_and_kills_only_the_windows_own() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);

    let failure = tree.force_by(&machine, &every_process(&table));

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(machine.sent(libc::SIGSTOP), the_windows());
    assert_eq!(machine.sent(libc::SIGKILL), the_windows());
}

#[test]
fn a_tree_with_no_program_signals_nothing_whatever_is_selected() {
    let machine = Machine::new(strangers_in_a_session_nobody_leads());
    let table = machine.processes().expect("the table");
    // A pane whose program could not be read: nothing is under it, and nothing was seen.
    let mut tree = ProcessTree::new();
    tree.session = Some(STRANGERS);

    let asked = tree.ask_by(&machine, None).expect("asked");
    let failure = tree.force_by(&machine, &every_process(&table));

    assert!(machine.signalled_none());
    assert!(asked.identities.is_empty());
    assert!(failure.is_none(), "{failure:?}");
}

#[test]
fn a_close_signals_the_windows_by_its_parents_and_its_session_and_none_of_what_the_selection_adds()
{
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    // The selection names the job a detached shell left in the window's session, which is
    // the window's, and carries on a process an earlier pass found: one that left the
    // session (`setsid`), whose shell is gone and which no look saw under the program.
    let left = identity_of(&table, LEFT);
    let chosen = tree.select(&table, &HashMap::from([(left.pid, left)]));
    assert!(
        chosen.contains_key(&DETACHED) && chosen.contains_key(&LEFT),
        "the selection names both: {chosen:?}"
    );

    let failure = tree.force_by(&machine, &[left]);

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(machine.sent(libc::SIGSTOP), the_windows());
    assert_eq!(machine.sent(libc::SIGKILL), the_windows());
}

#[test]
fn a_close_ends_what_a_program_that_exited_left_in_its_session_though_init_is_now_its_parent() {
    // T-375: a window's program exits over a job that holds its terminal, which no look
    // saw (the window was never marked), and the system names init as the process that
    // started the job (as it does for a job that execs after its starter has gone). The
    // program has gone and nothing holds its number: the session is the window's.
    let table = a_window_among_strangers();
    let tree = tree_of(&table);
    // The job, and the strangers around it: everything but the program and its shells.
    let after = Machine::new(table[4..].to_vec());
    assert_eq!(
        after.processes().expect("the table")[0].identity.pid,
        DETACHED
    );

    let asked = tree.ask_by(&after, None).expect("asked");
    assert_eq!(after.sent(libc::SIGTERM), [DETACHED]);
    assert_eq!(after.sent(libc::SIGHUP), [DETACHED]);
    assert!(!asked.failed());
    let failure = tree.force_by(&after, &[]);

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(after.sent(libc::SIGSTOP), [DETACHED]);
    assert_eq!(
        after.sent(libc::SIGKILL),
        [PROGRAM, DETACHED],
        "the job and none of the strangers; the program too: a force of the whole window \
         signals the root it owns, which the real signal finds gone and sends nothing to"
    );
}

#[test]
fn a_job_seen_under_the_program_is_ended_by_a_close_though_its_parent_has_gone_since() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    // A look while the shell runs.
    tree.members_by(&machine).expect("a look");

    // The shell is gone and its job runs on, in the shell's own session; the selection
    // carries it on, with the job that left the window's session, which no look saw.
    let (job, left) = (identity_of(&table, JOB), identity_of(&table, LEFT));
    let later = Machine::new(vec![
        table[0].clone(),
        table[2].clone(),
        table[5].clone(),
        table[9].clone(),
    ]);
    let failure = tree.force_by(&later, &[job, left]);

    assert!(failure.is_none(), "{failure:?}");
    assert_eq!(later.sent(libc::SIGKILL), [PROGRAM, JOB]);
}

/// The numbers of `members`, in order.
fn numbers_of(members: &[Member]) -> Vec<i32> {
    members
        .iter()
        .map(|member| i32::try_from(member.key.pid).expect("a number"))
        .collect()
}

#[test]
fn a_close_signals_the_panes_group_whole_while_its_program_leads_it_and_else_by_the_bound() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);

    tree.signal_group_by(&machine, PROGRAM, libc::SIGTERM)
        .expect("signalled");

    assert_eq!(machine.sent_to_groups(libc::SIGTERM), [PROGRAM]);
    assert!(machine.sent(libc::SIGTERM).is_empty());

    // The program has gone and a stranger has its number: it leads a group of that number,
    // as what it started does, and the window's own member of it is the one signalled.
    let later = Machine::new(vec![
        Process {
            group: PROGRAM,
            ..table[1].clone()
        },
        Process {
            group: PROGRAM,
            ..listed(4_000_090, 9900, 1)
        },
    ]);
    tree.signal_group_by(&later, PROGRAM, libc::SIGKILL)
        .expect("signalled");

    assert!(later.sent_to_groups(libc::SIGKILL).is_empty());
    assert_eq!(later.sent(libc::SIGKILL), [SHELL]);
}

#[test]
fn a_look_that_must_not_miss_a_process_is_hidden_where_one_of_the_windows_cannot_be_read() {
    // Astraeus on 472b1d96: a command that `exec`s `sudo` becomes a process of another
    // user, which no look reads and which the table leaves out; the program, or a command
    // of its, still names it as one it started.
    let readable = Machine::new(a_window_among_strangers());
    let table = readable.processes().expect("the table");
    let tree = tree_of(&table);
    let Readable::All(seen) = tree.readable_members_by(&readable).expect("a look") else {
        panic!("all of it read");
    };
    assert_eq!(numbers_of(&seen), the_windows());
    for parent in [PROGRAM, SHELL, DEEP] {
        let hidden = Machine::new(a_window_among_strangers()).with_unreadable(parent, 4_000_099);
        assert_eq!(
            tree.readable_members_by(&hidden).expect("a look"),
            Readable::Unreadable,
            "a process {parent} started that cannot be read"
        );
    }
    // One that a stranger started is none of the window's.
    let elsewhere = Machine::new(a_window_among_strangers()).with_unreadable(BUILD, 4_000_099);
    assert!(matches!(
        tree.readable_members_by(&elsewhere).expect("a look"),
        Readable::All(_)
    ));
    // A tree with no program cannot be read whole.
    assert_eq!(
        ProcessTree::new()
            .readable_members_by(&readable)
            .expect("a look"),
        Readable::Unsettled
    );
}

#[test]
fn a_process_born_or_ended_while_a_look_read_the_tree_hides_nothing() {
    // Calliope on 136a7158: a child born, or ended unreaped, between the table and the
    // listing of children made the look say it could not tell.
    let readable = Machine::new(a_window_among_strangers());
    let table = readable.processes().expect("the table");
    let tree = tree_of(&table);
    let ended = Machine::new(a_window_among_strangers()).with_zombie(SHELL, 4_000_098);
    assert!(matches!(
        tree.readable_members_by(&ended).expect("a look"),
        Readable::All(_)
    ));
    let born = Machine::new(a_window_among_strangers()).with_born(SHELL, 4_000_097);
    assert!(matches!(
        tree.readable_members_by(&born).expect("a look"),
        Readable::All(_)
    ));
}

#[test]
fn a_tree_that_kept_changing_while_it_was_read_is_told_apart_from_one_another_user_holds() {
    // Astraeus on 02a6cd4f: a look that ran out of tries, every one of them meeting a child
    // born since its table, was said "another user's".
    let readable = Machine::new(a_window_among_strangers());
    let table = readable.processes().expect("the table");
    let tree = tree_of(&table);
    let churning = Machine::new(a_window_among_strangers()).with_churning(SHELL);
    assert_eq!(
        tree.readable_members_by(&churning).expect("a look"),
        Readable::Unsettled,
        "no process of another user's"
    );
}

#[test]
fn a_process_that_changed_while_a_look_read_it_is_looked_at_again_and_is_no_other_users() {
    // Astraeus on eb9f25ef: a child of ours that ended between a look's two reads of it was
    // taken for another user's.
    let readable = Machine::new(a_window_among_strangers());
    let table = readable.processes().expect("the table");
    let tree = tree_of(&table);
    let changed = Machine::new(a_window_among_strangers()).with_changed(SHELL, 4_000_096);
    assert!(matches!(
        tree.readable_members_by(&changed).expect("a look"),
        Readable::All(_)
    ));
}
