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
    let said = String::from_utf8(said).unwrap();
    assert!(
        !said.contains("hunter2hunter") && !said.contains("abcd1234efgh"),
        "{said}"
    );
    assert!(!log.to_string().contains("hunter2hunter"));
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
fn what_stopped_the_run_leaves_through_the_edge_a_usage_a_usage_still() {
    let edge = edge();
    let failed = edge.failure(Error::Refused(
        "codex mcp list said: password=hunter2hunter {\"servers\":[]}".to_owned(),
    ));
    assert_eq!(
        failed.to_string(),
        "codex mcp list said: password=[masked] {\"servers\":[]}"
    );
    assert!(matches!(
        edge.failure(Error::Usage("unknown flag: --token=abcd1234efgh".to_owned())),
        Error::Usage(said) if said == "unknown flag: --token=[masked]"
    ));
}

/// What the windows of a rig printed, as a test has them print: each window's text.
#[derive(Clone, Default)]
struct Windows(Arc<Mutex<BTreeMap<String, String>>>);

impl Windows {
    /// The window `pane` prints `text`.
    fn print(&self, pane: &str, text: &str) {
        self.0
            .lock()
            .unwrap()
            .entry(pane.to_owned())
            .or_default()
            .push_str(text);
    }
}

impl Streams for Windows {
    fn panes(&self) -> Vec<(String, usize)> {
        let printed = self.0.lock().unwrap();
        printed
            .iter()
            .map(|(pane, text)| (pane.clone(), text.len()))
            .collect()
    }

    fn screen(&self, pane: &str) -> String {
        self.0
            .lock()
            .unwrap()
            .get(pane)
            .cloned()
            .unwrap_or_default()
    }
}

/// Astra's window: a secret's name, its value, and eleven lines of status after; and
/// the last twelve lines of its screen, which begin at the value.
fn printed_with_a_password() -> (String, Vec<String>) {
    let status: String = (1..=11)
        .map(|step| format!("step {step} of 11 done\n"))
        .collect();
    let printed = format!("$ export DB_PASSWORD=\n\"correct horse battery staple\"\n{status}");
    let tail = printed.lines().skip(1).map(str::to_owned).collect();
    (printed, tail)
}

#[test]
fn a_screen_leaves_in_the_light_of_all_its_window_printed_and_a_line_of_words_reads_nothing() {
    let edge = edge();
    let windows = Windows::default();
    edge.reads(windows.clone());
    let (printed, tail) = printed_with_a_password();
    windows.print("p1-long-claude-1-olive-valley", &printed);
    // A line of the run's own words is said with what was learned so far: nothing yet.
    let value = "the value: correct horse battery staple";
    assert_eq!(edge.shown(value), value);
    // A screen is masked once every window is read whole.
    let shown = edge.screen(&tail);
    assert_eq!(shown[0], "\"[masked]\"");
    assert_eq!(shown[1..], tail[1..]);
    assert_eq!(edge.shown(value), "the value: [masked]");
    // A window that printed more is read again.
    windows.print(
        "p1-long-claude-1-olive-valley",
        "export API_TOKEN=\n'abcd1234 efgh5678'\n",
    );
    assert_eq!(
        edge.screen(&["'abcd1234 efgh5678'".to_owned()]),
        ["'[masked]'"]
    );
}

#[test]
fn the_report_and_what_the_rig_says_of_itself_leave_in_the_light_of_all_the_windows_printed() {
    let (printed, tail) = printed_with_a_password();
    // What the rig says of itself.
    let edge = edge();
    let windows = Windows::default();
    edge.reads(windows.clone());
    windows.print("p1-chief", &printed);
    let shown = edge.masker().screen(&tail);
    assert_eq!(shown[0], "\"[masked]\"");
    // The report kept.
    let edge = self::edge();
    let windows = Windows::default();
    edge.reads(windows.clone());
    windows.print("p1-chief", &printed);
    let folder = tempfile::tempdir().unwrap();
    let value = json!({ "breaks": [{ "evidence": { "screen": tail } }] });
    let (json_file, text_file) = edge
        .keep(
            folder.path(),
            "long",
            &value,
            "the value: correct horse battery staple",
        )
        .unwrap();
    let kept: Value = serde_json::from_str(&files::read_string(&json_file).unwrap()).unwrap();
    assert_eq!(kept["breaks"][0]["evidence"]["screen"][0], "\"[masked]\"");
    assert_eq!(
        files::read_string(&text_file).unwrap(),
        "the value: [masked]"
    );
}

#[test]
fn what_stopped_the_run_leaves_in_the_light_of_all_the_windows_printed() {
    // A wait that gave up, quoting the last lines of the window's screen.
    let (printed, tail) = printed_with_a_password();
    let edge = edge();
    let windows = Windows::default();
    edge.reads(windows.clone());
    windows.print("p1-chief", &printed);
    let said = format!("gave up waiting for the chief; its screen: {}", tail[0]);
    let failed = edge.failure(Error::Refused(said.clone()));
    assert_eq!(
        failed.to_string(),
        "gave up waiting for the chief; its screen: \"[masked]\""
    );
    assert!(matches!(
        edge.failure(Error::Usage(said)),
        Error::Usage(said) if !said.contains("horse")
    ));
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
