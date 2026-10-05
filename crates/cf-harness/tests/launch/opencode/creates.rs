//! The conversation a fresh window opens on, as `describe('opencode
//! createSession')` of `tests/opencode-launch.test.mjs` holds Node's: one
//! empty session made on a throwaway `serve`, which is stopped before the id
//! returns and on every failure. The throwaway server is scripted: what
//! Node ran as a real child (a process checked to be gone, a stand-in server
//! answering for real) is a scripted child whose end the test reads.

use std::path::Path;

use cf_base::path;
use cf_harness::seams::loopback::{Method, Request};
use cf_harness::testing::{ChildScript, Driver, Ends, Served};
use serde_json::json;

use super::stage::{planned, served, words, Home, Stage, Wanted};
use crate::scenarios::component;

/// What a throwaway server's scripted child is asked to do, as the scripted
/// programs keep it: the last child started.
fn reaped(stage: &Stage) -> bool {
    stage.started.children.borrow().last().unwrap().exited()
}

/// A fresh window prepared on what the test scripted: the id, or why not.
fn made(stage: &Stage) -> Result<String, String> {
    stage
        .prepare(&Wanted::default())
        .map(|plan| plan.native_session.unwrap())
}

/// A throwaway server that is up and makes a session as `answer` says.
fn serving(stage: &Stage, answer: Served, child: Ends) {
    stage
        .fakes
        .loopback
        .serve("GET /global/health", [served(200, br#"{"healthy":true}"#)]);
    stage.fakes.loopback.serve("POST /session", [answer]);
    stage.fakes.processes.child(
        "opencode",
        ChildScript {
            lines: Vec::new(),
            ends: child,
        },
    );
}

/// A session `id` made in `directory`.
fn session(id: &str, directory: &str) -> Served {
    served(
        200,
        json!({ "id": id, "directory": directory })
            .to_string()
            .as_bytes(),
    )
}

#[test]
fn creates_exactly_one_empty_session_and_frees_the_port() {
    let stage = Stage::with(Home::new().sharing("CF_FIXTURE_PING", "pong"), None);
    stage.serves_creation("ses_abc123");
    let plan = stage.prepare(&Wanted::default()).unwrap();
    let id = plan.native_session.clone().unwrap();
    assert!(
        id.starts_with("ses_") && id[4..].bytes().all(|byte| byte.is_ascii_alphanumeric()),
        "{id}"
    );
    let asked = stage.fakes.loopback.take_asked();
    let posts: Vec<&Request> = asked
        .iter()
        .filter(|request| request.method == Method::Post)
        .collect();
    assert_eq!(posts.len(), 1);
    assert_eq!(posts[0].body.as_deref(), Some(&b"{}"[..]));
    let work = stage.home.work();
    assert!(
        posts[0]
            .url
            .ends_with(&format!("/session?directory={}", component(&work))),
        "{}",
        posts[0].url
    );
    let started = stage.started.started.borrow();
    let program = &started[0];
    let port = plan.argv[2].clone();
    assert_eq!(
        program.args,
        words(&["serve", "--port", &port, "--hostname", "127.0.0.1"])
    );
    assert_eq!(program.cwd.as_deref(), Some(Path::new(&work)));
    assert_eq!(
        program.env.text("OPENCODE_SERVER_USERNAME"),
        Some("opencode")
    );
    assert_eq!(
        program.env.text("OPENCODE_SERVER_PASSWORD"),
        planned(&plan, "OPENCODE_SERVER_PASSWORD")
    );
    assert_eq!(
        program.env.text("CF_FIXTURE_PING"),
        Some("pong"),
        "what the engine runs with is inherited"
    );
    assert_eq!(
        program.env.text("OPENCODE_CONFIG_CONTENT"),
        planned(&plan, "OPENCODE_CONFIG_CONTENT"),
        "the role's configuration is the throwaway server's too"
    );
    assert!(
        reaped(&stage),
        "the temporary server is reaped before return"
    );
}

#[test]
fn two_launches_make_two_conversations_each_on_a_server_of_its_own() {
    let stage = Stage::new();
    stage.serves_creation("ses_first1");
    stage.serves_creation("ses_second2");
    let first = made(&stage).unwrap();
    let second = made(&stage).unwrap();
    assert_ne!(first, second);
    let started = stage.started.started.borrow();
    assert_eq!(started.len(), 2);
    assert_ne!(started[0].args, started[1].args, "a port of its own");
}

#[test]
fn a_server_that_cannot_start_is_refused_without_a_child() {
    let stage = Stage::new();
    let failed = made(&stage).unwrap_err();
    assert_eq!(failed, "opencode serve failed to start");
    assert!(stage.started.children.borrow().is_empty());
}

#[test]
fn surfaces_an_early_exit_and_unauthorized_without_leaking_secrets() {
    let stage = Stage::new();
    stage.fakes.processes.child(
        "opencode",
        ChildScript {
            lines: Vec::new(),
            ends: Ends::Itself,
        },
    );
    assert_eq!(made(&stage).unwrap_err(), "opencode serve exited early");
    let stage = Stage::new();
    stage
        .fakes
        .loopback
        .serve("GET /global/health", [served(401, b"unauthorized")]);
    stage.fakes.processes.child(
        "opencode",
        ChildScript {
            lines: Vec::new(),
            ends: Ends::Asked,
        },
    );
    let failed = made(&stage).unwrap_err();
    assert_eq!(failed, "opencode session unauthorized");
    let started = stage.started.started.borrow();
    let password = started[0].env.text("OPENCODE_SERVER_PASSWORD").unwrap();
    assert!(!failed.contains(password));
    assert!(reaped(&stage));
}

#[test]
fn rejects_malformed_invalid_id_wrong_dir_and_oversized_responses_and_reaps_the_child() {
    let cases = [
        ("bad-json", "opencode session returned invalid JSON"),
        ("invalid-id", "opencode session returned an invalid id"),
        ("wrong-dir", "opencode session returned the wrong directory"),
        (
            "oversized",
            "opencode session returned an oversized response",
        ),
    ];
    for (mode, sentence) in cases {
        // Each stage is its own home, so the folder a session is made in is
        // the stage's.
        let stage = Stage::new();
        let answer = match mode {
            "bad-json" => served(200, b"not json{"),
            "invalid-id" => session("bad", &stage.home.work()),
            "wrong-dir" => session("ses_abc123", "/elsewhere"),
            _ => served(200, &vec![b'x'; 1024 * 1024 + 1]),
        };
        serving(&stage, answer, Ends::Asked);
        assert_eq!(made(&stage).unwrap_err(), sentence, "{mode}");
        assert!(reaped(&stage), "{mode}: child reaped");
    }
}

#[test]
fn times_out_on_hanging_endpoints_and_reaps_an_ignored_sigterm() {
    let stage = Stage::new();
    // A health check that never answers: every poll held until it is given up on.
    stage
        .fakes
        .loopback
        .serve("GET /global/health", (0..30).map(|_| Served::Held));
    stage.fakes.processes.child(
        "opencode",
        ChildScript {
            lines: Vec::new(),
            ends: Ends::Asked,
        },
    );
    assert_eq!(made(&stage).unwrap_err(), "opencode session timed out");
    assert!(reaped(&stage), "hanging child reaped");
    let stage = Stage::new();
    let made_here = session("ses_stubborn1", &stage.home.work());
    serving(&stage, made_here, Ends::Forced);
    assert_eq!(made(&stage).unwrap(), "ses_stubborn1");
    assert!(reaped(&stage), "the kill after the ask reaps the child");
}

// Node's own cases ended a real child by SIGKILL; a child that goes on when
// it is forced is only scripted, and on Windows an end is always forced.
#[cfg(unix)]
#[test]
fn a_server_that_will_not_stop_fails_the_creation_and_is_stopped_a_second_time() {
    let stage = Stage::new();
    let made_here = session("ses_abc123", &stage.home.work());
    serving(&stage, made_here, Ends::Never);
    assert_eq!(
        made(&stage).unwrap_err(),
        "opencode session failed to stop the server"
    );
    assert!(!reaped(&stage), "it never ended");
}

#[cfg(unix)]
#[test]
fn the_failure_of_a_creation_is_the_answer_even_when_its_server_will_not_stop() {
    let stage = Stage::new();
    serving(&stage, session("bad", &stage.home.work()), Ends::Never);
    assert_eq!(
        made(&stage).unwrap_err(),
        "opencode session returned an invalid id"
    );
}

#[test]
fn a_session_in_a_folder_that_is_not_there_is_refused_in_a_sentence_of_its_own() {
    let stage = Stage::new();
    let wanted = Wanted {
        directory: Some(path::join(&[&stage.home.root, "missing"])),
        ..Wanted::default()
    };
    assert_eq!(
        stage.prepare(&wanted).err().as_deref(),
        Some("opencode session needs a working directory")
    );
    assert!(stage.started.started.borrow().is_empty());
}

#[cfg(unix)]
#[test]
fn a_working_folder_that_is_a_link_is_the_folder_it_leads_to_for_the_server_and_its_session() {
    let stage = Stage::new();
    let link = path::join(&[&stage.home.root, "link"]);
    std::os::unix::fs::symlink(stage.home.work(), &link).unwrap();
    // The server names the folder as the system does, not as the link.
    stage.serves_creation("ses_abc123");
    let wanted = Wanted {
        directory: Some(link),
        ..Wanted::default()
    };
    let plan = stage.prepare(&wanted).unwrap();
    assert_eq!(plan.native_session.as_deref(), Some("ses_abc123"));
    let work = stage.home.work();
    let started = stage.started.started.borrow();
    assert_eq!(started[0].cwd.as_deref(), Some(Path::new(&work)));
    let asked = stage.fakes.loopback.take_asked();
    let post = asked
        .iter()
        .find(|request| request.method == Method::Post)
        .unwrap();
    assert!(
        post.url
            .ends_with(&format!("/session?directory={}", component(&work))),
        "{}",
        post.url
    );
}

#[test]
fn a_server_that_answers_as_the_time_is_up_is_not_asked_for_a_session() {
    // Node: `if (remaining <= 0) throw new Error('opencode session timed
    // out')`. The recorder cannot make a head arrive in the instant a timer
    // is due, so the clock is moved here: 149 polls that fail, each followed
    // by a pause of 100 ms, then one held for the 100 ms that are left.
    let stage = Stage::new();
    let polls = (0..149).map(|_| Served::NoHead).chain([Served::Held]);
    stage.fakes.loopback.serve("GET /global/health", polls);
    stage.fakes.processes.child(
        "opencode",
        ChildScript {
            lines: Vec::new(),
            ends: Ends::Asked,
        },
    );
    let mut driver = Driver::default();
    driver.begin(0, stage.preparing(&Wanted::default()));
    for _ in 0..149 {
        assert!(driver.run().is_empty());
        assert!(stage.fakes.time.fire_next(i64::MAX));
    }
    assert!(driver.run().is_empty());
    assert_eq!(
        stage.fakes.loopback.waits(0),
        ["GET /global/health"],
        "the last poll is held"
    );
    let healthy = served(200, br#"{"healthy":true}"#);
    assert!(stage.fakes.loopback.release("GET /global/health", healthy));
    assert!(stage.fakes.time.fire_next(i64::MAX));
    let [(_, answer)] = &driver.run()[..] else {
        panic!("the work is done");
    };
    assert_eq!(
        answer.as_ref().err().map(String::as_str),
        Some("opencode session timed out")
    );
    let asked = stage.fakes.loopback.take_asked();
    assert!(
        asked.iter().all(|request| request.method == Method::Get),
        "no session was asked for"
    );
}
