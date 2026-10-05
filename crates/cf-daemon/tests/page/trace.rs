//! Reading a trace as `tests/goldens/daemon/FORMAT.md` says: the file is
//! gzipped JSON, read as text first, the folder of this run put in the place of
//! `«root»`, the ledger's file in the place of `«ledger»`, and a time of its own
//! where Node wrote `«now»` (the stamps of a roster file); then parsed.

use std::io::Read;
use std::path::Path;

use serde_json::Value;

/// The folder of the recordings.
const GOLDENS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/goldens");

/// What stands in the place of the time a writer stamped from its own clock.
const NOW: &str = "2026-10-05T10:00:00.000Z";

/// The names of the page's traces, in the order the suites ran them.
pub fn names() -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(GOLDENS)
        .expect("the recordings: npm run goldens:daemon")
        .filter_map(|entry| {
            let name = entry.ok()?.file_name().into_string().ok()?;
            let name = name.strip_suffix(".json.gz")?.to_owned();
            (name.starts_with("core-page-") || name.starts_with("corners-page-")).then_some(name)
        })
        .collect();
    names.sort();
    names
}

/// Text as it stands inside a JSON string: a path's backslashes escaped.
fn escaped(path: &Path) -> String {
    let quoted = serde_json::to_string(&path.display().to_string()).unwrap();
    quoted[1..quoted.len() - 1].to_owned()
}

/// The trace `name`, made for a run in `root` over the ledger at `ledger`.
pub fn load(name: &str, root: &Path, ledger: &Path) -> Value {
    let file = Path::new(GOLDENS).join(format!("{name}.json.gz"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&file).unwrap())
        .read_to_string(&mut text)
        .unwrap();
    let text = text
        .replace("«root»", &escaped(root))
        .replace("«ledger»", &escaped(ledger))
        .replace("«now»", NOW);
    serde_json::from_str(&text).unwrap()
}
