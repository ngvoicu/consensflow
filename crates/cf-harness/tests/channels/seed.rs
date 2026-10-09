//! A window's first message through its own server, against a server of the
//! test's own on the machine's clock and loopback (Node's `describe('OpenCode
//! worker seed uses the native API after the TUI starts')`): the task posted
//! once, with the roster's model for a new conversation and the conversation's
//! own for a resumed one, after the server's health answers, and never retried.
//! A seed has no signal to cancel it by, since no caller passes one. The same
//! cases are held once more on a clock a test moves by hand, in
//! `tests/launch/opencode/seeds.rs`.

mod server;

use std::time::Duration;

use cf_base::env::Env;
use cf_harness::opencode::{seed_session, Seed, LIFETIME_MS};
use cf_harness::seams::SystemProcesses;
use serde_json::json;
use tempfile::TempDir;

use crate::support::{matches, real_folder, run, wires};
use server::{Mode, Window};

/// What a seed is asked to post.
#[derive(Default)]
struct Task<'a> {
    session: &'a str,
    text: &'a str,
    model: Option<&'a str>,
    variant: Option<&'a str>,
    resume: bool,
    /// How long the server has to answer and the task to be posted: a window's
    /// where there is none.
    lifetime_ms: Option<u64>,
}

/// `task` seeded to `window`, in `folder`.
async fn seeded(window: &Window, folder: &TempDir, task: Task<'_>) -> Result<(), String> {
    let processes = SystemProcesses::new(Env::default());
    let directory = folder.path().to_string_lossy();
    let seed = Seed {
        session: task.session,
        directory: &directory,
        text: task.text,
        model: task.model,
        variant: task.variant,
        resume: task.resume,
        lifetime_ms: task.lifetime_ms.unwrap_or(LIFETIME_MS),
    };
    seed_session(wires(&processes), &window.channel(), &seed).await
}

/// A window whose server is up from the start, and the folder it works in.
async fn up(mode: Mode) -> (Window, TempDir) {
    let window = Window::start(mode).await;
    window.ready();
    (window, tempfile::tempdir().unwrap())
}

/// A server whose first health poll fails as `mode` says is asked again, and
/// the task posted once.
async fn recovers_from(mode: Mode) {
    let (window, folder) = up(mode).await;
    let task = Task {
        session: "ses_health123",
        text: "Tell me a joke.",
        lifetime_ms: Some(2000),
        ..Task::default()
    };
    seeded(&window, &folder, task).await.unwrap();
    assert!(window.polls().len() >= 2);
    let posts = window.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].json()["parts"][0]["text"], "Tell me a joke.");
}

#[test]
fn recovers_from_health_hang_when_later_readiness_probes_succeed() {
    run(recovers_from(Mode::HealthHang));
}

#[test]
fn recovers_from_health_body_hang_when_later_readiness_probes_succeed() {
    run(recovers_from(Mode::HealthBodyHang));
}

/// A server that says it is up after `wait`, to which the task is posted once,
/// with the roster's model, to the conversation's prompt in the folder.
async fn posted_after(wait: Duration, session: &str, lifetime_ms: Option<u64>) {
    let window = Window::start(Mode::Ok).await;
    window.ready_in(wait);
    let folder = tempfile::tempdir().unwrap();
    let text = "Only the actual task.\nKeep all of it.";
    let task = Task {
        session,
        text,
        model: Some("opencode/model/variant"),
        lifetime_ms,
        ..Task::default()
    };
    seeded(&window, &folder, task).await.unwrap();
    let posts = window.posts();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].path(), format!("/session/{session}/prompt_async"));
    assert_eq!(
        posts[0].query("directory"),
        Some(real_folder(folder.path()))
    );
    assert_eq!(
        posts[0].json(),
        json!({
            "parts": [{ "type": "text", "text": text }],
            "model": { "providerID": "opencode", "modelID": "model/variant" },
        })
    );
}

#[test]
fn waits_for_readiness_then_sends_the_exact_task_and_roster_model_once() {
    run(posted_after(
        Duration::from_millis(60),
        "ses_native123",
        Some(2000),
    ));
}

#[test]
fn does_not_override_the_existing_native_model_on_a_resumed_conversation() {
    run(async {
        let (window, folder) = up(Mode::Ok).await;
        let task = Task {
            session: "ses_resume123",
            text: "follow-up",
            resume: true,
            model: Some("wrong/edited-roster"),
            variant: Some("max"),
            lifetime_ms: Some(2000),
        };
        seeded(&window, &folder, task).await.unwrap();
        assert_eq!(
            window.posts()[0].json(),
            json!({
                "parts": [{ "type": "text", "text": "follow-up" }],
                "model": { "providerID": "openrouter", "modelID": "native-model" },
                "variant": "medium",
                "agent": "review",
            })
        );
    });
}

/// A new conversation seeded with the roster's `variant`: the prompt carries
/// it, and no settings were read.
async fn sends_selected(variant: &str) {
    let (window, folder) = up(Mode::Ok).await;
    let task = Task {
        session: "ses_effort123",
        text: "task",
        model: Some("openrouter/openai/gpt-6-astra"),
        variant: Some(variant),
        lifetime_ms: Some(2000),
        ..Task::default()
    };
    seeded(&window, &folder, task).await.unwrap();
    let body = window.posts()[0].json();
    assert_eq!(body["variant"], variant);
    assert_eq!(
        body["model"],
        json!({ "providerID": "openrouter", "modelID": "openai/gpt-6-astra" })
    );
    assert_eq!(window.reads().len(), 0);
}

#[test]
fn sends_selected_low_to_the_native_prompt_api() {
    run(sends_selected("low"));
}

#[test]
fn sends_selected_medium_to_the_native_prompt_api() {
    run(sends_selected("medium"));
}

#[test]
fn sends_explicit_native_default_rather_than_falling_back_to_roster_or_agent_effort() {
    run(async {
        let (window, folder) = up(Mode::NativeDefault).await;
        let task = Task {
            session: "ses_default123",
            text: "task",
            resume: true,
            variant: Some("max"),
            lifetime_ms: Some(2000),
            ..Task::default()
        };
        seeded(&window, &folder, task).await.unwrap();
        assert_eq!(window.posts()[0].json()["variant"], "default");
    });
}

/// A resumed conversation whose settings cannot be read as `mode` says: the
/// seed fails, and no task was posted.
async fn sends_no_prompt_after(mode: Mode) {
    let (window, folder) = up(mode).await;
    let task = Task {
        session: "ses_read123",
        text: "task",
        resume: true,
        lifetime_ms: Some(150),
        ..Task::default()
    };
    seeded(&window, &folder, task).await.unwrap_err();
    assert_eq!(window.posts().len(), 0);
}

#[test]
fn sends_no_prompt_after_native_read_failure() {
    run(sends_no_prompt_after(Mode::NativeReadFailure));
}

#[test]
fn sends_no_prompt_after_invalid_native_model() {
    run(sends_no_prompt_after(Mode::InvalidNativeModel));
}

#[test]
fn sends_no_prompt_after_native_read_hang() {
    run(sends_no_prompt_after(Mode::NativeReadHang));
}

/// A task whose post fails as `mode` says is posted once, and the seed says its
/// admission is uncertain and that it was not retried.
async fn reports_uncertain_admission_after(mode: Mode) {
    let (window, folder) = up(mode).await;
    let task = Task {
        session: "ses_once123",
        text: "send once",
        lifetime_ms: Some(200),
        ..Task::default()
    };
    let failed = seeded(&window, &folder, task).await.unwrap_err();
    assert!(matches(&failed, "uncertain.*not retried"), "{failed}");
    assert_eq!(window.posts().len(), 1);
}

#[test]
fn reports_uncertain_admission_and_never_retries_after_disconnect() {
    run(reports_uncertain_admission_after(Mode::Disconnect));
}

#[test]
fn reports_uncertain_admission_and_never_retries_after_hang() {
    run(reports_uncertain_admission_after(Mode::Hang));
}

#[test]
fn tolerates_slow_native_startup_past_the_old_15s_budget() {
    run(posted_after(
        Duration::from_millis(15_500),
        "ses_slow123",
        None,
    ));
}

#[test]
fn refuses_failed_authentication_before_sending_task_bytes() {
    run(async {
        let (window, folder) = up(Mode::Unauthorized).await;
        let task = Task {
            session: "ses_auth123",
            text: "must not send",
            lifetime_ms: Some(2000),
            ..Task::default()
        };
        let failed = seeded(&window, &folder, task).await.unwrap_err();
        assert!(matches(&failed, "unauthorized"), "{failed}");
        assert_eq!(window.posts().len(), 0);
    });
}
