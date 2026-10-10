//! The app as the smoke starts it, with a stand-in for it: a script that reports
//! as the page does and reads its input until it ends.
#![cfg(unix)]

use std::fs;
use std::time::Duration;

use super::*;
use crate::updater_smoke::say::Line;
use crate::updater_smoke::testing::stand_in_for_the_app;

struct Stand {
    folder: tempfile::TempDir,
    say: Say,
    lines: std::sync::mpsc::Receiver<Line>,
}

impl Stand {
    fn new() -> Self {
        let (say, lines) = Say::channel();
        Self {
            folder: tempfile::tempdir().unwrap(),
            say,
            lines,
        }
    }

    /// Starts a stand-in whose body is `script`, expecting `expected` failures.
    fn start(&self, script: &str, expected: &[&str]) -> App {
        let binary = self.folder.path().join("app");
        stand_in_for_the_app(&binary, script);
        let cwd = fs::canonicalize(self.folder.path()).unwrap();
        launch_app(
            &Launch {
                binary: &binary,
                env: &Env::from_vars([("PATH", "/usr/bin:/bin")]),
                cwd: &cwd,
                expected,
                waits: Waits::new(Duration::from_secs(30)),
            },
            &self.say,
        )
        .unwrap()
    }

    /// The lines said so far.
    fn said(&self) -> Vec<Line> {
        self.lines.try_iter().collect()
    }
}

/// Ends what a test started, whatever the test found.
struct Kills(App);

impl Drop for Kills {
    fn drop(&mut self) {
        self.0.kill_recorded();
    }
}

#[test]
fn keeps_the_reports_of_the_page_says_them_as_they_come_and_nothing_else_the_app_writes() {
    let stand = Stand::new();
    let app = stand.start(
        "say update-boot '{\"currentVersion\":\"1.0.0\"}'\necho an ordinary line of output\necho trouble >&2\nreads",
        &[],
    );
    let _kills = Kills(app.clone());
    let boot = app
        .wait_for("the boot", |event| {
            event.name() == "update-boot" && event.data("currentVersion") == Some(&json!("1.0.0"))
        })
        .unwrap();
    assert!(boot.pid().is_some_and(|pid| pid > 1));
    assert_eq!(
        app.events(),
        std::slice::from_ref(&boot),
        "a line that is no report is none"
    );
    assert_eq!(
        stand.said(),
        [Line::Out(format!("updater probe {}", boot.json()))]
    );
    // What it says on its error stream is kept for a failure to show.
    let stderr = || locked(&app.0.reports).stderr.clone();
    crate::updater_smoke::processes::until(
        "the app's error stream is read",
        Duration::from_secs(60),
        || Ok(stderr().contains("trouble").then_some(())),
    )
    .unwrap();
    assert_eq!(stderr(), "trouble\n");
}

#[test]
fn the_apps_input_is_a_fifo_whose_end_is_the_quit_both_the_app_and_this_side_read() {
    let stand = Stand::new();
    let app = stand.start("say update-boot '{}'\nreads", &[]);
    let _kills = Kills(app.clone());
    app.wait_for("the boot", |event| event.name() == "update-boot")
        .unwrap();
    assert!(app.any_alive());
    // Told to go on, the app reads the line the page's blocked install waits for.
    app.continue_updater().unwrap();
    let got = app
        .wait_for("the line", |event| event.name() == "got")
        .unwrap();
    assert_eq!(got.data("line"), Some(&json!("continue-updater")));
    // Its quit is the end of its input.
    app.close_input();
    crate::updater_smoke::processes::until(
        "every app process exits",
        Duration::from_secs(60),
        || Ok((!app.any_alive()).then_some(())),
    )
    .unwrap();
    assert_eq!(
        app.exited().unwrap(),
        Exit {
            code: Some(0),
            signal: None
        }
    );
    assert!(app
        .exited()
        .unwrap_err()
        .to_string()
        .contains("asked for twice"));
    assert!(app
        .continue_updater()
        .unwrap_err()
        .to_string()
        .contains("input is closed"));
}

#[test]
fn the_input_outlives_the_process_the_updater_restarts_as_another() {
    let stand = Stand::new();
    // The original reports and starts the restarted app, which inherits its input, and ends.
    let app = stand.start(
        "if [ \"${1:-}\" != again ]; then
  say update-boot '{\"currentVersion\":\"1.0.0\"}'
  \"$0\" again <&0 &
  exit 0
fi
say update-restarted '{\"currentVersion\":\"2.0.0\"}'
reads",
        &[],
    );
    let _kills = Kills(app.clone());
    let first = app
        .wait_for("the boot", |event| event.name() == "update-boot")
        .unwrap();
    let second = app
        .wait_for("the restart", |event| event.name() == "update-restarted")
        .unwrap();
    assert_ne!(first.pid(), second.pid(), "another process");
    assert_eq!(
        app.exited().unwrap().code,
        Some(0),
        "the original has ended"
    );
    // What is left is the restarted one, which the page's pid says is the app too.
    assert!(app.any_alive());
    app.continue_updater().unwrap();
    let got = app
        .wait_for("the line", |event| event.name() == "got")
        .unwrap();
    assert_eq!(got.pid(), second.pid());
    app.close_input();
    crate::updater_smoke::processes::until(
        "the restarted app quits",
        Duration::from_secs(60),
        || Ok((!app.any_alive()).then_some(())),
    )
    .unwrap();
}

#[test]
fn a_failure_the_page_reports_ends_the_wait_unless_the_case_is_about_it() {
    let stand = Stand::new();
    let script = "say update-failure '{\"error\":\"the update feed is unavailable\"}'\necho trouble >&2\nreads";

    let unexpected = stand.start(script, &[]);
    let _kills = Kills(unexpected.clone());
    let said = unexpected
        .wait_for("a boot", |event| event.name() == "update-boot")
        .unwrap_err()
        .to_string();
    assert!(
        said.starts_with("packaged app reported failure: {\"event\":\"update-failure\""),
        "{said}"
    );
    assert!(said.contains("the update feed is unavailable"), "{said}");
    assert!(said.contains("\nstderr: "), "{said}");

    // Waited for like any report, once the case says it is about it.
    let expected = stand.start(script, &["update-failure"]);
    let _more_kills = Kills(expected.clone());
    let failure = expected
        .wait_for("the refusal", |event| event.name() == "update-failure")
        .unwrap();
    assert_eq!(
        failure.data("error"),
        Some(&json!("the update feed is unavailable"))
    );
    // and nothing else it reports fails, so that waiting for something that never comes times out.
    let waits = Waits::new(Duration::from_millis(300));
    let patient = App(Arc::new(Inner {
        reports: Arc::clone(&expected.0.reports),
        child: Mutex::new(None),
        control: Mutex::new(None),
        waits,
    }));
    let said = patient
        .wait_for("a boot", |event| event.name() == "update-boot")
        .unwrap_err()
        .to_string();
    assert_eq!(said, "a boot did not happen within 300 ms");
}

#[test]
fn a_failure_the_case_did_not_expect_is_one_of_the_five_the_page_reports() {
    for name in [
        "update-failure",
        "page-error",
        "page-rejection",
        "failed",
        "deadline",
    ] {
        let stand = Stand::new();
        let app = stand.start(&format!("say {name} '{{}}'\nreads"), &[]);
        let _kills = Kills(app.clone());
        let said = app.wait_for("nothing", |_| false).unwrap_err().to_string();
        assert!(said.contains(&format!("\"event\":\"{name}\"")), "{said}");
    }
    // Any other report is none.
    let stand = Stand::new();
    let app = stand.start("say update-blocked '{}'\nreads", &[]);
    let _kills = Kills(app.clone());
    assert!(app
        .wait_for("the report", |event| event.name() == "update-blocked")
        .is_ok());
}

#[test]
fn a_line_that_is_a_report_and_not_json_is_a_failure_of_its_own() {
    let stand = Stand::new();
    let app = stand.start("echo 'consensflow-selftest {broken'\nreads", &[]);
    let _kills = Kills(app.clone());
    let said = app
        .wait_for("a boot", |event| event.name() == "update-boot")
        .unwrap_err()
        .to_string();
    assert!(said.contains("\"event\":\"malformed-report\""), "{said}");
    assert!(said.contains("consensflow-selftest {broken"), "{said}");
}

#[test]
fn a_report_that_settles_the_wait_the_other_way_says_what_it_means() {
    let stand = Stand::new();
    let app = stand.start(
        "say update-restarted '{\"currentVersion\":\"2.0.0\"}'\nreads",
        &[],
    );
    let _kills = Kills(app.clone());
    let said = app
        .wait_for_unless(
            "the refusal",
            |event| event.name() == "update-failure",
            |event| {
                (event.name() == "update-restarted")
                    .then_some("the app took an update it should have refused")
            },
        )
        .unwrap_err()
        .to_string();
    assert!(
        said.starts_with(
            "the app took an update it should have refused: {\"event\":\"update-restarted\""
        ),
        "{said}"
    );
}

#[test]
fn every_process_the_app_has_been_is_ended_with_what_is_under_it() {
    let stand = Stand::new();
    let marker = stand.folder.path().join("sleeping");
    let app = stand.start(
        &format!(
            "sleep 300 &\necho $! > '{}'\nsay update-boot '{{}}'\nreads",
            marker.display()
        ),
        &[],
    );
    let _kills = Kills(app.clone());
    app.wait_for("the boot", |event| event.name() == "update-boot")
        .unwrap();
    let sleeping: u32 = crate::updater_smoke::processes::until(
        "the sleep is started",
        Duration::from_secs(60),
        || {
            Ok(fs::read_to_string(&marker)
                .ok()
                .and_then(|text| text.trim().parse().ok()))
        },
    )
    .unwrap();
    assert!(alive(sleeping));
    assert!(app.any_alive());

    app.kill_recorded();

    crate::updater_smoke::processes::until("all of it is gone", Duration::from_secs(60), || {
        Ok((!app.any_alive() && !alive(sleeping)).then_some(()))
    })
    .unwrap();
    // The process that was started ended by a signal, which is said.
    assert_eq!(app.exited().unwrap().signal, Some(9));
}

#[test]
fn a_report_names_a_pid_only_when_it_says_one_that_is_a_process() {
    let event = |text: &str| Event(serde_json::from_str(text).unwrap());
    assert_eq!(
        event(r#"{"event":"a","pid":42,"data":{"k":1}}"#).pid(),
        Some(42)
    );
    for text in [
        r#"{"event":"a"}"#,
        r#"{"event":"a","pid":0}"#,
        r#"{"event":"a","pid":-5}"#,
        r#"{"event":"a","pid":"42"}"#,
        r#"{"event":"a","pid":1.5}"#,
        r#"{"event":"a","pid":99999999999}"#,
    ] {
        assert_eq!(event(text).pid(), None, "{text}");
    }
    let named = event(r#"{"event":"update-boot","data":{"currentVersion":"1.0.0"}}"#);
    assert_eq!(named.name(), "update-boot");
    assert_eq!(named.data("currentVersion"), Some(&json!("1.0.0")));
    assert_eq!(named.data("other"), None);
    assert_eq!(event("{}").name(), "");
    assert_eq!(event("{}").data("x"), None);
    assert_eq!(
        named.json(),
        r#"{"event":"update-boot","data":{"currentVersion":"1.0.0"}}"#
    );
}
