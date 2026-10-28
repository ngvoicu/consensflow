//! A Muse window against a scripted pane host, a manual clock and session logs written
//! where Muse writes them: which session it names, its readiness, a resume Muse would
//! not make, a turn taken back, and a session it was switched to.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::json;

use super::*;
use crate::testing::{finished, Answer, Driver, Fakes, ScriptedHost, EPOCH_MS};

/// The pane the window opened in.
fn pane() -> Pane {
    Pane {
        id: "p1-muse-1".to_owned(),
        generation: 1,
    }
}

/// A home with Muse's data folder in it, and the fakes a window runs on.
struct Home {
    dir: tempfile::TempDir,
    env: Env,
    fakes: Fakes,
}

impl Home {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let env = Env::from_vars([
            ("HOME", dir.path().to_str().unwrap()),
            ("XDG_DATA_HOME", dir.path().join("data").to_str().unwrap()),
        ]);
        let fakes = Fakes::new(&env);
        Self { dir, env, fakes }
    }

    fn said(&self) -> PathBuf {
        self.dir.path().join("sessions.jsonl")
    }

    /// A window on `session` where given.
    fn window(&self, session: Option<&str>) -> MuseWindow {
        let services = self.fakes.services(&self.env, Path::new(""));
        MuseWindow::new(&services, session, self.said(), 30_000)
    }

    /// Writes `log` as the session `id`'s, its first lines naming the process `pid`.
    fn log(&self, id: &str, pid: u32, log: &str) {
        let folder = self
            .dir
            .path()
            .join("data/muse/sessions/2026/09/19")
            .join(id);
        fs::create_dir_all(&folder).unwrap();
        let facts = json!({
            "schema_version": 1, "stream": { "kind": "session", "id": id },
            "payload": { "kind": "route_facts", "record": { "cwd": "/work", "pid": pid } },
        });
        fs::write(folder.join("session.jsonl"), format!("{facts}\n{log}")).unwrap();
    }

    /// The hook's word that the window started `id` (`source` how).
    fn hook_said(&self, id: &str, source: &str) {
        let line = json!({ "session": id, "source": source });
        let mut text = fs::read_to_string(self.said()).unwrap_or_default();
        text.push_str(&format!("{line}\n"));
        fs::write(self.said(), text).unwrap();
    }
}

/// A UUIDv7 made `after_ms` after the tests' clock starts.
fn v7(after_ms: i64) -> String {
    let hex = format!("{:012x}", EPOCH_MS + after_ms);
    format!(
        "{}-{}-7000-8000-00000000000{}",
        &hex[..8],
        &hex[8..],
        after_ms % 10
    )
}

const RULE: &str = "──────────────────────────────────────────────────────";

/// What the pane host says of a window whose last lines are `lines`.
fn snapshot(lines: &[&str]) -> Answer {
    Answer::Now(Ok(
        json!({ "ok": true, "bottom": lines, "unsent": false, "typed": 0 }),
    ))
}

/// A window at its prompt, its box holding `typed`.
fn at_prompt(typed: &str) -> Answer {
    let first = format!("❯ {typed}");
    snapshot(&[
        "  Muse Code 1.4.13",
        "",
        "── Voice input (⌥ + v to start) ──────────────────────",
        &first,
        RULE,
        "  echo · /work · YOLO",
    ])
}

fn ok() -> Answer {
    Answer::Now(Ok(json!({ "ok": true })))
}

/// The requests the host was asked, by operation, with a paste's words.
fn asked(host: &ScriptedHost) -> Vec<String> {
    host.take_asked()
        .into_iter()
        .map(|(op, body)| match op.as_str() {
            "pane.write_paste" => format!("paste {}", body["body"].as_str().unwrap()),
            "pane.input" => format!("keys {}", body["bytes"]),
            _ => op,
        })
        .collect()
}

#[test]
fn a_fresh_window_names_the_session_its_own_process_opened_before_any_hook_says_it() {
    let home = Home::new();
    let window = Rc::new(home.window(None));
    window.opened(Some(40_085));
    let host = Rc::new(ScriptedHost::default());
    // Its first message is pasted only once it is ready, and it is held until its
    // session is known: no hook speaks before a first prompt, so the log naming its
    // process is the one way it learns it.
    assert_eq!(
        finished(window.ready(&*host, &pane())),
        Ok(Readiness::Held(Held::Because(HOLD.to_owned())))
    );
    let mut driver = Driver::default();
    let (starting, asking) = (Rc::clone(&window), Rc::clone(&host));
    driver.begin(0, async move { starting.started(&*asking).await });
    assert!(driver.run().is_empty(), "nothing says a session yet");
    // Another process's session, and then the window's own, a second in.
    home.log(&v7(500), 40_086, "");
    home.fakes.time.fire_next(EPOCH_MS + 1_000);
    assert!(driver.run().is_empty());
    let own = v7(900);
    home.log(&own, 40_085, "");
    home.fakes.time.fire_next(EPOCH_MS + 2_000);
    assert_eq!(driver.run(), [(0, Ok(Some(own.clone())))]);
    assert!(asked(&host).is_empty(), "nothing is pasted as it starts");
    host.answer("pane.snapshot", [at_prompt("")]);
    assert_eq!(
        finished(window.ready(&*host, &pane())),
        Ok(Readiness::Ready)
    );
}

#[test]
fn a_fresh_window_whose_process_says_nothing_is_given_up_with_why_and_a_hook_s_word_will_do() {
    let home = Home::new();
    let window = Rc::new(home.window(None));
    let host = Rc::new(ScriptedHost::default());
    let mut driver = Driver::default();
    let (starting, asking) = (Rc::clone(&window), Rc::clone(&host));
    driver.begin(0, async move { starting.started(&*asking).await });
    let mut settled = driver.run();
    while home.fakes.time.fire_next(EPOCH_MS + 31_000) {
        settled.extend(driver.run());
    }
    assert_eq!(
        settled,
        [(
            0,
            Err("Muse never said which session it opened (no log of its process came)".to_owned())
        )]
    );
    assert_eq!(finished(window.came_up()), None, "still nothing");
    // The hook said it since: the window came up after all.
    home.hook_said("01a0bd3d-2800-7000-8000-000000000001", "startup");
    assert_eq!(
        finished(window.came_up()),
        Some(Some("01a0bd3d-2800-7000-8000-000000000001".to_owned()))
    );
}

const PICKER: [&str; 4] = [
    "",
    "  that session is already open in another window — pick another session (held by pid 40913 on",
    "  muse-host.local for 12s; if that window is gone, run `kill 40913` on muse-host.local)",
    "  0 / 0 · 0%  enter resume  esc exit  ctrl+c exit  tab focus sort/filter",
];

#[test]
fn a_resume_muse_will_not_make_for_another_window_is_unfit_in_muse_s_words_and_takes_nothing() {
    let home = Home::new();
    let session = "01a17bc9-e758-77f3-ac8d-a725edf373ae";
    let window = home.window(Some(session));
    window.opened(Some(40_085));
    let host = ScriptedHost::default();
    assert_eq!(finished(window.started(&host)), Ok(None));
    assert!(
        asked(&host).is_empty(),
        "a resumed window knows its session"
    );
    host.answer("pane.snapshot", [snapshot(&PICKER)]);
    let Ok(Readiness::Unfit(why)) = finished(window.ready(&host, &pane())) else {
        panic!("not unfit");
    };
    assert!(
        why.starts_with("Muse did not resume the session: that session is already open"),
        "{why}"
    );
    assert!(why.contains("kill 40913"), "{why}");
    assert_eq!(asked(&host), ["pane.snapshot"]);
}

#[test]
fn a_window_is_ready_at_its_prompt_and_held_before_it_for_another_session_or_for_words_the_human_left(
) {
    let home = Home::new();
    let session = v7(100);
    home.log(&session, 40_085, "");
    let window = home.window(Some(&session));
    window.opened(Some(40_085));
    let host = ScriptedHost::default();
    host.answer(
        "pane.snapshot",
        [snapshot(&["  Muse Code 1.4.13", ""]), at_prompt("")],
    );
    let ready = |host: &ScriptedHost| finished(window.ready(host, &pane())).unwrap();
    assert_eq!(
        ready(&host),
        Readiness::Held(Held::Because(STARTING.to_owned()))
    );
    assert_eq!(ready(&host), Readiness::Ready);
    // The human typed into it: the box on the screen says whether anything is left.
    let typed = |typed: &str| {
        let first = format!("❯ {typed}");
        Answer::Now(Ok(json!({
            "ok": true, "unsent": true, "typed": 3, "typedQuietMs": 6_000, "outputQuietMs": 6_000,
            "bottom": ["── Voice input (⌥ + v to start) ────────", first, RULE, "  echo · YOLO"],
        })))
    };
    host.answer("pane.snapshot", [typed("half a sentence"), typed("")]);
    assert_eq!(ready(&host), Readiness::Held(Held::Unsent));
    assert_eq!(ready(&host), Readiness::Ready);
    // A session the window started since (a /clear) is not the one it is known on.
    let cleared = v7(5_000);
    home.log(&cleared, 40_085, "");
    assert_eq!(ready(&host), Readiness::Held(Held::ShowsAnother));
    assert!(matches!(
        finished(window.deliver(&host, &pane(), "words")).unwrap(),
        Admission::Refused { .. }
    ));
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some(cleared.as_str()));
}

/// A log of one turn whose interrupt came before its first word: the run cancelled
/// and its message put back.
fn retracted() -> &'static str {
    include_str!("../../../tests/fixtures/muse/retracted.jsonl")
}

/// The fixture's first turn alone, up to its retract.
fn taken_back() -> String {
    let mut kept = String::new();
    for line in retracted().lines() {
        kept.push_str(line);
        kept.push('\n');
        if line.contains("\"run_retracted\"") {
            break;
        }
    }
    kept
}

#[test]
fn a_turn_taken_back_is_read_so_and_its_box_cleared_before_the_next_paste_only_where_it_holds_words(
) {
    let home = Home::new();
    let session = v7(100);
    home.log(&session, 40_085, &taken_back());
    let window = home.window(Some(&session));
    window.opened(Some(40_085));
    let observed = finished(window.observe()).unwrap();
    assert!(observed.took_back && observed.settled, "{observed:?}");
    let host = ScriptedHost::default();
    host.answer("pane.snapshot", [at_prompt("Stop me before a word.")]);
    host.answer("pane.input", [ok()]);
    host.answer("pane.write_paste", [ok()]);
    let delivered = finished(window.deliver(&host, &pane(), "The next one.")).unwrap();
    assert_eq!(delivered, Admission::Admitted { queued: false });
    assert_eq!(
        asked(&host),
        ["pane.snapshot", "keys [3]", "paste The next one."]
    );
    // Read again taken back, but the box is empty now: Control-C would stop a turn at
    // work there, and nothing is pressed.
    finished(window.observe()).unwrap();
    host.answer("pane.snapshot", [at_prompt("")]);
    host.answer("pane.write_paste", [ok()]);
    finished(window.deliver(&host, &pane(), "Again.")).unwrap();
    assert_eq!(asked(&host), ["pane.snapshot", "paste Again."]);
}

#[test]
fn a_turn_taken_back_whose_box_is_out_of_view_is_refused_in_words_and_nothing_is_pressed_or_pasted()
{
    let home = Home::new();
    let session = v7(100);
    home.log(&session, 40_085, &taken_back());
    let window = home.window(Some(&session));
    window.opened(Some(40_085));
    assert!(finished(window.observe()).unwrap().took_back);
    // A put-back brief taller than the host's last lines: Muse keeps the box's tail in
    // view under its rule, and its first line, with the mark, is out of it (T-829's probe,
    // Muse 1.4.14, 45 lines in a 40-row window).
    let mut tall = vec!["── Voice input (⌥ + v to start) ──────────────────────".to_owned()];
    tall.extend((9..=45).map(|line| format!("  l{line:02}")));
    tall.extend([RULE.to_owned(), "  echo · /work · YOLO".to_owned()]);
    let host = ScriptedHost::default();
    host.answer(
        "pane.snapshot",
        [Answer::Now(Ok(
            json!({ "ok": true, "bottom": tall, "unsent": false, "typed": 0 }),
        ))],
    );
    let refused = finished(window.deliver(&host, &pane(), "The next one.")).unwrap();
    let Admission::Refused { reason } = refused else {
        panic!("not refused: {refused:?}");
    };
    assert!(
        reason.contains("input box") && reason.contains("out of view"),
        "{reason}"
    );
    assert_eq!(
        asked(&host),
        ["pane.snapshot"],
        "nothing pressed, nothing pasted"
    );
    // Once the box shows whole again, it is cleared and the paste goes in.
    host.answer("pane.snapshot", [at_prompt("Stop me before a word.")]);
    host.answer("pane.input", [ok()]);
    host.answer("pane.write_paste", [ok()]);
    finished(window.deliver(&host, &pane(), "The next one.")).unwrap();
    assert_eq!(
        asked(&host),
        ["pane.snapshot", "keys [3]", "paste The next one."]
    );
}

#[test]
fn the_engine_is_told_the_box_is_empty_only_where_muse_draws_it_empty() {
    let home = Home::new();
    let window = home.window(Some(&v7(100)));
    let of = |answer: Answer| match answer {
        Answer::Now(Ok(snapshot)) => snapshot,
        _ => unreachable!(),
    };
    assert!(window.box_read_empty(&of(at_prompt(""))));
    assert!(!window.box_read_empty(&of(at_prompt("half a sentence"))));
    // A box whose first line is out of view, and a screen with no box: not empty.
    let tall =
        json!({ "ok": true, "bottom": ["── Voice input ──────────", "  l44", "  l45", RULE] });
    assert!(!window.box_read_empty(&tall));
    assert!(!window.box_read_empty(&of(snapshot(&PICKER))));
}

#[test]
fn a_window_follows_the_session_its_hook_says_it_resumed_into() {
    let home = Home::new();
    let session = v7(100);
    home.log(&session, 40_085, "");
    let window = home.window(Some(&session));
    window.opened(Some(40_085));
    assert_eq!(finished(window.observe()).unwrap().switched, None);
    home.hook_said(&session, "startup");
    assert_eq!(finished(window.observe()).unwrap().switched, None);
    // `/resume` of an older session: only the hook says it.
    let older = "01a00000-0000-7000-8000-000000000009";
    home.hook_said(older, "resume");
    assert_eq!(
        finished(window.observe()).unwrap().switched.as_deref(),
        Some(older)
    );
    window.follow(older);
    assert_eq!(finished(window.observe()).unwrap().switched, None);
}

#[test]
fn a_session_the_window_s_process_opens_after_its_hook_spoke_is_the_one_it_shows() {
    let home = Home::new();
    let session = v7(100);
    home.log(&session, 40_085, "");
    let window = home.window(Some(&session));
    window.opened(Some(40_085));
    home.hook_said(&session, "startup");
    // A /clear opens a session at once; its hook speaks only at its first prompt. The
    // session began after the hook's word was written (the clock of both is the
    // machine's).
    use cf_base::time::Clock;
    let now = cf_base::time::SystemClock.now_ms();
    let hex = format!("{:012x}", now + 2_000);
    let cleared = format!("{}-{}-7000-8000-000000000042", &hex[..8], &hex[8..]);
    home.log(&cleared, 40_085, "");
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some(cleared.as_str()));
}

#[test]
fn a_window_cleared_with_no_hook_and_no_word_of_its_process_is_followed_by_its_log() {
    let home = Home::new();
    let session = v7(100);
    home.log(&session, 40_085, "");
    let window = home.window(Some(&session));
    window.opened(Some(40_085));
    assert_eq!(finished(window.observe()).unwrap().switched, None);
    // `/clear`: a new session started from the window's, its log naming no process, and
    // no SessionStart hook for it (Muse 1.4.13).
    let cleared = v7(4_000);
    let folder = home
        .dir
        .path()
        .join("data/muse/sessions/2026/09/19")
        .join(&cleared);
    fs::create_dir_all(&folder).unwrap();
    let started = json!({
        "schema_version": 1, "stream": { "kind": "session", "id": cleared },
        "payload_type": "session.started",
        "payload": { "kind": "session_started", "previous_session_stream": { "kind": "session", "id": session } },
    });
    fs::write(folder.join("session.jsonl"), format!("{started}\n")).unwrap();
    let observed = finished(window.observe()).unwrap();
    assert_eq!(observed.switched.as_deref(), Some(cleared.as_str()));
    let host = ScriptedHost::default();
    assert_eq!(
        finished(window.ready(&host, &pane())).unwrap(),
        Readiness::Held(Held::ShowsAnother)
    );
    window.follow(&cleared);
    assert_eq!(finished(window.observe()).unwrap().switched, None);
}
