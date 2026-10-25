//! What leaves a long run, through its edge: every text hidden where it looks like a
//! key or a token, the run's names and the names of its files shown.

use serde_json::json;

use super::*;

/// The names of a run whose worker is `long-claude-1-olive-valley`.
fn edge() -> Edge {
    let mut names = Names::default();
    names.insert("long-claude-1-olive-valley");
    names.insert("p1-long-claude-1-olive-valley");
    let edge = Edge::default();
    edge.knows(&names);
    edge
}

#[test]
fn a_text_leaves_with_a_key_hidden_and_the_runs_names_shown() {
    let edge = edge();
    assert_eq!(
        edge.shown("p1-long-claude-1-olive-valley said OPENAI_API_KEY=abcd1234efgh"),
        "p1-long-claude-1-olive-valley said OPENAI_API_KEY=[masked]"
    );
}

#[test]
fn the_console_says_and_keeps_through_the_edge_what_the_run_notes_and_says() {
    let edge = edge();
    let mut said = Vec::new();
    let mut floor = edge.floor(&mut said);
    floor
        .note("gave up waiting for the chief: its screen: export DB_PASSWORD=hunter2hunter")
        .unwrap();
    floor.say("report: token=abcd1234efgh").unwrap();
    // A screen it shows (the chief's, as a wait gives up) is masked as one text first.
    let rows = ["$ export DB_PASSWORD=", "\"correct horse battery staple\""].map(str::to_owned);
    assert_eq!(
        floor.screen(&rows),
        ["$ export DB_PASSWORD=", "\"[masked]\""]
    );
    let log = floor.log();
    drop(floor);
    // What it notes is kept for the report alone (a window's words among it): the
    // console says what the run says, and no more.
    let said = String::from_utf8(said).unwrap();
    assert_eq!(said, "report: token=[masked]\n");
    let log = log.to_string();
    assert!(
        log.contains("its screen: export DB_PASSWORD=[masked]"),
        "{log}"
    );
    assert!(!log.contains("hunter2hunter"), "{log}");
}

#[test]
fn the_report_is_kept_through_the_edge_each_text_of_it_and_a_screen_as_one_text() {
    let edge = edge();
    let folder = tempfile::tempdir().unwrap();
    let value = json!({
        "verdict": "FAIL",
        "error": "the trust failed: password=hunter2hunter",
        "breaks": [{
            "said": "@long-claude-1-olive-valley's window closed by itself",
            "evidence": {
                "rows": ["token=abcd1234efgh"],
                "screen": ["$ export DB_PASSWORD=", "\"correct horse battery staple\""],
            },
        }],
        "seconds": 840,
    });
    let (json_file, text_file) = edge
        .keep(
            &folder.path().join("long"),
            "2026-10-16T08-20-35-569Z-long-chief-claude-staff-claude-devin",
            &value,
            "# FAIL\n\n```\nexport DB_PASSWORD=hunter2hunter\n```\n",
        )
        .unwrap();
    let kept: Value = serde_json::from_str(&files::read_string(&json_file).unwrap()).unwrap();
    assert_eq!(kept["error"], "the trust failed: password=[masked]");
    let evidence = &kept["breaks"][0]["evidence"];
    assert_eq!(evidence["rows"][0], "token=[masked]");
    assert_eq!(
        evidence["screen"],
        json!(["$ export DB_PASSWORD=", "\"[masked]\""])
    );
    assert_eq!(
        kept["breaks"][0]["said"],
        "@long-claude-1-olive-valley's window closed by itself"
    );
    assert_eq!(kept["seconds"], 840);
    assert_eq!(
        files::read_string(&text_file).unwrap(),
        "# FAIL\n\n```\nexport DB_PASSWORD=[masked]\n```\n"
    );
    // Where the files are is said whole: their names are the run's own.
    let mut said = Vec::new();
    let mut floor = edge.floor(&mut said);
    floor
        .say(format!("  report: {}", json_file.display()))
        .unwrap();
    drop(floor);
    assert_eq!(
        String::from_utf8(said).unwrap(),
        format!("  report: {}\n", json_file.display())
    );
}

#[test]
fn what_stopped_the_run_is_kept_in_the_reports_folder_and_said_by_where_it_is() {
    // A wait that gave up says the chief's screen: a value whose name the screen's tail
    // cut off, which no mask knows, and a password it does.
    let edge = edge();
    let folder = tempfile::tempdir().unwrap();
    let failed = edge.failure(
        Error::Refused(
            "gave up waiting for the chief; its screen: \"correct horse battery staple\" password=hunter2hunter"
                .to_owned(),
        ),
        folder.path(),
    );
    let kept: Vec<PathBuf> = std::fs::read_dir(folder.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert!(kept[0].to_string_lossy().ends_with("-long-stopped.txt"));
    assert_eq!(
        failed.to_string(),
        format!(
            "the long run stopped before its report; what stopped it is kept in {}",
            kept[0].display()
        )
    );
    assert_eq!(
        files::read_string(&kept[0]).unwrap(),
        "gave up waiting for the chief; its screen: \"correct horse battery staple\" password=[masked]\n"
    );
    // A usage's error is the person's own words, said through the edge.
    assert!(matches!(
        edge.failure(
            Error::Usage("unknown flag: --token=abcd1234efgh".to_owned()),
            folder.path()
        ),
        Error::Usage(said) if said == "unknown flag: --token=[masked]"
    ));
}

#[test]
fn the_ledger_is_kept_whole_beside_the_report_and_said_by_where_it_is() {
    use rusqlite::Connection;
    let folder = tempfile::tempdir().unwrap();
    let home = folder.path().join("home-a1b2c3d4e5f6a7b8c9d0");
    files::make_dir(&home).unwrap();
    let file = home.join("consensflow.db");
    // A ledger in write-ahead mode, its last rows in the log and not yet in the file.
    let live = Connection::open(&file).unwrap();
    live.query_row("PRAGMA journal_mode = WAL", [], |_| Ok(()))
        .unwrap();
    live.execute_batch(
        "CREATE TABLE event (kind TEXT); INSERT INTO event VALUES ('window.activity');",
    )
    .unwrap();
    let reports = folder.path().join("reports");
    let edge = edge();
    let kept = edge
        .keep_ledger(
            &reports,
            "20261019-212400-long-chief-claude-staff-claude",
            &file,
        )
        .unwrap();
    assert_eq!(
        kept,
        reports.join("20261019-212400-long-chief-claude-staff-claude-ledger.db")
    );
    // One database, whole: no log beside it.
    assert!(!reports
        .join("20261019-212400-long-chief-claude-staff-claude-ledger.db-wal")
        .exists());
    let copy = Connection::open(&kept).unwrap();
    let kind: String = copy
        .query_row("SELECT kind FROM event", [], |row| row.get(0))
        .unwrap();
    assert_eq!(kind, "window.activity");
    // Where it is, the run's own name.
    let shown = edge.shown(&kept.display().to_string());
    assert_eq!(shown, kept.display().to_string());
    // A ledger there is none of is said.
    assert!(edge
        .keep_ledger(&reports, "other", &home.join("none.db"))
        .is_err());
}

#[test]
fn the_masker_the_rig_is_given_masks_a_screen_as_one_text() {
    // The secret's name ends one row and its value is the next: each row told apart
    // would keep the value.
    let rows = ["$ export DB_PASSWORD=", "\"correct horse battery staple\""].map(str::to_owned);
    assert_eq!(
        edge().masker().screen(&rows),
        ["$ export DB_PASSWORD=", "\"[masked]\""]
    );
}

#[test]
fn what_the_rig_says_of_itself_goes_through_the_edge_with_the_names_known_by_then() {
    let edge = Edge::default();
    let masker = edge.masker();
    assert_eq!(
        masker.shown("--- p1-long-claude-1-olive-valley: DB_PASSWORD=hunter2hunter"),
        "--- [masked]: DB_PASSWORD=[masked]"
    );
    // Names learned after the masker was given are shown by it.
    let mut names = Names::default();
    names.insert("p1-long-claude-1-olive-valley");
    edge.knows(&names);
    assert_eq!(
        masker.shown("--- p1-long-claude-1-olive-valley: DB_PASSWORD=hunter2hunter"),
        "--- p1-long-claude-1-olive-valley: DB_PASSWORD=[masked]"
    );
}
