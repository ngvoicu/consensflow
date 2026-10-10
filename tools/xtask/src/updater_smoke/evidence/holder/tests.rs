//! The ledger's one holder, as a second ConsensFlow finds it.

use super::*;

const DB: &str = "/box/state/consensflow.db";

fn held() -> Attempt {
    Attempt {
        pid: 1,
        code: Some(1),
        signal: None,
        out: String::new(),
        err: format!("cf: another ConsensFlow has {DB} open\n"),
    }
}

#[test]
fn is_refused_in_the_ledgers_own_words_with_no_handle_line_out() {
    assert_ledger_held(&held(), DB).unwrap();
}

#[test]
fn is_no_proof_when_the_second_one_ran_or_was_refused_for_something_else() {
    let scenarios = [
        (
            "a second owner",
            Attempt {
                code: Some(0),
                err: String::new(),
                ..held()
            },
            "ended 0",
        ),
        (
            "a killed probe",
            Attempt {
                code: None,
                signal: Some(9),
                ..held()
            },
            "never ended",
        ),
        (
            "a missing program",
            Attempt {
                err: "cf: command not found\n".into(),
                ..held()
            },
            "not for the ledger's lock",
        ),
        (
            "another home's ledger",
            Attempt {
                err: "cf: another ConsensFlow has /elsewhere/consensflow.db open\n".into(),
                ..held()
            },
            "not for the ledger's lock",
        ),
        (
            "a handle line",
            Attempt {
                out: "{\"url\":\"http://x\",\"token\":\"t\"}\n".into(),
                ..held()
            },
            "printed a handle line",
        ),
    ];
    for (what, attempt, words) in scenarios {
        let said = assert_ledger_held(&attempt, DB).unwrap_err().to_string();
        assert!(said.contains(words), "{what}: {said}");
    }
}

#[test]
fn takes_the_database_by_its_path_whatever_is_in_it() {
    let db = "/box/state (1)/consensflow.db";
    let attempt = Attempt {
        err: format!("cf: another ConsensFlow has {db} open\n"),
        ..held()
    };
    assert_ledger_held(&attempt, db).unwrap();
    assert_ledger_held(&attempt, "/box/state (1)/consensflow.d.").unwrap_err();
}

#[cfg(unix)]
mod a_second_consensflow_on_the_machine {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// A bundle whose `cf` is the script `cf`, in the folder of a machine that is made.
    fn bundle_with(sandbox: &Sandbox, cf: &str) -> BundleInfo {
        let app = sandbox.copy.clone();
        let file = crate::updater_smoke::bundle::cf_of(&app);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, cf).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
        BundleInfo {
            binary: app.join("Contents/MacOS/app"),
            cf: file,
            label: "the app".into(),
            version: "3.0.0-alpha.82".into(),
            node: false,
            cli_version: None,
            app,
        }
    }

    #[test]
    fn is_run_as_the_daemon_is_and_says_how_it_ended() {
        let parent = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::make(parent.path()).unwrap();
        let bundle = bundle_with(
            &sandbox,
            "#!/bin/sh\necho \"cf $* in $PWD, home $CONSENSFLOW_HOME, node ${CONSENSFLOW_NODE:-none}\" >&2\nexit 1\n",
        );
        let attempt = second_daemon(&bundle, &sandbox).unwrap();
        assert_eq!((attempt.code, attempt.signal), (Some(1), None));
        assert_eq!(attempt.out, "");
        assert_eq!(
            attempt.err,
            format!(
                "cf ui --json --no-open in {}, home {}, node none\n",
                sandbox.probe.display(),
                sandbox.state.display()
            )
        );
        assert!(attempt.pid > 0);
    }

    #[test]
    fn is_told_the_bundles_node_where_the_bundle_has_one() {
        let parent = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::make(parent.path()).unwrap();
        let mut bundle = bundle_with(&sandbox, "#!/bin/sh\necho \"${CONSENSFLOW_NODE:-none}\"\n");
        bundle.node = true;
        let attempt = second_daemon(&bundle, &sandbox).unwrap();
        assert_eq!(
            attempt.out.trim_end(),
            sandbox.copy.join("Contents/MacOS/node").to_string_lossy()
        );
        assert_eq!(attempt.code, Some(0));
    }

    #[test]
    fn is_the_proof_of_the_one_holder_when_the_ledger_refuses_it_and_is_recorded_as_a_probe() {
        let parent = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::make(parent.path()).unwrap();
        let db = sandbox.state.join("consensflow.db");
        let bundle = bundle_with(
            &sandbox,
            &format!(
                "#!/bin/sh\necho \"cf: another ConsensFlow has {} open\" >&2\nexit 1\n",
                db.display()
            ),
        );
        let mut probes = BTreeSet::new();
        ledger_held(&bundle, &sandbox, &mut probes).unwrap();
        assert_eq!(probes.len(), 1);
        // One that took the ledger is no refusal, and is a probe all the same.
        let bundle = bundle_with(&sandbox, "#!/bin/sh\nexit 0\n");
        let said = ledger_held(&bundle, &sandbox, &mut probes).unwrap_err();
        assert!(said.to_string().contains("ended 0"), "{said}");
        assert_eq!(probes.len(), 2);
    }

    #[test]
    fn is_ended_by_a_signal_the_run_says_so() {
        let parent = tempfile::tempdir().unwrap();
        let sandbox = Sandbox::make(parent.path()).unwrap();
        let bundle = bundle_with(&sandbox, "#!/bin/sh\nkill -9 $$\n");
        let attempt = second_daemon(&bundle, &sandbox).unwrap();
        assert_eq!((attempt.code, attempt.signal), (None, Some(9)));
        let said = assert_ledger_held(&attempt, "x").unwrap_err();
        assert!(said.to_string().contains("never ended"), "{said}");
    }
}
