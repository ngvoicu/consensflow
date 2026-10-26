//! A whole run, on a repository made for the test: the `git` and the `tar` of the
//! machine, and an `scp` and an `ssh` that are shell scripts, which write down
//! what they were asked and do as the case wants. Where the system has no
//! `/bin/sh` the stand-ins cannot be made, and the cases are not: what they stand
//! for (a program that writes on two streams, that ends with a status, that is
//! ended by a stop) is tried on every system by the tests of `session`.
#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use cf_base::env::Env;

use super::*;
use crate::windows::options::DEFAULT_DIR;
use crate::windows::script::{RUN, SYNCED, TREE};
use crate::windows::testing::script_of;

/// The id of the run whose tree the first line of `log` (scp's) says was sent to
/// `<host>:<dir>.<id>.tgz`, which is the run's own name.
fn id_in(log: &[String]) -> String {
    let scp: Vec<&str> = log[0].split('\t').collect();
    assert_eq!(&scp[..2], ["scp", "-q"], "{log:?}");
    let name = scp[2]
        .strip_prefix(&format!("zeewin:{DEFAULT_DIR}."))
        .and_then(|rest| rest.strip_suffix(".tgz"))
        .unwrap_or_else(|| panic!("not a tarball of the run's own: {}", scp[2]));
    assert_eq!(name.len(), 17, "{name}");
    name.to_owned()
}

/// What an ssh is asked to run, as a person would read it: the script, decoded.
fn decoded(line: &str) -> String {
    let encoded = line
        .strip_prefix("powershell -NoProfile -NonInteractive -EncodedCommand ")
        .unwrap();
    script_of(encoded)
}

/// A machine for the tests: a checkout with files in it, and the folder of the
/// stand-ins' programs and of what they write down.
struct Rig {
    checkout: tempfile::TempDir,
    bin: tempfile::TempDir,
    context: Context,
}

impl Rig {
    fn new() -> Self {
        Self::on(None)
    }

    /// A rig whose scp writes into `home`, the machine's profile folder (its own, in
    /// the rig's folder of programs, when none): two rigs that are given the same one
    /// are two runs against one machine.
    fn on(home: Option<&std::path::Path>) -> Self {
        let checkout = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        let home = home.map_or_else(|| bin.path().join("home"), std::path::Path::to_path_buf);
        fs::create_dir_all(&home).unwrap();
        let path = std::env::join_paths(std::iter::once(bin.path().to_path_buf()).chain(
            std::env::split_paths(Env::from_process().os("PATH").unwrap()),
        ))
        .unwrap();
        let own = Env::from_process();
        let vars: Vec<(OsString, OsString)> = own
            .iter()
            .filter(|(name, _)| !name.to_string_lossy().starts_with("GIT_") && *name != "PATH")
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([
                ("PATH".into(), path),
                ("CF_LOG".into(), bin.path().join("log").into_os_string()),
                (
                    "CF_TARBALL".into(),
                    bin.path().join("tree.tgz").into_os_string(),
                ),
                ("CF_FIRST".into(), bin.path().join("first").into_os_string()),
                ("CF_HOME".into(), home.into_os_string()),
            ])
            .collect();
        let context = Context {
            root: checkout.path().to_path_buf(),
            env: Env::from_vars(vars),
        };
        let rig = Self {
            checkout,
            bin,
            context,
        };
        rig.program("waits", WAITS);
        rig.git(&["init", "-q"]);
        rig.file("keep.txt", "kept");
        rig.file("gone.txt", "to be deleted");
        rig.git(&["add", "-A"]);
        fs::remove_file(rig.checkout.path().join("gone.txt")).unwrap();
        rig
    }

    fn git(&self, words: &[&str]) {
        let asked = Invocation::new("git", &self.context.root).args(words);
        let answered = process::capture(&asked, &self.context.env).unwrap();
        assert_eq!(answered.code, 0, "{}", answered.stderr);
    }

    fn file(&self, name: &str, text: &str) {
        fs::write(self.checkout.path().join(name), text).unwrap();
    }

    /// A program of the machine this runs on, which is this script.
    fn program(&self, name: &str, body: &str) {
        let path = self.bin.path().join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// What the stand-ins wrote down, a line for each thing they were asked.
    fn log(&self) -> Vec<String> {
        fs::read_to_string(self.bin.path().join("log"))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn options(&self) -> Options {
        Options {
            machine: Some(Machine::at("zeewin")),
            build: false,
            dir: DEFAULT_DIR.into(),
            inside: ".".into(),
            env: vec![("CONSENSFLOW_TEST_DAEMON".into(), "native".into())],
            command: vec![
                "cargo".into(),
                "xtask".into(),
                "test".into(),
                "integration".into(),
            ],
        }
    }

    fn tarball(&self) -> PathBuf {
        self.bin.path().join("tree.tgz")
    }
}

/// The stand-in `waits FILE [TRIES]`, which goes on when the file is there and fails after
/// `TRIES` looks at 50 ms (1,200 when none: a minute) without it. A case's stand-ins wait for a
/// file that the other side of the case makes, and a side that was refused before it got there
/// never makes it: with no limit the wait is forever.
const WAITS: &str = r#"n=0
while [ ! -e "$1" ]; do
  n=$((n + 1))
  if [ "$n" -gt "${2:-1200}" ]; then echo "waits: $1 was never made" >&2; exit 1; fi
  sleep 0.05
done"#;

/// An scp that keeps what it is sent, and says so: in the rig's own file, and in the
/// machine's profile folder under the name it was sent to, as scp writes it.
const KEEPS: &str = r#"printf 'scp\t%s\t%s\n' "$1" "$3" >> "$CF_LOG"; cp "$2" "$CF_TARBALL"; cp "$2" "$CF_HOME/${3#*:}""#;

/// What the machine's script says once it has unpacked the tree it was sent: the
/// identity file the tarball holds, as the line `== tree <identity>`.
const SAYS_TREE: &str =
    r#"if t=$(tar -xzOf "$CF_TARBALL" .windows-tree 2>/dev/null); then echo "== tree $t"; fi;"#;

/// An ssh as zeewin's is: it says a word on each stream, the tree it unpacked (but
/// for the lock's refusal, 75, which unpacks nothing), then how the command ended
/// (`== exit N`, which is `said`), and then ends with 1 if that was a failure and 0
/// if not, for its shell makes every failure 1.
fn machine(said: i32) -> String {
    let flattened = i32::from(said != 0);
    let tree = if said == 75 { "" } else { SAYS_TREE };
    format!(
        r#"printf 'ssh\t%s\t%s\n' "$1" "$2" >> "$CF_LOG"; echo "out"; echo "err" >&2; {tree} echo "== exit {said}"; exit {flattened}"#
    )
}

/// An ssh that says a word and ends with `status`, as a machine does that does not
/// say how the command ended (a connection that was lost is one).
fn silent(status: i32) -> String {
    format!(r#"printf 'ssh\t%s\t%s\n' "$1" "$2" >> "$CF_LOG"; echo "out"; exit {status}"#)
}

/// An ssh that goes on until it is ended (the first time), and that answers
/// the second time it is asked (the run's end there) with `status`. It writes
/// down its own process's id as well, which is what is to be gone.
fn holds(status: u8) -> String {
    format!(
        r#"printf 'ssh\t%s\t%s\t%s\n' "$1" "$2" "$$" >> "$CF_LOG"
if [ -e "$CF_FIRST" ]; then exit {status}; fi
: > "$CF_FIRST"
echo started
exec sleep 120"#
    )
}

fn run_it(rig: &Rig, interrupt: &Interrupt) -> (Result<Outcome, Error>, String, String) {
    run_on(rig, &Machine::at("zeewin"), interrupt)
}

/// A run of the rig's options on `machine`: how it ended, and what it wrote on each stream.
fn run_on(
    rig: &Rig,
    machine: &Machine,
    interrupt: &Interrupt,
) -> (Result<Outcome, Error>, String, String) {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ended = run(
        &rig.context,
        machine,
        &rig.options(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        interrupt,
    );
    (
        ended,
        String::from_utf8(out).unwrap(),
        String::from_utf8(err).unwrap(),
    )
}

#[test]
fn a_run_sends_the_tree_then_runs_the_script_there_and_answers_what_the_command_answered() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    // The machine's lock: 75 said, and ssh ends with 1.
    rig.program("ssh", &machine(75));
    let (ended, out, err) = run_it(&rig, &Interrupt::default());
    assert_eq!(ended.unwrap(), Outcome::Exited(75));
    // What the machine said is the run's output, its last line too.
    assert_eq!((out.as_str(), err.as_str()), ("out\n== exit 75\n", "err\n"));

    // The tree went first, to a tarball in the profile that is this run's own, then ssh was asked.
    let log = rig.log();
    assert_eq!(log.len(), 2, "{log:?}");
    let id = id_in(&log);
    let ssh: Vec<&str> = log[1].split('\t').collect();
    assert_eq!(&ssh[..2], ["ssh", "zeewin"]);
    let command = command_line(&["cargo", "xtask", "test", "integration"]);
    let env = rig.options().env;
    assert_eq!(
        decoded(ssh[2]),
        remote_script(&Script {
            id: &id,
            dir: DEFAULT_DIR,
            inside: ".",
            steps: &[],
            command: &command,
            env: &env,
            node: r"C:\Program Files\nodejs",
        })
    );

    // What went over is the tree as it is on disk, with the list of it.
    let there = tempfile::tempdir().unwrap();
    let untar = Invocation::new("tar", there.path())
        .arg("xzf")
        .arg(rig.tarball())
        .arg("-C")
        .arg(there.path());
    assert_eq!(process::capture(&untar, &rig.context.env).unwrap().code, 0);
    assert_eq!(
        fs::read_to_string(there.path().join("keep.txt")).unwrap(),
        "kept"
    );
    assert!(!there.path().join("gone.txt").exists());
    assert_eq!(
        fs::read_to_string(there.path().join(SYNCED)).unwrap(),
        "keep.txt\n"
    );
    // With the tree's identity, which the machine says and which the run holds it to:
    // the commit (none yet here) and a hash of the one file that went.
    let identity = fs::read_to_string(there.path().join(TREE)).unwrap();
    assert!(identity.starts_with("none "), "{identity}");
    assert_eq!(identity.trim_end().len(), "none ".len() + 16, "{identity}");
    // And what the run is asked for, as the script that reads it from there is: the steps (none),
    // the variables, the command; none of it is on ssh's command line.
    assert_eq!(
        fs::read_to_string(there.path().join(RUN)).unwrap(),
        run_script(&Script {
            id: &id,
            dir: DEFAULT_DIR,
            inside: ".",
            steps: &[],
            command: &command,
            env: &env,
            node: r"C:\Program Files\nodejs",
        })
    );
    assert!(
        !decoded(ssh[2]).contains("integration"),
        "{}",
        decoded(ssh[2])
    );
    // And what it was made in is gone, with the list it was made with.
    for name in [SYNCED, TREE, RUN] {
        assert!(!rig.checkout.path().join(name).exists(), "{name}");
    }
}

/// The laptop as a run meets it: `zeewin2`, Windows' own SSH server, which answers a script that
/// asks for the free disk (`PSDrive`) with `free_gib` and runs any other as zeewin's does
/// ([`machine`]); and `zeelin`, its Ubuntu, which says `zeelin` (`held`, `free`) of poker-lab's
/// lock, or does not answer (`none`: ssh's 255, as a WSL that is not running gives). Each ssh is
/// written down as its host and its command, the words before the host being ssh's options.
fn laptop(zeelin: &str, free_gib: u64) -> String {
    let run = machine(0);
    format!(
        r#"for word; do host=$last; last=$word; done
printf '%s\n' "$*" >> "$CF_LOG.words"
case "$host" in
  zeelin)
    printf 'ssh\t%s\t%s\n' "$host" "$last" >> "$CF_LOG"
    case "{zeelin}" in
      held) echo held ;;
      free) echo free ;;
      *) echo "ssh: connect to host 10.6.0.6 port 22: Operation timed out" >&2; exit 255 ;;
    esac
    exit 0 ;;
esac
script=$(printf '%s' "${{last##* }}" | base64 -d 2>/dev/null | iconv -f UTF-16LE -t UTF-8 2>/dev/null)
case "$script" in *PSDrive*) printf 'ssh\t%s\t%s\n' "$host" "$last" >> "$CF_LOG"; echo "free {free_gib}"; exit 0 ;; esac
set -- "$host" "$last"
{run}"#
    )
}

/// The rig's scp, which keeps what it is sent whatever the path there.
const SENDS: &str = r#"printf 'scp\t%s\t%s\n' "$1" "$3" >> "$CF_LOG"; cp "$2" "$CF_TARBALL""#;

/// The lines of the rig's log that are ssh's to `host`, their commands.
fn asked(log: &[String], host: &str) -> Vec<String> {
    log.iter()
        .filter_map(|line| line.strip_prefix(&format!("ssh\t{host}\t")))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_run_on_zeewin2_goes_through_its_own_ssh_server_as_zeewins_does_with_ci_s_node() {
    let rig = Rig::new();
    rig.program("scp", SENDS);
    rig.program("ssh", &laptop("free", 614));
    let zeewin2 = Machine::named("zeewin2").unwrap();
    let (ended, _, err) = run_on(&rig, &zeewin2, &Interrupt::default());
    assert_eq!(ended.unwrap(), Outcome::Exited(0), "{err}");
    let log = rig.log();
    // zeelin was asked of poker-lab's lock, zeewin2 of its disk, then the tree went and ran.
    assert_eq!(
        asked(&log, "zeelin"),
        ["if [ -e ~/.compile-lock/owner ]; then echo held; else echo free; fi"]
    );
    let words = fs::read_to_string(rig.bin.path().join("log.words")).unwrap();
    assert!(
        words
            .lines()
            .next()
            .unwrap()
            .contains("-o ConnectTimeout=5"),
        "zeelin is asked with a short timeout: {words}"
    );
    let scp: Vec<&str> = log
        .iter()
        .find(|line| line.starts_with("scp\t"))
        .unwrap()
        .split('\t')
        .collect();
    let name = scp[2]
        .strip_prefix(&format!("zeewin2:{DEFAULT_DIR}."))
        .and_then(|rest| rest.strip_suffix(".tgz"))
        .unwrap_or_else(|| panic!("not sent to the profile as zeewin's is: {}", scp[2]));
    let on_zeewin2 = asked(&log, "zeewin2");
    assert_eq!(on_zeewin2.len(), 2, "the disk, then the run: {log:?}");
    let encoded = on_zeewin2[1]
        .strip_prefix("powershell -NoProfile -NonInteractive -EncodedCommand ")
        .unwrap_or_else(|| panic!("not zeewin's line: {}", on_zeewin2[1]));
    let command = command_line(&["cargo", "xtask", "test", "integration"]);
    let env = rig.options().env;
    let script = Script {
        id: name,
        dir: DEFAULT_DIR,
        inside: ".",
        steps: &[],
        command: &command,
        env: &env,
        node: r"$env:USERPROFILE\cf-tools\node",
    };
    assert_eq!(script_of(encoded), remote_script(&script));
    let there = tempfile::tempdir().unwrap();
    let untar = Invocation::new("tar", there.path())
        .arg("xzf")
        .arg(rig.tarball())
        .arg("-C")
        .arg(there.path());
    assert_eq!(process::capture(&untar, &rig.context.env).unwrap().code, 0);
    let run = fs::read_to_string(there.path().join(RUN)).unwrap();
    assert_eq!(run, run_script(&script));
    assert!(
        run.contains(r#"$env:Path = "$env:USERPROFILE\cf-tools\node;$env:USERPROFILE\.cargo\bin;" + $env:Path"#),
        "{run}"
    );
}

#[test]
fn a_run_on_zeewin2_while_poker_lab_holds_its_lock_is_refused_with_75_and_sends_nothing() {
    let rig = Rig::new();
    rig.program("scp", SENDS);
    rig.program("ssh", &laptop("held", 614));
    let zeewin2 = Machine::named("zeewin2").unwrap();
    let (ended, out, _) = run_on(&rig, &zeewin2, &Interrupt::default());
    assert_eq!(ended.unwrap(), Outcome::Exited(75));
    assert_eq!(
        out,
        "zeewin2 is busy: poker-lab's job holds it (~/.compile-lock/owner on zeelin), and no run goes while it does.\n"
    );
    let log = rig.log();
    assert_eq!(asked(&log, "zeelin").len(), 1, "{log:?}");
    assert!(asked(&log, "zeewin2").is_empty(), "{log:?}");
    assert!(!log.iter().any(|line| line.starts_with("scp")), "{log:?}");
}

#[test]
fn a_run_on_zeewin2_whose_ubuntu_does_not_answer_goes_on_and_says_why() {
    let rig = Rig::new();
    rig.program("scp", SENDS);
    rig.program("ssh", &laptop("none", 614));
    let zeewin2 = Machine::named("zeewin2").unwrap();
    let (ended, _, err) = run_on(&rig, &zeewin2, &Interrupt::default());
    assert_eq!(ended.unwrap(), Outcome::Exited(0), "{err}");
    assert!(
        err.starts_with(
            "windows: zeelin did not answer (ssh ended with status 255: ssh: connect to host 10.6.0.6 port 22: Operation timed out), so Ubuntu is not running there and no poker-lab job can hold its lock: going on\n"
        ),
        "{err}"
    );
    let log = rig.log();
    assert_eq!(asked(&log, "zeewin2").len(), 2, "{log:?}");
}

#[test]
fn a_run_on_zeewin2_short_of_disk_is_refused_with_75_and_sends_nothing() {
    let rig = Rig::new();
    rig.program("scp", SENDS);
    rig.program("ssh", &laptop("free", 30));
    let zeewin2 = Machine::named("zeewin2").unwrap();
    let (ended, out, _) = run_on(&rig, &zeewin2, &Interrupt::default());
    assert_eq!(ended.unwrap(), Outcome::Exited(75));
    assert_eq!(
        out,
        "zeewin2 is short of disk: a run needs 50 GiB free there, and it has 30.\n"
    );
    let log = rig.log();
    assert_eq!(
        asked(&log, "zeewin2").len(),
        1,
        "only the disk was asked: {log:?}"
    );
    assert!(!log.iter().any(|line| line.starts_with("scp")), "{log:?}");
}

#[test]
fn a_run_on_zeewin_asks_nothing_first() {
    let rig = Rig::new();
    rig.program("scp", SENDS);
    rig.program("ssh", &laptop("held", 1));
    let (ended, _, _) = run_on(
        &rig,
        &Machine::named("zeewin").unwrap(),
        &Interrupt::default(),
    );
    assert_eq!(ended.unwrap(), Outcome::Exited(0));
    let log = rig.log();
    assert!(log[0].starts_with("scp\t"), "{log:?}");
    assert_eq!(log.len(), 2, "{log:?}");
}

#[test]
fn a_run_holds_the_machine_to_the_tree_it_sent_and_what_the_machine_says_of_it_is_shown() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &machine(0));
    let (ended, out, err) = run_it(&rig, &Interrupt::default());
    assert_eq!(ended.unwrap(), Outcome::Exited(0));
    assert_eq!(err, "err\n");
    // The line is the identity the tarball carried (the fake machine reads it from there).
    let there = tempfile::tempdir().unwrap();
    let untar = Invocation::new("tar", there.path())
        .arg("xzf")
        .arg(rig.tarball())
        .arg("-C")
        .arg(there.path());
    assert_eq!(process::capture(&untar, &rig.context.env).unwrap().code, 0);
    let identity = fs::read_to_string(there.path().join(TREE)).unwrap();
    assert_eq!(
        out,
        format!("out\n== tree {identity}== exit 0\n"),
        "the tree line is the machine's, between what it said and how it ended"
    );
}

/// An ssh that says the tree it unpacked is not the one this run sent, and goes on until
/// it is ended (the first time), then answers the second time it is asked (the run's end
/// there) with `status`: the pid of the first is written down.
fn another_tree(status: u8) -> String {
    format!(
        r#"printf 'ssh\t%s\t%s\t%s\n' "$1" "$2" "$$" >> "$CF_LOG"
if [ -e "$CF_FIRST" ]; then exit {status}; fi
: > "$CF_FIRST"
echo "== tree 0123456789abcdef0123456789abcdef01234567 fedcba9876543210"
exec sleep 120"#
    )
}

#[test]
fn a_run_told_another_tree_than_it_sent_ends_there_and_fails_saying_both() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &another_tree(0));
    let (ended, out, _err) = run_it(&rig, &Interrupt::default());
    let said = ended.unwrap_err().to_string();
    // What the machine said is shown, and what was sent is said beside it.
    assert!(
        out.contains("== tree 0123456789abcdef0123456789abcdef01234567 fedcba9876543210"),
        "{out}"
    );
    assert!(
        said.starts_with("the machine ran another tree than the one this run sent"),
        "{said}"
    );
    assert!(
        said.contains(
            "it said `== tree 0123456789abcdef0123456789abcdef01234567 fedcba9876543210`"
        ) && said.contains("this run sent `== tree none "),
        "{said}"
    );
    assert!(said.ends_with("the run was ended there"), "{said}");
    // The run was ended there (a second ssh, with the script that ends this run), and
    // the ssh that held it is gone.
    let log = rig.log();
    assert_eq!(log.len(), 3, "{log:?}");
    let ends: Vec<&str> = log[2].split('\t').collect();
    assert_eq!(decoded(ends[2]), stop_script(DEFAULT_DIR, &id_in(&log)));
    let held: Vec<&str> = log[1].split('\t').collect();
    assert!(!cf_process::alive(held[3].parse().unwrap()), "{log:?}");
}

#[test]
fn a_run_that_ended_well_without_the_machine_saying_which_tree_it_ran_did_not_end_well() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    // The machine says how the command ended, and no tree (a script that did not unpack).
    rig.program(
        "ssh",
        r#"printf 'ssh\t%s\t%s\n' "$1" "$2" >> "$CF_LOG"; echo "out"; echo "== exit 0""#,
    );
    let (ended, _out, err) = run_it(&rig, &Interrupt::default());
    assert_eq!(ended.unwrap(), Outcome::Exited(1));
    assert_eq!(
        err,
        "windows: no `== tree …` line said which tree the machine ran, and a status 0 is not taken for a pass without it: answering 1\n"
    );
}

#[test]
fn a_run_answers_what_the_machine_said_of_the_command_whatever_ssh_answered() {
    // The machine's shell turns a failure into 1 (the lock's 75, the command's 7),
    // and a success into 0: the run answers what was said, and says nothing more.
    for (said, ssh) in [(75, 1), (7, 1), (255, 1), (0, 0), (130, 1)] {
        let rig = Rig::new();
        rig.program("scp", KEEPS);
        rig.program("ssh", &machine(said));
        let (ended, _, err) = run_it(&rig, &Interrupt::default());
        assert_eq!(ended.unwrap(), Outcome::Exited(said), "ssh {ssh}");
        assert_eq!(err, "err\n", "{said}");
    }
}

#[test]
fn a_run_whose_machine_said_nothing_of_the_command_answers_what_ssh_did_and_says_so() {
    // A connection lost, a machine gone: ssh's status is what there is, and 0 is no success.
    for (ssh, answer, why) in [
        (255, 255, "so ssh's own status is the answer: 255"),
        (1, 1, "so ssh's own status is the answer: 1"),
        (
            0,
            1,
            "and ssh's status 0 is not taken for a command's without it: answering 1",
        ),
    ] {
        let rig = Rig::new();
        rig.program("scp", KEEPS);
        rig.program("ssh", &silent(ssh));
        let (ended, out, err) = run_it(&rig, &Interrupt::default());
        assert_eq!(ended.unwrap(), Outcome::Exited(answer), "ssh {ssh}");
        assert_eq!(out, "out\n");
        assert_eq!(
            err,
            format!("windows: no `== exit N` line ended the output, {why}\n"),
            "ssh {ssh}"
        );
    }
}

#[test]
fn a_run_asks_nothing_of_the_machine_when_the_tree_could_not_be_sent() {
    let rig = Rig::new();
    rig.program("scp", r#"echo "scp: connection refused" >&2; exit 3"#);
    rig.program("ssh", &machine(0));
    let (ended, out, err) = run_it(&rig, &Interrupt::default());
    let said = ended.unwrap_err().to_string();
    assert!(said.starts_with("`scp -q "), "{said}");
    assert!(
        said.contains(" zeewin:consensflow-build.") && said.ends_with(".tgz` ended with status 3"),
        "{said}"
    );
    // What scp said is the run's own words, said as it said them.
    assert_eq!(
        (out.as_str(), err.as_str()),
        ("", "scp: connection refused\n")
    );
    assert!(rig.log().is_empty(), "{:?}", rig.log());
}

#[test]
fn a_stand_in_waits_for_a_file_that_is_made_and_gives_up_on_one_that_never_is() {
    let rig = Rig::new();
    let waits = |file: &str| {
        let asked = Invocation::new("waits", &rig.context.root)
            .arg(rig.checkout.path().join(file))
            .arg("3");
        let mut waiting = process::spawn(&asked, &rig.context.env, Stdio::null(), false).unwrap();
        // Three looks are a moment; a wait that is still going after a minute has no limit.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while waiting.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= until {
                waiting.kill().unwrap();
                waiting.wait().unwrap();
                panic!("the wait for {file} was still going a minute after its three looks");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        waiting.wait_with_output().unwrap()
    };
    let found = waits("keep.txt");
    assert!(
        found.status.success() && found.stderr.is_empty(),
        "{found:?}"
    );
    let given_up = waits("never-made.txt");
    assert_eq!(given_up.status.code(), Some(1), "{given_up:?}");
    let said = String::from_utf8_lossy(&given_up.stderr);
    assert!(said.ends_with("never-made.txt was never made\n"), "{said}");
}

#[test]
fn a_step_that_failed_as_a_stop_ended_it_is_the_stop_when_the_stop_is_heard_a_moment_after() {
    let rig = Rig::new();
    // The scp that is ended by the Ctrl-C: it says it ran, and ends badly.
    rig.program("scp", r#": > "$CF_FIRST"; exit 3"#);
    rig.program("ssh", &machine(0));
    // The grace is long: the wait for the stop ends when the stop is heard.
    let interrupt = Interrupt::graced(std::time::Duration::from_secs(60));
    let late = interrupt.clone();
    let ran = rig.bin.path().join("first");
    // The program is told of the stop after the scp has failed, not before. It waits for the
    // scp for a minute at most, and not past the run's end: a run that was refused before it
    // sent anything never reaches the scp, and nothing else would end the wait.
    let over = Arc::new(AtomicBool::new(false));
    let hearing = {
        let over = Arc::clone(&over);
        std::thread::spawn(move || {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
            while !ran.exists() {
                if over.load(Ordering::SeqCst) || std::time::Instant::now() >= until {
                    return false;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
            late.raise();
            true
        })
    };
    let (ended, out, err) = run_it(&rig, &interrupt);
    over.store(true, Ordering::SeqCst);
    assert!(
        hearing.join().unwrap(),
        "the stop was never raised, for the scp never ran: the run ended with {:?}",
        ended.as_ref().map_err(ToString::to_string)
    );
    assert_eq!(ended.unwrap(), Outcome::Stopped);
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(rig.log().is_empty(), "no ssh was asked: {:?}", rig.log());
}

#[test]
fn a_stop_ends_the_run_there_by_a_second_ssh_that_runs_the_script_that_ends_it() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &holds(0));
    let interrupt = Interrupt::default();
    let raise = interrupt.clone();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    // The stop is heard once the run has said it started.
    struct Then<'a>(&'a mut Vec<u8>, Interrupt);
    impl Write for Then<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.extend_from_slice(bytes);
            self.1.raise();
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let ended = run(
        &rig.context,
        &Machine::at("zeewin"),
        &rig.options(),
        &mut Console {
            out: &mut Then(&mut out, raise),
            err: &mut err,
        },
        &interrupt,
    );
    assert_eq!(ended.unwrap(), Outcome::Stopped);
    assert_eq!(String::from_utf8(out).unwrap(), "started\n");
    assert!(err.is_empty());
    let log = rig.log();
    assert_eq!(log.len(), 3, "{log:?}");
    let ends: Vec<&str> = log[2].split('\t').collect();
    assert_eq!(&ends[..2], ["ssh", "zeewin"]);
    assert_eq!(decoded(ends[2]), stop_script(DEFAULT_DIR, &id_in(&log)));
    // The ssh that held the run is gone: ended, before the machine was asked to end it.
    let held: Vec<&str> = log[1].split('\t').collect();
    assert!(!cf_process::alive(held[3].parse().unwrap()), "{log:?}");
}

#[test]
fn a_stop_the_machine_could_not_be_asked_to_carry_out_is_said() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &holds(5));
    let interrupt = Interrupt::default();
    let raise = interrupt.clone();
    struct Then(Interrupt);
    impl Write for Then {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.raise();
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut err = Vec::new();
    let ended = run(
        &rig.context,
        &Machine::at("zeewin"),
        &rig.options(),
        &mut Console {
            out: &mut Then(raise),
            err: &mut err,
        },
        &interrupt,
    );
    assert_eq!(ended.unwrap(), Outcome::Stopped);
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "windows: the run may still be going on the machine: ssh ended with status 5 when asked to end it\n"
    );
}

#[test]
fn a_stop_heard_before_ssh_has_started_ends_nothing_there_for_nothing_has_started() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &machine(0));
    let interrupt = Interrupt::default();
    interrupt.raise();
    let (ended, out, err) = run_it(&rig, &interrupt);
    assert_eq!(ended.unwrap(), Outcome::Stopped);
    assert_eq!((out.as_str(), err.as_str()), ("", ""));
    assert!(rig.log().is_empty(), "{:?}", rig.log());
}

#[test]
fn a_run_whose_output_cannot_be_written_is_ended_there_too_and_says_why() {
    struct Closed;
    impl Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &holds(0));
    let mut err = Vec::new();
    let ended = run(
        &rig.context,
        &Machine::at("zeewin"),
        &rig.options(),
        &mut Console {
            out: &mut Closed,
            err: &mut err,
        },
        &Interrupt::default(),
    );
    let said = ended.unwrap_err().to_string();
    assert_eq!(said, "could not write what the run says: closed");
    // The machine was asked to end it: a third thing written down.
    assert_eq!(rig.log().len(), 3, "{:?}", rig.log());
}

#[test]
fn what_cannot_be_sent_is_refused_before_anything_is_done() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &machine(0));
    // What a run is asked for has no limit but the shell's (it goes in the tarball); the
    // folder's name is in the command line, and one that makes it too long is refused.
    let mut options = rig.options();
    options.dir = "d".repeat(900);
    options.build = true;
    let refused = check(&options).unwrap_err().to_string();
    assert!(
        refused.starts_with("the script for the machine is "),
        "{refused}"
    );
    assert!(refused.ends_with("use a shorter --dir"), "{refused}");
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ended = run(
        &rig.context,
        &Machine::at("zeewin"),
        &options,
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
    );
    assert_eq!(ended.unwrap_err().to_string(), refused);
    assert!(rig.log().is_empty());
    // And twenty variables of a hundred characters each, which the line could not hold, are
    // sent: they ride in the tarball.
    let mut options = rig.options();
    options.env = (0..20)
        .map(|at| (format!("CONSENSFLOW_TEST_VARIABLE_{at}"), "x".repeat(100)))
        .collect();
    options.build = true;
    check(&options).unwrap();
}

/// What a tarball on the machine holds: the names of its members, one to a line.
fn members(rig: &Rig, tarball: &std::path::Path) -> String {
    let listing = Invocation::new("tar", &rig.context.root)
        .arg("-tzf")
        .arg(tarball);
    let answered = process::capture(&listing, &rig.context.env).unwrap();
    assert_eq!(answered.code, 0, "{}", answered.stderr);
    answered.stdout
}

/// Where a rig's scp sent the tree, as the machine's profile folder has it.
fn sent_to(rig: &Rig, home: &std::path::Path) -> PathBuf {
    let scp: Vec<String> = rig.log()[0].split('\t').map(str::to_owned).collect();
    let (_, name) = scp[2].split_once(':').unwrap();
    home.join(name)
}

#[test]
fn two_runs_that_overlap_send_their_trees_to_files_of_their_own() {
    // The machine has one profile folder, and both runs scp into it. Run A's script is
    // on its way (its ssh is held, as zeewin's takes seconds to start PowerShell) when
    // run B, from another tree, sends its tree: what B sends must not be what A unpacks.
    let home = tempfile::tempdir().unwrap();
    let (a, b) = (Rig::on(Some(home.path())), Rig::on(Some(home.path())));
    a.file("a-only.txt", "from A's tree");
    b.file("b-only.txt", "from B's tree");
    a.program("scp", KEEPS);
    b.program("scp", KEEPS);
    a.program(
        "ssh",
        &format!(
            r#"printf 'ssh\t%s\t%s\n' "$1" "$2" >> "$CF_LOG"; waits "$CF_FIRST" || exit 97; {SAYS_TREE} echo "== exit 0""#
        ),
    );
    b.program("ssh", &machine(0));

    let (a_ended, b_ended, a_file, b_file, a_held, b_held) = std::thread::scope(|scope| {
        let a_run = scope.spawn(|| run_it(&a, &Interrupt::default()));
        // A's tree is on the machine and A's script has not unpacked it yet.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while a.log().len() < 2 {
            assert!(std::time::Instant::now() < until, "run A never reached ssh");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let b_ended = run_it(&b, &Interrupt::default()).0;
        let (a_file, b_file) = (sent_to(&a, home.path()), sent_to(&b, home.path()));
        let (a_held, b_held) = (members(&a, &a_file), members(&b, &b_file));
        // Let A's script go on, now that what it was sent has been looked at.
        fs::write(a.bin.path().join("first"), "").unwrap();
        (
            a_run.join().unwrap().0,
            b_ended,
            a_file,
            b_file,
            a_held,
            b_held,
        )
    });
    a_ended.unwrap();
    b_ended.unwrap();

    // The file run A sent its tree to still holds A's tree, and B's holds B's.
    assert!(
        a_held.lines().any(|name| name == "a-only.txt") && !a_held.lines().any(|name| name == "b-only.txt"),
        "run A's tree was replaced on the machine before its script unpacked it: {} holds {a_held:?}",
        a_file.display()
    );
    assert!(
        b_held.lines().any(|name| name == "b-only.txt"),
        "{} holds {b_held:?}",
        b_file.display()
    );
    assert_ne!(a_file, b_file, "two runs sent their trees to the same file");
}

/// The machine's script as far as the lock goes, for a stand-in: the lock is a file that is
/// created in one act or not at all (`set -C`, which makes the shell's `>` fail on a file that
/// exists, as `FileShare.None` makes the open of a file that is open), so of two runs that
/// ask at once one has it. The one that has it unpacks (here: says the tree of the tarball
/// it was sent), waits until the other has been refused, and ends well; the other is
/// refused, takes away what it sent and ends with 75. Each run's tarball is the one its own
/// `scp` was told of, which is what its script names.
const RACES: &str = r#"printf 'ssh\t%s\t%s\n' "$1" "$2" >> "$CF_LOG"
sent=$(awk -F'\t' '$1 == "scp" { print $3 }' "$CF_LOG"); mine="$CF_HOME/${sent#*:}"
if ( set -C; : > "$CF_HOME/lock" ) 2>/dev/null; then
  echo "== tree $(tar -xzOf "$mine" .windows-tree)"
  waits "$CF_HOME/refused" || exit 97
  rm -f "$CF_HOME/lock" "$mine"
  printf '\n== exit 0\n'; exit 0
else
  echo "A run is still going on this machine: one at a time."
  rm -f "$mine"; : > "$CF_HOME/refused"
  printf '\n== exit 75\n'; exit 1
fi"#;

#[test]
fn of_two_runs_that_ask_for_the_lock_together_one_runs_and_the_other_ends_with_75_having_sent_nothing_that_stays(
) {
    // Both runs start at once (from two trees, as two workers' do) on one machine whose
    // lock is created or refused in one act: whichever is first, there is one of each.
    let home = tempfile::tempdir().unwrap();
    let (a, b) = (Rig::on(Some(home.path())), Rig::on(Some(home.path())));
    a.file("a-only.txt", "from A's tree");
    b.file("b-only.txt", "from B's tree");
    for rig in [&a, &b] {
        rig.program("scp", KEEPS);
        rig.program("ssh", RACES);
    }
    let (from_a, from_b) = std::thread::scope(|scope| {
        let a_run = scope.spawn(|| run_it(&a, &Interrupt::default()));
        let b_run = scope.spawn(|| run_it(&b, &Interrupt::default()));
        (a_run.join().unwrap(), b_run.join().unwrap())
    });
    let statuses = (
        from_a.0.as_ref().ok().map(|ended| format!("{ended:?}")),
        from_b.0.as_ref().ok().map(|ended| format!("{ended:?}")),
    );
    let (won, refused) = match (&from_a.0, &from_b.0) {
        (Ok(Outcome::Exited(0)), Ok(Outcome::Exited(75))) => (&from_a, &from_b),
        (Ok(Outcome::Exited(75)), Ok(Outcome::Exited(0))) => (&from_b, &from_a),
        _ => panic!("one run and one refused with 75, not {statuses:?}"),
    };
    // The one that ran said the tree it sent; the other said none and ran nothing.
    assert!(won.1.contains("== tree none "), "{}", won.1);
    assert!(!refused.1.contains("== tree "), "{}", refused.1);
    assert!(
        refused.1.contains("A run is still going on this machine"),
        "{}",
        refused.1
    );
    // Nothing of either stays on the machine but the lock the stand-in leaves.
    let left: Vec<String> = fs::read_dir(home.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(left, ["refused"], "{left:?}");
}

#[test]
fn a_run_told_another_tree_whose_end_there_the_machine_could_not_be_asked_for_says_it_may_go_on() {
    let rig = Rig::new();
    rig.program("scp", KEEPS);
    rig.program("ssh", &another_tree(5));
    let (ended, _out, err) = run_it(&rig, &Interrupt::default());
    let said = ended.unwrap_err().to_string();
    // Not "ended there": the machine said it had not, and the lock may be held until the run ends.
    assert!(!said.contains("the run was ended there"), "{said}");
    assert!(
        said.ends_with(
            "the run may still be going on the machine: ssh ended with status 5 when asked to end it"
        ),
        "{said}"
    );
    // Said once, in the error, and not again on the error stream.
    assert_eq!(err, "");
}
