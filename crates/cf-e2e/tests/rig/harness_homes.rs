//! The homes of the real harnesses a live run's windows run on, held to what they say
//! without spending a token (the programs are never started here: a rig only has the
//! variables its windows would be given): a rig clears the environment and a harness reads
//! its login by what the rig sets, so what the rig sets last must be the run's own home, and
//! the owner's files must be as they were when the rig ends (T-724, after T-696: a live run
//! on Pi refreshed the owner's `openai` login because no case ever set a throwaway Pi
//! folder).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use cf_e2e::live::isolation::Isolation;
use cf_e2e::live::{self, Harness, Installed};
use cf_e2e::rig::Rig;
use serde_json::json;

use crate::{Outcome, FAKE_AGENT};

/// What the owner's setups hold that no run may touch.
const SENTINEL: &str = "SENTINEL-the-owners-own";

/// A pretend owner: a home with the three harnesses' setups in it, each file holding the
/// sentinel, and a Pi login of a key.
struct Owner {
    home: tempfile::TempDir,
}

type Made<T> = Result<T, Box<dyn std::error::Error>>;

impl Owner {
    fn new() -> Made<Self> {
        let owner = Self {
            home: tempfile::tempdir()?,
        };
        for file in [
            ".pi/agent/settings.json",
            ".pi/agent/models.json",
            ".pi/agent/mcp-auth.json",
            ".codex/config.toml",
            ".config/devin/config.json",
            ".local/share/devin/credentials.toml",
            ".local/share/devin/cli/trusted_workspaces.json",
            ".local/share/devin/cli/sessions.db",
        ] {
            owner.write(file, SENTINEL)?;
        }
        owner.write(
            ".pi/agent/auth.json",
            &json!({ "openrouter": { "type": "api_key", "key": "sk-or-OWNERS" } }).to_string(),
        )?;
        owner.write(".codex/auth.json", r#"{"OPENAI_API_KEY":"sk-OWNERS"}"#)?;
        Ok(owner)
    }

    fn path(&self, below: &str) -> PathBuf {
        self.home.path().join(below)
    }

    fn write(&self, below: &str, text: &str) -> Made<()> {
        let file = self.path(below);
        fs::create_dir_all(file.parent().ok_or("a file below the home has a folder")?)?;
        fs::write(file, text)?;
        Ok(())
    }

    /// Every file below the owner's home and what it holds, in order: the owner's setup, whole.
    fn everything(&self) -> Made<Vec<(String, Vec<u8>)>> {
        fn walk(folder: &Path, base: &Path, into: &mut Vec<(String, Vec<u8>)>) -> Made<()> {
            let mut entries: Vec<_> = fs::read_dir(folder)?.flatten().collect();
            entries.sort_by_key(fs::DirEntry::file_name);
            for entry in entries {
                let path = entry.path();
                let name = path.strip_prefix(base)?.display().to_string();
                if entry.file_type()?.is_dir() {
                    into.push((format!("{name}/"), Vec::new()));
                    walk(&path, base, into)?;
                } else {
                    into.push((name, fs::read(&path)?));
                }
            }
            Ok(())
        }
        let mut found = Vec::new();
        walk(self.home.path(), self.home.path(), &mut found)?;
        Ok(found)
    }

    /// The installs of Pi, Codex and Devin on this owner's machine, each with the home the kit
    /// makes it, and what a case that clears the environment would still carry.
    fn installs(&self) -> Vec<Installed> {
        let home = self.home.path();
        let run = Duration::from_secs(3600);
        vec![
            Installed::at(Harness::Pi, "/nowhere/pi", home).with_isolation(Arc::new(
                Isolation::pi(
                    home,
                    &self.path(".pi/agent"),
                    "openrouter/meta/muse-spark-1.3",
                    run,
                ),
            )),
            Installed::at(Harness::Codex, "/nowhere/codex", self.path(".codex"))
                .with_isolation(Arc::new(Isolation::codex(home, &self.path(".codex")))),
            Installed::at(Harness::Devin, "/nowhere/devin", home).with_isolation(Arc::new(
                Isolation::devin(home, &self.path(".config"), &self.path(".local/share")),
            )),
        ]
    }
}

/// The path a variable of the rig names, which must be below the owner's home in none.
fn named(rig: &Rig, name: &str) -> PathBuf {
    PathBuf::from(
        rig.var(name)
            .unwrap_or_else(|| panic!("the rig has no {name}")),
    )
}

#[test]
fn a_rig_that_clears_the_environment_still_runs_each_harness_on_a_home_of_the_runs_own() -> Outcome
{
    let owner = Owner::new()?;
    // The case sets the owner's folders itself, as a case that reads a login by hand did, and
    // unsets one: the kit's variables are the last word.
    let config = live::rig(&owner.installs(), FAKE_AGENT)
        .var("CODEX_HOME", owner.path(".codex").to_string_lossy())
        .var(
            "PI_CODING_AGENT_DIR",
            owner.path(".pi/agent").to_string_lossy(),
        )
        .var(
            "XDG_DATA_HOME",
            owner.path(".local/share").to_string_lossy(),
        )
        .unset("XDG_CONFIG_HOME");
    let rig = Rig::start(config)?;
    let home = owner.home.path();
    for name in [
        "CODEX_HOME",
        "PI_CODING_AGENT_DIR",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
    ] {
        let at = named(&rig, name);
        assert!(!at.starts_with(home), "{name} is {}", at.display());
        assert!(at.is_dir(), "{name} is {}", at.display());
    }
    // The owner's home is the windows' all the same (their project folders are below it), and
    // none of the variables a harness reads its login by names it.
    assert_eq!(named(&rig, "HOME"), home);
    // The homes hold the logins (a key, a Codex login) and nothing else of the owner's.
    let pi = named(&rig, "PI_CODING_AGENT_DIR");
    assert!(fs::read_to_string(pi.join("auth.json"))?.contains("sk-or-OWNERS"));
    assert!(!pi.join("settings.json").exists() && !pi.join("models.json").exists());
    let codex = named(&rig, "CODEX_HOME");
    assert!(codex.join("auth.json").is_file() && !codex.join("config.toml").exists());
    let devin = named(&rig, "XDG_DATA_HOME").join("devin");
    assert!(devin.join("credentials.toml").is_file() && !devin.join("cli").exists());
    rig.close()?;
    Ok(())
}

#[test]
fn a_run_that_writes_only_in_its_homes_leaves_the_owners_setup_as_it_was() -> Outcome {
    let owner = Owner::new()?;
    let before = owner.everything()?;
    let rig = Rig::start(live::rig(&owner.installs(), FAKE_AGENT))?;
    // What the harnesses write as they run: sessions, settings, a trust table, a database.
    let write = |folder: PathBuf, file: &str| -> Made<()> {
        let file = folder.join(file);
        fs::create_dir_all(file.parent().ok_or("a file has a folder")?)?;
        fs::write(file, "the run's own")?;
        Ok(())
    };
    write(
        named(&rig, "PI_CODING_AGENT_DIR"),
        "sessions/--run--/s.jsonl",
    )?;
    write(named(&rig, "PI_CODING_AGENT_DIR"), "settings.json")?;
    write(named(&rig, "CODEX_HOME"), "config.toml")?;
    write(
        named(&rig, "CODEX_HOME"),
        "sessions/2026/10/26/rollout.jsonl",
    )?;
    write(named(&rig, "XDG_DATA_HOME"), "devin/cli/sessions.db")?;
    write(
        named(&rig, "XDG_DATA_HOME"),
        "devin/cli/trusted_workspaces.json",
    )?;
    write(named(&rig, "XDG_CACHE_HOME"), "devin/telemetry_state.json")?;
    rig.close()?;
    assert_eq!(
        owner.everything()?,
        before,
        "the owner's files and folders are as they were"
    );
    Ok(())
}

#[test]
fn a_run_that_changes_the_owners_files_fails_the_rig_that_ends_and_names_every_file() -> Outcome {
    let owner = Owner::new()?;
    let rig = Rig::start(live::rig(&owner.installs(), FAKE_AGENT))?;
    // A window that ignored its variable would have written these.
    owner.write(".pi/agent/auth.json", "refreshed")?;
    owner.write(
        ".pi/agent/sessions/--Users-me-.consensflow-candidate-live-pi--/s.jsonl",
        "{}",
    )?;
    owner.write(
        ".codex/config.toml",
        "[projects.\"/run\"]\ntrust_level = \"trusted\"\n",
    )?;
    owner.write(
        ".local/share/devin/cli/trusted_workspaces.json",
        "[\"/run\"]",
    )?;
    let failed = rig.close().unwrap_err().to_string();
    for named in [
        "~/.pi/agent/auth.json is not as it was",
        "~/.pi/agent/sessions gained --Users-me-.consensflow-candidate-live-pi--",
        "~/.codex/config.toml is not as it was",
        "~/.local/share/devin/cli/trusted_workspaces.json is not as it was",
    ] {
        assert!(failed.contains(named), "{named}: {failed}");
    }
    assert!(
        !failed.contains("refreshed") && !failed.contains("sk-or-OWNERS"),
        "{failed}"
    );
    Ok(())
}

#[test]
fn a_pi_login_that_will_not_last_the_run_stops_the_rig_before_anything_starts() -> Outcome {
    let owner = Owner::new()?;
    let soon = jiff::Timestamp::now().as_millisecond() + 10 * 60 * 1000;
    owner.write(
        ".pi/agent/auth.json",
        &json!({ "openai": { "type": "oauth", "access": "A", "refresh": "R", "expires": soon } })
            .to_string(),
    )?;
    let home = owner.home.path();
    let pi =
        Installed::at(Harness::Pi, "/nowhere/pi", home).with_isolation(Arc::new(Isolation::pi(
            home,
            &owner.path(".pi/agent"),
            "openai/gpt-6-luna",
            Duration::from_secs(3600),
        )));
    let Err(failed) = Rig::start(live::rig(&[pi], FAKE_AGENT)) else {
        panic!("a rig started on a Pi login that ends in ten minutes");
    };
    let failed = failed.to_string();
    assert!(failed.contains("`openai` expires in"), "{failed}");
    assert!(failed.contains("at least 1 h 30 min more"), "{failed}");
    assert!(
        !failed.contains("\"A\"") && !failed.contains("refresh\":"),
        "{failed}"
    );
    Ok(())
}
