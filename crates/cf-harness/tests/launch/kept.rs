//! Where ConsensFlow keeps its copy of Specmint for a harness, as every
//! harness's launch tests hold it: in `integrations/specmint/<harness>/<variant>-<commit>`
//! of ConsensFlow's folder, the same for every launch and in no launch's folder.
//! The commit is the one the vendored manifest pins, read from the checkout and
//! not by the adapters.

use std::fs;
use std::path::Path;

use cf_base::path;
use cf_proto::specs::Variant;
use serde_json::Value;

/// The upstream commit the vendored manifest pins `variant` at.
fn commit(variant: Variant) -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skill/specmint/manifest.json");
    let manifest: Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    manifest[variant.as_str()]["commit"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// The folder the copy of `variant` is kept in for `harness` (`claude`, `pi`,
/// `devin`, `opencode` or `codex`), in ConsensFlow's own folder `home`: what a
/// harness that reads the skills of the folders it is given is given.
pub fn root(home: &str, harness: &str, variant: Variant) -> String {
    path::join(&[
        home,
        "integrations",
        "specmint",
        harness,
        &format!("{}-{}", variant.as_str(), commit(variant)),
    ])
}

/// The folder of skills in it: what OpenCode is told to read.
pub fn skills(home: &str, harness: &str, variant: Variant) -> String {
    path::join(&[&root(home, harness, variant), ".claude", "skills"])
}

/// The skill's own folder, whose `SKILL.md` the chief's role text names.
pub fn skill(home: &str, harness: &str, variant: Variant) -> String {
    path::join(&[&skills(home, harness, variant), variant.skill_name()])
}

/// The names in `folder`, sorted: what a folder of skills holds.
pub fn names_in(folder: &str) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
