//! What every case is made of: the inputs held to what the update needs, the
//! machine of a case, and the evidence the cases share, on fake bundles and on
//! scripts that stand in for the app.

use std::time::Duration;

use super::*;
use crate::context::Context;
use crate::updater_smoke::evidence::Kind;
use crate::updater_smoke::say::Line;
use crate::updater_smoke::signing::{generate_key, tauri_bin};
use crate::updater_smoke::testing::{env, fake_bundle, Fake};

fn refusal<T: std::fmt::Debug>(result: Result<T>) -> String {
    result.unwrap_err().to_string()
}

/// The checkout, and the key a run is made with where the Tauri CLI can make one.
fn run_key(folder: &Path) -> Option<(PathBuf, Key)> {
    let root = Context::new(&Env::default()).unwrap().root;
    if !tauri_bin(&root).exists() {
        eprintln!("skipped: the Tauri CLI is not installed");
        return None;
    }
    let key = generate_key(&root, &folder.join("keys"), &env()).unwrap();
    Some((root, key))
}

struct Apps {
    from: PathBuf,
    to: PathBuf,
}

fn apps(folder: &Path, from: &Fake, to: &Fake) -> Apps {
    Apps {
        from: fake_bundle(&folder.join("from"), from),
        to: fake_bundle(&folder.join("to"), to),
    }
}

fn update() -> Fake {
    Fake {
        version: "3.0.0-alpha.83",
        node: false,
        ..Fake::default()
    }
}

fn load(folder: &Path, root: &Path, key: &Key, from: &Path, to: &Path) -> Result<Inputs> {
    Inputs::load(&Given {
        from_app: from,
        release: Release::Flip,
        to_app: to,
        key,
        keep: false,
        waits: Waits::new(Duration::from_secs(1)),
        machines: &folder.join("machines"),
        checkout: root,
        env: &env(),
    })
}

mod inputs;

#[test]
fn what_an_install_leaves_behind_is_what_is_in_its_staging_folder_and_beside_the_app() {
    let parent = tempfile::tempdir().unwrap();
    let sandbox = Sandbox::make(parent.path()).unwrap();
    assert_eq!(staging_left(&sandbox).unwrap(), Vec::<String>::new());
    fs::create_dir_all(&sandbox.copy).unwrap();
    assert_eq!(
        staging_left(&sandbox).unwrap(),
        Vec::<String>::new(),
        "the app is what the folder holds"
    );
    fs::create_dir_all(sandbox.state.join("app").join("updates").join("extract-1")).unwrap();
    fs::create_dir_all(sandbox.apps.join("ConsensFlow.app.old")).unwrap();
    let mut left = staging_left(&sandbox).unwrap();
    left.sort();
    assert_eq!(left, ["ConsensFlow.app.old", "updates/extract-1"]);
}

/// Waits as long as a machine under load needs for the stand-in app to have said
/// something, which the checks under test then find and are not themselves
/// asked to wait for.
fn reports(app: &App) {
    crate::updater_smoke::processes::until(
        "the stand-in reports",
        Duration::from_secs(120),
        || Ok((!app.events().is_empty()).then_some(())),
    )
    .unwrap();
}

/// Processes that stand in for the windows' chiefs and the daemon, ended with the test.
struct Sleepers(Vec<std::process::Child>);

impl Sleepers {
    /// Starts one, and says its pid; with a `name`, a case's machine records it as that stand-in's.
    fn start(&mut self, kase: &Case, name: Option<&str>) -> u32 {
        let child = crate::process::spawn(
            &crate::process::Invocation::new("/bin/sleep", &kase.sandbox.probe).arg("300"),
            &Env::default(),
            std::process::Stdio::null(),
            false,
        )
        .unwrap();
        let pid = child.id();
        if let Some(name) = name {
            fs::write(
                kase.sandbox.pids.join(format!("{name}-{pid}.pid")),
                format!("{pid}\n"),
            )
            .unwrap();
        }
        self.0.push(child);
        pid
    }
}

impl Drop for Sleepers {
    fn drop(&mut self) {
        for child in &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// A machine for a case on fake bundles, with the app's binary a script that is `script`.
fn machine<'a>(inputs: &'a Inputs, say: &Say, script: &str) -> Case<'a> {
    let kase = Case::start(inputs, say).unwrap();
    let binary = kase.installed.binary.clone();
    fs::write(&binary, format!("#!/bin/sh\n{script}\n")).unwrap();
    kase
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn a_cases_machine_has_the_installed_app_in_place_and_its_feed_up_and_goes_with_a_case_that_passed()
{
    let folder = tempfile::tempdir().unwrap();
    let Some((root, key)) = run_key(folder.path()) else {
        return;
    };
    let made = apps(folder.path(), &Fake::default(), &update());
    let inputs = load(folder.path(), &root, &key, &made.from, &made.to).unwrap();
    let (say, lines) = Say::channel();
    let mut kase = Case::start(&inputs, &say).unwrap();
    assert_eq!(kase.installed.version, "3.0.0-alpha.82");
    assert_eq!(kase.installed.app, kase.sandbox.copy);
    assert_eq!(
        digest_manifest(&kase.sandbox.copy).unwrap(),
        inputs.from_manifest,
        "the copy is the installed app, byte for byte"
    );
    assert!(kase.feed.url.starts_with("https://127.0.0.1:"));
    assert!(kase.probes.is_empty() && kase.app.is_none() && !kase.finished);
    kase.note("what it found");
    let root_of_machine = kase.sandbox.root.clone();
    kase.finished = true;
    kase.cleanup();
    assert!(
        !root_of_machine.exists(),
        "a case that passed takes its machine away"
    );
    drop(kase);
    drop(say);
    assert_eq!(
        lines.iter().collect::<Vec<_>>(),
        [Line::Out("    # what it found".into())]
    );
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn a_cases_machine_is_kept_for_a_case_that_failed_and_for_a_run_told_to_keep_them() {
    let folder = tempfile::tempdir().unwrap();
    let Some((root, key)) = run_key(folder.path()) else {
        return;
    };
    let made = apps(folder.path(), &Fake::default(), &update());
    let mut inputs = load(folder.path(), &root, &key, &made.from, &made.to).unwrap();
    for (finished, keep) in [(false, false), (true, true), (false, true)] {
        inputs.keep = keep;
        let (say, lines) = Say::channel();
        let mut kase = Case::start(&inputs, &say).unwrap();
        kase.finished = finished;
        let machine = kase.sandbox.root.clone();
        fs::create_dir_all(kase.sandbox.state.join("app")).unwrap();
        fs::write(
            kase.sandbox.state.join("app").join("app.log"),
            "what the app said",
        )
        .unwrap();
        fs::write(
            kase.sandbox.state.join("daemon.log"),
            "what the daemon said",
        )
        .unwrap();
        kase.cleanup();
        assert!(machine.exists(), "finished {finished}, keep {keep}");
        assert_eq!(
            kase.report(),
            format!(
                "\nthe machine is kept at {}\napp.log:\nwhat the app said\ndaemon.log:\nwhat the daemon said",
                machine.display()
            )
        );
        drop(kase);
        drop(say);
        assert_eq!(
            lines.iter().collect::<Vec<_>>(),
            [Line::Out(format!(
                "updater smoke: the case's machine is kept at {}",
                machine.display()
            ))]
        );
    }
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn the_app_is_started_in_the_cases_machine_with_its_own_environment_and_is_ended_with_the_case() {
    let folder = tempfile::tempdir().unwrap();
    let Some((root, key)) = run_key(folder.path()) else {
        return;
    };
    let made = apps(folder.path(), &Fake::default(), &update());
    let inputs = load(folder.path(), &root, &key, &made.from, &made.to).unwrap();
    let (say, _lines) = Say::channel();
    // A stand-in that says what it was given.
    let mut kase = machine(
        &inputs,
        &say,
        "say() { printf 'consensflow-selftest {\"event\":\"%s\",\"pid\":%s,\"data\":%s}\\n' \"$1\" \"$$\" \"$2\"; }
say env \"{\\\"home\\\":\\\"$CONSENSFLOW_HOME\\\",\\\"expected\\\":\\\"$CONSENSFLOW_SELFTEST_UPDATER_EXPECTED\\\",\\\"url\\\":\\\"$CONSENSFLOW_SELFTEST_UPDATER_URL\\\",\\\"key\\\":\\\"$CONSENSFLOW_SELFTEST_UPDATER_KEY\\\",\\\"cert\\\":\\\"$CONSENSFLOW_SELFTEST_UPDATER_CERT\\\",\\\"deadline\\\":\\\"$CONSENSFLOW_SELFTEST_DEADLINE_MS\\\",\\\"cwd\\\":\\\"$PWD\\\"}\"
while IFS= read -r line; do :; done",
    );
    let app = kase.start_app("3.0.0-alpha.83", &[]).unwrap();
    reports(&app);
    let said = app
        .wait_for("what it was given", |event| event.name() == "env")
        .unwrap();
    let data = |key: &str| {
        said.data(key)
            .and_then(|value| value.as_str())
            .unwrap()
            .to_string()
    };
    assert_eq!(data("home"), kase.sandbox.state.to_string_lossy());
    assert_eq!(data("expected"), "3.0.0-alpha.83");
    assert_eq!(data("url"), kase.feed.url);
    assert_eq!(data("key"), inputs.key.public_key_file.to_string_lossy());
    assert_eq!(
        data("cert"),
        kase.sandbox.tls.join("root.pem").to_string_lossy()
    );
    assert_eq!(data("deadline"), "11000", "a wait and ten seconds");
    assert_eq!(data("cwd"), kase.sandbox.root.to_string_lossy());
    assert!(app.any_alive());
    kase.cleanup();
    assert!(!app.any_alive(), "the app goes with the case");
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn an_app_that_does_not_boot_as_asked_is_not_the_one_the_case_waits_for() {
    let folder = tempfile::tempdir().unwrap();
    let Some((root, key)) = run_key(folder.path()) else {
        return;
    };
    let made = apps(folder.path(), &Fake::default(), &update());
    let inputs = load(folder.path(), &root, &key, &made.from, &made.to).unwrap();
    let (say, _lines) = Say::channel();
    let boot = "say() { printf 'consensflow-selftest {\"event\":\"%s\",\"pid\":%s,\"data\":%s}\\n' \"$1\" \"$$\" \"$2\"; }
say update-boot '{\"currentVersion\":\"3.0.0-alpha.82\"}'
while IFS= read -r line; do :; done";
    let mut kase = machine(&inputs, &say, boot);
    let app = kase.start_app("3.0.0-alpha.83", &[]).unwrap();
    reports(&app);
    // At another version than the one asked, no boot happens.
    let said = refusal(boot_evidence(&kase, &app, "3.0.0-alpha.99", None));
    assert!(
        said.starts_with("an update boot at 3.0.0-alpha.99 did not happen within "),
        "{said}"
    );
    // The app that booted is not the one that was replaced.
    let pid = app.events()[0].pid().unwrap();
    let said = refusal(boot_evidence(&kase, &app, "3.0.0-alpha.82", Some(pid)));
    assert_eq!(said, "the app that booted is the one that was replaced");
    // A script is no executable of the bundle: the process table says what runs.
    let said = refusal(boot_evidence(&kase, &app, "3.0.0-alpha.82", None));
    assert!(
        said.contains(&format!("pid {pid} is not the app: ")),
        "{said}"
    );
    kase.cleanup();
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn two_windows_open_and_the_install_refused_for_them_is_what_the_ready_snapshot_shows() {
    let folder = tempfile::tempdir().unwrap();
    let Some((root, key)) = run_key(folder.path()) else {
        return;
    };
    let made = apps(folder.path(), &Fake::default(), &update());
    let inputs = load(folder.path(), &root, &key, &made.from, &made.to).unwrap();
    let (say, _lines) = Say::channel();
    let report = |phase: &str, blockers: &str| {
        format!(
            "say() {{ printf 'consensflow-selftest {{\"event\":\"%s\",\"pid\":%s,\"data\":%s}}\\n' \"$1\" \"$$\" \"$2\"; }}
say update-blocked '{{\"phase\":\"{phase}\",\"blockers\":{blockers}}}'
while IFS= read -r line; do
  say got \"{{\\\"line\\\":\\\"$line\\\"}}\"
done"
        )
    };
    let mut sleepers = Sleepers(Vec::new());
    // Two windows, as the stand-ins of two chiefs: the install is blocked, and nothing of the page's changed.
    let mut kase = machine(&inputs, &say, &report("ready", "[1,2]"));
    let app = kase.start_app("3.0.0-alpha.83", &[]).unwrap();
    reports(&app);
    let first = Booted {
        app: app.events()[0].pid().unwrap_or(1),
        daemon: Daemon {
            pid: sleepers.start(&kase, None),
            kind: Kind::Native,
            runtime: "rust 3.0.0".into(),
            command: String::new(),
        },
    };
    let said = refusal(blocked_evidence(&kase, &app, &first));
    assert!(
        said.starts_with("two stand-in chief processes did not happen within "),
        "{said}"
    );
    let chiefs = [
        sleepers.start(&kase, Some("claude")),
        sleepers.start(&kase, Some("codex")),
    ];
    let found = blocked_evidence(&kase, &app, &first).unwrap();
    let mut sorted = found.clone();
    sorted.sort_unstable();
    let mut expected = chiefs.to_vec();
    expected.sort_unstable();
    assert_eq!(sorted, expected);
    // A third is not two, and a chief that is gone is not alive.
    let third = sleepers.start(&kase, Some("pi"));
    assert!(refusal(blocked_evidence(&kase, &app, &first))
        .contains("two stand-in chief processes did not happen"));
    crate::updater_smoke::processes::kill(third);
    fs::remove_file(kase.sandbox.pids.join(format!("pi-{third}.pid"))).unwrap();
    // The page tells the install to go on, and the windows close.
    release_panes(&kase, &app, &[]).unwrap();
    crate::updater_smoke::processes::until(
        "the stand-in answers",
        Duration::from_secs(120),
        || {
            Ok(app
                .events()
                .iter()
                .any(|event| event.name() == "got")
                .then_some(()))
        },
    )
    .unwrap();
    let got = app
        .wait_for("the line", |event| event.name() == "got")
        .unwrap();
    assert_eq!(
        got.data("line").and_then(|line| line.as_str()),
        Some("continue-updater")
    );
    let said = refusal(release_panes(&kase, &app, &chiefs));
    assert!(
        said.starts_with("two stand-in chiefs close did not happen within "),
        "{said}"
    );
    for pid in chiefs {
        crate::updater_smoke::processes::kill(pid);
    }
    release_panes(&kase, &app, &chiefs).unwrap();
    crate::updater_smoke::processes::kill(first.daemon.pid);
    kase.cleanup();

    // What the snapshot says is held to what the case needs of it.
    for (phase, blockers, words) in [
        (
            "installing",
            "[1,2]",
            "the blocked install's phase is installing, not ready",
        ),
        (
            "ready",
            "[1]",
            "the ready snapshot did not expose both open panes",
        ),
        (
            "ready",
            "[1,2,3]",
            "the ready snapshot did not expose both open panes",
        ),
        (
            "ready",
            "2",
            "the ready snapshot did not expose both open panes",
        ),
    ] {
        let mut kase = machine(&inputs, &say, &report(phase, blockers));
        let app = kase.start_app("3.0.0-alpha.83", &[]).unwrap();
        reports(&app);
        let first = Booted {
            app: 1,
            daemon: Daemon {
                pid: 1,
                kind: Kind::Native,
                runtime: String::new(),
                command: String::new(),
            },
        };
        let said = refusal(blocked_evidence(&kase, &app, &first));
        assert!(said.contains(words), "{phase} {blockers}: {said}");
        kase.cleanup();
    }
}

#[test]
#[cfg_attr(not(target_os = "macos"), ignore = "the bundles are macOS apps")]
fn the_installed_app_that_is_in_place_has_the_ledger_held_by_what_refuses_a_second_one() {
    let folder = tempfile::tempdir().unwrap();
    let Some((root, key)) = run_key(folder.path()) else {
        return;
    };
    let made = apps(folder.path(), &Fake::default(), &update());
    let inputs = load(folder.path(), &root, &key, &made.from, &made.to).unwrap();
    let (say, _lines) = Say::channel();
    let mut kase = Case::start(&inputs, &say).unwrap();
    // A `cf` that is refused in the ledger's own words.
    let db = kase.sandbox.state.join("consensflow.db");
    fs::write(
        &kase.installed.cf,
        format!(
            "#!/bin/sh\necho \"cf: another ConsensFlow has {} open\" >&2\nexit 1\n",
            db.display()
        ),
    )
    .unwrap();
    held_evidence(&mut kase).unwrap();
    assert_eq!(kase.probes.len(), 1, "the second ConsensFlow is a probe");
    held_evidence(&mut kase).unwrap();
    assert_eq!(kase.probes.len(), 2);
    kase.cleanup();
}
