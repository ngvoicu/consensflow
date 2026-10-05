//! The daemon's seams of the engine: what a window starts with, the ids and
//! files of a launch, an adapter for each harness, and work that goes on apart.

use std::cell::Cell;

use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;

use super::*;
use crate::files::{Log, Trace};
use crate::testing::scene;

fn participant<'a>(project: &'a ProjectView, handle: &str) -> &'a ParticipantView {
    project
        .participants
        .iter()
        .find(|participant| participant.handle == handle)
        .unwrap()
}

#[test]
fn a_window_starts_with_its_url_project_participant_runtime_and_the_bundle_first_on_its_path() {
    let scene = scene();
    let env = Env::from_vars([
        ("CONSENSFLOW_NODE", "/opt/consensflow/node"),
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/home/me"),
    ]);
    let window = WindowEnv::new(&env, "http://127.0.0.1:4242", "/opt/consensflow/bin");
    let started = window.env(participant(&scene.project, "zeus"), &scene.project);
    let delimiter = if cfg!(windows) { ';' } else { ':' };
    assert_eq!(
        started,
        [
            (
                "CONSENSFLOW_URL".to_owned(),
                "http://127.0.0.1:4242".to_owned()
            ),
            (
                "CONSENSFLOW_PROJECT".to_owned(),
                scene.project.id.to_string()
            ),
            ("CONSENSFLOW_PARTICIPANT".to_owned(), "zeus".to_owned()),
            (
                "CONSENSFLOW_NODE".to_owned(),
                "/opt/consensflow/node".to_owned()
            ),
            (
                "PATH".to_owned(),
                format!("/opt/consensflow/bin{delimiter}/usr/bin:/bin")
            ),
        ]
    );
}

#[test]
fn the_runtime_is_passed_on_only_when_the_daemon_was_given_one_and_never_guessed() {
    let scene = scene();
    for env in [
        Env::from_vars([("PATH", "/usr/bin")]),
        Env::from_vars([("CONSENSFLOW_NODE", ""), ("PATH", "/usr/bin")]),
    ] {
        let window = WindowEnv::new(&env, "http://127.0.0.1:1", "/bin-of-the-bundle");
        let started = window.env(participant(&scene.project, "chief"), &scene.project);
        let names: Vec<&str> = started.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "CONSENSFLOW_URL",
                "CONSENSFLOW_PROJECT",
                "CONSENSFLOW_PARTICIPANT",
                "PATH"
            ]
        );
    }
}

#[test]
fn a_daemon_with_no_path_gives_its_windows_the_bundle_alone() {
    let scene = scene();
    for env in [Env::default(), Env::from_vars([("PATH", "")])] {
        let window = WindowEnv::new(&env, "http://127.0.0.1:1", "/bundle/bin");
        let started = window.env(participant(&scene.project, "chief"), &scene.project);
        assert_eq!(
            started.last(),
            Some(&("PATH".to_owned(), "/bundle/bin".to_owned()))
        );
    }
}

#[test]
fn the_chief_is_given_its_staff_and_every_role_the_cf_of_its_window() {
    let scene = scene();
    let texts = RoleTexts::new(Env::default(), "/bundle/bin/cf".to_owned());
    let chief = texts
        .instructions(participant(&scene.project, "chief"), &scene.project)
        .unwrap();
    assert!(
        chief.contains("| zeus | worker | Standard work |"),
        "{chief}"
    );
    assert!(chief.contains("Here `cf` is /bundle/bin/cf."), "{chief}");
    let worker = texts
        .instructions(participant(&scene.project, "zeus"), &scene.project)
        .unwrap();
    assert!(worker.contains("Here `cf` is /bundle/bin/cf."), "{worker}");
}

#[test]
fn a_role_that_has_no_text_fails_its_launch_in_the_words_of_why() {
    let scene = scene();
    let texts = RoleTexts::new(Env::default(), "/bundle/bin/cf".to_owned());
    let mut human = participant(&scene.project, "zeus").clone();
    human.role = "human".to_owned();
    let failed = texts.instructions(&human, &scene.project).unwrap_err();
    assert_eq!(failed, "no role instructions for human");
}

#[test]
fn every_launch_has_an_id_of_its_own_in_the_shape_of_a_uuid() {
    let ids = RandomLaunchIds;
    let drawn: Vec<LaunchId> = (0..20).map(|_| ids.draw()).collect();
    for id in &drawn {
        assert_eq!(id.as_str().len(), 36);
        assert_eq!(LaunchId::new(id.as_str()).as_ref(), Some(id));
    }
    for (at, id) in drawn.iter().enumerate() {
        assert!(!drawn[..at].contains(id), "{}", id.as_str());
    }
}

#[test]
fn a_launch_s_files_are_forgotten_from_every_harness_s_folder() {
    let home = tempfile::tempdir().unwrap();
    let launch = RandomLaunchIds.draw();
    for harness in ["claude", "pi", "devin", "opencode"] {
        let folder = home
            .path()
            .join("integrations")
            .join(harness)
            .join(launch.as_str());
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("settings.json"), "{}").unwrap();
    }
    let kept = home
        .path()
        .join("integrations")
        .join("claude")
        .join("not-a-launch");
    std::fs::create_dir_all(&kept).unwrap();
    let errors = Rc::new(Errors::new(
        Rc::new(Log::new(home.path())),
        Rc::new(Trace::new(home.path())),
    ));
    LaunchFolders::new(home.path().to_path_buf(), errors).forget(&launch);
    for harness in ["claude", "pi", "devin", "opencode"] {
        let folder = home
            .path()
            .join("integrations")
            .join(harness)
            .join(launch.as_str());
        assert!(!folder.exists(), "{harness}");
    }
    assert!(kept.exists(), "what is no launch's stays");
}

#[test]
fn every_harness_the_ledger_names_has_its_adapter_and_no_other_word_has() {
    let home = tempfile::tempdir().unwrap();
    let services = Fakes::new(&Env::default()).services(&Env::default(), home.path());
    let adapters = HarnessAdapters::new(&services);
    for harness in Harness::ALL {
        assert!(
            adapters.adapter(harness.kind()).is_some(),
            "{}",
            harness.kind()
        );
    }
    // The CLI's own name is not the ledger's word for Claude.
    assert!(adapters.adapter("claude").is_none());
    for word in ["", "image", "kimi", "Codex", "mystery"] {
        assert!(adapters.adapter(word).is_none(), "{word:?}");
    }
}

#[tokio::test]
async fn work_that_goes_on_apart_runs_on_the_local_set_and_a_panic_in_it_is_written_down() {
    let home = tempfile::tempdir().unwrap();
    let errors = Rc::new(Errors::new(
        Rc::new(Log::new(home.path())),
        Rc::new(Trace::new(home.path())),
    ));
    let spawn = DaemonSpawn::new(errors);
    tokio::task::LocalSet::new()
        .run_until(async {
            let ran = Rc::new(Cell::new(false));
            let marked = Rc::clone(&ran);
            spawn.spawn(Box::pin(async move { marked.set(true) }));
            spawn.spawn(Box::pin(async { panic!("a launch went wrong") }));
            tokio::task::yield_now().await;
            tokio::task::yield_now().await;
            assert!(ran.get());
        })
        .await;
    let log = std::fs::read_to_string(home.path().join("daemon.log")).unwrap();
    assert!(log.contains("error a task failed"), "{log}");
    assert!(log.contains("panic: a launch went wrong"), "{log}");
}
