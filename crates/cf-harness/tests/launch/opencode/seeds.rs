//! A window's first message through its own server, as `describe('OpenCode
//! worker seed uses the native API after the TUI starts')` of Node's OpenCode
//! launch suite: the task posted once, with the roster's model for a new
//! conversation and the conversation's own for a resumed one, after the
//! server's health answers, and never retried. A window has only the 60 s its
//! adapter gives it, so what Node gave a short timeout runs here on the test's
//! clock. The two cases of Node's that cancel the startup by a signal are not
//! here: no adapter passes one. `tests/channels/seed.rs` runs the cases against
//! a server on real sockets, on the machine's own clock.

use std::rc::Rc;

use cf_harness::contract::Window;
use cf_harness::seams::loopback::{Method, Request};
use cf_harness::testing::{Driver, Sent, Served};
use serde_json::{json, Value};
use url::form_urlencoded::byte_serialize;

use super::stage::{finish, served, Stage, Wanted, MODEL};

/// The task a window is opened with.
const TASK: &str = "Only the actual task.\nKeep all of it.";

/// A server that is up.
fn up() -> Served {
    served(200, b"{}")
}

/// The settings of a conversation OpenCode kept, as its server answers them.
fn native(fields: &Value) -> Served {
    served(200, fields.to_string().as_bytes())
}

/// A fresh window on `session`, opened with `task` on `model` and `effort`.
fn fresh_window(
    stage: &Stage,
    session: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Rc<dyn Window> {
    stage.serves_creation(session);
    let wanted = Wanted {
        message: Some(TASK.to_owned()),
        model: model.map(str::to_owned),
        effort: effort.map(str::to_owned),
        ..Wanted::default()
    };
    let window = stage.prepare(&wanted).unwrap().window;
    stage.fakes.loopback.take_asked();
    window
}

/// `session` resumed, its follow-up `task`, the roster saying `model` and `effort`.
fn resumed_window(
    stage: &Stage,
    session: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Rc<dyn Window> {
    let wanted = Wanted {
        resume: Some(session.to_owned()),
        message: Some("follow-up".to_owned()),
        model: model.map(str::to_owned),
        effort: effort.map(str::to_owned),
        ..Wanted::default()
    };
    stage.prepare(&wanted).unwrap().window
}

/// The task submitted, the clock moved on whenever the window waits.
fn submit(stage: &Stage, window: &Rc<dyn Window>) -> Result<Option<String>, String> {
    let window = Rc::clone(window);
    finish(&stage.fakes, async move { window.started().await })
}

/// What was asked of the server, by method.
fn asked(stage: &Stage, method: Method) -> Vec<Request> {
    let all = stage.fakes.loopback.take_asked();
    all.into_iter()
        .filter(|request| request.method == method)
        .collect()
}

/// The JSON a request carried.
fn body(request: &Request) -> Value {
    serde_json::from_slice(request.body.as_deref().unwrap()).unwrap()
}

/// How a request writes the folder the window works in.
fn directory(stage: &Stage) -> String {
    byte_serialize(stage.home.work().as_bytes()).collect()
}

#[test]
fn recovers_from_a_health_check_that_hangs_or_whose_body_hangs_when_later_probes_succeed() {
    for hung in [
        Served::Held,
        Served::Head {
            status: 200,
            body: Sent::Held,
        },
    ] {
        let stage = Stage::new();
        let window = fresh_window(&stage, "ses_health123", Some(MODEL), Some("high"));
        stage
            .fakes
            .loopback
            .serve("GET /global/health", [hung, up()]);
        stage.fakes.loopback.serve(
            "POST /session/ses_health123/prompt_async",
            [served(204, b"")],
        );
        assert_eq!(submit(&stage, &window), Ok(None));
        let asked = stage.fakes.loopback.take_asked();
        let health = asked
            .iter()
            .filter(|request| request.url.ends_with("/global/health"))
            .count();
        assert!(health >= 2, "{health}");
        let posts: Vec<&Request> = asked
            .iter()
            .filter(|request| request.method == Method::Post)
            .collect();
        assert_eq!(posts.len(), 1);
        assert_eq!(body(posts[0])["parts"][0]["text"], TASK);
    }
}

#[test]
fn waits_for_readiness_then_sends_the_exact_task_and_roster_model_once() {
    let stage = Stage::new();
    let window = fresh_window(
        &stage,
        "ses_native123",
        Some("opencode/model/variant"),
        None,
    );
    stage.fakes.loopback.serve(
        "GET /global/health",
        [served(503, b"{}"), served(503, b"{}"), up()],
    );
    stage.fakes.loopback.serve(
        "POST /session/ses_native123/prompt_async",
        [served(204, b"")],
    );
    assert_eq!(submit(&stage, &window), Ok(None));
    let posts = asked(&stage, Method::Post);
    assert_eq!(posts.len(), 1);
    assert_eq!(
        posts[0].url,
        format!(
            "http://127.0.0.1:{}/session/ses_native123/prompt_async?directory={}",
            port_of(&posts[0]),
            directory(&stage)
        )
    );
    assert_eq!(
        body(&posts[0]),
        json!({
            "parts": [{ "type": "text", "text": TASK }],
            "model": { "providerID": "opencode", "modelID": "model/variant" },
        })
    );
}

/// The port a request went to.
fn port_of(request: &Request) -> &str {
    request.url["http://127.0.0.1:".len()..]
        .split('/')
        .next()
        .unwrap()
}

#[test]
fn does_not_override_the_existing_native_model_on_a_resumed_conversation() {
    let stage = Stage::new();
    let window = resumed_window(
        &stage,
        "ses_resume123",
        Some("wrong/edited-roster"),
        Some("max"),
    );
    let settings = json!({
        "id": "ses_resume123",
        "agent": "review",
        "model": { "id": "native-model", "providerID": "openrouter", "variant": "medium" },
    });
    stage.fakes.loopback.serve("GET /global/health", [up()]);
    stage
        .fakes
        .loopback
        .serve("GET /session/ses_resume123", [native(&settings)]);
    stage.fakes.loopback.serve(
        "POST /session/ses_resume123/prompt_async",
        [served(204, b"")],
    );
    assert_eq!(submit(&stage, &window), Ok(None));
    let posts = asked(&stage, Method::Post);
    assert_eq!(
        body(&posts[0]),
        json!({
            "parts": [{ "type": "text", "text": "follow-up" }],
            "model": { "providerID": "openrouter", "modelID": "native-model" },
            "variant": "medium",
            "agent": "review",
        })
    );
}

#[test]
fn sends_the_selected_effort_to_the_native_prompt_api() {
    for variant in ["low", "medium"] {
        let stage = Stage::new();
        let window = fresh_window(
            &stage,
            "ses_effort123",
            Some("openrouter/openai/gpt-6-astra"),
            Some(variant),
        );
        stage.fakes.loopback.serve("GET /global/health", [up()]);
        stage.fakes.loopback.serve(
            "POST /session/ses_effort123/prompt_async",
            [served(204, b"")],
        );
        assert_eq!(submit(&stage, &window), Ok(None));
        let everything = stage.fakes.loopback.take_asked();
        let posts: Vec<&Request> = everything
            .iter()
            .filter(|request| request.method == Method::Post)
            .collect();
        let sent = body(posts[0]);
        assert_eq!(sent["variant"], variant);
        assert_eq!(
            sent["model"],
            json!({ "providerID": "openrouter", "modelID": "openai/gpt-6-astra" })
        );
        let reads = everything
            .iter()
            .filter(|request| request.method == Method::Get && request.url.contains("/session/"))
            .count();
        assert_eq!(reads, 0, "a fresh conversation's settings are the roster's");
    }
}

#[test]
fn sends_the_explicit_native_default_rather_than_falling_back_to_roster_or_agent_effort() {
    let stage = Stage::new();
    let window = resumed_window(&stage, "ses_default123", None, Some("max"));
    let settings = json!({ "id": "ses_default123", "agent": "review", "model": { "id": "native-model", "providerID": "openrouter" } });
    stage.fakes.loopback.serve("GET /global/health", [up()]);
    stage
        .fakes
        .loopback
        .serve("GET /session/ses_default123", [native(&settings)]);
    stage.fakes.loopback.serve(
        "POST /session/ses_default123/prompt_async",
        [served(204, b"")],
    );
    assert_eq!(submit(&stage, &window), Ok(None));
    assert_eq!(body(&asked(&stage, Method::Post)[0])["variant"], "default");
}

#[test]
fn sends_no_prompt_after_a_settings_read_that_fails_names_no_model_or_hangs() {
    let settings =
        |model: Value| native(&json!({ "id": "ses_read123", "agent": "review", "model": model }));
    for (name, answer) in [
        ("native-read-failure", served(503, b"{}")),
        ("invalid-native-model", settings(json!({}))),
        ("native-read-hang", Served::Held),
    ] {
        let stage = Stage::new();
        let window = resumed_window(&stage, "ses_read123", None, None);
        stage.fakes.loopback.serve("GET /global/health", [up()]);
        stage
            .fakes
            .loopback
            .serve("GET /session/ses_read123", [answer]);
        assert!(submit(&stage, &window).is_err(), "{name}");
        assert!(asked(&stage, Method::Post).is_empty(), "{name}");
    }
}

#[test]
fn reports_uncertain_admission_and_never_retries_after_a_disconnect_or_a_hang() {
    for (name, answer) in [("disconnect", Served::NoHead), ("hang", Served::Held)] {
        let stage = Stage::new();
        let window = fresh_window(&stage, "ses_once123", Some(MODEL), Some("high"));
        stage.fakes.loopback.serve("GET /global/health", [up()]);
        stage
            .fakes
            .loopback
            .serve("POST /session/ses_once123/prompt_async", [answer]);
        let failed = submit(&stage, &window).unwrap_err();
        assert_eq!(
            failed, "OpenCode task admission is uncertain; task was not retried",
            "{name}"
        );
        assert_eq!(asked(&stage, Method::Post).len(), 1, "{name}");
    }
}

#[test]
fn tolerates_slow_native_startup_past_the_old_15s_budget() {
    let stage = Stage::new();
    let window = fresh_window(&stage, "ses_slow123", Some("opencode/model/variant"), None);
    // The server answers at 15.5 s: a poll every tenth of a second before.
    let not_yet = std::iter::repeat_with(|| served(503, b"{}")).take(155);
    stage
        .fakes
        .loopback
        .serve("GET /global/health", not_yet.chain([up()]));
    stage
        .fakes
        .loopback
        .serve("POST /session/ses_slow123/prompt_async", [served(204, b"")]);
    assert_eq!(submit(&stage, &window), Ok(None));
    let posts = asked(&stage, Method::Post);
    assert_eq!(posts.len(), 1);
    assert!(posts[0].url.contains("/session/ses_slow123/prompt_async"));
}

#[test]
fn refuses_failed_authentication_before_sending_task_bytes() {
    let stage = Stage::new();
    let window = fresh_window(&stage, "ses_auth123", Some(MODEL), Some("high"));
    stage
        .fakes
        .loopback
        .serve("GET /global/health", [served(401, b"")]);
    let failed = submit(&stage, &window).unwrap_err();
    assert!(failed.contains("unauthorized"), "{failed}");
    assert!(asked(&stage, Method::Post).is_empty());
}

#[test]
fn a_window_opened_with_no_task_submits_nothing_and_one_given_an_empty_task_is_refused() {
    let stage = Stage::new();
    stage.serves_creation("ses_abc123");
    let none = Wanted {
        message: None,
        ..Wanted::default()
    };
    let window = stage.prepare(&none).unwrap().window;
    stage.fakes.loopback.take_asked();
    assert_eq!(submit(&stage, &window), Ok(None));
    assert!(stage.fakes.loopback.take_asked().is_empty());
    stage.serves_creation("ses_abc123");
    let empty = Wanted {
        message: Some(String::new()),
        ..Wanted::default()
    };
    let window = stage.prepare(&empty).unwrap().window;
    assert_eq!(
        submit(&stage, &window),
        Err("invalid OpenCode task launch".to_owned())
    );
}

#[test]
fn settings_that_arrive_as_the_time_is_up_end_the_startup_before_anything_is_posted() {
    // Node: `lifetime.signal.throwIfAborted()` before the post. The recorder
    // cannot make a body arrive in the instant a timer is due, so the clock
    // is moved here, after the body is let go and before the work looks.
    let stage = Stage::new();
    let window = resumed_window(&stage, "ses_race123", None, None);
    let settings = json!({ "id": "ses_race123", "model": { "id": "native", "providerID": "p" } });
    stage.fakes.loopback.serve("GET /global/health", [up()]);
    stage.fakes.loopback.serve(
        "GET /session/ses_race123",
        [Served::Head {
            status: 200,
            body: Sent::Held,
        }],
    );
    let mut driver = Driver::default();
    let starting = Rc::clone(&window);
    driver.begin(0, async move { starting.started().await });
    assert!(driver.run().is_empty());
    assert_eq!(stage.fakes.loopback.waits(0), ["GET /session/ses_race123"]);
    let body = settings.to_string().into_bytes();
    assert!(stage
        .fakes
        .loopback
        .release_body("GET /session/ses_race123", Ok(body)));
    assert!(stage.fakes.time.fire_next(i64::MAX));
    let [(_, answer)] = &driver.run()[..] else {
        panic!("the work is done");
    };
    assert_eq!(answer, &Err("OpenCode task startup timed out".to_owned()));
    assert!(asked(&stage, Method::Post).is_empty());
}
