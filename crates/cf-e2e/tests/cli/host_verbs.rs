//! `the host-integration verbs are gone, not hidden`: `cf` answers `hosts`,
//! `install` and `uninstall` as it answers any word that is no command.

use cf_e2e::ScratchHome;

use crate::Outcome;

fn answered_as_no_command(verb: &str) -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf([verb, "claude"])?;
    assert_ne!(ran.code, Some(0), "{ran}");
    assert!(ran.output().contains("unknown command"), "{ran}");
    Ok(())
}

#[test]
fn no_longer_answers_hosts() -> Outcome {
    answered_as_no_command("hosts")
}

#[test]
fn no_longer_answers_install() -> Outcome {
    answered_as_no_command("install")
}

#[test]
fn no_longer_answers_uninstall() -> Outcome {
    answered_as_no_command("uninstall")
}
