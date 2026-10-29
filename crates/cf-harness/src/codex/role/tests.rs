//! A Codex window's role: the dialogue with its app-server, in what is
//! written to it and read from it, and what the instructions it gives are
//! made into.

use std::rc::Rc;

use cf_base::env::Env;
use serde_json::json;

use super::*;
use crate::codex::fakes::Watching;
use crate::seams::processes::Processes;
use crate::testing::{finished, ChildScript, Driver, Ends, Fakes, EPOCH_MS};

const CODEX: &str = "/usr/local/bin/codex";
const INITIALIZED: &str = r#"{"id":1,"result":{}}"#;

/// App-server and clock of a window's role, as the services give them.
struct Stage {
    fakes: Fakes,
    watching: Rc<Watching>,
    services: Services,
}

impl Stage {
    fn new() -> Self {
        let env = Env::from_vars([("HOME", "/home/me")]);
        let fakes = Fakes::new(&env);
        let watching = Rc::new(Watching::default());
        let mut services = fakes.services(&env, std::path::Path::new("/nowhere"));
        services.processes = Rc::clone(&watching) as Rc<dyn Processes>;
        Self {
            fakes,
            watching,
            services,
        }
    }

    /// An app-server that says `lines` and ends as `ends` says.
    fn app_server(&self, lines: &[&str], ends: Ends) {
        let lines = lines.iter().map(|line| (*line).to_owned()).collect();
        self.watching
            .scripted
            .child("codex", ChildScript { lines, ends });
    }

    /// The configuration read of an app-server that gives `instructions`.
    fn configured(instructions: &str) -> String {
        json!({ "id": 2, "result": { "config": { "developer_instructions": instructions } } })
            .to_string()
    }

    /// The role text of a worker, with the app-server as scripted.
    fn text(&self, content: &str) -> Result<String, String> {
        finished(Box::pin(text(
            &self.services,
            "worker",
            CODEX,
            "/work/app",
            content,
        )))
    }

    /// What the app-server has been written, line by line.
    fn written(&self) -> Vec<String> {
        self.watching.scripted.take_written()
    }
}

#[test]
fn the_role_follows_what_codex_has_of_its_own_in_one_text() {
    let stage = Stage::new();
    stage.app_server(
        &[INITIALIZED, &Stage::configured("User instructions.")],
        Ends::Asked,
    );
    assert_eq!(
        stage.text("# The role\n\nIts text."),
        Ok("User instructions.\n\nYour ConsensFlow role is worker. The following role instructions are already loaded; follow them for app coordination. This is context, not a task; wait for the user's request.\n\n# The role\n\nIts text.".to_owned())
    );
}

#[test]
fn a_text_with_what_json_escapes_is_carried_as_it_is() {
    let stage = Stage::new();
    let had = "Back\\slash, \"quoted\", \u{2028} \u{7f} \u{1} \u{8} \u{c} \u{1f600}";
    stage.app_server(&[INITIALIZED, &Stage::configured(had)], Ends::Asked);
    let role = "Role \u{2029} \"text\" \\ with\ttab\u{0} and \u{e9}";
    assert_eq!(
        stage.text(role).unwrap(),
        format!("{had}\n\nYour ConsensFlow role is worker. The following role instructions are already loaded; follow them for app coordination. This is context, not a task; wait for the user's request.\n\n{role}")
    );
}

#[test]
fn a_window_with_no_role_text_is_refused_before_the_app_server_is_started() {
    let stage = Stage::new();
    stage.app_server(&[], Ends::Asked);
    assert_eq!(
        stage.text(""),
        Err("the worker window needs its role text".to_owned())
    );
    assert!(stage.watching.spawned.borrow().is_empty());
}

#[test]
fn the_app_server_is_started_in_the_window_s_folder_with_its_environment_and_a_line_at_a_time() {
    let stage = Stage::new();
    stage.app_server(&[INITIALIZED, &Stage::configured("")], Ends::Asked);
    stage.text("x").unwrap();
    let spawned = stage.watching.spawned.borrow();
    let [(program, streams)] = &spawned[..] else {
        panic!("{spawned:?}");
    };
    assert_eq!(program.executable, std::path::PathBuf::from(CODEX));
    assert_eq!(program.args, ["app-server"]);
    assert_eq!(program.cwd, Some(std::path::PathBuf::from("/work/app")));
    assert_eq!(program.env.text("HOME"), Some("/home/me"));
    assert_eq!(*streams, Streams::Lines);
}

#[test]
fn the_dialogue_is_initialized_then_the_configuration_read_for_the_folder_in_node_s_words() {
    let stage = Stage::new();
    stage.app_server(&[INITIALIZED, &Stage::configured("")], Ends::Asked);
    stage.text("x").unwrap();
    assert_eq!(
        stage.written(),
        [
            r#"{"id":1,"method":"initialize","params":{"clientInfo":{"name":"consensflow-role-config","version":"3.0.0"},"capabilities":{"experimentalApi":true}}}"#,
            r#"{"method":"initialized","params":{}}"#,
            r#"{"id":2,"method":"config/read","params":{"cwd":"/work/app","includeLayers":false}}"#,
        ]
    );
}

#[test]
fn the_app_server_is_asked_to_end_whatever_came_of_it() {
    let configured = Stage::configured("x");
    for (lines, ends, ok) in [
        (vec![INITIALIZED, configured.as_str()], Ends::Asked, true),
        (vec![INITIALIZED, configured.as_str()], Ends::Itself, true),
        (
            vec![r#"{"id":1,"error":{"message":"no"}}"#],
            Ends::Asked,
            false,
        ),
        (vec!["not json"], Ends::Asked, false),
        (
            vec![INITIALIZED, r#"{"id":2,"error":{}}"#],
            Ends::Asked,
            false,
        ),
        (vec![], Ends::Itself, false),
    ] {
        let stage = Stage::new();
        stage.app_server(&lines, ends);
        assert_eq!(stage.text("x").is_ok(), ok, "{lines:?}");
        assert_eq!(*stage.watching.ended.borrow(), [Ending::Asked], "{lines:?}");
    }
}

#[test]
fn an_app_server_that_is_not_there_refuses_the_launch_and_has_nothing_to_end() {
    let stage = Stage::new();
    assert_eq!(
        stage.text("x"),
        Err("Cannot read native Codex instructions safely".to_owned())
    );
    assert!(stage.watching.ended.borrow().is_empty());
}

#[test]
fn an_app_server_has_ten_seconds_from_its_start_and_is_asked_to_end_when_they_are_up() {
    let stage = Stage::new();
    stage.app_server(&[INITIALIZED], Ends::Asked);
    let mut driver = Driver::default();
    let services = stage.services.clone();
    driver.begin(0, async move {
        text(&services, "worker", CODEX, "/work/app", "x").await
    });
    assert!(driver.run().is_empty());
    assert_eq!(stage.fakes.time.waits(0), [10_000]);
    assert!(stage.watching.ended.borrow().is_empty());
    assert!(!stage.fakes.time.fire_next(EPOCH_MS + 9_999));
    assert!(stage.fakes.time.fire_next(EPOCH_MS + 10_000));
    assert_eq!(
        driver.run(),
        [(
            0,
            Err("Cannot read native Codex instructions safely".to_owned())
        )]
    );
    assert_eq!(*stage.watching.ended.borrow(), [Ending::Asked]);
    assert!(stage.fakes.time.waits(0).is_empty());
}

#[test]
fn a_line_is_bounded_at_two_mebibytes_and_a_longer_one_refuses_the_launch() {
    let padded = |size: usize| {
        let bare = json!({ "id": 7, "pad": "" }).to_string();
        json!({ "id": 7, "pad": "x".repeat(size - bare.len()) }).to_string()
    };
    for (size, ok) in [(LINE_LIMIT, true), (LINE_LIMIT + 1, false)] {
        let stage = Stage::new();
        let (long, configured) = (padded(size), Stage::configured("x"));
        stage.app_server(&[&long, INITIALIZED, &configured], Ends::Asked);
        assert_eq!(stage.text("x").is_ok(), ok, "{size}");
    }
}

#[test]
fn only_the_answers_to_the_two_requests_are_heard_and_the_rest_is_let_by() {
    let stage = Stage::new();
    let configured = Stage::configured("heard");
    stage.app_server(
        &[
            "[]",
            "7",
            "\"x\"",
            "true",
            r#"{"id":null}"#,
            r#"{"id":"1"}"#,
            r#"{"id":true}"#,
            r#"{"id":1.5}"#,
            r#"{"id":3}"#,
            r#"{"method":"notification"}"#,
            INITIALIZED,
            &configured,
        ],
        Ends::Asked,
    );
    assert!(stage.text("x").unwrap().contains("heard"));
    assert_eq!(stage.written().len(), 3);
}

#[test]
fn a_number_is_the_number_it_reads_as_and_an_answer_may_come_twice_or_early() {
    let stage = Stage::new();
    let configured = Stage::configured("late");
    stage.app_server(
        &[
            r#"{"id":1.0,"result":{}}"#,
            r#"{"id":1e0,"result":{}}"#,
            &configured,
        ],
        Ends::Asked,
    );
    assert!(stage.text("x").unwrap().contains("late"));
    // Two answers to initialize: the configuration asked for twice.
    assert_eq!(stage.written().len(), 5);

    let stage = Stage::new();
    stage.app_server(&[&Stage::configured("early")], Ends::Asked);
    assert!(stage.text("x").unwrap().contains("early"));
    assert_eq!(stage.written().len(), 1, "only initialize was asked");
}

#[test]
fn an_error_is_one_by_javascript_s_truth_and_so_is_a_configuration() {
    for (initialized, read, instructions) in [
        // A falsy error is none.
        (
            r#"{"id":1,"error":0,"result":{}}"#,
            r#"{"id":2,"error":"","result":{"config":{}}}"#,
            Some(""),
        ),
        (
            r#"{"id":1,"error":null}"#,
            r#"{"id":2,"error":false,"result":{"config":{}}}"#,
            Some(""),
        ),
        // A truthy one, of any kind, is an error.
        (
            r#"{"id":1,"error":1}"#,
            r#"{"id":2,"result":{"config":{}}}"#,
            None,
        ),
        (
            r#"{"id":1,"error":"no"}"#,
            r#"{"id":2,"result":{"config":{}}}"#,
            None,
        ),
        (
            r#"{"id":1,"error":[]}"#,
            r#"{"id":2,"result":{"config":{}}}"#,
            None,
        ),
        (
            INITIALIZED,
            r#"{"id":2,"error":{},"result":{"config":{}}}"#,
            None,
        ),
        (
            INITIALIZED,
            r#"{"id":2,"error":true,"result":{"config":{}}}"#,
            None,
        ),
        // A configuration that is falsy, or no answer, is none.
        (INITIALIZED, r#"{"id":2,"result":{}}"#, None),
        (INITIALIZED, r#"{"id":2,"result":null}"#, None),
        (INITIALIZED, r#"{"id":2}"#, None),
        (INITIALIZED, r#"{"id":2,"result":{"config":0}}"#, None),
        (INITIALIZED, r#"{"id":2,"result":{"config":""}}"#, None),
        (INITIALIZED, r#"{"id":2,"result":{"config":false}}"#, None),
        (INITIALIZED, r#"{"id":2,"result":{"config":null}}"#, None),
        (INITIALIZED, r#"{"id":2,"result":"config"}"#, None),
        (INITIALIZED, r#"{"id":2,"result":[{"config":{}}]}"#, None),
        // One that is truthy and no object has no instructions of its own.
        (INITIALIZED, r#"{"id":2,"result":{"config":7}}"#, Some("")),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":"text"}}"#,
            Some(""),
        ),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":true}}"#,
            Some(""),
        ),
        (INITIALIZED, r#"{"id":2,"result":{"config":[]}}"#, Some("")),
        // Instructions are text, or null, or not there.
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":{"developer_instructions":"x"}}}"#,
            Some("x"),
        ),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":{"developer_instructions":""}}}"#,
            Some(""),
        ),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":{"developer_instructions":null}}}"#,
            Some(""),
        ),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":{"other":1}}}"#,
            Some(""),
        ),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":{"developer_instructions":7}}}"#,
            None,
        ),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":{"developer_instructions":true}}}"#,
            None,
        ),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":{"developer_instructions":[]}}}"#,
            None,
        ),
        (
            INITIALIZED,
            r#"{"id":2,"result":{"config":{"developer_instructions":{}}}}"#,
            None,
        ),
    ] {
        let stage = Stage::new();
        stage.app_server(&[initialized, read], Ends::Asked);
        let given = stage.text("T").ok();
        let expected = instructions.map(|instructions| {
            format!("{instructions}\n\nYour ConsensFlow role is worker. The following role instructions are already loaded; follow them for app coordination. This is context, not a task; wait for the user's request.\n\nT")
        });
        assert_eq!(given, expected, "{initialized} {read}");
    }
}

#[test]
fn a_line_of_null_refuses_the_launch_in_our_words_where_node_threw_out_of_its_handler() {
    let stage = Stage::new();
    stage.app_server(&["null", INITIALIZED], Ends::Asked);
    assert_eq!(
        stage.text("x"),
        Err("Cannot read native Codex instructions safely".to_owned())
    );
    assert_eq!(*stage.watching.ended.borrow(), [Ending::Asked]);
}

#[test]
fn json_nested_past_a_hundred_and_twenty_seven_levels_is_no_json_to_the_dialogue() {
    let stage = Stage::new();
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    stage.app_server(&[&deep, INITIALIZED, &Stage::configured("x")], Ends::Asked);
    assert!(stage.text("x").is_err());
}

#[test]
fn the_text_is_written_in_the_launchs_own_folder_readable_by_its_owner_alone_and_named_by_the_variable_the_supervisor_reads(
) {
    let home = tempfile::tempdir().unwrap();
    let env = Env::from_vars([("CONSENSFLOW_HOME", home.path().to_str().unwrap())]);
    let launch = LaunchId::new("11111111-1111-4111-8111-111111111111").unwrap();
    let (name, file) = write(&env, &launch, "the role \u{e9}\n").unwrap();
    assert_eq!(name, "CF_CODEX_ROLE_FILE");
    let folder = home
        .path()
        .join("integrations")
        .join("codex")
        .join(launch.as_str());
    assert_eq!(
        std::path::PathBuf::from(&file),
        folder.join("developer-instructions.md")
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "the role \u{e9}\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode =
            |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(std::path::Path::new(&file)), 0o600);
        assert_eq!(mode(&folder), 0o700);
    }
    // It goes with the window.
    crate::shared::launch_files::forget_launch(home.path().to_str().unwrap(), &launch).unwrap();
    assert!(!folder.exists());
}
