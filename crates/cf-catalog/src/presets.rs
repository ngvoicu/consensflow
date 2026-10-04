//! The presets (`AGENT_PRESETS` and `MODEL_LABELS`, `hosts/lib/presets.js`)
//! as data: what the crate embeds, and how it reads them.

use serde::Deserialize;
use std::collections::BTreeMap;

/// The presets and model labels as `presets.js` has them, built into the binary.
const BUNDLED: &str = include_str!("../data/presets.json");

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

/// What `data/presets.json` holds: the presets in the order the JavaScript
/// lists them, and each model's label by its key.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(crate) struct Data {
    pub(crate) presets: Vec<Preset>,
    pub(crate) model_labels: BTreeMap<String, String>,
}

/// The data built into this binary.
pub(crate) fn bundled() -> serde_json::Result<Data> {
    serde_json::from_str(BUNDLED)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_data_reads_with_every_preset_whole() {
        let data = bundled().unwrap();
        assert_eq!(data.presets.len(), 119);
        assert_eq!(
            data.model_labels.get("gpt-6-astra").map(String::as_str),
            Some("GPT-6 Astra")
        );
    }

    #[test]
    fn a_field_the_catalog_does_not_know_is_refused_not_dropped() {
        let preset = r#"{"preset":"a","id":"a","name":"A","label":"A","description":"A","kind":"codex","model":"m"}"#;
        assert!(serde_json::from_str::<Preset>(preset).is_ok());
        let extra = preset.replace(r#""model":"m""#, r#""model":"m","colour":"green""#);
        assert!(serde_json::from_str::<Preset>(&extra).is_err());
        let data = |extra: &str| format!(r#"{{"presets":[{preset}],"modelLabels":{{}}{extra}}}"#);
        assert!(serde_json::from_str::<Data>(&data("")).is_ok());
        assert!(serde_json::from_str::<Data>(&data(r#","tiers":{}"#)).is_err());
    }
}
