use std::time::Duration;

use super::*;

const APP: &str = "/box/Applications/ConsensFlow.app";

fn node_daemon() -> String {
    format!("{APP}/Contents/MacOS/node {APP}/Contents/Resources/cli/bin/cf.mjs ui --json --no-open")
}

fn native_daemon() -> String {
    format!("{APP}/Contents/Resources/cli/bin/cf ui --json --no-open")
}

fn row(pid: u32, ppid: u32, command: &str) -> Row {
    Row {
        pid,
        ppid,
        state: "S".into(),
        command: command.into(),
    }
}

fn node_start(pid: u32) -> String {
    format!("2026-10-07T05:33:49.964Z info start pid {pid} node v26.7.0 home /box/state\n")
}

fn rust_start(pid: u32) -> String {
    format!("2026-10-07T05:33:52.788Z info start pid {pid} rust 3.0.0-alpha.81 home /box/state\n")
}

fn refused(pid: u32) -> String {
    format!("{}2026-10-07T05:33:52.790Z info exit 1\n", rust_start(pid))
}

fn app_row() -> Row {
    row(100, 1, &format!("{APP}/Contents/MacOS/app"))
}

/// What the daemon of the app at `APP`, pid 100, is found to be.
fn found(log: &str, table: &[Row], probes: &[u32]) -> Result<Daemon> {
    daemon_evidence(&Seen {
        log,
        table,
        app: 100,
        bundle: APP,
        probes,
    })
}

fn refuses(log: &str, table: &[Row], probes: &[u32], words: &str) {
    let said = found(log, table, probes).unwrap_err().to_string();
    assert!(said.contains(words), "{words:?} is not in {said}");
}

#[test]
fn is_nodes_daemon_when_the_apps_child_runs_cf_mjs_and_the_log_says_node() {
    let daemon = found(
        &node_start(200),
        &[app_row(), row(200, 100, &node_daemon())],
        &[],
    )
    .unwrap();
    assert_eq!(
        (daemon.pid, daemon.kind, daemon.runtime.as_str()),
        (200, Kind::Node, "node v26.7.0")
    );
}

#[test]
fn is_the_native_daemon_when_the_apps_child_runs_cf_and_the_log_says_rust() {
    let daemon = found(
        &rust_start(300),
        &[app_row(), row(300, 100, &native_daemon())],
        &[],
    )
    .unwrap();
    assert_eq!((daemon.pid, daemon.kind), (300, Kind::Native));
    assert_eq!(daemon.runtime, "rust 3.0.0-alpha.81");
    assert_eq!(daemon.command, native_daemon());
}

#[test]
fn takes_the_daemon_an_update_started_after_the_one_it_replaced_whose_start_line_is_still_in_the_log(
) {
    let log = node_start(200) + &rust_start(300);
    let daemon = found(&log, &[app_row(), row(300, 100, &native_daemon())], &[]).unwrap();
    assert_eq!(daemon.pid, 300);
}

#[test]
fn refuses_an_app_with_no_daemon_of_its_own_running_nothing_is_ready_yet() {
    refuses(&node_start(200), &[app_row()], &[], "has 0 daemons");
    refuses(&node_start(200), &[app_row()], &[], "and these run: none");
}

#[test]
fn refuses_a_daemon_that_is_not_the_apps_child() {
    refuses(
        &node_start(200),
        &[app_row(), row(200, 1, &node_daemon())],
        &[],
        "has 0 daemons",
    );
    // What runs is said, whose it is and what it runs.
    refuses(
        &node_start(200),
        &[app_row(), row(200, 1, &node_daemon())],
        &[],
        &format!("200 (parent 1) {}", node_daemon()),
    );
}

#[test]
fn refuses_a_daemon_of_another_bundle_it_is_not_this_apps() {
    let elsewhere = node_daemon().replace(APP, "/elsewhere/ConsensFlow.app");
    refuses(
        &node_start(200),
        &[app_row(), row(200, 100, &elsewhere)],
        &[],
        "has 0 daemons",
    );
}

#[test]
fn refuses_a_home_with_a_second_daemon_running_one_daemon_holds_the_ledger() {
    refuses(
        &(node_start(200) + &node_start(201)),
        &[
            app_row(),
            row(200, 100, &node_daemon()),
            row(201, 77, &node_daemon()),
        ],
        &[],
        "one daemon serves the home",
    );
}

#[test]
fn refuses_a_daemon_that_never_logged_its_start() {
    let table = [app_row(), row(200, 100, &node_daemon())];
    refuses("", &table, &[], "logged no start line");
    refuses(&node_start(999), &table, &[], "logged no start line");
}

#[test]
fn refuses_a_log_that_says_one_daemon_where_the_process_is_the_other() {
    refuses(
        &rust_start(200),
        &[app_row(), row(200, 100, &node_daemon())],
        &[],
        "the log says rust",
    );
}

#[test]
fn refuses_a_daemon_that_logged_an_error() {
    let log = format!(
        "{}2026-10-07T05:33:53.000Z error the agents file could not be used\n",
        rust_start(300)
    );
    refuses(
        &log,
        &[app_row(), row(300, 100, &native_daemon())],
        &[],
        "logged errors",
    );
}

#[test]
fn refuses_an_earlier_daemon_of_the_app_that_was_refused_its_start_and_not_a_probes_refusal() {
    let log = refused(250) + &rust_start(300);
    let table = [app_row(), row(300, 100, &native_daemon())];
    refuses(&log, &table, &[], "failed to start");
    // A second ConsensFlow the smoke tried is refused by design.
    assert_eq!(found(&log, &table, &[250]).unwrap().pid, 300);
}

#[test]
fn reads_a_process_by_its_command_nodes_daemon_runs_cf_mjs_the_native_one_is_cf() {
    assert_eq!(kind_of_command(&node_daemon()), Kind::Node);
    assert_eq!(kind_of_command(&native_daemon()), Kind::Native);
    let app = format!("{APP}/Contents/MacOS/app");
    assert_eq!(daemon_rows(&[row(1, 0, &app)], APP).len(), 0);
    // Words of their own: a program whose name ends with "ui" is no daemon of the app's.
    let table = [
        row(
            2,
            1,
            &format!("{APP}/Contents/Resources/cli/bin/cf ui --json --no-open"),
        ),
        row(
            3,
            1,
            &format!("{APP}/Contents/Resources/cli/bin/xui --json --no-open"),
        ),
        row(
            4,
            1,
            &format!("{APP}/Contents/Resources/cli/bin/cf ui --json --no-open now"),
        ),
    ];
    assert_eq!(
        daemon_rows(&table, APP)
            .iter()
            .map(|row| row.pid)
            .collect::<Vec<_>>(),
        [2]
    );
}

#[test]
fn takes_the_app_for_the_process_that_is_the_bundles_executable_and_no_other() {
    let binary = format!("{APP}/Contents/MacOS/app");
    assert_eq!(assert_app(&[app_row()], 100, &binary).unwrap().pid, 100);
    let said = |table: &[Row]| assert_app(table, 100, &binary).unwrap_err().to_string();
    assert!(said(&[]).contains("is not running"));
    assert!(said(&[row(100, 1, "/bin/sleep 5")]).contains("is not the app"));
    let zombie = Row {
        state: "Z".into(),
        ..app_row()
    };
    assert!(said(&[zombie]).contains("is not running"));
}

#[test]
fn reads_what_a_daemon_log_holds_its_starts_the_lines_of_each_and_the_errors_among_them() {
    let log = node_start(200)
        + "2026-10-07T05:33:50.000Z info something\n"
        + &rust_start(300)
        + "2026-10-07T05:33:53.000Z error trouble\n"
        + "    cause of it\n";
    assert_eq!(starts_of(&log), [200, 300]);
    assert_eq!(lines_of(&log, 200).len(), 2);
    assert_eq!(
        lines_of(&log, 300),
        [
            "2026-10-07T05:33:52.788Z info start pid 300 rust 3.0.0-alpha.81 home /box/state",
            "2026-10-07T05:33:53.000Z error trouble",
            "    cause of it",
        ]
    );
    assert_eq!(lines_of(&log, 5), Vec::<&str>::new());
    assert_eq!(
        error_lines(&lines_of(&log, 300)),
        ["2026-10-07T05:33:53.000Z error trouble"]
    );
    assert_eq!(error_lines(&lines_of(&log, 200)), Vec::<&str>::new());
}

#[test]
fn reads_the_daemon_from_the_runtime_its_start_line_names_whichever_wrote_it() {
    let node = start_line(&node_start(4242), Some(4242)).unwrap();
    assert_eq!(
        (node.kind, node.runtime.as_str(), node.pid),
        (Kind::Node, "node v26.7.0", 4242)
    );
    assert_eq!(node.line, node_start(4242).trim_end());
    let rust = start_line(&rust_start(4242), Some(4242)).unwrap();
    assert_eq!(
        (rust.kind, rust.runtime.as_str()),
        (Kind::Native, "rust 3.0.0-alpha.81")
    );
}

#[test]
fn reads_the_start_line_of_the_process_it_is_asked_about_the_last_one_a_log_holds_for_it() {
    let log = [
        node_start(4242).trim_end(),
        "2026-10-06T10:00:01.000Z info stop: stdin ended; rss 90 MB",
        "2026-10-06T10:01:00.000Z info start pid 77 rust 3.0.0 home /tmp/consensflow",
        "2026-10-06T10:02:00.000Z info start pid 4242 rust 3.0.0 home /tmp/consensflow",
        "",
    ]
    .join("\n");
    assert_eq!(start_line(&log, Some(77)).unwrap().kind, Kind::Native);
    assert_eq!(
        start_line(&log, Some(4242)).unwrap().kind,
        Kind::Native,
        "a pid that started twice is the later start"
    );
    assert_eq!(
        start_line(&log, None).unwrap().pid,
        4242,
        "with no pid, the last start"
    );
    assert_eq!(start_line(&log, Some(5)), None);
    assert_eq!(start_line("", Some(4242)), None);
}

#[test]
fn takes_a_line_for_a_start_only_when_it_is_one() {
    for log in [
        "2026-10-06T10:00:00.000Z info alive: 3 passes, node v26.8.1 rust 1\n",
        "2026-10-06T10:00:00.000Z error start pid 4242 node v26.8.1 home /h\n",
        "2026-10-06T10:00:00.000Z info start pid 4242 go1.26 home /h\n",
        "  2026-10-06T10:00:00.000Z info start pid 4242 node v26.8.1 home /h\n",
        "2026-10-06T10:00:00.000Z info start pid 4242 node v home /h\n",
        "2026-10-06T10:00:00.000Z info start pid 4242 node v26.8.1\n",
        "2026-10-06T10:00:00.000Z info start pid x node v26.8.1 home /h\n",
    ] {
        assert_eq!(start_line(log, Some(4242)), None, "{log}");
    }
}

#[test]
fn takes_an_error_only_when_it_is_the_level_of_a_line() {
    let lines = [
        "2026-10-07T05:33:53.000Z error trouble",
        "2026-10-07T05:33:53.000Z info an error happened",
        "error is the first word",
        "2026-10-07T05:33:53.000Z errors are not the level",
        " 2026-10-07T05:33:53.000Z error with a space before",
    ];
    assert_eq!(error_lines(&lines), [lines[0]]);
}

mod the_apps_log {
    use super::*;

    // Paths are made as the platform makes them: the app writes them so.
    fn cf() -> std::path::PathBuf {
        [
            "/box",
            "Applications",
            "ConsensFlow.app",
            "Contents",
            "Resources",
            "cli",
            "bin",
            "cf",
        ]
        .iter()
        .collect()
    }

    fn said(cf: &Path) -> String {
        format!(
            "consensflow: starting the daemon: {} ui --json --no-open",
            cf.display()
        )
    }

    const FLIPS: &str =
        "consensflow: starting the native daemon: the default, there is no /box/state/use-node";

    #[test]
    fn says_the_bundles_cf_by_its_path() {
        assert_started_daemon(&format!("{}\n", said(&cf())), &cf()).unwrap();
        // The log is written on by every app of the home: the line of the one that started it is among them.
        assert_started_daemon(
            &format!("{FLIPS}\n{}\nconsensflow: something else\n", said(&cf())),
            &cf(),
        )
        .unwrap();
    }

    #[test]
    fn does_not_take_the_flips_sentence_another_bundles_cf_or_silence() {
        let other: std::path::PathBuf = [
            "/box",
            "Other.app",
            "Contents",
            "Resources",
            "cli",
            "bin",
            "cf",
        ]
        .iter()
        .collect();
        for log in [
            String::new(),
            format!("{FLIPS}\n"),
            "consensflow: starting Node's daemon: /box/state/use-node is there, the way back to Node\n"
                .to_string(),
            format!("{}\n", said(&other)),
            "consensflow: the terminal command is not repaired\n".to_string(),
        ] {
            let refused = assert_started_daemon(&log, &cf()).unwrap_err().to_string();
            assert!(refused.contains("does not say the app started"), "{log}: {refused}");
        }
    }

    #[test]
    fn shows_the_end_of_a_long_log_when_it_does_not_say_it() {
        let long = format!("{}\nthe end\n", "x".repeat(5000));
        let refused = assert_started_daemon(&long, &cf()).unwrap_err().to_string();
        assert!(refused.ends_with("the end\n"), "{refused}");
        assert!(refused.len() < 2600, "{}", refused.len());
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
        assert_eq!(tail("", 3), "");
        assert_eq!(tail("héllo wörld", 5), "wörld");
    }
}

mod the_daemons_of_a_home_once_every_app_is_gone {
    use super::*;

    fn log() -> String {
        node_start(200) + &rust_start(300) + &refused(250)
    }

    #[test]
    fn were_refused_nothing_but_the_probes_the_smoke_tried() {
        assert_only_probes_refused(&log(), &BTreeSet::from([250])).unwrap();
        assert_only_probes_refused(&(node_start(200) + &rust_start(300)), &BTreeSet::new())
            .unwrap();
    }

    #[test]
    fn say_so_when_a_daemon_the_app_started_was_refused_the_ledger_or_a_probe_was_not() {
        let said = assert_only_probes_refused(&log(), &BTreeSet::new()).unwrap_err();
        assert!(
            said.to_string().contains("were refused the ledger"),
            "{said}"
        );
        let said = assert_only_probes_refused(
            &(node_start(200) + &refused(250)),
            &BTreeSet::from([250, 251]),
        )
        .unwrap_err();
        assert!(
            said.to_string().contains("were refused the ledger"),
            "{said}"
        );
    }
}

#[test]
#[cfg_attr(not(unix), ignore = "the process table is ps's")]
fn waits_for_the_daemon_of_the_app_and_says_what_was_seen_last_when_it_never_comes() {
    use crate::updater_smoke::sandbox::Sandbox;
    let parent = tempfile::tempdir().unwrap();
    let sandbox = Sandbox::make(parent.path()).unwrap();
    let waits = Waits::new(Duration::from_millis(300));
    let said = daemon_of(&sandbox, &waits, 1, Path::new(APP), &BTreeSet::new())
        .unwrap_err()
        .to_string();
    assert!(
        said.starts_with(
            "the daemon of pid 1 is running and logged its start did not happen within 300 ms: "
        ),
        "{said}"
    );
    assert!(said.contains("the app (pid 1) has 0 daemons"), "{said}");
    // The logs are the home's: nothing there yet is no text.
    assert_eq!(
        (daemon_log(&sandbox), app_log(&sandbox)),
        (String::new(), String::new())
    );
    fs::create_dir_all(sandbox.state.join("app")).unwrap();
    fs::write(sandbox.state.join("daemon.log"), "d").unwrap();
    fs::write(sandbox.state.join("app").join("app.log"), "a").unwrap();
    assert_eq!(
        (daemon_log(&sandbox), app_log(&sandbox)),
        ("d".into(), "a".into())
    );
}
