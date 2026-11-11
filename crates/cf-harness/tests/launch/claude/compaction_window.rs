//! The size a project chose for its Claude members' windows to compact at (spec
//! smaller-contexts, SCX-25): the window Claude Code is given by the settings file
//! of the member's own launch (`autoCompactWindow`), which is T-1016's route. The
//! file is written under the ConsensFlow home for each launch, and Claude's own
//! files are never touched. With no size the file is as it always was, byte for
//! byte. The chief's launch is given none by the engine, so it is never asked of
//! this adapter.

use super::*;

/// A worker's settings file as it has always been written, byte for byte: Claude's
/// permission mode, the classic renderer, the board's answer to a question and the
/// Stop hook. The question hook runs the bundle's own `cf` by its whole path, quoted
/// (`{cf}`, T-1049): a login shell can find another `cf` first. On a Mac it holds the reads
/// Claude is denied besides (T-1194, `private_folders`).
#[cfg(not(windows))]
const WORKER_SETTINGS: &str = r#"{"permissions":{"defaultMode":"bypassPermissions","deny":["Read(~/Music)","Read(~/Music/**)","Read(~/Pictures/*.photoslibrary)","Read(~/Pictures/*.photoslibrary/**)"]},"skipDangerousModePermissionPrompt":true,"tui":"default","hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":[{"type":"command","command":"{cf} hook claude","timeout":3600}]}],"Stop":[{"hooks":[{"type":"command","command":"exit 0"}]}]}}"#;
#[cfg(windows)]
const WORKER_SETTINGS: &str = r#"{"permissions":{"defaultMode":"bypassPermissions"},"skipDangerousModePermissionPrompt":true,"tui":"default","hooks":{"PreToolUse":[{"matcher":"AskUserQuestion","hooks":[{"type":"command","command":"{cf} hook claude","timeout":3600}]}],"Stop":[{"hooks":[{"type":"command","command":"exit 0"}]}]}}"#;

/// [`WORKER_SETTINGS`] as `home`'s adapter writes it: its bundle's `cf`, quoted.
pub(super) fn worker_settings(home: &Home) -> String {
    let cf = format!(
        "{}/bundle/bin/cf{}",
        home.root.replace('\\', "/"),
        if cfg!(windows) { ".exe" } else { "" }
    );
    WORKER_SETTINGS.replacen("{cf}", &format!("'{cf}'"), 1)
}

/// The chief's: the same, with Claude's own question dialog in place of the board's.
#[cfg(not(windows))]
pub(super) const CHIEF_SETTINGS: &str = r#"{"permissions":{"defaultMode":"bypassPermissions","deny":["Read(~/Music)","Read(~/Music/**)","Read(~/Pictures/*.photoslibrary)","Read(~/Pictures/*.photoslibrary/**)"]},"skipDangerousModePermissionPrompt":true,"tui":"default","hooks":{"PreToolUse":[],"Stop":[{"hooks":[{"type":"command","command":"exit 0"}]}]}}"#;
#[cfg(windows)]
pub(super) const CHIEF_SETTINGS: &str = r#"{"permissions":{"defaultMode":"bypassPermissions"},"skipDangerousModePermissionPrompt":true,"tui":"default","hooks":{"PreToolUse":[],"Stop":[{"hooks":[{"type":"command","command":"exit 0"}]}]}}"#;

/// The settings file the prepared launch gave the window, as written.
pub(super) fn settings_text(plan: &Prepared) -> String {
    let at = plan
        .argv
        .iter()
        .position(|arg| arg == "--settings")
        .expect("a Claude window launches with its own settings file");
    fs::read_to_string(&plan.argv[at + 1]).unwrap()
}

pub(super) fn settings_of(plan: &Prepared) -> Value {
    serde_json::from_str(&settings_text(plan)).unwrap()
}

#[test]
fn a_launch_with_no_size_writes_the_settings_file_it_always_wrote_byte_for_byte() {
    let home = Home::new();
    let adapter = home.adapter();
    assert_eq!(
        settings_text(&prepare(&adapter, &Request::default()).unwrap()),
        worker_settings(&home)
    );
    assert_eq!(
        settings_text(&prepare(&adapter, &Request::chief()).unwrap()),
        CHIEF_SETTINGS
    );
}

#[test]
fn every_members_launch_is_given_the_size_as_the_window_in_its_settings_file_and_nothing_else_moves(
) {
    for role in ["worker", "advisor", "reviewer", "tester", "designer"] {
        let home = Home::new();
        let adapter = home.adapter();
        let plain = Request {
            role,
            ..Request::default()
        };
        let sized = Request {
            compaction: Some(400_000),
            ..plain_copy(&plain)
        };
        let without = prepare(&adapter, &plain).unwrap();
        let without_settings = settings_of(&without);
        let given = prepare(&adapter, &sized).unwrap();
        let mut settings = settings_of(&given);
        assert_eq!(settings["autoCompactWindow"], json!(400_000), "{role}");
        // Nothing else of the file moves, and nothing of the launch beyond it.
        settings
            .as_object_mut()
            .unwrap()
            .remove("autoCompactWindow");
        assert_eq!(settings, without_settings, "{role}");
        assert_eq!(
            without_session(&given.argv),
            without_session(&without.argv),
            "{role}: the same command line"
        );
        assert_eq!(given.env, without.env, "{role}: the same environment");
        assert_eq!(given.drop_env, without.drop_env, "{role}");
    }
}

#[test]
fn the_size_is_given_to_a_window_resumed_as_to_one_launched() {
    let home = Home::new();
    let session = "0f8fad5b-d9cb-469f-a165-70867728950e";
    home.transcript(session, &[user_line(session, 1, "Write the parser")]);
    let adapter = home.adapter();
    let resumed = Request {
        compaction: Some(250_000),
        ..Request::resumed(session)
    };
    let plan = prepare(&adapter, &resumed).unwrap();
    assert!(
        plan.argv.iter().any(|arg| arg == "--resume"),
        "it is resumed"
    );
    assert_eq!(settings_of(&plan)["autoCompactWindow"], json!(250_000));
    // And a window resumed on a project with no size is as it was.
    let plain = prepare(&adapter, &Request::resumed(session)).unwrap();
    assert!(settings_of(&plain).get("autoCompactWindow").is_none());
}

#[test]
fn the_size_is_written_in_the_launch_s_own_file_and_in_no_file_of_the_owner_s() {
    let home = Home::new();
    let owner = path::join(&[home.var("CLAUDE_CONFIG_DIR"), "settings.json"]);
    fs::create_dir_all(home.var("CLAUDE_CONFIG_DIR")).unwrap();
    fs::write(&owner, "{\"autoCompactWindow\":\"auto\"}\n").unwrap();
    let plan = prepare(
        &home.adapter(),
        &Request {
            compaction: Some(400_000),
            ..Request::default()
        },
    )
    .unwrap();
    let at = plan
        .argv
        .iter()
        .position(|arg| arg == "--settings")
        .unwrap();
    assert!(
        plan.argv[at + 1].starts_with(&home.launch_folder()),
        "under the launch's own folder: {}",
        plan.argv[at + 1]
    );
    assert_eq!(
        fs::read_to_string(&owner).unwrap(),
        "{\"autoCompactWindow\":\"auto\"}\n",
        "the owner's settings are as they were"
    );
    // No other file of Claude's under its config folder was made for it.
    let made: Vec<_> = fs::read_dir(home.var("CLAUDE_CONFIG_DIR"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(made, ["settings.json"]);
}

/// A command line without the id of the conversation it names (`--session-id`), which a
/// fresh launch draws anew.
pub(super) fn without_session(argv: &[String]) -> Vec<&str> {
    let mut words: Vec<&str> = argv.iter().map(String::as_str).collect();
    let at = words
        .iter()
        .position(|word| *word == "--session-id")
        .unwrap();
    words.drain(at..=at + 1);
    words
}

/// `request` again, for a field of it to change: the request has no `Clone`.
pub(super) fn plain_copy(request: &Request) -> Request {
    Request {
        role: request.role,
        handle: request.handle,
        instructions: request.instructions,
        resume: request.resume.clone(),
        previous_gone: request.previous_gone,
        message: request.message.clone(),
        agent: request.agent,
        specs: request.specs,
        compaction: request.compaction,
        advisor_off: request.advisor_off,
        suggestion_off: request.suggestion_off,
        folder: request.folder,
        directory: request.directory.clone(),
    }
}
