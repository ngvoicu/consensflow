//! What the human says of specs for every project that does not say otherwise
//! (`cf_proto::specs::AppSpecs`): whether they are on, and the folder and the
//! Specmint variant a project uses when it names none. Each chief's launch reads
//! them, and the page alone sets them.
//!
//! They are kept in `agents.json` in ConsensFlow's folder, the file the released
//! 3.0.4 shares, where it leaves them as they are. That file once held the human's
//! saved agents too; this build reads none of them (spec `harness-models`: a member
//! is a harness, a model, an effort and a tier) and writes back every key of the
//! file it does not set as it found it, `agents` among them, for an older build.
//!
//! Whether they are on is `preferences.specs` (only when on), which 3.0.4 reads and
//! writes again. The folder and the variant are a key of the file's own,
//! `specsDefaults: {folder, variant}` (each only where named, and the key only where
//! one is): 3.0.4 sets `preferences` to an object of its two choices at each save,
//! and keeps every other key of the file as it is. An earlier build of this one kept
//! them in `preferences` (`specsFolder`, `specsVariant`): they are read there where
//! `specsDefaults` does not say them, and moved at its next write.

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_base::file::{error_code, is_missing, write_whole};
use cf_base::home::config_root;
use cf_base::js;
use cf_base::json::from_slice_exact;
use cf_base::path;
use cf_base::refusal::Refusal;
use cf_proto::specs::{check, AppSpecs, Variant};
use serde_json::{Map, Value};

/// The key of the file the folder and the variant are kept under.
const DEFAULTS: &str = "specsDefaults";

/// Where an earlier build of this one kept the folder and the variant, in
/// `preferences`: read where `specsDefaults` says nothing, never written.
const EARLIER: [&str; 2] = ["specsFolder", "specsVariant"];

/// The file, inside ConsensFlow's folder, joined as Node's `path.join` joined it: a
/// `..` in `CONSENSFLOW_HOME` takes off the folder before it by the text alone. None
/// when the environment names no folder to keep it in.
pub fn settings_path(env: &Env) -> Option<PathBuf> {
    config_root(env)
        .map(|root| PathBuf::from(path::join(&[&root.to_string_lossy(), "agents.json"])))
}

/// The app's settings file at `path`, asked afresh at each use, as a launch and a
/// page read it.
#[derive(Debug, Clone)]
pub struct SettingsFile {
    path: PathBuf,
}

impl SettingsFile {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// What the file says of specs for every project now.
    pub fn app_specs(&self) -> Result<AppSpecs, Refusal> {
        Ok(app_specs_of(&read_file(&self.path)?))
    }

    /// Sets what `said` says of specs for every project (`{on, folder, variant}`, a
    /// part left out as it was), each part checked before anything is written, and
    /// answers them as kept.
    pub fn set_app_specs(&self, said: &Value) -> Result<AppSpecs, Refusal> {
        set_app_specs(&self.path, said)
    }
}

/// What a launch reads of the app: off, naming no folder and no variant, where the
/// file says nothing or cannot be read.
impl cf_engine::seams::AppSettings for SettingsFile {
    fn app_specs(&self) -> AppSpecs {
        SettingsFile::app_specs(self).unwrap_or_default()
    }
}

/// The file as the human left it: the object it holds, its keys in the order
/// JavaScript enumerates them; an empty one where there is no file. Reading creates
/// nothing. A file that cannot be read, or holds no object, is said, and nothing is
/// written over it.
fn read_file(path: &Path) -> Result<Map<String, Value>, Refusal> {
    // Node says EISDIR for a directory on every system.
    if path.is_dir() {
        return Err(unreadable(path, "cannot be read (EISDIR)"));
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if is_missing(&error) => return Ok(Map::new()),
        Err(error) => {
            return Err(unreadable(
                path,
                &format!("cannot be read ({})", error_code(&error)),
            ))
        }
    };
    match from_slice_exact(&bytes).map_err(|_| unreadable(path, "is not valid JSON"))? {
        Value::Object(fields) => Ok(fields),
        _ => Err(unreadable(path, "is not a settings file")),
    }
}

/// The refusal that says the file at `path` is not one ConsensFlow can use.
fn unreadable(path: &Path, why: &str) -> Refusal {
    Refusal::new(
        "settings-file-unreadable",
        format!(
            "Your settings file {} {why}: fix it or move it away. ConsensFlow left it as it is.",
            path.display()
        ),
    )
}

/// What the file's `preferences` keep under `key`, as written.
fn kept<'a>(file: &'a Map<String, Value>, key: &str) -> Option<&'a Value> {
    file.get("preferences")
        .and_then(|preferences| preferences.get(key))
}

/// What the file says of specs for every project. Off unless it says `true` itself,
/// and no folder or variant unless it names a folder a project may keep its specs in
/// and a variant Specmint has: a file edited by hand to say anything else reads as one
/// that says nothing, so no launch is ever given a folder that leaves its project.
fn app_specs_of(file: &Map<String, Value>) -> AppSpecs {
    let defaults = file.get(DEFAULTS);
    let text = |key: &str, earlier: &str| {
        defaults
            .and_then(|defaults| defaults.get(key))
            .or_else(|| kept(file, earlier))
            .and_then(Value::as_str)
    };
    AppSpecs {
        on: kept(file, "specs") == Some(&Value::Bool(true)),
        folder: text("folder", EARLIER[0]).and_then(|folder| check::folder(folder).ok().flatten()),
        variant: text("variant", EARLIER[1]).and_then(Variant::from_name),
    }
}

/// Sets what `said` says of specs for every project, as the page says it (`{on,
/// folder, variant}`): `on` is true or false, `folder` is text (blank is none) or
/// `null` to name none again, as a project's own folder is checked, and `variant` is
/// one of the four names or `null`. A part it leaves out is as it was. Every part is
/// checked, in that order, before anything is written; then the file is written, every
/// key it does not set as it was, and made where there is none.
fn set_app_specs(path: &Path, said: &Value) -> Result<AppSpecs, Refusal> {
    let mut file = read_file(path)?;
    let mut next = app_specs_of(&file);
    if let Some(on) = said.get("on") {
        next.on = on.as_bool().ok_or_else(|| {
            Refusal::new(
                "preference-specs",
                "specs for every project are on (true) or off (false)",
            )
        })?;
    }
    if let Some(folder) = said.get("folder") {
        next.folder = folder_said(folder)?;
    }
    if let Some(variant) = said.get("variant") {
        next.variant = variant_said(variant)?;
    }
    keep(&mut file, &next);
    save(path, &file)?;
    Ok(next)
}

/// `app` set as the file keeps it: whether specs are on in `preferences` (only when
/// they are, in its place where it had one), the earlier places of the folder and the
/// variant taken out; every other key of `preferences` as it was. The folder and the
/// variant under `specsDefaults`, beside what a later build keeps there, the key taken
/// out where it holds nothing. A file made now starts as 3.0.4 reads one: its version
/// and an empty list of agents first.
fn keep(file: &mut Map<String, Value>, app: &AppSpecs) {
    if file.is_empty() {
        file.insert("schemaVersion".to_owned(), Value::from(1));
        file.insert("agents".to_owned(), Value::Array(Vec::new()));
    }
    let mut preferences = match file.get("preferences") {
        Some(Value::Object(before)) => before.clone(),
        _ => Map::new(),
    };
    for earlier in EARLIER {
        preferences.shift_remove(earlier);
    }
    if app.on {
        preferences.insert("specs".to_owned(), Value::Bool(true));
    } else {
        preferences.shift_remove("specs");
    }
    file.insert("preferences".to_owned(), Value::Object(preferences));
    let mut defaults = Map::new();
    if let Some(folder) = &app.folder {
        defaults.insert("folder".to_owned(), Value::String(folder.clone()));
    }
    if let Some(variant) = app.variant {
        defaults.insert(
            "variant".to_owned(),
            Value::String(variant.as_str().to_owned()),
        );
    }
    if let Some(Value::Object(before)) = file.get(DEFAULTS) {
        let later = before
            .iter()
            .filter(|(key, _)| !matches!(key.as_str(), "folder" | "variant"));
        defaults.extend(later.map(|(key, value)| (key.clone(), value.clone())));
    }
    if defaults.is_empty() {
        file.shift_remove(DEFAULTS);
    } else {
        file.insert(DEFAULTS.to_owned(), Value::Object(defaults));
    }
}

/// Writes `file` whole or not at all, as `JSON.stringify(file, null, 2)` writes it
/// with a line break after.
fn save(path: &Path, file: &Map<String, Value>) -> Result<(), Refusal> {
    let text = format!(
        "{}\n",
        js::stringify_indented(&Value::Object(file.clone()), 2)
    );
    write_whole(path, text.as_bytes())
        .map_err(|error| Refusal::new("settings-file-unwritable", error.to_string()))
}

/// The folder every project keeps its specs in unless it names its own: text, which
/// is none where it is blank, or `null` to name none again.
fn folder_said(value: &Value) -> Result<Option<String>, Refusal> {
    let refused = |words: String| Refusal::new("preference-specs-folder", words);
    match value {
        Value::Null => Ok(None),
        Value::String(text) => check::folder(text).map_err(refused),
        _ => Err(refused(check::folder_refused("it is text"))),
    }
}

/// The variant every project plans with unless it names its own: one of the four
/// names, or `null` to name none again.
fn variant_said(value: &Value) -> Result<Option<Variant>, Refusal> {
    let refused = |words: String| Refusal::new("preference-specs-variant", words);
    match value {
        Value::Null => Ok(None),
        Value::String(name) => check::variant(name).map(Some).map_err(refused),
        _ => Err(refused("the specs variant is a name".to_owned())),
    }
}

#[cfg(test)]
mod tests;
