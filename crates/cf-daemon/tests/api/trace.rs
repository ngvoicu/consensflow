//! Node's traces, read as `tests/goldens/daemon/FORMAT.md` says: gzipped JSON,
//! one per test, with the ledger's path put in the text. What else differs
//! from one run to the next (the API's address, a window's token) is put
//! where it is used ([`Names`]).

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Where Node's recordings are.
pub fn goldens() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("goldens")
}

/// The names of the traces of the suites called `suites` (`core-api` has
/// `core-api-001`, `core-api-002`…), in order.
pub fn names(suites: &[&str]) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(goldens())
        .expect("the goldens: npm run goldens:daemon")
        .filter_map(|entry| {
            let file = entry.ok()?.file_name().to_string_lossy().into_owned();
            let name = file.strip_suffix(".json.gz")?;
            let (suite, number) = name.rsplit_once('-')?;
            let numbered = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
            (numbered && suites.contains(&suite)).then(|| name.to_owned())
        })
        .collect();
    found.sort();
    found
}

/// The trace `name`, with `ledger` where it says «ledger».
pub fn load(name: &str, ledger: &Path) -> Value {
    let file = goldens().join(format!("{name}.json.gz"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(File::open(&file).expect("a trace"))
        .read_to_string(&mut text)
        .expect("a gzipped trace");
    // As JSON writes the path: the recorder's text has it inside a string.
    let path = serde_json::to_string(&ledger.display().to_string()).expect("a path as text");
    let text = text.replace("«ledger»", &path[1..path.len() - 1]);
    serde_json::from_str(&text).expect("a trace that is JSON")
}

/// What the trace names that differs from one run to the next: the host and
/// port of the API, and each window's token by the name the trace gave it
/// (`«token:T1»`, in the order they were issued).
pub struct Names {
    address: String,
    tokens: HashMap<String, String>,
}

impl Names {
    pub fn new(address: &str) -> Self {
        Self {
            address: address.to_owned(),
            tokens: HashMap::new(),
        }
    }

    /// Window `name` has `token`.
    pub fn issued(&mut self, name: &str, token: String) {
        self.tokens.insert(name.to_owned(), token);
    }

    /// The token a window was issued, or the text itself for what no
    /// `issue` named (a token the test made up).
    pub fn token(&self, name: &str) -> String {
        self.tokens
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_owned())
    }

    /// `text` with the names put back.
    pub fn put(&self, text: &str) -> String {
        let mut text = text.replace("«api»", &self.address);
        for (name, token) in &self.tokens {
            text = text.replace(&format!("«token:{name}»"), token);
        }
        text
    }
}
