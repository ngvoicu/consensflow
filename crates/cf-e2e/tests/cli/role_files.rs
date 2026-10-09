//! `role files belong to pane launch, not CLI administration`: what `cf` does
//! with the role files and the manifest of an older install, and the skills
//! administration it no longer has.

use cf_e2e::{files, ScratchHome};

use crate::{launcher, role_file, Outcome};

/// A home with a `claude` on its PATH, as a person who has the harness has.
fn home_with_claude() -> cf_e2e::Result<ScratchHome> {
    let home = ScratchHome::new()?;
    home.stand_in("claude")?;
    Ok(home)
}

#[test]
fn roster_edits_setup_and_diagnostic_reads_leave_role_files_and_old_manifests_alone() -> Outcome {
    let home = home_with_claude()?;
    let role = role_file(&home);
    files::write(&role, "role canary")?;
    let manifest = home.consensflow().join("skills-manifest.json");
    files::write(&manifest, r#"{"files":{}}"#)?;
    for args in [
        &[
            "agent",
            "add",
            "mine",
            "--harness",
            "claude",
            "--model",
            "example",
        ][..],
        &["agent", "edit", "mine", "--effort", "high"],
        &["agent", "list", "--json"],
        &["catalog"],
        &["doctor"],
        &["setup"],
        &["agent", "remove", "mine"],
    ] {
        let ran = home.cf(args)?;
        assert_eq!(ran.code, Some(0), "{}", ran.stderr);
        assert_eq!(
            files::read_string(&role)?,
            "role canary",
            "{}",
            args.join(" ")
        );
        assert_eq!(files::read_string(&manifest)?, r#"{"files":{}}"#);
    }
    assert!(home.bin().join(launcher()).exists());
    Ok(())
}

#[test]
fn skills_administration_is_absent_including_forced_uninstall() -> Outcome {
    let home = home_with_claude()?;
    for action in ["install", "update", "status", "uninstall"] {
        let ran = home.cf(["skills", action, "--force"])?;
        assert_eq!(ran.code, Some(1), "{ran}");
        assert!(ran.stderr.contains("unknown command"), "{ran}");
    }
    Ok(())
}
