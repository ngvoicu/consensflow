//! The catalog and the roster (`hosts/lib/presets.js`, `src/catalog.js`,
//! `src/roster.js`): the ready-made agents every harness ships, each agent's
//! profile and work tier, and the human's own agents in `agents.json`.
//!
//! While the JavaScript runs the app, its presets are the one source:
//! `data/presets.json` is generated from them (`npm run goldens:catalog`)
//! and the unit suite holds it equal, so the two cannot drift. Nothing here
//! writes `agents.json` before the flip; the Node build is its one writer.
//!
//! `presets` reads the data, `profile` names a model and its road and tiers
//! its agent, `catalog` lists the presets by harness. The [`Catalog`] is
//! built once and handed down: it holds what a profile needs, so none of them
//! scans the presets again.

#![forbid(unsafe_code)]

mod catalog;
mod presets;
mod profile;

use std::collections::BTreeMap;

pub use catalog::{efforts, harness_for_kind, Group, HARNESSES};
pub use cf_proto::agents::{CatalogEntry, FoundEntry, Harness, Profile, WorkTier};
pub use presets::Preset;
pub use profile::{validate_work_tier, work_tier_info, Settings, WorkTierInfo, WORK_TIERS};

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// The bundled data does not read as presets: a build that would refuse
    /// every agent, said at start rather than at the first use.
    #[error("the bundled catalog cannot be read: {0}")]
    Bundled(#[from] serde_json::Error),
}

/// The catalog: the presets in the order the JavaScript lists them, each
/// model's label by its key, and what is worked out from them once: the
/// (harness, model) pairs the presets run, and the entries of every harness.
#[derive(Debug, Clone)]
pub struct Catalog {
    presets: Vec<Preset>,
    model_labels: BTreeMap<String, String>,
    known: profile::Known,
    groups: Vec<Group>,
}

impl Catalog {
    /// The catalog built into this binary: read once, in `main`, and handed down.
    pub fn bundled() -> Result<Self, CatalogError> {
        let data = presets::bundled()?;
        Ok(Self::new(data.presets, data.model_labels))
    }

    fn new(presets: Vec<Preset>, model_labels: BTreeMap<String, String>) -> Self {
        let known = profile::Known::of(&presets);
        let mut catalog = Self {
            presets,
            model_labels,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_catalog_reads_whole_every_field_known() {
        let catalog = Catalog::bundled().unwrap();
        assert_eq!(catalog.presets().len(), 119);
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
