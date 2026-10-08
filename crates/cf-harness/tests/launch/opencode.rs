//! OpenCode's adapter and its channel, as Node's OpenCode adapter and install
//! suites, `tests/opencode-launch.test.mjs` and the OpenCode cases of its
//! role-skills suite held them (TEST-BDC-05, IMPL-BDC-07), each case under its
//! sentence: how an OpenCode window is launched, the conversation made for it
//! first, its first message through its own server, how a message reaches it
//! through the plugin, what the plugin says of the window, and what is made for
//! it to load. Each test gets a throwaway home and a stand-in `opencode` on
//! PATH.
//!
//! A window comes only from a prepare here, where Node's tests made up a
//! launch bag: a test of a window on a known conversation prepares it as that
//! conversation resumed. The servers are scripted by route
//! (`ScriptedLoopback`), and the throwaway `serve` a scripted child. What Node
//! ran as real children (a stand-in server that answered for real, a process
//! checked to be gone) stays in Node; what a window passes its child is kept
//! here by a wrapper of the scripted programs (`stage`).

use std::path::Path;
use std::rc::Rc;

use cf_harness::contract::Admission;
use cf_harness::testing::{Answer, ScriptedHost};
use serde_json::{json, Value};

mod creates;
mod install;
mod looks;
mod roles;
mod seeds;
mod stage;

use stage::{finish, pane, planned, planned_bridge, served, words, Stage, Wanted, LAUNCH, MODEL};

#[test]
fn creates_the_conversation_first_and_opens_the_tui_on_its_own_server_in_full_permission_mode() {
    let stage = Stage::new();
    stage.serves_creation("ses_abc123");
    let plan = stage.prepare(&Wanted::default()).unwrap();
    assert_eq!(plan.native_session.as_deref(), Some("ses_abc123"));
    let port = plan.argv[2].clone();
    assert!(
        !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()),
        "{port}"
    );
    let mut argv = vec![stage.home.executable.clone()];
    argv.extend(words(&[
        "--port",
        &port,
        "--hostname",
        "127.0.0.1",
        "--session",
        "ses_abc123",
    ]));
    argv.extend(words(&["--model", MODEL, "--auto"]));
    assert_eq!(plan.argv, argv);
    assert_eq!(planned(&plan, "OPENCODE_SERVER_USERNAME"), Some("opencode"));
    let password = planned(&plan, "OPENCODE_SERVER_PASSWORD").unwrap();
    assert_eq!(password.len(), 32, "24 bytes, in base64url");
    let tui = planned(&plan, "OPENCODE_TUI_CONFIG")
        .unwrap()
        .replace('\\', "/");
    let (_, rest) = tui.split_once("extensions/opencode/").unwrap();
    let (hash, file) = rest.split_once('/').unwrap();
    assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()), "{hash}");
    assert_eq!(file, "hosts/opencode-extension/tui.json");
    let bridge: Value =
        serde_json::from_str(planned(&plan, "CF_OPENCODE_SESSION_BRIDGE").unwrap()).unwrap();
    assert_eq!(bridge["launchId"], LAUNCH);
    // One throwaway server, on the port the window then takes, in the
    // folder it works in, which is stopped before the id returns.
    let started = stage.started.started.borrow();
    assert_eq!(started.len(), 1);
    assert_eq!(
        started[0].args,
        words(&["serve", "--port", &port, "--hostname", "127.0.0.1"])
    );
    assert_eq!(
        started[0].cwd.as_deref(),
        Some(Path::new(&stage.home.work()))
    );
    let sent = stage.fakes.loopback.take_asked();
    assert_eq!(
        sent[0].url,
        format!("http://127.0.0.1:{port}/global/health")
    );
}

#[test]
fn resumes_the_conversation_it_has_without_creating_another() {
    let stage = Stage::new();
    let wanted = Wanted {
        resume: Some("ses_old".to_owned()),
        message: None,
        ..Wanted::default()
    };
    let plan = stage.prepare(&wanted).unwrap();
    assert!(
        stage.started.started.borrow().is_empty(),
        "no server was started"
    );
    assert!(stage.fakes.loopback.take_asked().is_empty());
    assert_eq!(plan.native_session.as_deref(), Some("ses_old"));
    assert_eq!(plan.argv[5..], words(&["--session", "ses_old", "--auto"]));
}

#[test]
fn delivers_through_the_real_channel_a_claim_then_the_plugin() {
    let stage = Stage::new();
    let plan = stage.fresh("ses_abc123");
    stage.fakes.loopback.take_asked();
    let bridge: Value =
        serde_json::from_str(planned(&plan, "CF_OPENCODE_SESSION_BRIDGE").unwrap()).unwrap();
    let token = bridge["token"].as_str().unwrap().to_owned();
    let host = Rc::new(ScriptedHost::default());
    let claim = |times: usize| {
        host.answer(
            "pane.claim",
            (0..times).map(|_| Answer::Now(Ok(json!({ "ok": true })))),
        )
    };
    let deliver = |text: &'static str| {
        let (window, host, pane) = (Rc::clone(&plan.window), Rc::clone(&host), pane());
        finish(&stage.fakes, async move {
            window.deliver(&*host, &pane, text).await
        })
    };
    claim(3);
    stage.fakes.loopback.serve(
        "POST /deliver",
        [
            served(200, br#"{"ok":true,"admitted":true}"#),
            served(200, br#"{"ok":true,"admitted":true}"#),
            served(
                200,
                br#"{"ok":false,"admitted":false,"bytesWritten":0,"error":"native-session-changed"}"#,
            ),
        ],
    );
    assert_eq!(deliver("hi"), Ok(Admission::Admitted { queued: true }));
    // OpenCode refuses half a character too: Node's second text began with half a surrogate pair, which it dropped.
    let messy = "half  of it, \u{1b}[31mred\u{1b}[0m and 50%\r60%";
    assert_eq!(deliver(messy), Ok(Admission::Admitted { queued: true }));
    assert_eq!(
        deliver("hi"),
        Ok(Admission::Refused {
            reason: "native-session-changed".to_owned()
        })
    );
    let claims = host.take_asked();
    let claim = (
        "pane.claim".to_owned(),
        json!({ "pane": "s1-zeus", "generation": 2 }),
    );
    assert_eq!(claims, [claim.clone(), claim.clone(), claim]);
    let posted = stage.fakes.loopback.take_asked();
    assert_eq!(posted.len(), 3);
    assert_eq!(posted[0].url, planned_bridge(&plan, "/deliver"));
    assert_eq!(
        posted[0].headers[0],
        ("authorization".to_owned(), format!("Bearer {token}"))
    );
    let body = |at: usize| -> Value {
        serde_json::from_slice(posted[at].body.as_deref().unwrap()).unwrap()
    };
    let mut first = body(0);
    assert!(first["expiresAt"].is_number());
    first["expiresAt"] = json!("a number");
    assert_eq!(
        first,
        json!({ "launchId": LAUNCH, "sessionId": "ses_abc123", "text": "hi", "expiresAt": "a number" })
    );
    assert_eq!(
        body(1)["text"],
        "half  of it, \u{241b}[31mred\u{241b}[0m and 50%\u{240d}60%"
    );
}
