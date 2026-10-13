use super::*;
use crate::shared::specmint::testing::{is_held_to, Fake};

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
