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
mod in_the_machines_claude_folder {
    use std::fs;
    use std::path::{Path, PathBuf};

    use cf_e2e::live::{Harness, Installed};
    use cf_e2e::rig::{project_name, Project, Rig};
    use serde_json::json;

    use crate::{secs, Outcome, FAKE_AGENT};

    /// A file of the machine's Claude folder `config`, written.
    fn at(config: &Path, relative: &str, text: &str) -> cf_e2e::Result<PathBuf> {
        let file = config.join(relative);
        cf_e2e::files::write(&file, text)?;
        Ok(file)
    }

    #[test]
    fn a_rig_whose_stand_ins_write_in_the_machines_claude_folder_takes_away_what_they_wrote_and_nothing_else(
    ) -> Outcome {
        // The machine is a folder of the test's own: the rig reads its Claude from there.
        let machine = tempfile::tempdir()?;
        let config = machine.path().join(".claude");
        let theirs = [
            at(&config, "projects/integration/theirs.jsonl", "{}\n")?,
            at(&config, "projects/-Users-me-work/c.jsonl", "{}\n")?,
            at(
                &config,
                "sessions/999999.json",
                r#"{"pid":999999,"sessionId":"theirs","status":"idle"}"#,
            )?,
            at(&config, "history.jsonl", "{}\n")?,
        ];
        let shims = tempfile::tempdir()?;
        let claude = Installed::at(Harness::Claude, "/nowhere/claude", machine.path());
        let mut rig = Rig::start(cf_e2e::live::rig_with_stand_ins(
            &[claude],
            shims.path(),
            Path::new(FAKE_AGENT),
        )?)?;
        // The project's folder: what a real Claude would keep of it is taken away too.
        let work = rig.workspace().join("live-folder");
        fs::create_dir_all(&work)?;
        let memory = at(
            &config,
            &format!(
                "projects/{}/memory/MEMORY.md",
                project_name(&work.to_string_lossy())
            ),
            "# what it learned",
        )?;
        rig.forget_claude_memory_of(&work);
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

        // The stand-ins wrote where the machine's Claude keeps its own, and not in the rig's home.
        let started = rig.started();
        assert!(started.len() >= 2, "a chief and a worker: {started:?}");
        let records: Vec<PathBuf> = started
            .iter()
            .map(|window| config.join(format!("projects/integration/{}.jsonl", window.session)))
            .collect();
        for record in &records {
            assert!(record.is_file(), "{} was not written", record.display());
        }
        rig.close()?;

        // Gone: what they wrote. Left: all that was the person's, and the folder they shared.
        for record in &records {
            assert!(!record.exists(), "{} is left", record.display());
        }
        for window in &started {
            assert!(!config
                .join(format!("sessions/{}.json", window.pid))
                .exists());
        }
        assert!(
            !memory.exists(),
            "what Claude kept of the run's folder is taken away"
        );
        for kept in &theirs {
            assert!(kept.exists(), "{} was the person's", kept.display());
        }
        assert!(config.join("projects/integration").is_dir());
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
    let window = Open::new(&rig, &install, "paste", "paste-claude")
        .without_trust()
        .open()?;
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
