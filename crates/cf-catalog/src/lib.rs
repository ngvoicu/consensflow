//! The catalog and the roster: the ready-made agents every harness ships, each
//! agent's profile and work tier, and the human's own agents in `agents.json`.
//!
//! `data/presets.json` is the one source of the presets and the model labels,
//! and it is edited directly: it was generated from Node's presets until that
//! code was deleted, and nothing generates it now. JSON holds no comments, so
//! `data/README.md` keeps the reasons behind its rows (the effort ceilings,
//! what each model was probed with), and the tests in `tests/catalog/` hold the
//! rules a test can hold.
//!
//! `presets` reads the data, `profile` names a model and its road and tiers
//! its agent, `catalog` lists the presets by harness, and `roster` reads the
//! human's file over them. The [`Catalog`] is built once and handed down: it
//! holds what a profile needs, so none of them scans the presets again.

#![forbid(unsafe_code)]

mod catalog;
mod presets;
mod profile;
mod roster;

use std::collections::BTreeMap;

pub use catalog::{efforts, Group};
pub use cf_proto::agents::{
    AgentView, CatalogEntry, FoundEntry, Harness, Preferences, Profile, WorkTier, WorkTierInfo,
};
pub use presets::{Preset, Retired};
pub use profile::{validate_work_tier, work_tier_info, Settings, WORK_TIERS};
pub use roster::{roster_path, AgentRow, Roster};

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// The bundled data does not read as presets: a build that would refuse
    /// every agent, said at start rather than at the first use.
    #[error("the bundled catalog cannot be read: {0}")]
    Bundled(#[from] serde_json::Error),
}

/// The catalog: the presets in the order the data lists them, each model's
/// label by its key, the agents it no longer carries, and what is worked out
/// from them once: the (harness, model) pairs the presets run, and the
/// entries of every harness.
#[derive(Debug, Clone)]
pub struct Catalog {
    presets: Vec<Preset>,
    model_labels: BTreeMap<String, String>,
    retired: Vec<Retired>,
    known: profile::Known,
    groups: Vec<Group>,
}

impl Catalog {
    /// The catalog built into this binary: read once, in `main`, and handed down.
    pub fn bundled() -> Result<Self, CatalogError> {
        let data = presets::bundled()?;
        let mut catalog = Self::new(data.presets, data.model_labels);
        catalog.retired = data.retired;
        Ok(catalog)
    }

    fn new(presets: Vec<Preset>, model_labels: BTreeMap<String, String>) -> Self {
        let known = profile::Known::of(&presets);
        let mut catalog = Self {
            presets,
            model_labels,
            retired: Vec::new(),
            known,
            groups: Vec::new(),
        };
        catalog.groups = catalog.grouped();
        catalog
    }

    /// The ready-made agents, in the catalog's order.
    pub fn presets(&self) -> &[Preset] {
        &self.presets
    }

    /// A model's label by its key (`gpt-6-astra`: "GPT-6 Astra").
    pub fn model_label(&self, key: &str) -> Option<&str> {
        self.model_labels.get(key).map(String::as_str)
    }

    /// The ready-made agent named `name` (one leading `@` taken off) that the
    /// catalog no longer carries, and why: none for any other name. A
    /// saved agent of the human's own under that name is theirs, and asked
    /// of the roster first.
    pub fn retired(&self, name: &str) -> Option<&Retired> {
        let name = name.strip_prefix('@').unwrap_or(name);
        self.retired.iter().find(|retired| retired.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_catalog_reads_whole_every_field_known() {
        let catalog = Catalog::bundled().unwrap();
        assert_eq!(catalog.presets().len(), 251);
        assert_eq!(
            catalog
                .presets()
                .iter()
                .filter(|preset| preset.designer)
                .count(),
            1,
            "one image agent"
        );
        assert_eq!(catalog.model_label("gpt-6-astra"), Some("GPT-6 Astra"));
        assert_eq!(catalog.model_label("no-such-model"), None);
    }
}
