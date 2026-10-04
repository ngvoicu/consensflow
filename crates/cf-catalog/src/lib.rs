//! The catalog and the roster (`hosts/lib/presets.js`, `src/catalog.js`,
//! `src/roster.js`): the ready-made agents every harness ships, each agent's
//! profile and work tier, and the human's own agents in `agents.json`.
//!
//! While the JavaScript runs the app, its presets are the one source:
//! `data/presets.json` is generated from them (`npm run goldens:catalog`)
//! and the unit suite holds it equal, so the two cannot drift. Nothing here
//! writes `agents.json` before the flip; the Node build is its one writer.

#![forbid(unsafe_code)]

use serde::Deserialize;
use std::collections::BTreeMap;

/// The presets and model labels as `presets.js` has them, built into the binary.
const BUNDLED: &str = include_str!("../data/presets.json");

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// The bundled data does not read as presets: a build that would refuse
    /// every agent, said at start rather than at the first use.
    #[error("the bundled catalog cannot be read: {0}")]
    Bundled(#[from] serde_json::Error),
}

/// A ready-made agent, as `AGENT_PRESETS` writes one. A field the JavaScript
/// adds and this does not know fails the build's own test, not a user.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    /// Its provenance: what a copy of it in `agents.json` names.
    pub preset: String,
    pub id: String,
    pub name: String,
    /// The one-line headline the roster shows.
    pub label: String,
    /// The paragraph the catalog card shows.
    pub description: String,
    /// The harness, in the payload's vocabulary (`claude-code`, `codex`, `pi`, `opencode`, `devin`).
    pub kind: String,
    pub model: String,
    pub effort: Option<String>,
    /// Pi's word for the effort.
    pub thinking: Option<String>,
    /// An image agent: a Codex agent whose image tool draws.
    #[serde(default)]
    pub designer: bool,
}

/// The catalog: the presets in the order the JavaScript lists them, and
/// each model's label by its key.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Catalog {
    presets: Vec<Preset>,
    model_labels: BTreeMap<String, String>,
}

impl Catalog {
    /// The catalog built into this binary: read once, in `main`, and handed down.
    pub fn bundled() -> Result<Self, CatalogError> {
        Ok(serde_json::from_str(BUNDLED)?)
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
