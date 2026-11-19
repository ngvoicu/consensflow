//! The run's exclusive upload folder and completion-gated cleanup on an owned profile.
use super::*;
use crate::process::{self, Invocation};
use cf_base::env::Env;

fn run(shell: &str, profile: &std::path::Path, script: &str) -> process::Captured {
    let env = Env::from_process()
        .with_var("USERPROFILE", &profile.display().to_string())
        .with_var(
            "XDG_CACHE_HOME",
            &profile.join("cache").display().to_string(),
        );
    let env = powershell::shell_environment(shell, env);
    powershell::with_shell_turn(shell, || {
        process::capture(
            &Invocation::new(shell, ".")
                .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
                .arg(encode_script(script).unwrap()),
            &env,
        )
    })
    .unwrap()
}

#[test]
fn completed_run_removes_only_its_own_folder_and_an_unknown_run_stays() {
    let Some(shell) = powershell::shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let profile = tempfile::tempdir().unwrap();
    let first = profile.path().join(format!("consensflow-wrun-{ID}"));
    let other_id = "0000abcd-12345679";
    let other = profile.path().join(format!("consensflow-wrun-{other_id}"));
    for id in [ID, other_id] {
        let claimed = run(shell, profile.path(), &claim_script(DIR, id));
        assert_eq!(claimed.code, 0, "{}", said(&claimed));
        assert!(claimed.stdout.contains(&format!("== claimed {id}")));
    }
    // An existing folder cannot be claimed by a second uploader.
    let collision = run(shell, profile.path(), &claim_script(DIR, ID));
    assert_ne!(collision.code, 0, "an existing folder was claimed twice");
    std::fs::write(first.join("tree.tgz"), b"first").unwrap();
    std::fs::write(other.join("tree.tgz"), b"other").unwrap();
    let premature = run(shell, profile.path(), &cleanup_script(ID));
    assert_ne!(premature.code, 0, "cleanup accepted no completion receipt");
    assert!(first.exists());
    let unknown = run(shell, profile.path(), &cleanup_script(other_id));
    assert_ne!(unknown.code, 0, "an unknown run was treated as complete");
    assert!(other.exists());
    std::fs::write(first.join("completed.txt"), format!("finished {other_id}")).unwrap();
    let wrong = run(shell, profile.path(), &cleanup_script(ID));
    assert_ne!(wrong.code, 0, "another run's receipt was accepted");
    assert!(first.exists());
    std::fs::write(first.join("completed.txt"), format!("finished {ID}")).unwrap();
    let completed = run(shell, profile.path(), &cleanup_script(ID));
    assert_eq!(completed.code, 0, "{}", said(&completed));
    assert!(!first.exists(), "the completed run's folder stayed");
    assert_eq!(std::fs::read(other.join("tree.tgz")).unwrap(), b"other");
    assert!(other.join("record.txt").is_file());
}

#[test]
fn a_finished_bootstrap_keeps_its_upload_until_its_receipt_is_read() {
    let Some(shell) = powershell::shell() else {
        eprintln!("skipped: no PowerShell on this machine");
        return;
    };
    let profile = tempfile::tempdir().unwrap();
    let claimed = run(shell, profile.path(), &claim_script(DIR, ID));
    assert_eq!(claimed.code, 0, "{}", said(&claimed));
    let folder = profile.path().join(format!("consensflow-wrun-{ID}"));
    let upload = folder.join("tree.tgz");
    std::fs::write(&upload, b"owned upload").unwrap();
    // An empty SystemRoot makes the real bootstrap refuse before unpacking.
    let system = profile.path().join("no-system");
    std::fs::create_dir(&system).unwrap();
    let system = system.display().to_string().replace('\'', "''");
    let ran = run(
        shell,
        profile.path(),
        &format!("$env:SystemRoot = '{system}'\n{}", bootstrap()),
    );
    assert_eq!(ran.code, 1, "{}", said(&ran));
    assert!(ran.stdout.contains("== exit 1"), "{}", said(&ran));
    assert_eq!(std::fs::read(&upload).unwrap(), b"owned upload");
    assert_eq!(
        std::fs::read_to_string(folder.join("completed.txt")).unwrap(),
        format!("finished {ID}")
    );
    let cleaned = run(shell, profile.path(), &cleanup_script(ID));
    assert_eq!(cleaned.code, 0, "{}", said(&cleaned));
    assert!(!folder.exists());
}
