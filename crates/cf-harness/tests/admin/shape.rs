//! What Node's record and the Rust answers are written in alike: where a
//! golden is, how a text names the scenario's folder, and the JSON of a
//! source and of detection.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use cf_base::env::Env;
use cf_harness::admin::Source;
use cf_harness::detect::{detect_harnesses, harness_path, known_harnesses, missing_harnesses};
use cf_proto::agents::Harness;
use regex::Regex;
use serde_json::{json, Map, Value};

/// The name of this system's goldens: Windows' or, as every other system
/// the tests run on, macOS's.
pub const PLATFORM: &str = if cfg!(windows) { "win32" } else { "darwin" };

/// A golden's contents.
pub fn golden(name: &str) -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/goldens/admin")
        .join(name);
    let contents = fs::read_to_string(&file).unwrap_or_else(|error| {
        panic!(
            "{}: {error}: record it on this system with `npm run goldens:admin`",
            file.display()
        )
    });
    serde_json::from_str(&contents).unwrap()
}

/// The text of a field of a record, or none where it has none.
pub fn field<'a>(value: &'a Value, name: &str) -> &'a str {
    value[name].as_str().unwrap_or_default()
}

/// A path as Node writes one: without the prefix Windows gives a resolved
/// one.
fn plain(path: &str) -> String {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}");
    }
    path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
}

/// A folder as the system names it, as Node's recorder names its own
/// (`realpath`).
pub fn root_of(dir: &Path) -> String {
    plain(&fs::canonicalize(dir).unwrap().to_string_lossy())
}

/// `value` with each of its texts mapped through `change`.
fn map_texts(value: &Value, change: &impl Fn(&str) -> String) -> Value {
    match value {
        Value::String(text) => Value::String(change(text)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| map_texts(item, change)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(key, field)| (key.clone(), map_texts(field, change)))
                .collect::<Map<_, _>>(),
        ),
        other => other.clone(),
    }
}

/// What a scenario writes `$ROOT` for, written as the folder.
pub fn substitute(value: &Value, root: &str) -> Value {
    map_texts(value, &|text| text.replace("$ROOT", root))
}

/// The hash that names a bundle of an extension, which depends on the files
/// of this build.
static BUNDLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([\\/]extensions[\\/](?:pi|opencode)[\\/])[0-9a-f]{64}").unwrap()
});

/// A text as Node's record writes it: the folder as `$ROOT`, a bundle's hash
/// as `$HASH`.
pub fn normalize(value: &Value, root: &str) -> Value {
    map_texts(value, &|text| {
        BUNDLE
            .replace_all(&text.replace(root, "$ROOT"), "${1}$$HASH")
            .into_owned()
    })
}

/// A source as `releaseSource` says it.
pub fn source_json(source: &Source) -> Value {
    json!({
        "url": source.url,
        "format": source.format.as_str(),
        "distribution": source.distribution,
        "update": source.update,
    })
}

/// What detection says of the environment, as the recorder writes it.
pub fn detection_json(env: &Env) -> Value {
    let known = known_harnesses();
    let names = |harnesses: &[Harness]| -> Vec<&'static str> {
        harnesses.iter().map(|harness| harness.as_str()).collect()
    };
    let paths: Map<String, Value> = known
        .iter()
        .map(|harness| {
            let found = harness_path(*harness, env)
                .map_or(Value::Null, |path| json!(path.to_string_lossy()));
            (harness.as_str().to_owned(), found)
        })
        .collect();
    json!({
        "known": names(&known),
        "missing": names(&missing_harnesses(env)),
        "detected": detect_harnesses(env),
        "paths": paths,
    })
}

/// A value as text that tells key order apart, which `Value`'s equality
/// does not.
pub fn text(value: &Value) -> String {
    serde_json::to_string(value).unwrap()
}

/// A value to read when two differ.
pub fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap()
}
