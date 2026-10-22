//! The reports brought back, on a checkout made for the test: an `ssh` and an `scp` that are shell
//! scripts, which write down what they were asked and answer as the case wants (the listing the
//! machine gives, the files it holds, what fails). Where the system has no `/bin/sh` the stand-ins
//! cannot be made, and the cases are not.
#![cfg(unix)]

use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use cf_base::env::Env;

use super::*;
use crate::windows::options::DEFAULT_DIR;

/// A checkout, and the folder of the stand-ins' programs and of what they hold and write down.
struct Rig {
    checkout: tempfile::TempDir,
    bin: tempfile::TempDir,
    context: Context,
}

/// An ssh that counts what it is asked: the first time it says how a run ended, if the case has a
/// run (`CF_RUN` is its status), and every other time it lists what the machine holds
/// (`CF_LISTING`), ending with `CF_LIST_STATUS`.
const SSH: &str = r#"n=$(cat "$CF_COUNT" 2>/dev/null || echo 0); n=$((n+1)); echo $n > "$CF_COUNT"
printf 'ssh\t%s\n' "$n" >> "$CF_LOG"
if [ "$n" -eq 1 ] && [ -n "$CF_RUN" ]; then echo out; echo "== exit $CF_RUN"; exit 0; fi
cat "$CF_LISTING"
exit "${CF_LIST_STATUS:-0}""#;

/// An scp that keeps the tree it is sent, and copies from what the machine holds
/// (`CF_REMOTE`) what it is asked for, but for the names `CF_FAIL` lists.
const SCP: &str = r#"printf 'scp\t%s\t%s\n' "$2" "$3" >> "$CF_LOG"
case "$2" in
  zeewin:*)
    rel=${2#zeewin:consensflow-build/evals/reports/}
    if grep -qx "$rel" "$CF_FAIL" 2>/dev/null; then echo "scp: no such file" >&2; exit 4; fi
    cp "$CF_REMOTE/$rel" "$3" ;;
  *) cp "$2" "$CF_TARBALL" ;;
esac"#;

impl Rig {
    fn new() -> Self {
        let checkout = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
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
                ("CF_COUNT".into(), bin.path().join("count").into_os_string()),
                (
                    "CF_TARBALL".into(),
                    bin.path().join("tree.tgz").into_os_string(),
                ),
                (
                    "CF_LISTING".into(),
                    bin.path().join("listing").into_os_string(),
                ),
                (
                    "CF_REMOTE".into(),
                    bin.path().join("remote").into_os_string(),
                ),
                ("CF_FAIL".into(), bin.path().join("fail").into_os_string()),
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
        rig.program("ssh", SSH);
        rig.program("scp", SCP);
        fs::write(rig.bin.path().join("listing"), "").unwrap();
        fs::create_dir_all(rig.bin.path().join("remote")).unwrap();
        rig
    }

    /// This rig's machine runs a command that ends with `status`, and is then asked for the
    /// reports: the first thing its ssh is asked is the run.
    fn runs_ending_with(mut self, status: i32) -> Self {
        let mut vars: Vec<(OsString, OsString)> = self
            .context
            .env
            .iter()
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .collect();
        vars.push(("CF_RUN".into(), status.to_string().into()));
        self.context.env = Env::from_vars(vars);
        self
    }

    /// A checkout a run can pack: a repository with a file in it.
    fn with_tree() -> Self {
        let rig = Self::new();
        fs::write(rig.checkout.path().join("keep.txt"), "kept").unwrap();
        for words in [&["init", "-q"][..], &["add", "-A"][..]] {
            let asked = Invocation::new("git", &rig.context.root).args(words);
            let answered = process::capture(&asked, &rig.context.env).unwrap();
            assert_eq!(answered.code, 0, "{}", answered.stderr);
        }
        rig
    }

    fn program(&self, name: &str, body: &str) {
        let path = self.bin.path().join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// What the machine holds: the file `name` (from `evals/reports`, with `/`) and its text.
    fn remote(&self, name: &str, text: &str) {
        let path = self.bin.path().join("remote").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    /// What the machine lists: each of `lines`, a name and a tab and a size.
    fn lists(&self, lines: &[(&str, usize)]) {
        let text: String = lines
            .iter()
            .map(|(name, size)| format!("{name}\t{size}\n"))
            .collect();
        fs::write(self.bin.path().join("listing"), text).unwrap();
    }

    /// The file `name` of this checkout's `evals/reports`, if it has it.
    fn here(&self, name: &str) -> Option<String> {
        fs::read_to_string(self.checkout.path().join("evals/reports").join(name)).ok()
    }

    fn put_here(&self, name: &str, text: &str) {
        let path = self.checkout.path().join("evals/reports").join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn log(&self) -> Vec<String> {
        fs::read_to_string(self.bin.path().join("log"))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    fn options(&self) -> Options {
        Options {
            host: "zeewin".into(),
            build: false,
            dir: DEFAULT_DIR.into(),
            inside: ".".into(),
            env: Vec::new(),
            command: vec!["cargo".into(), "xtask".into(), "live".into(), "long".into()],
        }
    }

    /// The reports brought back, and what was said of it.
    fn bring(&self) -> (Result<(), Error>, String) {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let ended = bring_back(
            &self.context,
            &self.options(),
            &mut Console {
                out: &mut out,
                err: &mut err,
            },
        );
        assert!(out.is_empty());
        (ended, String::from_utf8(err).unwrap())
    }
}

/// Of the files a name is, the ones named in `log`'s scp lines.
fn copied(log: &[String]) -> Vec<&str> {
    log.iter()
        .filter(|line| line.starts_with("scp\tzeewin:"))
        .map(|line| line.split('\t').nth(1).unwrap())
        .collect()
}

#[test]
fn the_reports_this_checkout_lacks_are_brought_back_and_the_ones_it_has_are_kept_as_they_are() {
    let rig = Rig::new();
    rig.remote("long/2026-10-20T12-00-00-000Z-long.md", "report");
    rig.remote("long/2026-10-20T12-00-00-000Z-long-ledger.db", "ledger!!");
    rig.remote("2026-10-20T13-00-00-000Z-six+decisions-1.json", "{}");
    rig.lists(&[
        ("long/2026-10-20T12-00-00-000Z-long.md", 6),
        ("long/2026-10-20T12-00-00-000Z-long-ledger.db", 8),
        ("2026-10-20T13-00-00-000Z-six+decisions-1.json", 2),
    ]);
    // This one is here already, and is not the machine's text: it is not replaced.
    rig.put_here("long/2026-10-20T12-00-00-000Z-long.md", "mine");

    let (ended, said) = rig.bring();
    ended.unwrap();
    assert_eq!(
        rig.here("long/2026-10-20T12-00-00-000Z-long.md").as_deref(),
        Some("mine")
    );
    assert_eq!(
        rig.here("long/2026-10-20T12-00-00-000Z-long-ledger.db")
            .as_deref(),
        Some("ledger!!")
    );
    assert_eq!(
        rig.here("2026-10-20T13-00-00-000Z-six+decisions-1.json")
            .as_deref(),
        Some("{}")
    );
    assert_eq!(
        said,
        "windows: brought back 2 of the machine's reports into evals/reports: \
         long/2026-10-20T12-00-00-000Z-long-ledger.db, 2026-10-20T13-00-00-000Z-six+decisions-1.json\n"
    );
    // One ssh to list, then one scp for each file that was not here, from the folder of the copy.
    let log = rig.log();
    assert_eq!(log[0], "ssh\t1");
    assert_eq!(
        copied(&log),
        [
            "zeewin:consensflow-build/evals/reports/long/2026-10-20T12-00-00-000Z-long-ledger.db",
            "zeewin:consensflow-build/evals/reports/2026-10-20T13-00-00-000Z-six+decisions-1.json",
        ]
    );
    // No copy under a name of its own is left.
    let left: Vec<PathBuf> = walk(&rig.checkout.path().join("evals"));
    assert!(
        left.iter()
            .all(|file| file.extension().is_none_or(|e| e != "part")),
        "{left:?}"
    );
}

/// Every file under `folder`.
fn walk(folder: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(folder).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

#[test]
fn what_is_here_already_or_the_matrix_writes_is_not_asked_for_and_nothing_is_said_of_it() {
    let rig = Rig::new();
    rig.put_here("long/a.md", "mine");
    rig.lists(&[("long/a.md", 4), ("windows/2026-10-20T10-00-00/run.log", 9)]);
    let (ended, said) = rig.bring();
    ended.unwrap();
    assert_eq!(said, "");
    // Only the listing was asked for.
    assert_eq!(rig.log(), ["ssh\t1"]);
    // And a machine with nothing lists nothing.
    let rig = Rig::new();
    let (ended, said) = rig.bring();
    ended.unwrap();
    assert_eq!((said.as_str(), rig.log()), ("", vec!["ssh\t1".to_owned()]));
}

#[test]
fn a_name_that_is_no_reports_is_said_and_is_never_copied_or_made_a_path() {
    let rig = Rig::new();
    rig.remote("long/ok.md", "fine");
    let names = [
        "../outside.json",
        "long/../../outside.json",
        "/abs.json",
        "long//double.json",
        "long/a b.md",
        "long/a;b.md",
        "long/$(id).md",
        "long/a'b.md",
        "",
    ];
    let listed: Vec<(&str, usize)> = names
        .iter()
        .copied()
        .chain(["long/ok.md"])
        .map(|name| (name, 4))
        .collect();
    rig.lists(&listed);
    let (ended, said) = rig.bring();
    ended.unwrap();
    for name in &names {
        assert!(
            said.contains(&format!(
                "windows: not brought back, for it is no name of a report's: {name:?}\n"
            )),
            "{name}: {said}"
        );
    }
    assert_eq!(rig.here("long/ok.md").as_deref(), Some("fine"));
    assert_eq!(copied(&rig.log()).len(), 1, "{:?}", rig.log());
    assert!(!rig.checkout.path().join("outside.json").exists());
    assert!(!rig.checkout.path().join("evals/outside.json").exists());
}

#[test]
fn a_copy_that_is_not_the_size_the_machine_listed_is_taken_back_and_said() {
    let rig = Rig::new();
    rig.remote("long/short.md", "abc");
    rig.lists(&[("long/short.md", 99)]);
    let (ended, said) = rig.bring();
    ended.unwrap();
    assert_eq!(
        said,
        "windows: the report long/short.md could not be brought back: 3 bytes came, and the machine listed 99\n"
    );
    assert_eq!(rig.here("long/short.md"), None);
    assert_eq!(rig.here("long/short.md.part"), None);
}

#[test]
fn a_file_that_cannot_be_copied_is_said_and_the_others_are_still_brought() {
    let rig = Rig::new();
    rig.remote("long/a.md", "aaa");
    rig.remote("long/c.md", "ccc");
    rig.lists(&[("long/a.md", 3), ("long/b.md", 3), ("long/c.md", 3)]);
    fs::write(rig.bin.path().join("fail"), "long/b.md\n").unwrap();
    let (ended, said) = rig.bring();
    ended.unwrap();
    assert_eq!(
        said,
        "windows: the report long/b.md could not be brought back: scp ended with status 4: scp: no such file\n\
         windows: brought back 2 of the machine's reports into evals/reports: long/a.md, long/c.md\n"
    );
    assert_eq!(rig.here("long/a.md").as_deref(), Some("aaa"));
    assert_eq!(rig.here("long/b.md"), None);
    assert_eq!(rig.here("long/c.md").as_deref(), Some("ccc"));
}

#[test]
fn a_listing_that_fails_is_said_and_nothing_is_asked_after_it() {
    let rig = Rig::new();
    rig.lists(&[("long/a.md", 3)]);
    rig.program(
        "ssh",
        r#"printf 'ssh\n' >> "$CF_LOG"; cat "$CF_LISTING"; exit 5"#,
    );
    let (ended, said) = rig.bring();
    ended.unwrap();
    assert_eq!(
        said,
        "windows: the reports on the machine could not be listed, and none was brought back: ssh ended with status 5\n"
    );
    assert_eq!(rig.log(), ["ssh"]);
}

#[test]
fn many_files_are_counted_after_the_first_few() {
    let names: Vec<String> = (1..=6).map(|n| format!("long/{n}.md")).collect();
    assert_eq!(
        told(&names).unwrap(),
        "brought back 6 of the machine's reports into evals/reports: \
         long/1.md, long/2.md, long/3.md, long/4.md and 2 more"
    );
    assert_eq!(told(&[]), None);
}

#[test]
fn the_machines_listing_is_read_for_its_names_and_sizes_and_nothing_else() {
    let (files, refused) = parse(
        "long/a.md\t12\r\nnot a listing line\nlong/b.md\tmany\nwindows/x/run.log\t4\n\
         ../x.json\t1\n2026.json\t0\n\tlong\t7\n",
    );
    assert_eq!(
        files,
        [
            Listed {
                name: "long/a.md".into(),
                size: 12
            },
            Listed {
                name: "2026.json".into(),
                size: 0
            }
        ]
    );
    assert_eq!(refused, ["../x.json"]);
    for good in [
        "a",
        "a.b",
        "long/2026-10-20T12-25-35-830Z-long-ledger.db",
        "x+y_z-1.json",
    ] {
        assert!(is_report_name(good), "{good}");
    }
    for bad in [
        "", "/", "a/", "/a", "a//b", ".", "..", "a/./b", "a b", "a:b", "é.md", "a\\b",
    ] {
        assert!(!is_report_name(bad), "{bad:?}");
    }
}

#[test]
fn a_run_that_ended_there_with_a_status_brings_its_reports_back_and_answers_the_commands_status() {
    let rig = Rig::with_tree().runs_ending_with(7);
    rig.remote("long/run.md", "report");
    rig.lists(&[("long/run.md", 6)]);

    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ended = run_keeping(
        &rig.context,
        &rig.options(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
    );
    // The command's own status stands, and what it left is here.
    assert_eq!(ended.unwrap(), Outcome::Exited(7));
    assert_eq!(rig.here("long/run.md").as_deref(), Some("report"));
    assert_eq!(String::from_utf8(out).unwrap(), "out\n== exit 7\n");
    assert_eq!(
        String::from_utf8(err).unwrap(),
        "windows: brought back 1 of the machine's reports into evals/reports: long/run.md\n"
    );
    // The tree went, the run was asked, then the reports: listed, and the one copied.
    let log = rig.log();
    assert_eq!(log.len(), 4, "{log:?}");
    assert!(
        log[0].starts_with("scp\t") && log[0].ends_with("\tzeewin:consensflow-build.tgz"),
        "{log:?}"
    );
    assert_eq!(&log[1..3], ["ssh\t1", "ssh\t2"]);
    assert!(
        log[3].starts_with("scp\tzeewin:consensflow-build/evals/reports/long/run.md\t"),
        "{log:?}"
    );
}

/// The matrix runs its evals through the same machine, and the eval's own report is one of what
/// stays there: it is brought back after each run too.
#[test]
fn the_matrixs_machine_brings_the_reports_back_too() {
    use crate::windows::matrix::{Machine, Remote};

    let rig = Rig::with_tree().runs_ending_with(0);
    rig.remote("2026-10-21T10-00-00-000Z-simple-fix-1.json", "{}");
    rig.lists(&[("2026-10-21T10-00-00-000Z-simple-fix-1.json", 2)]);
    let interrupt = Interrupt::default();
    let mut machine = Machine {
        context: &rig.context,
        interrupt: &interrupt,
    };
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ended = machine.run(
        &rig.options(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
    );
    assert_eq!(ended.unwrap(), Outcome::Exited(0));
    assert_eq!(
        rig.here("2026-10-21T10-00-00-000Z-simple-fix-1.json")
            .as_deref(),
        Some("{}")
    );
}

#[test]
fn a_run_that_could_not_be_sent_brings_nothing_back_and_asks_nothing() {
    let rig = Rig::with_tree().runs_ending_with(0);
    rig.program("scp", r#"echo "scp: connection refused" >&2; exit 3"#);
    rig.lists(&[("long/run.md", 6)]);
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let ended = run_keeping(
        &rig.context,
        &rig.options(),
        &mut Console {
            out: &mut out,
            err: &mut err,
        },
        &Interrupt::default(),
    );
    assert!(ended.is_err());
    assert!(rig.log().is_empty(), "{:?}", rig.log());
}
