//! A run of ssh, tried on programs of the tests' own: the test program itself,
//! started to run one of its ignored tests as the program a case needs (it speaks
//! on both streams, it holds on to be ended, it exits as it is told), the way
//! `stops` tries a stop. Which of them runs, and what it waits for, is
//! said by the case in variables and files, so that no case depends on how long
//! anything takes.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;
use std::time::Instant;

use super::*;

/// The line the machine says of the tree it unpacked, as a run that sent this tree is told it.
const TREE: &str = "== tree 0123456789abcdef0123456789abcdef01234567 89abcdef01234567";

const SAYS: &str = "windows::session::tests::helper_speaks";
const HOLDS: &str = "windows::session::tests::helper_holds";
const EXITS: &str = "windows::session::tests::helper_exits";

/// The file a program waits for, which the case makes when it has seen what it waits to see.
const FLAG: &str = "CF_WINDOWS_TEST_FLAG";
/// The status a program exits with.
const STATUS: &str = "CF_WINDOWS_TEST_STATUS";
/// Lines a program says on its standard output last, apart by `|`, each ended as
/// PowerShell ends one (a carriage return and a line feed).
const SAID: &str = "CF_WINDOWS_TEST_SAID";

/// The test program, started to run `helper`, which the cases give variables.
fn program(helper: &str) -> Invocation {
    let this = std::env::current_exe().unwrap();
    Invocation::new(this, ".").args(["--ignored", "--exact", helper, "--nocapture"])
}

fn flag_of(env: &Env) -> PathBuf {
    PathBuf::from(env.os(FLAG).unwrap())
}

/// Waits for the file the case makes: for a program that is to go on only when
/// the case has seen what it said.
fn wait_for_flag(env: &Env) {
    let flag = flag_of(env);
    for _ in 0..600 {
        if flag.exists() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    // The case never made it: a run that ends with this is no run the case asked for.
    std::process::exit(99);
}

fn status_of(env: &Env) -> i32 {
    env.text(STATUS)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0)
}

/// A program that says a line on each stream, waits for the case to have seen
/// the first, says one more, and exits as it is told.
#[test]
#[ignore = "a program the tests of session start"]
fn helper_speaks() {
    let env = Env::from_process();
    println!("out one");
    eprintln!("err one");
    wait_for_flag(&env);
    println!("out two");
    std::process::exit(status_of(&env));
}

/// A program that says who it is and then goes on until it is ended: or for a
/// minute, which no case waits for (a run that has not ended it by then is the
/// case's failure, and it is not left running for ever).
#[test]
#[ignore = "a program the tests of session start"]
fn helper_holds() {
    println!("held {}", std::process::id());
    for _ in 0..60 {
        thread::sleep(Duration::from_secs(1));
    }
    std::process::exit(98);
}

/// A program that waits for the case to have made its file, says what it is told
/// to say last, and exits as it is told.
#[test]
#[ignore = "a program the tests of session start"]
fn helper_exits() {
    let env = Env::from_process();
    wait_for_flag(&env);
    // A line that begins with `!` is said on the error stream.
    for line in env.text(SAID).unwrap_or_default().split('|') {
        match line.strip_prefix('!') {
            Some(line) => eprint!("{line}\r\n"),
            None if !line.is_empty() => print!("{line}\r\n"),
            None => {}
        }
    }
    std::process::exit(status_of(&env));
}

/// A stream that keeps what it is written, and when it has seen `seen` does what the case says.
struct Says<F: FnMut()> {
    text: Vec<u8>,
    seen: &'static str,
    then: F,
    done: bool,
}

impl<F: FnMut()> Says<F> {
    fn new(seen: &'static str, then: F) -> Self {
        Self {
            text: Vec::new(),
            seen,
            then,
            done: false,
        }
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.text).into_owned()
    }
}

impl<F: FnMut()> Write for Says<F> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.text.extend_from_slice(bytes);
        if !self.done && self.text().contains(self.seen) {
            self.done = true;
            (self.then)();
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// A stream that cannot be written.
struct Closed;

impl Write for Closed {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "the pipe is closed",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The environment of the test, with what a case gives the program it starts.
fn with(vars: &[(&str, &OsStr)]) -> Env {
    let mut env: Vec<(OsString, OsString)> = Env::from_process()
        .iter()
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect();
    env.extend(
        vars.iter()
            .map(|(name, value)| ((*name).into(), (*value).to_owned())),
    );
    Env::from_vars(env)
}

/// A run of the program that exits with `status` after saying `said` (lines apart by
/// `|`), held to the tree line `tree`: what the run came to, what it wrote on its output
/// stream and on its error stream.
fn exits_held_to(status: &str, said: &str, tree: &str) -> (Ended, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let flag = dir.path().join("flag");
    fs::write(&flag, "").unwrap();
    let env = with(&[
        (FLAG, flag.as_os_str()),
        (STATUS, OsStr::new(status)),
        (SAID, OsStr::new(said)),
    ]);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ended = stream(
        &program(EXITS),
        &env,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
        tree,
    )
    .unwrap();
    (
        ended,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

/// A run of the program that exits with `status` after saying the tree it was sent and
/// then `said`: what the run came to, and what it wrote on its error stream.
fn exits(status: &str, said: &str) -> (Ended, String) {
    let (ended, _, err) = exits_held_to(status, &format!("{TREE}|{said}"), TREE);
    (ended, err)
}

/// What a run came to, as text.
fn shown(ended: &Ended) -> String {
    match ended {
        Ended::Exited(status) => format!("exited {status}"),
        Ended::Stopped => "stopped".into(),
        Ended::Unwritten(cause) => format!("unwritten: {cause}"),
        Ended::WrongTree(said) => format!("another tree: {said}"),
    }
}

/// The pid a program that holds says it has.
fn pid_in(text: &str) -> u32 {
    text.lines()
        .find_map(|line| line.strip_prefix("held "))
        .and_then(|pid| pid.trim().parse().ok())
        .unwrap_or_else(|| panic!("no pid in {text:?}"))
}

#[test]
fn what_a_run_writes_is_written_as_it_comes_to_the_stream_it_came_on() {
    let dir = tempfile::tempdir().unwrap();
    let flag = dir.path().join("flag");
    let env = with(&[(FLAG, flag.as_os_str()), (STATUS, OsStr::new("75"))]);
    // The program goes on only when its first line has been written here: that it
    // is, while the program is still running, is what writing it as it comes is.
    let mut out = Says::new("out one", || fs::write(&flag, "").unwrap());
    let mut err = Vec::new();
    let ended = stream(
        &program(SAYS),
        &env,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
        TREE,
    )
    .unwrap();
    // It said nothing of how it ended, so ssh's status is the answer: and said so.
    assert_eq!(shown(&ended), "exited 75");
    let (out, err) = (out.text(), String::from_utf8(err).unwrap());
    assert!(
        out.find("out one").unwrap() < out.find("out two").unwrap(),
        "{out}"
    );
    let mut said = err.lines();
    assert_eq!(said.next(), Some("err one"));
    assert!(said
        .next()
        .unwrap()
        .starts_with("windows: no `== exit N` line"));
    assert!(!out.contains("err one") && !err.contains("out one") && !err.contains("out two"));
}

#[test]
fn the_machines_last_line_is_how_the_command_ended_whatever_status_ssh_ends_with() {
    // A shell that makes every failure 1 is what the line is for (zeewin's).
    for (status, said, answer) in [
        ("1", "== exit 7", "exited 7"),
        ("1", "== exit 75", "exited 75"),
        ("1", "== exit 255", "exited 255"),
        ("0", "== exit 7", "exited 7"),
        ("1", "== exit 0", "exited 0"),
        ("0", "== exit 0", "exited 0"),
        // What came before it is no matter, and a line that ends the output as it ends.
        ("1", "first|second|== exit 3", "exited 3"),
        ("3", "== exit 3", "exited 3"),
    ] {
        let (ended, err) = exits(status, said);
        assert_eq!(shown(&ended), answer, "ssh {status}, said {said:?}");
        assert_eq!(err, "", "nothing to say: ssh {status}, said {said:?}");
    }
}

#[test]
fn a_line_on_the_error_stream_is_no_word_of_the_machines_about_the_command() {
    // The script writes its last line on the standard output; what a command says on
    // the error stream, or what ssh does, may look like it and is not it.
    let (ended, err) = exits("3", "!== exit 7");
    assert_eq!(shown(&ended), "exited 3");
    assert_eq!(
        err,
        "== exit 7\r\nwindows: no `== exit N` line ended the output, so ssh's own status is the answer: 3\n"
    );
    // And the last of the standard output is still read, whatever the error stream says after it.
    let (ended, err) = exits("1", "== exit 7|!an error after it");
    assert_eq!(shown(&ended), "exited 7");
    assert_eq!(err, "an error after it\r\n");
}

#[test]
fn a_status_no_program_can_end_with_is_answered_as_1_and_said() {
    for (said, number) in [
        ("== exit 256", "256"),
        ("== exit 4096", "4096"),
        ("== exit -1", "-1"),
        ("== exit -1073741819", "-1073741819"),
    ] {
        let (ended, err) = exits("1", said);
        assert_eq!(shown(&ended), "exited 1", "{said}");
        assert_eq!(
            err,
            format!("windows: the command ended with status {number}, which no status here can hold: answering 1\n")
        );
    }
}

#[test]
fn without_the_line_nothing_is_known_of_the_command_but_what_ssh_answered_and_that_is_never_0() {
    let no_line = "windows: no `== exit N` line ended the output";
    for (status, said, answer, why) in [
        ("1", "", "exited 1", "so ssh's own status is the answer: 1"),
        ("3", "", "exited 3", "so ssh's own status is the answer: 3"),
        (
            "254",
            "",
            "exited 254",
            "so ssh's own status is the answer: 254",
        ),
        (
            "0",
            "",
            "exited 1",
            "and ssh's status 0 is not taken for a command's without it: answering 1",
        ),
        // The line is the last one or it is none: an earlier one, or a line that is
        // not the line, is what a command can write.
        (
            "3",
            "== exit 0|and then more",
            "exited 3",
            "so ssh's own status is the answer: 3",
        ),
        (
            "0",
            "== exit 7|after",
            "exited 1",
            "and ssh's status 0 is not taken for a command's without it: answering 1",
        ),
        (
            "3",
            "== exit seven",
            "exited 3",
            "so ssh's own status is the answer: 3",
        ),
        (
            "3",
            "== exit 7 and more",
            "exited 3",
            "so ssh's own status is the answer: 3",
        ),
        (
            "3",
            "x== exit 7",
            "exited 3",
            "so ssh's own status is the answer: 3",
        ),
    ] {
        let (ended, err) = exits(status, said);
        assert_eq!(shown(&ended), answer, "ssh {status}, said {said:?}");
        assert_eq!(
            err,
            format!("{no_line}, {why}\n"),
            "ssh {status}, said {said:?}"
        );
    }
}

#[test]
fn a_stop_ends_the_program_whole_and_is_said_and_what_it_had_written_is_kept() {
    let interrupt = Interrupt::default();
    let raise = interrupt.clone();
    let mut out = Says::new("held ", move || raise.raise());
    let mut err = Vec::new();
    let ended = stream(
        &program(HOLDS),
        &Env::from_process(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &interrupt,
        TREE,
    )
    .unwrap();
    assert!(matches!(ended, Ended::Stopped), "{ended:?}");
    // Ended, and waited for: it is not there.
    assert!(!cf_process::alive(pid_in(&out.text())), "{}", out.text());
}

#[test]
fn a_run_whose_output_cannot_be_written_is_ended_and_says_why() {
    let mut err = Vec::new();
    let ended = stream(
        &program(HOLDS),
        &Env::from_process(),
        &mut Console {
            out: &mut Closed,
            err: &mut err,
        },
        &Interrupt::default(),
        TREE,
    )
    .unwrap();
    let Ended::Unwritten(cause) = ended else {
        panic!("{ended:?}");
    };
    assert_eq!(cause.kind(), io::ErrorKind::BrokenPipe);
}

#[test]
fn a_status_a_stop_may_have_made_waits_a_moment_for_the_stop_and_another_does_not() {
    // With nothing said by the machine: what ssh ends with when the Ctrl-C ended
    // it (a signal, or its own 255), and the stop is heard a moment after: within
    // the grace, which here is long, for the wait ends when the stop is heard.
    let answer = |code, interrupt: &Interrupt| shown(&exited(code, None, true, interrupt).0);
    let interrupt = Interrupt::graced(Duration::from_secs(60));
    let late = interrupt.clone();
    let telling = thread::spawn(move || {
        thread::sleep(Duration::from_millis(50));
        late.raise();
    });
    assert_eq!(answer(Some(255), &interrupt), "stopped");
    telling.join().unwrap();
    // Not heard within the grace, it is the status it is (a signal is told as 1).
    assert_eq!(answer(Some(255), &Interrupt::default()), "exited 255");
    assert_eq!(answer(None, &Interrupt::default()), "exited 1");
    // Heard, the same two are the stop's; any other status is ssh's, heard or not,
    // and a 0 that the machine did not stand behind is no success.
    assert_eq!(answer(Some(255), &interrupt), "stopped");
    assert_eq!(answer(None, &interrupt), "stopped");
    assert_eq!(answer(Some(3), &interrupt), "exited 3");
    assert_eq!(answer(Some(0), &interrupt), "exited 1");
}

#[test]
fn what_ssh_did_when_the_machine_said_nothing_is_told_in_a_note_whatever_it_did() {
    // ssh gone by a signal, or with a status a system that keeps more than a byte
    // may give (Windows'): neither is a status to answer, and both are told.
    let none = Interrupt::default();
    let told = |code| {
        let (ended, note) = exited(code, None, true, &none);
        (shown(&ended), note.unwrap())
    };
    let no_line = "no `== exit N` line ended the output";
    assert_eq!(
        told(None),
        (
            "exited 1".into(),
            format!("{no_line}, and ssh was ended by a signal: answering 1")
        )
    );
    for code in [256, 4096, -1] {
        assert_eq!(
            told(Some(code)),
            (
                "exited 1".into(),
                format!("{no_line}, and ssh's status {code} is no status here: answering 1")
            )
        );
    }
    // A plain failure of ssh's is its own, said as it is.
    assert_eq!(
        told(Some(255)),
        (
            "exited 255".into(),
            format!("{no_line}, so ssh's own status is the answer: 255")
        )
    );
}

#[test]
fn what_the_machine_said_is_the_answer_before_ssh_or_a_stop_is_looked_at() {
    // ssh gone as a Ctrl-C ends it, and the stop heard: yet the command had said how
    // it ended, which is a thing that happened, and the stop is not one that ended it.
    let heard = Interrupt::default();
    heard.raise();
    for code in [None, Some(255), Some(1), Some(0)] {
        assert_eq!(shown(&exited(code, Some(7), true, &heard).0), "exited 7");
    }
    // Nor is a stop waited for, where it was not heard.
    let patient = Interrupt::graced(Duration::from_secs(60));
    let started = Instant::now();
    assert_eq!(
        shown(&exited(Some(255), Some(0), true, &patient).0),
        "exited 0"
    );
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
fn a_program_that_is_not_there_is_said_and_starts_nothing() {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let missing = Invocation::new("no-such-program-for-the-windows-tests", ".");
    let said = stream(
        &missing,
        &Env::default(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
        TREE,
    )
    .unwrap_err();
    assert!(
        matches!(said, Error::Process(process::Failure::NotFound { .. })),
        "{said}"
    );
}

#[test]
fn a_machine_that_says_another_tree_than_the_one_sent_ends_the_run_and_what_it_said_is_shown() {
    let other = "== tree 0123456789abcdef0123456789abcdef01234567 0000000000000000";
    let (ended, out, err) = exits_held_to("0", &format!("{other}|== exit 0"), TREE);
    assert_eq!(shown(&ended), format!("another tree: {other}"));
    // Written as it came, before it was held to what was sent: a person can read it.
    assert!(out.contains(other), "{out}");
    assert_eq!(err, "");
    // The commit alone, or the hash alone, is another tree as well.
    for said in [
        "== tree 0123456789abcdef0123456789abcdef01234567",
        "== tree 89abcdef01234567",
        "== tree ",
        "== tree  0123456789abcdef0123456789abcdef01234567 89abcdef01234567",
    ] {
        let (ended, _, _) = exits_held_to("0", &format!("{said}|== exit 0"), TREE);
        assert_eq!(shown(&ended), format!("another tree: {said}"), "{said:?}");
    }
}

#[test]
fn a_run_that_ended_with_0_without_saying_which_tree_it_ran_did_not_end_well() {
    // 0 is a pass, and a pass of no one's tree is none.
    let (ended, _, err) = exits_held_to("1", "== exit 0", TREE);
    assert_eq!(shown(&ended), "exited 1");
    assert_eq!(
        err,
        "windows: no `== tree …` line said which tree the machine ran, and a status 0 is not taken for a pass without it: answering 1\n"
    );
    // A failure needs no tree said: the lock's refusal and an unpack that failed
    // end before there is one, and what they ended with is the answer.
    for (status, line) in [("1", "== exit 75"), ("1", "== exit 7"), ("1", "== exit 1")] {
        let (ended, _, err) = exits_held_to(status, line, TREE);
        assert_eq!(
            shown(&ended),
            format!("exited {}", line.trim_start_matches("== exit "))
        );
        assert_eq!(err, "");
    }
}

#[test]
fn only_the_first_line_that_says_a_tree_counts_and_what_a_command_prints_later_is_its_own() {
    let (ended, out, err) = exits_held_to(
        "0",
        &format!("{TREE}|== tree a command prints this too|== exit 0"),
        TREE,
    );
    assert_eq!(shown(&ended), "exited 0");
    assert!(out.contains("a command prints this too"), "{out}");
    assert_eq!(err, "");
    // And a line that only holds the words is not the line: it must begin with them.
    let (ended, _, _) = exits_held_to("0", &format!("say == tree x|{TREE}|== exit 0"), TREE);
    assert_eq!(shown(&ended), "exited 0");
}

#[test]
fn a_tree_that_was_said_does_not_change_what_a_failed_run_answers() {
    let (ended, err) = exits("1", "== exit 75");
    assert_eq!(shown(&ended), "exited 75");
    assert_eq!(err, "");
    // No tree, and no exit line either: ssh's own status, as before, and said so.
    let (ended, _, err) = exits_held_to("3", "", TREE);
    assert_eq!(shown(&ended), "exited 3");
    assert_eq!(
        err,
        "windows: no `== exit N` line ended the output, so ssh's own status is the answer: 3\n"
    );
}
