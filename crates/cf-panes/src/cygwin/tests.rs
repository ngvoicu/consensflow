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
