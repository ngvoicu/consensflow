//! The models Devin offers, as `devin models list --format json` lists them: the
//! account's, fetched from Devin's server with its login (research-02 §3: 45 families and
//! 906 variants on 2026-10-28, 2 s). Signed out it fails ("Not logged in"), and with no
//! network too: it keeps no copy.
//!
//! Devin's effort is part of each variant's id, before any suffix that changes the price
//! (`claude-opus-5-5-medium-fast`), and is no field. A variant's id is read from its end:
//! past the suffixes ([`SUFFIXES`]), a word of [`EFFORTS`] is its effort, and the id
//! without it is the model. A model keeps the exact id of each effort, which a launch
//! passes: the two are never joined. An id with no such word is a model with no choice of
//! effort, and a new suffix reads as such, never as a wrong id. The `fusion` family (a
//! model and a sidekick, each with its effort, 575 of the variants) is left out.

use std::path::PathBuf;
use std::time::Duration;

use cf_base::env::Env;
use cf_proto::agents::Harness;
use cf_proto::models::{ModelChoice, ModelListing};
use serde_json::Value;

use crate::detect::executable;
use crate::seams::processes::{Limits, Processes, Program};
use crate::seams::Time;

/// How the list is read, as the human could run it.
pub const HOW: &str = "devin models list --format json";

/// How long the list may take, and how much it may say (about 500 KB).
const LIMITS: Limits = Limits {
    timeout: Duration::from_secs(30),
    max_buffer: 8 << 20,
};

/// Devin's efforts, from the lowest: `none` is no thinking.
const EFFORTS: [&str; 7] = ["none", "minimal", "low", "medium", "high", "xhigh", "max"];

/// What may follow the effort in a variant's id: another price or another context.
const SUFFIXES: [&str; 5] = ["fast", "priority", "lightning", "1m", "thinking"];

/// The program the list is read from.
pub fn program(executable: PathBuf, env: &Env) -> Program {
    Program {
        executable,
        args: HOW.split(' ').skip(1).map(str::to_owned).collect(),
        cwd: None,
        env: env.clone(),
    }
}

/// A variant's model and effort: `claude-opus-5-5-fast` and `medium` for
/// `claude-opus-5-5-medium-fast`; the id itself and none where it names no effort.
fn split(uid: &str) -> (String, Option<&'static str>) {
    let words: Vec<&str> = uid.split('-').collect();
    let suffixes = words
        .iter()
        .rev()
        .take_while(|word| SUFFIXES.contains(word))
        .count();
    let at = words.len() - suffixes;
    let effort = at
        .checked_sub(1)
        .filter(|&before| before > 0)
        .and_then(|before| EFFORTS.into_iter().find(|effort| *effort == words[before]));
    match effort {
        Some(effort) => {
            let mut model = words[..at - 1].to_vec();
            model.extend_from_slice(&words[at..]);
            (model.join("-"), Some(effort))
        }
        None => (uid.to_owned(), None),
    }
}

/// A variant's label with its effort's words taken out: `Claude Opus 5.5 Fast` for
/// `Claude Opus 5.5 Medium Fast`, `GPT-6 Sol` for `GPT-6 Sol No Thinking`.
fn label_without(label: &str, effort: &str) -> String {
    let said = if effort == "none" { "no" } else { effort };
    let words: Vec<&str> = label.split(' ').collect();
    let Some(at) = words
        .iter()
        .rposition(|word| word.eq_ignore_ascii_case(said))
    else {
        return label.to_owned();
    };
    let thinking = words
        .get(at + 1)
        .is_some_and(|word| word.eq_ignore_ascii_case("thinking"));
    let mut kept = words[..at].to_vec();
    kept.extend_from_slice(&words[at + 1 + usize::from(thinking)..]);
    kept.join(" ")
}

/// What the list says, its models in the order Devin lists them, each with its efforts
/// from the lowest, read at `at`. Fails where it lists no model.
pub fn parse(listed: &Value, at: i64) -> Result<ModelListing, String> {
    let families = listed
        .get("families")
        .and_then(Value::as_array)
        .ok_or("Devin's model list has no families")?;
    let text = |row: &Value, name: &str| row.get(name).and_then(Value::as_str).map(str::to_owned);
    let mut models: Vec<ModelChoice> = Vec::new();
    let fusion = |family: &&Value| {
        ["family_uid", "slug"]
            .iter()
            .any(|name| text(family, name).is_some_and(|uid| uid.starts_with("fusion")))
    };
    for family in families.iter().filter(|family| !fusion(family)) {
        for variant in family
            .get("variants")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(uid) = text(variant, "model_uid").filter(|uid| !uid.is_empty()) else {
                continue;
            };
            let label = text(variant, "label").unwrap_or_else(|| uid.clone());
            let (id, effort) = split(&uid);
            let at = match models.iter().position(|model| model.id == id) {
                Some(at) => at,
                None => {
                    let label =
                        effort.map_or(label.clone(), |effort| label_without(&label, effort));
                    models.push(ModelChoice::new(id, label));
                    models.len() - 1
                }
            };
            if let Some(effort) = effort {
                models[at].launch_ids.insert(effort.to_owned(), uid);
            }
        }
    }
    for model in &mut models {
        model.efforts = EFFORTS
            .into_iter()
            .filter(|effort| model.launch_ids.contains_key(*effort))
            .map(str::to_owned)
            .collect();
    }
    if models.is_empty() {
        return Err("Devin lists no model".to_owned());
    }
    Ok(ModelListing::read(models, at, HOW))
}

/// The models `program` lists, or why it could not say: Devin's own words where it ended
/// with one (`Not logged in. Run devin auth login to authenticate.`).
pub async fn read(
    processes: &dyn Processes,
    program: Program,
    at: i64,
) -> Result<ModelListing, String> {
    let listed = processes.run(program, LIMITS).await.map_err(|failed| {
        let said = failed.message.lines().skip(1).collect::<Vec<_>>().join(" ");
        let said = said.trim();
        let said = said.strip_prefix("Error: ").unwrap_or(said);
        format!(
            "{HOW}: {}",
            if said.is_empty() {
                &failed.message
            } else {
                said
            }
        )
    })?;
    let listed: Value =
        serde_json::from_str(&listed).map_err(|error| format!("{HOW} said no JSON: {error}"))?;
    parse(&listed, at)
}

/// The models the Devin installed here lists for its login.
pub(crate) async fn listed(
    env: &Env,
    processes: &dyn Processes,
    time: &dyn Time,
) -> Result<ModelListing, String> {
    let executable = executable(Harness::Devin, env)?;
    let at = time.wall_ms();
    read(processes, program(PathBuf::from(executable), env), at).await
}

#[cfg(test)]
mod tests;
