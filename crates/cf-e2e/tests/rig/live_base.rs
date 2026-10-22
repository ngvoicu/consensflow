//! What the live tests stand on, held to what it says without spending a token
//! (the harnesses are the stand-in agent here): what the rig tells of the daemon
//! and its windows (the events it wrote, the keys it pressed and when, what a
//! window printed), a rig whose stand-ins write in a Claude folder that is the
//! person's and takes away what they wrote and no more, a real window opened as
//! the app opens one, and the harness code of the product answering what a live
//! test asks of it.

use std::path::Path;
use std::time::Duration;

use cf_e2e::live::window::Open;
use cf_e2e::live::{self, ask, trace, Harness, Installed};
use cf_e2e::rig::{Project, OPEN};
use cf_e2e::Error;
use serde_json::json;

use crate::decided_result::cf;
use crate::{rig, secs, Outcome, FAKE_AGENT};

/// The time, in milliseconds since the epoch, as the rig tells its presses.
fn now_ms() -> i64 {
    jiff::Timestamp::now().as_millisecond()
}

/// The state of the chief's window, as the board says it.
fn chief_state(project: &Project) -> cf_e2e::Result<String> {
    Ok(project.lane("chief")?["activity"]["state"]
        .as_str()
        .unwrap_or_default()
        .to_owned())
}

#[test]
fn the_rig_tells_what_the_daemon_wrote_the_keys_it_pressed_and_when_and_what_a_window_printed(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    let added = project.add_member("worker")?;
    let tier = added["member"]["tier"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let chief = rig.open_frame(&format!("p{}-chief", project.id()), OPEN)?;
    let before = now_ms();
    rig.tell(
        project.id(),
        &format!("DISPATCH --tier {tier} SLEEP 60 Reply with exactly: ONE_DONE"),
    )?;
    let mut pane = String::new();
    rig.wait_for("the worker to be at work in a window", secs(60), || {
        pane = project.lane("worker")?["pane"]["id"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        Ok(project.task(1)?["state"] == "working" && !pane.is_empty())
    })?;
    // The chief pauses it: the daemon stops the turn with the Escape key.
    cf(&rig, &chief, &["task", "pause", "T-1"])?;
    rig.wait_for(
        "the daemon to press Escape into the window",
        secs(60),
        || Ok(!rig.presses(&pane, &[27]).is_empty()),
    )?;
    let after = now_ms();

    let pressed = rig.presses(&pane, &[27]);
    assert!(
        pressed.iter().all(|at| (before..=after).contains(at)),
        "{pressed:?} is not between {before} and {after}"
    );
    assert!(pressed.windows(2).all(|pair| pair[0] <= pair[1]));
    assert_eq!(rig.keys_pressed(&pane, &[27]), pressed.len());
    // Another key, another window, a window that is none: no presses.
    assert!(rig.presses(&pane, &[3]).is_empty());
    assert!(rig.presses("p9-nobody", &[27]).is_empty());

    // What the daemon wrote, numbered in order, on the clock the presses are told by.
    let events = rig.events();
    assert!(events.len() > 3, "{events:?}");
    for (at, event) in events.iter().enumerate() {
        assert_eq!(event["n"], at + 1, "{event}");
    }
    let opened = events
        .iter()
        .find(|event| event["kind"] == "task.opened")
        .ok_or("the daemon wrote no task.opened")?;
    let opened_at = trace::at_ms(opened).ok_or("task.opened says no time")?;
    assert!(
        (before..=pressed[0]).contains(&opened_at),
        "the task was opened at {opened_at}, after {before} and before the Escape at {}",
        pressed[0]
    );
    // A look at them is the same events, and may be of more by then.
    assert!(trace::Trace::read(&rig).len() >= events.len());

    // What a window printed is its screen as text: the worker's, and the chief's.
    assert!(rig.screen(&pane).contains("fake agent started on"));
    let chief_pane = chief["id"].as_str().unwrap_or_default();
    assert!(rig.screen(chief_pane).contains("fake agent started on"));
    assert_eq!(rig.screen("p9-nobody"), "");
    rig.close()?;
    Ok(())
}

#[test]
fn a_tell_waits_as_long_as_it_is_given_for_the_chief_to_be_idle_and_says_so_when_it_is_not(
) -> Outcome {
    let rig = rig()?;
    let project = Project::open(&rig, "chief", json!({}))?;
    rig.tell(project.id(), "SLEEP 8 Reply with exactly: AWAKE")?;
    rig.wait_for("the chief to be at work", secs(30), || {
        Ok(chief_state(&project)? != "idle")
    })?;
    let failed = rig
        .tell_within(
            project.id(),
            "Reply with exactly: TOO_SOON",
            Duration::from_millis(300),
        )
        .unwrap_err();
    assert!(matches!(failed, Error::Timeout(_)), "{failed}");
    assert!(
        failed
            .to_string()
            .contains("waiting for the chief to be idle"),
        "{failed}"
    );
    // Given the time, it goes in once the chief's turn is over.
    rig.tell_within(project.id(), "Reply with exactly: IN_TIME", secs(60))?;
    rig.close()?;
    Ok(())
}

/// The rig whose Claude windows are the stand-in's or the real Claude Code's by
/// the model, through a shell script: a macOS and Linux rig.
#[cfg(unix)]
mod on_a_claude_config_of_the_cases_own {
    use std::path::Path;
    use std::sync::Arc;

    use cf_e2e::live::{Harness, Installed};
    use cf_e2e::rig::{Project, Rig};
    use serde_json::json;

    use crate::kept_config::Kept;
    use crate::{secs, Outcome, FAKE_AGENT};

    /// A rig on the machine `machine`, whose real Claude is `/nowhere/claude` (started through
    /// the shim made in `shims`) and whose Claude windows run on `folder`.
    fn started(
        machine: &Path,
        shims: &Path,
        folder: &Arc<Kept>,
    ) -> Result<Rig, Box<dyn std::error::Error>> {
        let claude = Installed::at(Harness::Claude, "/nowhere/claude", machine)
            .with_claude_config(folder.clone());
        Ok(Rig::start(cf_e2e::live::rig_with_stand_ins(
            &[claude],
            shims,
            Path::new(FAKE_AGENT),
        )?)?)
    }

    #[test]
    fn a_rig_whose_claude_windows_run_on_a_config_of_the_cases_own_writes_nothing_in_the_machines(
    ) -> Outcome {
        // The machine is a folder of the test's own, with a Claude folder of the person's.
        let machine = tempfile::tempdir()?;
        let persons = machine.path().join(".claude");
        cf_e2e::files::write(&persons.join("history.jsonl"), "{}\n")?;
        let kept = tempfile::tempdir()?;
        let folder = Kept::new(kept.path().join("claude-config"), None);
        let shims = tempfile::tempdir()?;
        let rig = started(machine.path(), shims.path(), &folder)?;
        assert_eq!(
            rig.var("CLAUDE_CONFIG_DIR"),
            folder.path.to_str(),
            "the daemon and every window run on the case's folder"
        );
        let work = rig.workspace().join("live-folder");
        std::fs::create_dir_all(&work)?;
        let project = Project::open(&rig, "chief", json!({ "directory": work }))?;
        let added = project.add_member("worker")?;
        let tier = added["member"]["tier"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        rig.tell(
            project.id(),
            &format!("DISPATCH --tier {tier} Reply with exactly: WORKER_OK"),
        )?;
        rig.wait_for("the worker's task to be done", secs(60), || {
            Ok(project.task(1)?["state"] == "done")
        })?;

        // The stand-ins, the chief's and the worker's, wrote where the daemon reads Claude's:
        // in the case's folder.
        let started = rig.started();
        assert!(started.len() >= 2, "a chief and a worker: {started:?}");
        for window in &started {
            let record = folder
                .path
                .join(format!("projects/integration/{}.jsonl", window.session));
            assert!(record.is_file(), "{} was not written", record.display());
        }
        assert_eq!(folder.asked_what_it_came_to(), 0);
        rig.close()?;

        assert_eq!(folder.asked_for(), 1);
        assert_eq!(folder.asked_what_it_came_to(), 1);
        // The person's Claude folder is as it was.
        let left: Vec<_> = std::fs::read_dir(&persons)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<Result<_, _>>()?;
        assert_eq!(left, ["history.jsonl"]);
        Ok(())
    }

    #[test]
    fn a_rig_that_ends_fails_with_what_its_claude_config_says_the_run_did_and_one_dropped_is_asked_once(
    ) -> Outcome {
        let machine = tempfile::tempdir()?;
        let kept = tempfile::tempdir()?;
        let said = "the run touched the person's settings";
        let folder = Kept::new(kept.path().join("claude-config"), Some(said));
        let shims = tempfile::tempdir()?;
        let rig = started(machine.path(), shims.path(), &folder)?;
        let failed = rig.close().unwrap_err().to_string();
        assert!(failed.contains(said), "{failed}");
        assert_eq!(folder.asked_what_it_came_to(), 1);

        // A rig a failed case dropped is asked what the run came to, once, and cannot fail the case again.
        let dropped = Kept::new(kept.path().join("another"), Some(said));
        drop(started(machine.path(), shims.path(), &dropped)?);
        assert_eq!(dropped.asked_what_it_came_to(), 1);
        Ok(())
    }

    #[test]
    fn a_claude_config_that_cannot_be_made_stops_the_rig_before_anything_starts() -> Outcome {
        let machine = tempfile::tempdir()?;
        // A folder that is a file's child cannot be made.
        let file = machine.path().join("a-file");
        std::fs::write(&file, "")?;
        let folder = Kept::new(file.join("claude-config"), None);
        let shims = tempfile::tempdir()?;
        let failed = started(machine.path(), shims.path(), &folder)
            .err()
            .map(|failed| failed.to_string());
        assert!(failed.is_some_and(|failed| failed.contains("claude-config")));
        assert_eq!(folder.asked_for(), 1);
        Ok(())
    }
}

#[test]
fn a_window_opens_as_the_app_opens_one_takes_a_paste_answers_and_is_read_through_the_products_code(
) -> Outcome {
    let rig = rig()?;
    // The stand-in is the Claude Code of this rig, and asks nothing of a folder.
    let home = rig.var("HOME").unwrap_or_default().to_owned();
    let install = Installed::at(Harness::Claude, FAKE_AGENT, &home);
    // The trust it asked of the folder is said where it was asked: the stand-in is asked none.
    let window = Open::new(&rig, &install, "paste", "paste-claude")
        .without_trust()
        .open_telling(&live::Log::new("a window"), "claude")?;
    let folder = Path::new(&home)
        .join(".consensflow-candidate")
        .join("live")
        .join("paste");
    assert_eq!(window.workspace(), folder);
    assert!(
        window.session().is_some(),
        "Claude is opened on an id it is given"
    );
    assert!(window.still(secs(60)), "{}", window.screen());
    assert!(window.screen().contains("fake agent started on"));
    // A window that drew its prompt is ready, and says nothing; its screen ends in what it drew.
    assert_eq!(window.ready(), Ok(()));
    assert!(window.tail().contains("fake agent started on"));

    // The app's settings for the launch were written in the folder, and the window runs through
    // them, with no MCP server and no browser, on the model of the harness's agent.
    let settings = folder
        .join(".consensflow")
        .join("integrations")
        .join("claude");
    assert!(settings.is_dir(), "{}", settings.display());
    let argv: Vec<&str> = window.argv().iter().map(String::as_str).collect();
    assert_eq!(argv[0], FAKE_AGENT);
    assert_eq!(argv[1], "--settings");
    assert!(
        // As paths, which a Windows path spells with either slash.
        Path::new(argv[2]).starts_with(&settings),
        "{} is not in {}",
        argv[2],
        settings.display()
    );
    assert_eq!(argv[3..5], ["--strict-mcp-config", "--no-chrome"]);
    let session = window
        .session()
        .ok_or("a Claude window is opened on an id")?;
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--session-id", session.as_str()]),
        "{argv:?}"
    );
    assert!(
        argv.windows(2)
            .any(|pair| pair == ["--model", "claude-sonnet-5-5"]),
        "{argv:?}"
    );

    // A message goes in as the daemon sends it, and the answer is on the screen and in the record.
    let body = window.given("Reply with exactly: 5555")?;
    let sent = window.send(&body, "5555")?;
    assert_eq!(sent.written["ok"], true, "{sent:?}");
    assert!(sent.shown, "{}", window.screen());
    let items = window.recorded();
    assert!(
        items
            .iter()
            .any(|item| item.role == "user" && item.text.contains("Reply with exactly: 5555")),
        "{items:?}"
    );
    assert!(
        items
            .iter()
            .any(|item| item.role == "assistant" && item.text.contains("5555")),
        "{items:?}"
    );

    // A turn at work is stopped by the interrupt the harness's adapter says, as the daemon presses
    // it: the Escape writes the record Claude writes of a turn the user interrupted.
    window.paste("SLEEP 60 Reply with exactly: NEVER")?;
    window.interrupt(&ask::interrupt("claude-code")?)?;
    let stopped = live::until(secs(30), Duration::from_millis(250), || {
        window
            .recorded()
            .into_iter()
            .find(|item| item.role == "user" && item.text.contains("[Request interrupted by user]"))
    });
    assert!(stopped.is_some(), "{:?}", window.recorded());

    // A key goes in as the daemon presses one; the window is ended when asked.
    assert_eq!(window.press(&[3])?["ok"], true);
    assert!(!window.closed());
    window.kill();
    rig.wait_for("the window to be gone", secs(30), || {
        Ok(rig.exits().iter().any(|exit| exit["id"] == "paste-claude"))
    })?;
    assert!(window.closed());
    // A window that ended did not draw its prompt, and what it says is what its screen ended in.
    let ended = window.ready().unwrap_err();
    assert!(ended.contains("fake agent started on"), "{ended}");
    drop(window);
    rig.close()?;
    Ok(())
}

#[test]
fn the_harness_code_of_the_product_answers_what_a_live_test_asks_of_it() -> Outcome {
    // Built before the first question, which a run asks for first of all.
    ask::prepare()?;
    assert_eq!(ask::window_text("a\r\nb\u{1b}c")?, "a\nb\u{241b}c");
    assert_eq!(ask::console_text("a — b → €")?, "a -- b -> EUR");
    let devin = ask::interrupt("devin")?;
    assert_eq!(
        (devin.presses, devin.close_after),
        (2, Some(Duration::from_secs(1)))
    );
    let claude = ask::interrupt("claude-code")?;
    assert_eq!((claude.presses, claude.close_after), (1, None));
    let argv = [
        "/home/me/.local/bin/claude".to_owned(),
        "--model".to_owned(),
        "a b".to_owned(),
    ];
    assert_eq!(ask::program(&argv, &[])?, argv);

    // A conversation nobody kept a record of is none, and so is a session no store has.
    let nowhere = tempfile::tempdir()?;
    let here = nowhere.path().to_string_lossy().into_owned();
    let env = [
        ("HOME".to_owned(), here.clone()),
        ("CLAUDE_CONFIG_DIR".to_owned(), here.clone()),
        ("XDG_DATA_HOME".to_owned(), here),
    ];
    assert_eq!(ask::records("claude-code", "no-such-session", &env)?, None);
    assert_eq!(
        ask::newest_session("devin", Path::new("/work/a"), 0, &env)?,
        None
    );
    // A window with no command line of its own is none, and a question that cannot be answered says why.
    assert_eq!(
        ask::start(&json!({ "kind": "kimi" }), Some("s"), None)?,
        None
    );
    let failed = ask::newest_session("claude-code", Path::new("/work/a"), 0, &env).unwrap_err();
    assert!(
        failed
            .to_string()
            .contains("opens on a session id of its own choosing"),
        "{failed}"
    );
    Ok(())
}
