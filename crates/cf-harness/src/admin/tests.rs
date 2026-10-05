//! The admin's calls as `tests/harness-admin.test.mjs` holds Node's, and the
//! cache and the pending map on a clock the test moves; what Node answers,
//! case by case, is held by the recorded goldens (`tests/admin`).

use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use cf_base::env::Env;
use cf_process::{CaptureFailed, Captured, Limits};
use cf_proto::agents::Harness;
use serde_json::Value;

use super::*;
use crate::testing::{finished, Driver, ManualTime, Response, ScriptedCapture, ScriptedLatest};
use crate::testing::{named, EPOCH_MS};

mod looks;
#[cfg(unix)]
mod real;

/// An admin over a throwaway home, on a clock and with programs and feeds the
/// test scripts.
pub(super) struct Fixture {
    pub(super) root: tempfile::TempDir,
    pub(super) env: Env,
    pub(super) time: Rc<ManualTime>,
    pub(super) capture: Rc<ScriptedCapture>,
    pub(super) latest: Rc<ScriptedLatest>,
    pub(super) admin: Rc<HarnessAdmin>,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let text = |folder: &str| root.path().join(folder).to_string_lossy().into_owned();
        let env = Env::from_vars([
            ("HOME", text("home")),
            ("PATH", text("bin")),
            ("CONSENSFLOW_HOME", text("consensflow")),
            ("CLAUDE_CONFIG_DIR", text("home/.claude")),
        ]);
        let time = Rc::new(ManualTime::new(EPOCH_MS));
        let capture = Rc::new(ScriptedCapture::default());
        let latest = Rc::new(ScriptedLatest::default());
        let admin = Rc::new(HarnessAdmin::new(
            env.clone(),
            Rc::clone(&time) as Rc<dyn Time>,
            Rc::clone(&latest) as Rc<dyn Latest>,
            Rc::clone(&capture) as Rc<dyn Capture>,
        ));
        Self {
            root,
            env,
            time,
            capture,
            latest,
            admin,
        }
    }

    /// An executable `command` in `folder` (under the root, its parts parted
    /// by `/`): where it is, spelled with the platform's separator, as `.exe`
    /// on Windows.
    pub(super) fn install(&self, folder: &str, command: &str) -> PathBuf {
        let folder = folder
            .split('/')
            .fold(self.root.path().to_path_buf(), |path, part| path.join(part));
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join(if cfg!(windows) {
            format!("{command}.exe")
        } else {
            command.to_owned()
        });
        fs::write(&file, "").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        }
        file
    }

    /// `check`, answered at once: every answer scripted is.
    pub(super) fn check(&self, id: Option<&str>, refresh: bool) -> Vec<Rc<Row>> {
        finished(Box::pin(self.admin.check(id, refresh))).unwrap()
    }

    /// The row of `id`, looked at again when `refresh`.
    pub(super) fn row(&self, id: &str, refresh: bool) -> Rc<Row> {
        self.check(Some(id), refresh).remove(0)
    }
}

/// What a program that ended with 0 wrote.
pub(super) fn says(stdout: &str) -> Response<Result<Captured, CaptureFailed>> {
    Response::Now(Ok(Captured {
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }))
}

/// A program that did not answer: out of time, or ended otherwise.
pub(super) fn fails(killed: bool) -> Response<Result<Captured, CaptureFailed>> {
    Response::Now(Err(CaptureFailed {
        message: "Command failed: x\n".to_owned(),
        code: (!killed).then_some(1),
        killed,
        stdout: String::new(),
        stderr: String::new(),
    }))
}

/// A row as the page reads it.
pub(super) fn json_of(row: &Row) -> Value {
    serde_json::to_value(row).unwrap()
}

#[test]
fn harnesses_not_installed_are_listed_in_order_without_running_or_asking_anything() {
    let fixture = Fixture::new();
    let rows = fixture.check(None, false);
    let ids: Vec<Harness> = rows.iter().map(|row| row.id).collect();
    assert_eq!(ids, known_harnesses());
    for row in &rows {
        assert!(!row.installed && row.path.is_none());
        assert_eq!(row.version, Version::NotInstalled);
        assert_eq!(row.update, Update::NotChecked);
    }
    assert!(fixture.capture.take_ran().is_empty());
    assert!(fixture.latest.take_asked().is_empty());
    assert_eq!(
        serde_json::to_string(&*rows[0]).unwrap(),
        format!(
            r#"{{"id":"devin","path":null,"installed":false,"checkedAt":{EPOCH_MS},"version":{{"state":"not-installed"}},"update":{{"state":"not-checked"}}}}"#
        ),
        "no distribution key where nothing was looked at"
    );
}

#[test]
fn a_name_that_is_no_harness_is_refused_before_anything_is_looked_at() {
    let fixture = Fixture::new();
    for name in ["../invalid", "", "claude-code", "Claude", "codex "] {
        let refused = finished(Box::pin(fixture.admin.check(Some(name), false)));
        assert_eq!(refused.unwrap_err(), "Unknown harness", "{name:?}");
    }
    assert!(fixture.capture.take_ran().is_empty());
}

#[test]
fn a_look_is_kept_and_looked_at_again_when_asked_to() {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture.capture.answer(
        "codex --version",
        [says("codex-cli 99.1.0\n"), says("codex-cli 99.1.0\n")],
    );
    fixture.latest.says(Harness::Codex, "99.2.0");
    fixture.latest.says(Harness::Codex, "99.2.0");
    let first = fixture.row("codex", false);
    assert_eq!(first.version.value(), Some("99.1.0"));
    assert!(matches!(first.update, Update::Available(_)));
    let again = fixture.row("codex", false);
    assert!(
        Rc::ptr_eq(&first, &again),
        "the row kept, not looked at again"
    );
    assert_eq!(fixture.latest.take_asked().len(), 1);
    let refreshed = fixture.row("codex", true);
    assert!(!Rc::ptr_eq(&first, &refreshed));
    assert_eq!(fixture.latest.take_asked().len(), 1);
    assert_eq!(fixture.capture.take_ran().len(), 2);
}

#[test]
fn a_row_is_kept_five_minutes_from_when_its_look_began_and_no_longer() {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture
        .capture
        .answer("codex --version", [says("1.0.0"), says("1.0.0")]);
    fixture.latest.says(Harness::Codex, "1.0.0");
    fixture.latest.says(Harness::Codex, "1.0.0");
    let first = fixture.row("codex", false);
    fixture.time.settle_at(EPOCH_MS + 299_999);
    assert!(
        Rc::ptr_eq(&first, &fixture.row("codex", false)),
        "still kept"
    );
    fixture.time.settle_at(EPOCH_MS + 300_000);
    let second = fixture.row("codex", false);
    assert!(!Rc::ptr_eq(&first, &second), "five minutes are over");
    assert_eq!(second.checked_at, EPOCH_MS + 300_000);
}

#[test]
fn a_look_is_dated_when_it_began_and_kept_from_then() {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture
        .capture
        .answer("codex --version", [Response::Held, says("2.0.0")]);
    fixture.latest.says(Harness::Codex, "2.0.0");
    fixture.latest.says(Harness::Codex, "2.0.0");
    let mut driver = Driver::default();
    let admin = Rc::clone(&fixture.admin);
    driver.begin(0, async move { admin.check(Some("codex"), false).await });
    assert!(driver.run().is_empty(), "it waits for the version");
    fixture.time.settle_at(EPOCH_MS + 200_000);
    assert!(fixture
        .capture
        .release("codex --version", Ok(captured("1.0.0"))));
    let settled = driver.run();
    let row = Rc::clone(&settled[0].1.as_ref().unwrap()[0]);
    assert_eq!(row.checked_at, EPOCH_MS, "dated by when it began");
    fixture.time.settle_at(EPOCH_MS + 299_999);
    assert!(Rc::ptr_eq(&row, &fixture.row("codex", false)));
    fixture.time.settle_at(EPOCH_MS + 300_000);
    assert!(
        !Rc::ptr_eq(&row, &fixture.row("codex", false)),
        "kept five minutes from the start, not from the end"
    );
}

/// What a program that ended with 0 wrote.
fn captured(stdout: &str) -> Captured {
    Captured {
        stdout: stdout.to_owned(),
        stderr: String::new(),
    }
}

#[test]
fn calls_that_ask_while_a_look_runs_share_it_a_refresh_too() {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture.capture.answer("codex --version", [Response::Held]);
    fixture.latest.says(Harness::Codex, "1.0.1");
    let mut driver = Driver::default();
    for (work, refresh) in [(0, false), (1, false), (2, true)] {
        let admin = Rc::clone(&fixture.admin);
        driver.begin(
            work,
            async move { admin.check(Some("codex"), refresh).await },
        );
    }
    assert!(driver.run().is_empty());
    assert_eq!(
        fixture.capture.take_ran().len(),
        1,
        "one look for the three"
    );
    assert!(fixture
        .capture
        .release("codex --version", Ok(captured("1.0.0"))));
    let settled = driver.run();
    assert_eq!(settled.len(), 3);
    let first = &settled[0].1.as_ref().unwrap()[0];
    for (_, rows) in &settled {
        assert!(
            Rc::ptr_eq(first, &rows.as_ref().unwrap()[0]),
            "the same row"
        );
    }
    assert_eq!(fixture.latest.take_asked().len(), 1);
}

#[test]
fn a_look_that_ended_is_forgotten_so_a_refresh_looks_again() {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture
        .capture
        .answer("codex --version", [says("1.0.0"), says("1.0.0")]);
    fixture.latest.says(Harness::Codex, "1.0.0");
    fixture.latest.says(Harness::Codex, "1.0.0");
    fixture.row("codex", false);
    assert!(fixture.admin.inner.pending.borrow().is_empty());
    fixture.row("codex", true);
    assert_eq!(fixture.capture.take_ran().len(), 2);
}

#[test]
fn a_look_given_up_leaves_no_look_running_for_the_next_call_to_wait_on() {
    let fixture = Fixture::new();
    fixture.install("bin", "codex");
    fixture
        .capture
        .answer("codex --version", [Response::Held, says("1.0.0")]);
    fixture.latest.says(Harness::Codex, "1.0.0");
    let mut driver = Driver::default();
    let admin = Rc::clone(&fixture.admin);
    driver.begin(0, async move { admin.check(Some("codex"), false).await });
    assert!(driver.run().is_empty());
    assert_eq!(fixture.admin.inner.pending.borrow().len(), 1);
    drop(driver);
    assert!(fixture.admin.inner.pending.borrow().is_empty());
    let row = fixture.row("codex", false);
    assert_eq!(row.version.value(), Some("1.0.0"), "looked at afresh");
}

#[test]
fn a_cli_that_moved_or_went_is_looked_at_again_whatever_the_time() {
    let fixture = Fixture::new();
    let first = fixture.install("bin", "codex");
    let second = fixture.install("later", "codex");
    let both = std::env::join_paths([first.parent().unwrap(), second.parent().unwrap()]).unwrap();
    let env = Env::from_vars(
        fixture
            .env
            .iter()
            .filter(|(name, _)| *name != "PATH")
            .map(|(name, value)| (name.to_owned(), value.to_owned()))
            .chain([("PATH".into(), both)]),
    );
    let admin = Rc::new(HarnessAdmin::new(
        env,
        Rc::clone(&fixture.time) as Rc<dyn Time>,
        Rc::clone(&fixture.latest) as Rc<dyn Latest>,
        Rc::clone(&fixture.capture) as Rc<dyn Capture>,
    ));
    fixture.capture.answer(
        "codex --version",
        [says("1.0.0"), says("2.0.0"), says("3.0.0")],
    );
    for release in ["1.0.0", "2.0.0", "3.0.0"] {
        fixture.latest.says(Harness::Codex, release);
    }
    let look = |admin: &Rc<HarnessAdmin>| {
        finished(Box::pin(admin.check(Some("codex"), false)))
            .unwrap()
            .remove(0)
    };
    let before = look(&admin);
    assert_eq!(before.path.as_deref(), first.to_str());
    assert!(Rc::ptr_eq(&before, &look(&admin)), "kept while it stays");
    fs::remove_file(&first).unwrap();
    let moved = look(&admin);
    assert_eq!(moved.path.as_deref(), second.to_str());
    assert_eq!(moved.version.value(), Some("2.0.0"));
    fs::remove_file(&second).unwrap();
    let gone = look(&admin);
    assert_eq!(
        (gone.installed, gone.version.clone()),
        (false, Version::NotInstalled)
    );
    assert!(
        Rc::ptr_eq(&gone, &look(&admin)),
        "a missing CLI is kept too"
    );
}

#[test]
fn the_program_asked_for_a_version_runs_in_the_home_with_the_admin_s_environment() {
    let fixture = Fixture::new();
    let codex = fixture.install("bin", "codex");
    fixture.capture.answer("codex --version", [says("1.0.0")]);
    fixture.latest.says(Harness::Codex, "1.0.0");
    fixture.row("codex", false);
    let ran = fixture.capture.take_ran();
    let [(program, limits)] = &ran[..] else {
        panic!("one program: {ran:?}")
    };
    assert_eq!(program.executable, codex);
    assert_eq!(program.args, ["--version"]);
    assert_eq!(program.cwd.as_deref(), fixture.env.path("HOME"));
    let given: Vec<_> = program.env.iter().collect();
    assert_eq!(given, fixture.env.iter().collect::<Vec<_>>());
    assert_eq!(
        *limits,
        Limits {
            timeout: std::time::Duration::from_millis(3000),
            max_buffer: 8192
        }
    );
    assert_eq!(named(program), "codex --version");
}
