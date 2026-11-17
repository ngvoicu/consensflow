//! Isolated live probe of Claude's `--add-dir` question and the app's first delivery.
use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant};

use cf_e2e::live::members::{delivered_result, member, stand_in_chief, Members, Plan};
use cf_e2e::live::window::{trust_claude, Open};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result};
use regex::Regex;
use serde_json::{json, Value};

const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

fn snapshot(rig: &Rig, id: &str) -> Result<Vec<String>> {
    let answer = rig.host(
        "pane.snapshot",
        json!({"id": id, "generation": 1, "bottom": 40}),
    )?;
    Ok(answer["bottom"]
        .as_array()
        .into_iter()
        .flatten()
        .map(live::text)
        .collect())
}

fn log_screen(log: &Log, label: &str, lines: &[String], added: &Path) {
    // Never write possible credentials to a log. A long mixed identifier or a credential
    // prefix is sufficient to withhold the entire screen, including false positives.
    let Ok(unsafe_shape) = Regex::new(
        r"(?i)(?:[A-Za-z0-9_-]{24,}|sk-|ghp_|github_pat_|bearer\s+\S+|password\s*[:=]|token\s*[:=])",
    ) else {
        log.say(format!(
            "{label}: full screen withheld because the possible-key filter could not start"
        ));
        return;
    };
    let text = lines.join("\n");
    let text = text.replace(
        &added.to_string_lossy().to_string(),
        "<run-owned added folder>",
    );
    let text = match added.canonicalize() {
        Ok(real) => text.replace(
            &real.to_string_lossy().to_string(),
            "<run-owned added folder>",
        ),
        Err(_) => text,
    };
    if unsafe_shape.is_match(&text) {
        log.say(format!(
            "{label}: full screen withheld by the possible-key filter"
        ));
    } else {
        log.block(label, &text);
    }
}

fn pane(rig: &Rig, id: &str, cwd: &Path, argv: &[String]) -> Result<()> {
    let answer = rig.host("pane.open", json!({"id": id, "generation": 1, "cwd": cwd, "argv": argv, "env": {}, "size": {"rows": 40, "cols": 120}}))?;
    if answer["ok"] == true {
        Ok(())
    } else {
        Err(Error::Daemon(format!("pane.open failed: {answer}")))
    }
}

fn close(rig: &Rig, id: &str) -> Result<()> {
    let answer = rig.close_pane(json!({"id": id, "generation": 1}))?;
    if answer["ok"] == true {
        Ok(())
    } else {
        Err(Error::Daemon(format!("pane close failed: {answer}")))
    }
}

fn press(rig: &Rig, id: &str, bytes: &[u8]) -> Result<()> {
    let answer: Value = rig.host(
        "pane.input",
        json!({"id": id, "generation": 1, "bytes": bytes}),
    )?;
    if answer["ok"] == true {
        Ok(())
    } else {
        Err(Error::Daemon("pane input failed".into()))
    }
}

/// A real Claude worker must pass the added-folder question on its own and
/// receive its first task. The screen's contents are not logged: only the
/// presence of the two fixed signs is retained.
fn app_delivers_first_message(log: &Log) -> Result<()> {
    let plan =
        Plan::new("added-folder-trust-probe", stand_in_chief()).with(member(Harness::Claude));
    let mut members = Members::open(&plan, Path::new(FAKE_AGENT), log)?;
    let outcome = (|| -> Result<()> {
        let agent = live::agents::id(Harness::Claude);
        members.add(&agent, &[])?;
        let task = members.give(&agent, "Reply with exactly: ADDED_TRUST_OK")?;
        let (mut saw_question, mut saw_prompt, mut done, mut received) =
            (false, false, false, false);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(180) {
            if let Some(window) = members.window_of(&agent)? {
                // Rig::screen keeps the output as well as the current snapshot;
                // either can show the short-lived question before the watch answers.
                let output = members.screen(window.id());
                saw_question |= output.contains("Yes, add it");
                if let Some(snapshot) = members.snapshot(&window.pane, 12) {
                    let tail = snapshot["tail"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(live::text)
                        .collect::<Vec<_>>()
                        .join("\n");
                    saw_question |= tail.contains("Yes, add it");
                    saw_prompt |= tail.contains("bypass permissions on");
                }
            }
            done = members.task(task)?["state"] == "done";
            received = delivered_result(&members.inbox(Some(members.chief()))?)
                .is_some_and(|body| body.contains("ADDED_TRUST_OK"));
            if saw_question && saw_prompt && done && received {
                break;
            }
            sleep(Duration::from_millis(250));
        }
        log.say(format!("app worker: added question seen={saw_question}; bypass prompt seen={saw_prompt}; first task done={done}; marker delivered={received}"));
        if saw_question && saw_prompt && done && received {
            Ok(())
        } else {
            Err(Error::Daemon(
                "the app did not prove the added-folder question, prompt and first delivery"
                    .to_owned(),
            ))
        }
    })();
    let closed = members.close();
    outcome?;
    closed
}

#[test]
#[ignore = "opt-in: isolated Claude Code added-folder trust probe"]
fn added_folder_trust_question() -> Result<()> {
    let log = Log::begin("live added-folder probe")?;
    ask::prepare()?;
    let install = log.found(Harness::Claude)?;
    let rig = Rig::start(live::rig(std::slice::from_ref(&install), FAKE_AGENT))?;
    let outcome = (|| -> Result<()> {
        let added = rig.home().join("probe-role");
        cf_e2e::door::make_dir(&added)?;
        let (mut argv, cwd) = {
            let base = Open::new(&rig, &install, "added-folder-probe", "base").open(&log)?;
            let result = (base.argv().to_vec(), base.workspace().to_path_buf());
            base.kill();
            result
        };
        argv.extend(["--add-dir".to_owned(), added.to_string_lossy().into_owned()]);
        pane(&rig, "unanswered", &cwd, &argv)?;
        sleep(Duration::from_secs(8));
        let first = snapshot(&rig, "unanswered")?;
        log_screen(&log, "first launch, unanswered", &first, &added);
        if !first.join("\n").contains("❯ No, exit") || !first.join("\n").contains("Yes, add it") {
            return Err(Error::Daemon(
                "the added-folder question did not open on No".to_owned(),
            ));
        }
        close(&rig, "unanswered")?;
        pane(&rig, "answer", &cwd, &argv)?;
        sleep(Duration::from_secs(8));
        let repeated = snapshot(&rig, "answer")?;
        log_screen(&log, "second launch, before answering", &repeated, &added);
        let asked = repeated.join("\n").contains("Yes, add it");
        if asked {
            press(&rig, "answer", b"\x1b[B")?;
            sleep(Duration::from_secs(2));
            let on_yes = snapshot(&rig, "answer")?;
            log_screen(&log, "second launch, after Down", &on_yes, &added);
            if !on_yes.join("\n").contains("❯ Yes, add it") {
                return Err(Error::Daemon("Down did not select Yes, add it".to_owned()));
            }
            press(&rig, "answer", b"\r")?;
            sleep(Duration::from_secs(5));
            let entered = snapshot(&rig, "answer")?;
            log_screen(&log, "second launch, after Enter", &entered, &added);
            let at_prompt = entered.join("\n").contains("bypass permissions on");
            log.say(format!("manual answer reached bypass prompt: {at_prompt}"));
            if !at_prompt {
                return Err(Error::Daemon(
                    "the manual answer did not reach the bypass prompt".to_owned(),
                ));
            }
        } else {
            return Err(Error::Daemon(
                "the unanswered added-folder question did not repeat".to_owned(),
            ));
        }
        close(&rig, "answer")?;
        pane(&rig, "after-answer", &cwd, &argv)?;
        sleep(Duration::from_secs(8));
        let after = snapshot(&rig, "after-answer")?;
        log_screen(&log, "third launch, after answer", &after, &added);
        if !after.join("\n").contains("Yes, add it") {
            return Err(Error::Daemon(
                "the added-folder question did not repeat in a new session".to_owned(),
            ));
        }
        close(&rig, "after-answer")?;
        log.say(format!(
            "first asks: {}; repeats unanswered: {}; asks after answer: {}",
            first.join("\n").contains("Yes, add it"),
            asked,
            after.join("\n").contains("Yes, add it")
        ));
        // Exercise the kit's own answerer with the same launch, rather than
        // only typing the answer by hand in the recording above.
        let kit_answer = trust_claude(&rig, &argv, &cwd)?;
        log.say(format!(
            "kit answerer for the run-owned added folder: {kit_answer}"
        ));
        if kit_answer != "trusted" {
            return Err(Error::Daemon(
                "the kit did not answer the added-folder question".to_owned(),
            ));
        }
        // The workspace is already trusted by the kit. Observe a directory
        // beneath it separately from the untrusted sibling used above.
        let covered = cwd.join("already-trusted-parent-child");
        cf_e2e::door::make_dir(&covered)?;
        let mut covered_argv = argv.clone();
        if let Some(last) = covered_argv.last_mut() {
            *last = covered.to_string_lossy().into_owned();
        }
        pane(&rig, "trusted-parent", &cwd, &covered_argv)?;
        sleep(Duration::from_secs(8));
        let covered_screen = snapshot(&rig, "trusted-parent")?;
        log_screen(
            &log,
            "added folder under a trusted workspace",
            &covered_screen,
            &covered,
        );
        log.say(format!(
            "added folder under trusted workspace asks: {}; at bypass prompt: {}",
            covered_screen.join("\n").contains("Yes, add it"),
            covered_screen.join("\n").contains("bypass permissions on")
        ));
        close(&rig, "trusted-parent")?;
        Ok(())
    })();
    let closed = rig.close_within(Duration::from_secs(30));
    outcome?;
    closed?;
    app_delivers_first_message(&log)?;
    log.passed();
    Ok(())
}
