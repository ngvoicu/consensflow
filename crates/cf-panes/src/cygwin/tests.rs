use super::*;

/// What Git Bash's `ps -W` said on zeewin (2026-10-21) while `bash -c "sh ./plain.sh"` ran a
/// script that ran `sleep 40`: a Windows program is listed by a number that is its Windows
/// number plus 0x400000 and a parent of 0, a Cygwin program by a number of its own, its parent
/// that Cygwin number, and its Windows number in the fourth column.
const PS_W: &str = "\
      PID    PPID    PGID     WINPID  TTY         UID    STIME COMMAND
  4203344       0       0       9040  ?              0 18:25:26 C:\\Program Files\\Git\\bin\\bash.exe
  4212332       0       0      18028  ?              0 18:25:26 C:\\Program Files\\Git\\usr\\bin\\bash.exe
     1464       1    1464       9736  cons0     197609 18:25:26 /usr/bin/sh
     1466    1464    1464      25892  cons0     197609 18:25:26 /usr/bin/sleep
";

#[test]
fn the_table_is_read_for_its_numbers_and_the_header_and_other_lines_say_nothing() {
    assert_eq!(
        table(PS_W),
        [
            Row {
                pid: 4_203_344,
                ppid: 0,
                winpid: 9040
            },
            Row {
                pid: 4_212_332,
                ppid: 0,
                winpid: 18028
            },
            Row {
                pid: 1464,
                ppid: 1,
                winpid: 9736
            },
            Row {
                pid: 1466,
                ppid: 1464,
                winpid: 25892
            },
        ]
    );
    assert_eq!(table("not a table\n\n  1 2 three 4\n"), []);
}

#[test]
fn the_parent_of_a_program_is_the_shell_git_bash_says_by_the_number_windows_has_for_it() {
    let rows = table(PS_W);
    // The sleep's parent is the sh: its Windows number, not its Cygwin one.
    assert_eq!(parent_of(&rows, 25892), Some(9736));
    // A process Git Bash does not know, and one whose parent is not in the table.
    assert_eq!(parent_of(&rows, 4242), None);
    let lone = table("  1466    1464    1464      25892  cons0 1 18:25:26 /usr/bin/sleep\n");
    assert_eq!(parent_of(&lone, 25892), None);
    // A parent with no Windows number is none.
    let nameless = table("  7 1 1 0 x\n  8 7 1 99 x\n");
    assert_eq!(parent_of(&nameless, 99), None);
}

#[test]
fn ps_is_asked_once_for_each_folder_and_what_it_does_not_say_is_left_out() {
    let git = PathBuf::from(r"C:\Program Files\Git\usr\bin");
    let other = PathBuf::from(r"C:\Other\bin");
    let orphans = [
        (25892, git.clone()),
        (31000, git.clone()),
        (40000, other.clone()),
    ];
    let mut asked = Vec::new();
    let parents = parents_of(&orphans, |folder| {
        asked.push(folder.to_path_buf());
        (folder == git).then(|| PS_W.to_owned())
    });
    assert_eq!(parents, HashMap::from([(25892, 9736)]));
    assert_eq!(asked, [git, other]);
}

/// Cygwin's `ps` begins each row with a status letter (`S` stopped, `I` reading its terminal, `O`
/// writing it) or a space, then the number in seven columns (`"%c %7d %7d %7d %10u …"`, read in
/// its `ps.cc`): a process that reads its terminal is `I`, and was no row (T-194, P3-1).
const PS_W_FLAGGED: &str = "\
      PID    PPID    PGID     WINPID  TTY         UID    STIME COMMAND
O     1464       1    1464       9736  cons0     197609 18:25:26 /usr/bin/sh
S     1466    1464    1464      25892  cons0     197609 18:25:26 /usr/bin/sleep
I     1470    1466    1466      31000  cons0     197609 18:25:27 /usr/bin/cat
";

#[test]
fn a_row_that_begins_with_a_status_letter_is_a_row_and_so_is_its_parent_s() {
    assert_eq!(
        table(PS_W_FLAGGED),
        [
            Row {
                pid: 1464,
                ppid: 1,
                winpid: 9736
            },
            Row {
                pid: 1466,
                ppid: 1464,
                winpid: 25892
            },
            Row {
                pid: 1470,
                ppid: 1466,
                winpid: 31000
            },
        ]
    );
    // The sleep's parent is the sh though both rows carry a letter, and so is the cat's the sleep.
    let rows = table(PS_W_FLAGGED);
    assert_eq!(parent_of(&rows, 25892), Some(9736));
    assert_eq!(parent_of(&rows, 31000), Some(25892));
    // A letter alone, a word of letters, or a pid after two letters are no status.
    assert_eq!(table("IO 1 2 3 4 x\n"), []);
    assert_eq!(table("I\n"), []);
    assert_eq!(table("sh 1 2 3 4 x\n"), []);
}

/// The folder `ps` is run from holds a Cygwin layer of its own, `msys-2.0.dll` or
/// `cygwin1.dll`, beside it (T-194, P3-7): a `ps.exe` that is not Git Bash's runs as it
/// is only where it can run at all.
#[test]
fn ps_is_run_from_a_folder_that_has_it_and_the_layer_it_runs_on() {
    let folder = Path::new(r"C:\Git\usr\bin");
    let has = |names: &'static [&'static str]| {
        move |path: &Path| {
            path.parent() == Some(folder)
                && path
                    .file_name()
                    .is_some_and(|name| names.contains(&name.to_string_lossy().as_ref()))
        }
    };
    assert!(runs_ps_in(folder, has(&["ps.exe", "msys-2.0.dll"])));
    assert!(runs_ps_in(folder, has(&["ps.exe", "cygwin1.dll"])));
    assert!(!runs_ps_in(folder, has(&["ps.exe"])));
    assert!(!runs_ps_in(folder, has(&["msys-2.0.dll"])));
    assert!(!runs_ps_in(folder, has(&[])));
}
