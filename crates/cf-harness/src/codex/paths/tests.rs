//! Whether Codex kept a thread's rollout: a file under its `sessions` named as
//! Codex names it, whatever else holds the thread's id.

use super::*;
use crate::shared::specmint::testing::{is_held_to, Fake};
use std::fs;
use std::path::Path;

#[test]
fn the_specmint_variant_the_human_installed_is_found_in_the_agents_skills_then_codexs_own_and_nowhere_else(
) {
    is_held_to(
        &|env, variant| global_specmint(env, variant),
        &[],
        &[".agents/skills", ".codex/skills"],
        &[
            ".claude/skills",
            ".config/opencode/skills",
            ".config/opencode/skill",
            ".pi/agent/skills",
            ".config/devin/skills",
            ".codex/skill",
        ],
    );
    // `CODEX_HOME` moves Codex's own folder, and the agents' stays where it is.
    is_held_to(
        &|env, variant| global_specmint(env, variant),
        &[("CODEX_HOME", "elsewhere")],
        &[".agents/skills", "elsewhere/skills"],
        &[".codex/skills"],
    );
}

#[test]
fn with_no_home_only_the_folder_codex_home_names_is_read() {
    let fake = Fake::new();
    let found = fake.install("codex/skills", "specmint-tdd", "specmint-tdd");
    // No `HOME`: the agents' folder has no place, and Codex's own is where `CODEX_HOME` says.
    let env = Env::from_vars([("CODEX_HOME", fake.at("codex"))]);
    assert_eq!(global_specmint(&env, Variant::Tdd), Some(found));
    assert_eq!(global_specmint(&Env::default(), Variant::Tdd), None);
}

const THREAD: &str = "019d4a7e-3c1b-7a52-9f0e-5b8d2c6a1e34";

/// The name Codex gives the rollout of `THREAD`.
fn rollout() -> String {
    format!("rollout-2026-10-05T01-00-00-{THREAD}.jsonl")
}

/// Files whose names hold `THREAD` and are not what Codex names its rollout:
/// a backup of it, a rollout of another thread whose id ends as this one does,
/// and names that are not a rollout's timestamp and thread.
fn lookalikes() -> Vec<String> {
    vec![
        format!("{}.bak", rollout()),
        format!("{}~", rollout()),
        format!("rollout-2026-10-05T01-00-00-{THREAD}-old.jsonl"),
        format!("rollout-2026-10-05T01-00-00-x{THREAD}.jsonl"),
        format!("rollout-2026-10-05T01-00-00-other-{THREAD}.jsonl"),
        format!("old-{}", rollout()),
        format!("rollout-2026-10-05T01-00-{THREAD}.jsonl"),
        format!("rollout-today-{THREAD}.jsonl"),
        format!("rollout-2026-10-05T01-00-00_{THREAD}.jsonl"),
        format!("rollout-{THREAD}.jsonl"),
        format!("{THREAD}.jsonl"),
    ]
}

/// `name` kept under the day's folder of the sessions of the Codex home `home`.
fn keep(home: &Path, name: &str) {
    let day = home.join("sessions").join("2026").join("10").join("05");
    fs::create_dir_all(&day).unwrap();
    fs::write(day.join(name), "").unwrap();
}

/// An environment whose Codex home is `home`.
fn at(home: &Path) -> Env {
    Env::from_vars([("CODEX_HOME", home.to_str().unwrap())])
}

/// Whether the rollout of `THREAD` is kept in the Codex home `home`.
fn kept(home: &Path) -> bool {
    rollout_kept(THREAD, &at(home)).unwrap()
}

#[test]
fn the_rollout_of_a_thread_is_kept_once_a_file_is_named_as_codex_names_it() {
    let home = tempfile::tempdir().unwrap();
    assert!(!kept(home.path()), "nothing kept yet");
    keep(home.path(), &rollout());
    assert!(kept(home.path()));
}

#[test]
fn a_file_that_holds_the_threads_id_in_its_name_and_is_not_its_rollout_is_none() {
    let home = tempfile::tempdir().unwrap();
    for name in lookalikes() {
        keep(home.path(), &name);
        assert!(!kept(home.path()), "{name}");
    }
}

#[test]
fn the_rollout_is_kept_though_files_that_look_like_it_are_beside_it() {
    let home = tempfile::tempdir().unwrap();
    for name in lookalikes() {
        keep(home.path(), &name);
    }
    keep(home.path(), &rollout());
    assert!(kept(home.path()));
}

#[test]
fn the_record_is_read_from_the_first_file_that_holds_the_id_where_only_the_rollout_kept_is_strict()
{
    let home = tempfile::tempdir().unwrap();
    keep(home.path(), &format!("{THREAD}.jsonl"));
    assert!(transcript(THREAD, &at(home.path())).unwrap().is_some());
    assert!(!kept(home.path()));
}
