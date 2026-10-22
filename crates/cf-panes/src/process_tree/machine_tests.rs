//! What the tree signals, on a machine made up (`testing`): a table of the window among
//! strangers, and a record of the signals the tree sends it, which reach no process. A
//! selection gone wrong, an ending and a close, what the bound refuses of them and what an
//! ending says of what it refused.

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
    tree.bound = Bound::new(tree.root);
    tree
}

/// A selection gone wrong: a lineage that names every process of the machine, as a
/// plant of the selection once did, and the ending signalled them all.
pub(super) fn every_process(table: &[Process]) -> Lineage {
    Lineage {
        identities: table.iter().map(|process| process.identity).collect(),
        members: Vec::new(),
        unverified: Vec::new(),
    }
}

/// The numbers of `table`'s processes the test names `of`, as the members of a lineage.
fn members_of(table: &[Process], of: &[i32]) -> Vec<Member> {
    of.iter()
        .map(|pid| Member {
            key: key_of(identity_of(table, *pid)),
            command: format!("sleep {pid}"),
            program: "sleep".to_owned(),
            exe: String::new(),
            group: None,
            parent: None,
        })
        .collect()
}

#[test]
fn a_selection_of_every_process_of_the_machine_asks_only_the_windows_own_to_end() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    let wrong = every_process(&table);

    let asked = tree
        .ask_by(&machine, Subject::Lineage(&wrong), None)
        .expect("asked");

    assert_eq!(machine.sent(libc::SIGTERM), the_windows());
    assert_eq!(machine.sent(libc::SIGHUP), the_windows());
    let waited_for: Vec<i32> = asked.identities.iter().map(|asked| asked.pid).collect();
    assert_eq!(waited_for, the_windows(), "no stranger is waited for");
    assert!(!asked.failed());
    assert_eq!(
        numbers_of(&asked.unverified),
        [the_tests_own_parent(), BUILD, USERS_SHELL, USERS_JOB, LEFT],
        "what it chose and the bound refused is kept apart, for the ending to name"
    );
}

#[test]
fn a_selection_of_every_process_of_the_machine_stops_and_kills_only_the_windows_own() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    let wrong = every_process(&table);

    let forced = tree.force_by(&machine, Subject::Lineage(&wrong), &[]);

    assert!(forced.failure.is_none(), "{:?}", forced.failure);
    assert_eq!(machine.sent(libc::SIGSTOP), the_windows());
    assert_eq!(machine.sent(libc::SIGKILL), the_windows());
    let named: Vec<i32> = forced
        .members
        .iter()
        .map(|member| i32::try_from(member.key.pid).expect("a number"))
        .collect();
    assert_eq!(named, the_windows(), "the strangers are no member of it");
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

    let forced = tree.force_by(&machine, Subject::Whole, &[left]);

    assert!(forced.failure.is_none(), "{:?}", forced.failure);
    assert_eq!(machine.sent(libc::SIGSTOP), the_windows());
    assert_eq!(machine.sent(libc::SIGKILL), the_windows());
    assert!(
        forced.unverified.is_empty(),
        "a close names nothing it ended or left, not what the bound refused"
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
    let forced = tree.force_by(&later, Subject::Whole, &[job, left]);

    assert!(forced.failure.is_none(), "{:?}", forced.failure);
    assert_eq!(later.sent(libc::SIGKILL), [PROGRAM, JOB]);
}

/// A turn whose work the bound refuses. Before the turn: the window's program, and a job in
/// its session whose parent has gone. After it the program has gone, and nothing holds its
/// number: the session is the pane's to the selection still, and a shell the job started, a
/// shell with a parent from before the mark, is proved the turn's. The leader is not there to
/// say the session is the window's, and no look saw the job. The tree, the mark and the
/// machine after.
fn a_turn_whose_work_the_bound_refuses() -> (ProcessTree, Baseline, Machine) {
    let shell = |pid, unique, parent| Process {
        name: "sh".to_owned(),
        session: Some(PROGRAM),
        ..listed(pid, unique, parent)
    };
    let before = vec![
        Process {
            session: Some(PROGRAM),
            ..listed(PROGRAM, 1000, 700)
        },
        Process {
            session: Some(PROGRAM),
            ..listed(DETACHED, 1400, 1)
        },
    ];
    let tree = tree_of(&before);
    // The mark is made of what the selection finds, which no look of the bound has seen.
    let mut baseline = Baseline::default();
    baseline.mark(
        &tree.members_in(&before, &tree.select(&before, &HashMap::new())),
        None,
        None,
    );
    let after = Machine::new(vec![before[1].clone(), shell(USERS_SHELL, 2100, 1400)]);
    (tree, baseline, after)
}

/// The numbers of `members`, in order.
fn numbers_of(members: &[Member]) -> Vec<i32> {
    members
        .iter()
        .map(|member| i32::try_from(member.key.pid).expect("a number"))
        .collect()
}

/// The numbers of the processes an ending tells, in order.
fn told(processes: &cf_proto::panes::Background) -> Vec<i32> {
    processes
        .processes
        .iter()
        .map(|process| i32::try_from(process.pid).expect("a number"))
        .collect()
}

#[test]
fn what_the_bound_refuses_is_no_part_of_the_turns_lineage_whoever_proves_it_the_turns() {
    let (tree, baseline, after) = a_turn_whose_work_the_bound_refuses();

    let lineage = tree.lineage_by(&after, &baseline).expect("the lineage");

    assert_eq!(lineage.identities, [], "nothing the bound refuses");
    assert!(lineage.members.is_empty());
    assert_eq!(
        numbers_of(&lineage.unverified),
        [USERS_SHELL],
        "what it refuses is kept apart"
    );
}

#[test]
fn what_the_bound_refuses_of_the_turns_work_is_named_as_left_and_nothing_is_signalled() {
    let (tree, baseline, after) = a_turn_whose_work_the_bound_refuses();
    let lineage = tree.lineage_by(&after, &baseline).expect("the lineage");
    let asked = tree
        .ask_by(&after, Subject::Lineage(&lineage), None)
        .expect("asked");

    let ending = tree.finish_by(&after, &lineage, asked);

    assert!(
        after.signalled_none(),
        "the bound refused it, and nothing was sent"
    );
    assert_eq!(ending.ended.total, 0, "{ending:?}");
    assert_eq!(
        told(&ending.left),
        [USERS_SHELL],
        "it may run on, and the chief is told: {ending:?}"
    );
}

#[test]
fn what_an_ending_finds_of_the_turns_work_and_the_bound_refuses_is_named_as_left_too() {
    let table = a_window_among_strangers();
    let tree = tree_of(&table);
    // The ending asked the window's shell, and it went. A job it started, in a session of
    // its own, runs: the system names the shell as the process that started it, and no
    // look saw the shell or the job under the program, so the bound has no word for it.
    let shell = identity_of(&table, SHELL);
    let lineage = Lineage {
        identities: vec![shell],
        members: members_of(&table, &[SHELL]),
        unverified: Vec::new(),
    };
    let asked = Asked {
        identities: vec![shell],
        unverified: Vec::new(),
        failed: false,
    };
    let job = Process {
        session: Some(4_000_030),
        ..listed(4_000_030, 1500, shell.unique)
    };
    let later = Machine::new(vec![table[0].clone(), job]);

    let ending = tree.finish_by(&later, &lineage, asked);

    assert!(later.signalled_none(), "the bound refused it: {ending:?}");
    assert_eq!(told(&ending.ended), [SHELL], "what it asked, which went");
    assert_eq!(told(&ending.left), [4_000_030], "{ending:?}");
}

#[test]
fn what_the_bound_refused_and_has_gone_by_the_end_is_not_named() {
    let (tree, baseline, after) = a_turn_whose_work_the_bound_refuses();
    let lineage = tree.lineage_by(&after, &baseline).expect("the lineage");

    // The refused shell has gone by the time the ending is done.
    let gone = Machine::new(vec![after.processes().expect("the table")[0].clone()]);
    let asked = tree
        .ask_by(&gone, Subject::Lineage(&lineage), None)
        .expect("asked");
    let ending = tree.finish_by(&gone, &lineage, asked);

    assert_eq!(ending.left.total, 0, "it is not there: {ending:?}");
    assert_eq!(ending.ended.total, 0, "and it was never ended: {ending:?}");
}

/// The window of a turn whose program started, after the mark, a run of the harness's
/// status line (a shell with the `git` it runs) and a command of the agent's (a shell
/// with a job), each in a session of its own, as Claude Code's tool shells are. The tree
/// and the mark; and the numbers of the processes of a look at the window, which the
/// program started `shells` of, each by its number, its identifier and the one that
/// started it.
fn a_window_of_shells(shells: &[(i32, u64, u64)]) -> (ProcessTree, Baseline, Machine) {
    let program = Process {
        session: Some(PROGRAM),
        ..listed(PROGRAM, 1000, 700)
    };
    let before = vec![program.clone()];
    let tree = tree_of(&before);
    let mut baseline = Baseline::default();
    baseline.mark(
        &tree.members_in(&before, &tree.select(&before, &HashMap::new())),
        None,
        None,
    );
    let mut table = before;
    table.extend(shells.iter().map(|(pid, unique, parent)| Process {
        name: "sh".to_owned(),
        session: Some(*pid),
        ..listed(*pid, *unique, *parent)
    }));
    (tree, baseline, Machine::new(table))
}

/// A run of the status line: the shell and its `git`; the agent's command: a shell and its
/// job. A later run of the status line is another two processes.
const RUN: [(i32, u64, u64); 2] = [(SHELL, 1100, 1000), (JOB, 1200, 1100)];
const NEXT_RUN: [(i32, u64, u64); 2] = [(4_000_015, 1500, 1000), (4_000_016, 1600, 1500)];
const COMMAND: [(i32, u64, u64); 2] = [(DEEP, 1300, 1000), (DETACHED, 1400, 1300)];

#[test]
fn a_lineage_is_what_two_looks_find_by_identity_and_a_run_that_ended_between_them_is_none_of_it() {
    let (tree, baseline, first) = a_window_of_shells(&[RUN, COMMAND].concat());
    let (_, _, second) = a_window_of_shells(&[NEXT_RUN, COMMAND].concat());
    let (_, _, quiet) = a_window_of_shells(&[]);

    let before = tree.lineage_by(&first, &baseline).expect("the first look");
    let after = tree
        .lineage_by(&second, &baseline)
        .expect("the second look");

    assert_eq!(
        numbers_of(&before.members),
        [SHELL, JOB, DEEP, DETACHED],
        "a look while the run lives names it"
    );
    assert_eq!(
        numbers_of(&after.members),
        [DEEP, DETACHED, 4_000_015, 4_000_016]
    );
    assert!(!before.is_empty());
    assert!(tree
        .lineage_by(&quiet, &baseline)
        .expect("a look")
        .is_empty());

    let steady = after.steady_since(&before);

    assert_eq!(
        numbers_of(&steady.members),
        [DEEP, DETACHED],
        "the next run is another process, and the last has ended"
    );
    let named: Vec<i32> = steady
        .identities
        .iter()
        .map(|identity| identity.pid)
        .collect();
    assert_eq!(
        named,
        [DEEP, DETACHED],
        "what is ended is what both looks find"
    );
    assert!(steady.unverified.is_empty());
}

#[test]
fn a_process_that_took_the_number_of_one_that_ended_is_not_the_lineage_it_was() {
    let (tree, baseline, first) = a_window_of_shells(&COMMAND);
    // The command's shell has ended and another process has its number, which began later.
    let mut table = first.processes().expect("the table");
    let reused = Process {
        identity: Identity {
            born: (5, 0),
            unique: 1999,
            ..table[1].identity
        },
        ..table[1].clone()
    };
    table[1] = reused;
    let second = Machine::new(table);

    let before = tree.lineage_by(&first, &baseline).expect("the first look");
    let after = tree
        .lineage_by(&second, &baseline)
        .expect("the second look");
    assert_eq!(
        numbers_of(&after.members),
        [DEEP],
        "the number is still there"
    );

    let steady = after.steady_since(&before);

    assert!(steady.is_empty(), "{steady:?}");
}

#[test]
fn what_the_bound_refuses_is_named_as_left_only_if_both_looks_find_it() {
    let (tree, baseline, after) = a_turn_whose_work_the_bound_refuses();
    let first = tree.lineage_by(&after, &baseline).expect("the first look");
    assert_eq!(numbers_of(&first.unverified), [USERS_SHELL]);

    // The same shell a moment later: it is still named, as left.
    let same = tree
        .lineage_by(&after, &baseline)
        .expect("the second look")
        .steady_since(&first);
    assert_eq!(numbers_of(&same.unverified), [USERS_SHELL]);

    // Another process that took its number, and began later: none of it.
    let mut table = after.processes().expect("the table");
    let shell = table
        .iter_mut()
        .find(|process| process.identity.pid == USERS_SHELL)
        .expect("the shell");
    shell.identity = Identity {
        born: (9, 0),
        unique: 2199,
        ..shell.identity
    };
    let later = tree
        .lineage_by(&Machine::new(table), &baseline)
        .expect("a look")
        .steady_since(&first);
    assert!(later.is_empty(), "{later:?}");
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
fn an_ending_names_as_ended_only_what_it_signalled() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    let tree = tree_of(&table);
    // A lineage that names the window's shell and job, and the test's own parent, which
    // is alive: whatever chose it.
    let lineage = Lineage {
        identities: vec![identity_of(&table, SHELL), identity_of(&table, JOB)],
        members: members_of(&table, &[SHELL, JOB, the_tests_own_parent()]),
        unverified: Vec::new(),
    };
    let asked = tree
        .ask_by(&machine, Subject::Lineage(&lineage), None)
        .expect("asked");

    let ending = tree.finish_by(&machine, &lineage, asked);

    let ended: Vec<i32> = ending
        .ended
        .processes
        .iter()
        .map(|ended| i32::try_from(ended.pid).expect("a number"))
        .collect();
    assert_eq!(
        ended,
        [SHELL, JOB, DEEP],
        "what it signalled, and what that started"
    );
    assert!(
        !ended.contains(&the_tests_own_parent()),
        "the test's own parent was never signalled and is not named: {ending:?}"
    );
    assert_eq!(ending.left.total, 0, "{ending:?}");
    assert_eq!(machine.sent(libc::SIGKILL), [SHELL, JOB, DEEP]);
}

#[test]
fn a_tree_with_no_program_signals_nothing_whatever_is_selected() {
    let machine = Machine::new(a_window_among_strangers());
    let table = machine.processes().expect("the table");
    // A pane whose program could not be read: nothing is under it, and nothing was seen.
    let tree = ProcessTree::new();
    let wrong = every_process(&table);

    let asked = tree
        .ask_by(&machine, Subject::Lineage(&wrong), None)
        .expect("asked");
    let forced = tree.force_by(&machine, Subject::Lineage(&wrong), &[]);

    assert!(machine.signalled_none());
    assert!(asked.identities.is_empty() && forced.members.is_empty());
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
            Readable::Hidden,
            "a process {parent} started that cannot be read"
        );
    }
    // One that a stranger started is none of the window's.
    let elsewhere = Machine::new(a_window_among_strangers()).with_unreadable(BUILD, 4_000_099);
    assert!(matches!(
        tree.readable_members_by(&elsewhere).expect("a look"),
        Readable::All(_)
    ));
    // A tree with no program says nothing.
    assert_eq!(
        ProcessTree::new()
            .readable_members_by(&readable)
            .expect("a look"),
        Readable::Hidden
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
