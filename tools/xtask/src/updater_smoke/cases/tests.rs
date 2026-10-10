use std::fs;
use std::path::Path;
use std::time::Duration;

use cf_base::env::Env;

use super::*;
use crate::context::Context;
use crate::updater_smoke::build::Release;
use crate::updater_smoke::bundle::digest_manifest;
use crate::updater_smoke::case::Given;
use crate::updater_smoke::feed::feed_document;
use crate::updater_smoke::processes::{until, Waits};
use crate::updater_smoke::say::Line;
use crate::updater_smoke::signing::{generate_key, tauri_bin};
use crate::updater_smoke::testing::{env, fake_bundle, https_get, stand_in_for_the_app, Fake};

fn names() -> Vec<String> {
    specs().into_iter().map(|spec| spec.name).collect()
}

#[test]
fn the_cases_are_these_in_this_order_with_the_names_the_plants_and_the_options_ask_for() {
    assert_eq!(
        names(),
        [
            "the update installs and restarts on the bundle's cf, keeps the ledger and repairs its own terminal command",
            "the update of a home that took the way back to Node starts the bundle's cf on the ledger Node's daemon wrote, and leaves the file",
            "an update signed by another key is refused and the installed app stays intact and running",
            "a signed update whose bundle fails the check (without-cf) is refused and the installed app stays intact and running",
            "a signed update whose bundle fails the check (tampered) is refused and the installed app stays intact and running",
            "an app replaced by hand, with the app quit, repairs the terminal command at its first start and keeps the ledger",
            "an app copied over by hand, with the app quit, repairs the terminal command at its first start and keeps the ledger",
        ]
    );
}

#[test]
fn a_word_asks_for_the_cases_whose_names_hold_it_and_none_asks_for_all() {
    let chosen = |words: &[&str]| -> Vec<usize> {
        let words: Vec<String> = words.iter().map(ToString::to_string).collect();
        names()
            .iter()
            .enumerate()
            .filter(|(_, name)| asked(name, &words))
            .map(|(at, _)| at)
            .collect()
    };
    assert_eq!(chosen(&[]), [0, 1, 2, 3, 4, 5, 6]);
    // What the product's plants ask for.
    assert_eq!(chosen(&["the update installs and restarts"]), [0]);
    assert_eq!(chosen(&["refused"]), [2, 3, 4]);
    assert_eq!(chosen(&["without-cf"]), [3]);
    assert_eq!(chosen(&["tampered"]), [4]);
    assert_eq!(chosen(&["by hand"]), [5, 6]);
    assert_eq!(chosen(&["replaced", "copied over"]), [5, 6]);
    assert_eq!(chosen(&["way back"]), [1]);
    assert_eq!(chosen(&["nothing of the kind"]), Vec::<usize>::new());
}

/// The inputs of a run, of fake bundles, where the Tauri CLI can make the run's key.
fn inputs(folder: &Path, release: Release, waits: Waits) -> Option<Inputs> {
    let root = Context::new(&Env::default()).unwrap().root;
    if !tauri_bin(&root).exists() {
        eprintln!("skipped: the Tauri CLI is not installed");
        return None;
    }
    let env = env();
    let key = generate_key(&root, &folder.join("keys"), &env).unwrap();
    let from = fake_bundle(&folder.join("from"), &Fake::default());
    let to = fake_bundle(
        &folder.join("to"),
        &Fake {
            version: "3.0.0-alpha.83",
            node: false,
            ..Fake::default()
        },
    );
    Some(
        Inputs::load(&Given {
            from_app: &from,
            release,
            to_app: &to,
            key: &key,
            keep: false,
            waits,
            machines: &folder.join("machines"),
            checkout: &root,
            env: &env,
        })
        .unwrap(),
    )
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn the_way_back_is_a_case_of_the_flip_release_alone() {
    let folder = tempfile::tempdir().unwrap();
    let Some(flip) = inputs(
        &folder.path().join("a"),
        Release::Flip,
        Waits::new(Duration::from_secs(1)),
    ) else {
        return;
    };
    let bridge = Inputs {
        release: Release::Bridge,
        ..inputs(
            &folder.path().join("b"),
            Release::Bridge,
            Waits::new(Duration::from_secs(1)),
        )
        .unwrap()
    };
    let skips = |inputs: &Inputs| -> Vec<Option<String>> {
        specs().iter().map(|spec| (spec.skip)(inputs)).collect()
    };
    assert_eq!(skips(&flip), vec![None; 7]);
    let mut expected = vec![None; 7];
    expected[1] = Some("the bridge release has no use-node file to take".to_string());
    assert_eq!(skips(&bridge), expected);
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn a_case_that_fails_says_what_it_saw_of_the_machine_and_leaves_the_machine_where_it_is() {
    let folder = tempfile::tempdir().unwrap();
    // Apps that are programs that end at once: nothing boots, which is what the case waits for.
    let Some(inputs) = inputs(
        folder.path(),
        Release::Flip,
        Waits::new(Duration::from_secs(2)),
    ) else {
        return;
    };
    let (say, lines) = Say::channel();
    let only = vec!["another key".to_string()];
    let tally = run_cases(&inputs, &only, &say);
    drop(say);
    assert_eq!(
        tally,
        Tally {
            passed: 0,
            failed: 1,
            skipped: 6
        }
    );
    let lines: Vec<Line> = lines.iter().collect();
    let text: Vec<String> = lines
        .iter()
        .map(|line| match line {
            Line::Out(text) | Line::Err(text) => text.clone(),
        })
        .collect();
    let said = text.join("\n");
    assert!(
        said.contains("-- an update signed by another key is refused"),
        "{said}"
    );
    assert!(
        said.contains("FAIL an update signed by another key is refused"),
        "{said}"
    );
    assert!(
        said.contains("an update boot at 3.0.0-alpha.83 did not happen")
            || said.contains("the app"),
        "{said}"
    );
    assert!(said.contains("the machine is kept at "), "{said}");
    assert!(said.contains("app.log:"), "{said}");
    assert_eq!(
        text.iter().filter(|line| line.starts_with("skip ")).count(),
        6,
        "{said}"
    );
    assert!(
        text.iter()
            .any(|line| line.contains("not among the cases asked for (another key)")),
        "{said}"
    );
    // The machine is where it says: a case that failed is read, not taken away.
    let kept = said
        .lines()
        .find_map(|line| line.strip_prefix("updater smoke: the case's machine is kept at "))
        .unwrap();
    assert!(Path::new(kept).join("state").is_dir(), "{kept}");
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn the_update_is_offered_to_the_apps_that_ask_signed_by_the_runs_key_and_whatever_app_is_named() {
    let folder = tempfile::tempdir().unwrap();
    let Some(inputs) = inputs(
        folder.path(),
        Release::Flip,
        Waits::new(Duration::from_secs(30)),
    ) else {
        return;
    };
    let (say, _lines) = Say::channel();
    let kase = Case::start(&inputs, &say).unwrap();
    let ca = kase.sandbox.tls.join("root.pem");
    let get = |path: &str| https_get(&kase.feed.url, &ca, "GET", path).unwrap();
    assert_eq!(
        get("/feed").0,
        404,
        "nothing is offered until a case offers it"
    );

    // The run's update, by default.
    let offered = offer_update(&kase, None).unwrap();
    assert_eq!(offered.version, "3.0.0-alpha.83");
    let (status, body) = get("/feed");
    assert_eq!(status, 200);
    let feed: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(feed, feed_document(&offered.version, &offered.signature));
    assert_eq!(get("/archive"), (200, offered.bytes.to_vec()));

    // Another app, where a case names one: what the feed offers is what it was last given.
    let other = fake_bundle(
        &folder.path().join("other"),
        &Fake {
            version: "3.0.0-alpha.90",
            node: false,
            ..Fake::default()
        },
    );
    let named = offer_update(&kase, Some(&other)).unwrap();
    assert_eq!(named.version, "3.0.0-alpha.90");
    assert_ne!(named.bytes, offered.bytes);
    let feed: serde_json::Value = serde_json::from_slice(&get("/feed").1).unwrap();
    assert_eq!(feed, feed_document("3.0.0-alpha.90", &named.signature));
    assert_eq!(get("/archive").1, named.bytes.to_vec());
    // The run's update was not touched by being archived.
    assert_eq!(digest_manifest(&inputs.to.app).unwrap(), inputs.to_manifest);
}

/// A case on fake bundles whose app is the script `script`, started, and said to have reported.
fn started_case<'a>(inputs: &'a Inputs, script: &str) -> (Case<'a>, super::super::app::App) {
    let (say, _lines) = Say::channel();
    let mut kase = Case::start(inputs, &say).unwrap();
    stand_in_for_the_app(&kase.installed.binary, script);
    let app = kase.start_app("3.0.0-alpha.83", &[]).unwrap();
    until("the stand-in reports", Duration::from_secs(120), || {
        Ok((!app.events().is_empty()).then_some(()))
    })
    .unwrap();
    (kase, app)
}

/// A process that runs until it is ended, which is what a daemon or a window's stand-in is to a quit.
fn sleeper() -> std::process::Child {
    crate::process::spawn(
        &crate::process::Invocation::new("/bin/sleep", Path::new(".")).arg("300"),
        &Env::default(),
        std::process::Stdio::null(),
        false,
    )
    .unwrap()
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn a_quit_is_the_end_of_the_apps_input_and_leaves_nothing_of_the_app_the_daemons_or_the_windows() {
    let folder = tempfile::tempdir().unwrap();
    let Some(inputs) = inputs(
        folder.path(),
        Release::Flip,
        Waits::new(Duration::from_secs(60)),
    ) else {
        return;
    };
    let stays = "say update-boot '{}'\nreads";

    // Everything the app had is gone, and the ledger had one holder.
    let (kase, _app) = started_case(&inputs, stays);
    let ended = quit(&kase, &[]).unwrap();
    assert_eq!((ended.code, ended.signal), (Some(0), None));

    // A daemon that outlived it.
    let (kase, _app) = started_case(&inputs, stays);
    let mut daemon = sleeper();
    let said = quit(&kase, &[daemon.id()]).unwrap_err().to_string();
    assert_eq!(
        said,
        format!("the daemon (pid {}) outlived the app", daemon.id())
    );
    daemon.kill().unwrap();
    daemon.wait().unwrap();

    // A window's stand-in that survived it.
    let (kase, _app) = started_case(&inputs, stays);
    let mut chief = sleeper();
    fs::write(
        kase.sandbox.pids.join(format!("claude-{}.pid", chief.id())),
        format!("{}\n", chief.id()),
    )
    .unwrap();
    let said = quit(&kase, &[]).unwrap_err().to_string();
    assert_eq!(said, "a stand-in chief survived app shutdown");
    chief.kill().unwrap();
    chief.wait().unwrap();

    // A daemon of the app that the ledger refused is not one of the probes.
    let (kase, _app) = started_case(&inputs, stays);
    fs::write(
        kase.sandbox.state.join("daemon.log"),
        "2026-10-07T05:33:52.790Z info exit 1\n",
    )
    .unwrap();
    let said = quit(&kase, &[]).unwrap_err().to_string();
    assert!(
        said.contains("1 daemons were refused the ledger, and 0 were probes"),
        "{said}"
    );

    // An app that does not end with its input is waited for until the wait is over.
    let (mut kase, _app) = started_case(&inputs, "say update-boot '{}'\nsleep 300");
    kase.waits = Waits::new(Duration::from_millis(500));
    let said = quit(&kase, &[]).unwrap_err().to_string();
    assert!(
        said.starts_with("every app process exits did not happen within "),
        "{said}"
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn a_quit_of_no_app_is_said_so_and_the_ledger_and_the_projects_are_those_of_the_cases_home() {
    let folder = tempfile::tempdir().unwrap();
    let Some(inputs) = inputs(
        folder.path(),
        Release::Flip,
        Waits::new(Duration::from_secs(30)),
    ) else {
        return;
    };
    let (say, _lines) = Say::channel();
    let kase = Case::start(&inputs, &say).unwrap();
    assert_eq!(
        quit(&kase, &[]).unwrap_err().to_string(),
        "there is no app to quit"
    );
    assert_eq!(
        projects_of(&kase),
        [
            kase.sandbox.workspace.clone(),
            kase.sandbox.workspace.join(".consensflow-updater-second")
        ]
    );
    // No daemon has been there to make the ledger.
    assert!(ledger_of(&kase).is_err());
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn a_machine_is_kept_by_a_run_that_is_told_to_and_the_cases_not_asked_for_start_none() {
    let folder = tempfile::tempdir().unwrap();
    let Some(inputs) = inputs(
        folder.path(),
        Release::Flip,
        Waits::new(Duration::from_secs(1)),
    ) else {
        return;
    };
    let (say, lines) = Say::channel();
    let tally = run_cases(&inputs, &["nothing of the kind".to_string()], &say);
    drop(say);
    assert_eq!(
        tally,
        Tally {
            passed: 0,
            failed: 0,
            skipped: 7
        }
    );
    assert_eq!(
        lines.iter().count(),
        7,
        "a line for each case that was skipped"
    );
    // None of them made a machine.
    assert!(!folder.path().join("machines").exists());
}
