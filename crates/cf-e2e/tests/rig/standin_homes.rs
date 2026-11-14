//! The parent of a stand-in rig must not consult the machine's Keychain.

use cf_e2e::live::{self, Harness, Installed};
use cf_e2e::rig::Rig;

use crate::{Outcome, FAKE_AGENT};

// This decoy is a nonexistent sibling of the private test temp directory, never
// any part of the machine owner's home. A lookup must refuse it before looking.
fn refuses_foreign_home(harness: Harness) {
    let temp = std::env::temp_dir();
    let foreign = temp
        .parent()
        .unwrap_or_else(|| panic!("temporary folder has no parent"))
        .join(format!("cf-foreign-home-{}", std::process::id()));
    let error = match std::panic::catch_unwind(|| {
        live::launcher::Watch::of(harness, &foreign.join("bin"), &foreign).look()
    }) {
        Err(error) => error,
        Ok(_) => panic!("the harness home lookup did not fail closed"),
    };
    let text = error
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| error.downcast_ref::<&str>().copied())
        .unwrap_or("");
    assert!(
        text.contains(harness.name()) && text.contains("lookup"),
        "{text}"
    );
}

#[test]
fn standin_isolation_claude_rig_refuses_a_foreign_home() {
    refuses_foreign_home(Harness::Claude);
}
#[test]
fn standin_isolation_codex_rig_refuses_a_foreign_home() {
    refuses_foreign_home(Harness::Codex);
}
#[test]
fn standin_isolation_pi_rig_refuses_a_foreign_home() {
    refuses_foreign_home(Harness::Pi);
}
#[test]
fn standin_isolation_devin_rig_refuses_a_foreign_home() {
    refuses_foreign_home(Harness::Devin);
}
#[test]
fn standin_isolation_muse_rig_refuses_a_foreign_home() {
    refuses_foreign_home(Harness::Muse);
}

#[test]
fn standin_isolation_parent_and_windows_bound_every_home() -> Outcome {
    let rig = Rig::start(crate::config())?;
    for name in [
        "HOME",
        "USERPROFILE",
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "APPDATA",
        "LOCALAPPDATA",
    ] {
        let parent = cf_e2e::process::own_var(name).expect("parent home variable missing");
        cf_e2e::standin_process::require_home(
            std::path::Path::new(&parent),
            &format!("parent {name} lookup"),
        );
        let child = rig
            .var(name)
            .unwrap_or_else(|| panic!("window {name} missing"));
        cf_e2e::standin_process::require_home(
            std::path::Path::new(child),
            &format!("window {name} lookup"),
        );
    }
    rig.close()?;
    Ok(())
}

#[test]
fn standin_isolation_replaces_every_inherited_home_before_any_case() -> Outcome {
    const CHILD: &str = "CF_STANDIN_HOME_PROBE";
    const HOMES: &[&str] = &[
        "HOME",
        "USERPROFILE",
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "PI_CODING_AGENT_DIR",
        "PI_CODING_AGENT_SESSION_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "APPDATA",
        "LOCALAPPDATA",
    ];
    if cf_e2e::process::own_var(CHILD).is_some() {
        for name in HOMES {
            let value = cf_e2e::process::own_var(name).expect("parent home variable missing");
            cf_e2e::standin_process::require_home(
                std::path::Path::new(&value),
                &format!("inherited {name} lookup"),
            );
        }
        return Ok(());
    }
    let decoy = cf_e2e::door::made("foreign-")?;
    let mut command = cf_e2e::process::Run::new(std::env::current_exe()?)
        .inheriting_env()
        .args([
            "--exact",
            "standin_homes::standin_isolation_replaces_every_inherited_home_before_any_case",
        ])
        .var(CHILD, "1");
    for name in HOMES {
        command = command.var(name, decoy.path());
    }
    let output = command.run()?;
    assert!(output.code == Some(0), "{}{}", output.stdout, output.stderr);
    Ok(())
}

#[test]
fn standin_isolation_claude_guard_does_not_call_security() -> Outcome {
    assert_eq!(
        cf_e2e::process::own_var("CF_E2E_STAND_IN").as_deref(),
        Some("1")
    );
    let home = cf_e2e::door::made("guard-")?;
    let command = home.path().join("claude");
    cf_e2e::door::write(&command, "fixture launcher")?;
    let claude = Installed::at(Harness::Claude, command, home.path());
    let rig = Rig::start(live::rig(&[claude], FAKE_AGENT))?;
    rig.close()?;
    Ok(())
}

#[test]
fn standin_isolation_nested_cargo_finds_its_compiler_without_path() -> Outcome {
    let project = cf_e2e::door::made("compiler-")?;
    cf_e2e::door::write(
        project.path().join("Cargo.toml"),
        "[package]\nname = \"standin-compiler-probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    )?;
    cf_e2e::door::make_dir(project.path().join("src"))?;
    cf_e2e::door::write(project.path().join("src/lib.rs"), "pub fn probe() {}\n")?;
    let output = cf_e2e::process::Run::cargo()
        .cwd(project.path())
        .var("PATH", "")
        .var("CARGO_TARGET_DIR", project.path().join("target"))
        .args(["check", "--offline", "-j", "4"])
        .run()?;
    assert_eq!(
        output.code,
        Some(0),
        "nested cargo could not use its compiler after PATH was bounded: {}{}",
        output.stdout,
        output.stderr
    );
    Ok(())
}

#[cfg(unix)]
#[test]
fn standin_isolation_launcher_refuses_a_link_outside_the_run() -> Outcome {
    let parent = std::env::temp_dir();
    let outside = cf_e2e::door::made_in(
        parent
            .parent()
            .unwrap_or_else(|| panic!("temporary folder has no parent")),
        "foreign-link-",
    )?;
    let target = outside.path().join("decoy");
    cf_e2e::door::write(&target, "synthetic launcher")?;
    let home = cf_e2e::door::made("link-home-")?;
    let link = home.path().join("claude");
    cf_e2e::door::symlink(&target, &link)?;
    let result = std::panic::catch_unwind(|| {
        live::launcher::Watch::of(Harness::Claude, &link, home.path()).look()
    });
    assert!(
        result.is_err(),
        "launcher lookup followed a link outside the run"
    );
    Ok(())
}
