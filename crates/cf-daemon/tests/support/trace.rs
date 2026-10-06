//! Node's traces, found and read as `tests/goldens/daemon/FORMAT.md` says, played
//! on the one thread the daemon runs on, and what a player held of them.

use std::fmt;
use std::fs::File;
use std::future::Future;
use std::io::Read;
use std::ops::AddAssign;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Where Node's recordings are.
fn goldens() -> PathBuf {
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

/// The trace `name`: its file gunzipped, and read as JSON.
///
/// What differs from one run to the next is named in it, and put where it is
/// used, by whoever owns the thing that differs: `«root»` by the world
/// (`World::expand`), `«now»` where the world writes a file and where a file is
/// compared, `«api»` and `«token:T1»` by the API's player. `«ledger»`, in
/// `ledger.file`, is left as it is: no player reads that field.
pub fn load(name: &str) -> Value {
    let file = goldens().join(format!("{name}.json.gz"));
    let mut text = String::new();
    flate2::read::GzDecoder::new(File::open(&file).expect("a trace"))
        .read_to_string(&mut text)
        .expect("a gzipped trace");
    serde_json::from_str(&text).expect("a trace that is JSON")
}

/// `play` on a runtime of its own and a local set, as the daemon runs: one
/// thread, so what `play` spawns is dropped with it.
pub fn locally<F: Future>(play: F) -> F::Output {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    tokio::task::LocalSet::new().block_on(&runtime, play)
}

/// What a player held of the traces it played: each count is a comparison it
/// made against what Node recorded.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Tally {
    pub traces: usize,
    /// Answers compared as bytes: the test's own exchanges with the API, and the
    /// requests `cf` made that the API took and answered as Node's did.
    pub exchanges: usize,
    pub operations: usize,
    /// Runs of `cf` whose output, error output and exit were compared.
    pub runs: usize,
    /// The database a ledger left, held to `ledger.final`.
    pub databases: usize,
}

impl AddAssign for Tally {
    fn add_assign(&mut self, other: Self) {
        self.traces += other.traces;
        self.exchanges += other.exchanges;
        self.operations += other.operations;
        self.runs += other.runs;
        self.databases += other.databases;
    }
}

impl fmt::Display for Tally {
    /// `18 traces, 111 exchanges, 18 database comparisons`: what was none is
    /// left out.
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        let counts = [
            (self.traces, "trace"),
            (self.exchanges, "exchange"),
            (self.operations, "operation"),
            (self.runs, "run"),
            (self.databases, "database comparison"),
        ];
        let said: Vec<String> = counts
            .iter()
            .filter(|(count, _)| *count > 0)
            .map(|(count, what)| format!("{count} {what}{}", if *count == 1 { "" } else { "s" }))
            .collect();
        write!(out, "{}", said.join(", "))
    }
}
