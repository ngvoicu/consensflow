//! `npm run parity:launch`, the Rust half: each launch Node's half
//! (`tests/parity/launch.mjs`) planned with Node's adapters in a root of its
//! own, planned here by the Rust adapters, through the switch
//! (`launch::adapter`), in the other root, with the same real CLIs, and held
//! to the same plan. Ignored by `cargo test`: only the npm script, which
//! makes the roots and plans Node's side first and names its file in
//! `CF_PARITY_LAUNCH`, runs it.
//!
//! Both sides are normalized here, by one normalizer: Node's half writes what
//! it found raw. Each root is written `$ROOT` in every spelling the plans
//! hold it in (as a path, in JSON, in a URL's query, as a file URL), and what
//! a plan draws (uuids, ports, tokens, OpenCode's conversation, Pi's name for
//! one, the hash of a bundle) is named by its order of appearance on each
//! side, so that what is drawn twice is the same name twice. What a plan is
//! given (the launch's id, a conversation it resumes) is not drawn: it stays
//! as it is. Pi's bundle is named by what is in it, which no root changes:
//! its hashes are held equal as they are as well.
//!
//! Held equal: each plan's argv, environment (in its order), variables
//! dropped and conversation; a refusal's sentence; and everything the plan
//! changed in the tree (paths, modes, the text of each file), but for the
//! folders a CLI keeps its own state in, which are counted and said. A case
//! made for a refusal must be refused by both sides, in its words. A
//! refusal that says a CLI could not answer (it has no login here, no
//! network, is too old or too slow) says nothing of the plan, nor of the
//! refusal such a case is for, which comes once the CLI has answered: the
//! case is skipped, with Node's reason, where Rust is refused in the same
//! words.
//!
//! Each side's plans are timed, and the table says how long each took.

// The run's own scaffolding: a failure in it is the test's.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod compare;
#[cfg(test)]
mod fixtures;
mod normalize;
mod report;
mod tree;
mod verdict;

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_harness::contract::{Agent, Launch, LaunchId};
use cf_harness::launch;
use cf_harness::records;
use cf_harness::seams::{
    Bundle, LoopbackPorts, Probes, Services, SystemEntropy, SystemLoopback, SystemProcesses,
    SystemTime, Time,
};
use cf_harness::testing::LocalRecords;
use cf_proto::agents::Harness;
use jiff::tz::TimeZone;
use serde::Deserialize;

use normalize::{normalize, Raw};
use report::{millis, report, Tally};
use tree::{changes, gained, snapshot};
use verdict::verdict;

/// A line of Node's file: its header, a harness it left out, or a case.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
enum Line {
    Header(Box<Header>),
    Skipped { kind: String, reason: String },
    Case(Box<Case>),
}

/// What both sides share: the Rust root and how each root reads in text, the
/// bundle ConsensFlow ships (the checkout's own, as Node names it), the
/// machine's zone, and the CLIs' folders, which are counted.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    zone: String,
    rust_root: String,
    forms: Sides<Spellings>,
    bundle: BundleFiles,
    owned: Vec<String>,
}

#[derive(Deserialize)]
struct Sides<T> {
    node: T,
    rust: T,
}

/// The ways a root reads in text besides as itself (`rootForms`,
/// `tests/parity/tree.mjs`).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Spellings {
    file_url: String,
    plain: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BundleFiles {
    bin: String,
    cf: String,
    pane_cf: String,
}

/// A launch to plan, and what each side did with it.
#[derive(Deserialize)]
struct Case {
    kind: String,
    name: String,
    /// What the refusal this case is to be refused with begins with.
    refuses: Option<String>,
    launch: Request,
    node: Found,
    rust: Setting,
}

/// A launch as a daemon asks for it: the same for both sides.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Request {
    launch_id: String,
    project: i64,
    handle: String,
    role: String,
    resume: Option<String>,
    message: Option<String>,
    agent: Option<Chosen>,
    instructions: String,
}

/// An agent's model and levels, as the launch gives them.
#[derive(Deserialize)]
struct Chosen {
    model: Option<String>,
    effort: Option<String>,
    thinking: Option<String>,
    #[serde(default)]
    designer: bool,
}

/// What Node's plan came to, raw: the environment and folder it was made in,
/// how it ended, what it changed in its root, what the CLIs made in theirs,
/// and how long it took in milliseconds.
#[derive(Deserialize)]
struct Found {
    env: Vec<(String, String)>,
    directory: String,
    outcome: Outcome,
    changes: Vec<Change>,
    owned: BTreeMap<String, i64>,
    ms: f64,
}

/// What the Rust side is given to plan in: its root's own places.
#[derive(Deserialize)]
struct Setting {
    env: Vec<(String, String)>,
    directory: String,
}

/// How a plan ended: the plan, or the sentence it was refused with.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Outcome {
    Plan(Plan),
    Refused(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Plan {
    argv: Vec<String>,
    env: Vec<(String, String)>,
    drop_env: Vec<String>,
    native_session: Option<String>,
}

/// A path made or changed, as it is now, or removed (`tree.mjs`): a file's
/// mode (none on Windows) and its text, or the SHA-256 of what is no UTF-8;
/// a link's target.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
struct Change {
    path: String,
    /// `file`, `dir`, `link` or `removed`.
    kind: String,
    #[serde(default)]
    mode: Option<u32>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    bytes: Option<String>,
    #[serde(default)]
    target: Option<String>,
}

impl Change {
    fn new(path: &str, kind: &str) -> Self {
        Self {
            path: path.to_owned(),
            kind: kind.to_owned(),
            mode: None,
            text: None,
            bytes: None,
            target: None,
        }
    }
}

/// What the Rust adapters made of a case, raw: how the plan ended, what it
/// changed in the root, what the CLIs made in their folders, and how long it
/// took.
struct Planned {
    outcome: Outcome,
    changes: Vec<Change>,
    owned: BTreeMap<String, i64>,
    took: Duration,
}

/// What the Rust adapters plan with: a runtime of one thread, as the engine's
/// is, the bundle and the zone as Node names them, the probes every launch
/// shares (a CLI is asked once as it is on disk, as Node asks it), and the
/// root they plan in.
struct Machine {
    runtime: tokio::runtime::Runtime,
    bundle: Bundle,
    zone: TimeZone,
    probes: Rc<Probes>,
    root: PathBuf,
    owned: Vec<String>,
}

impl Machine {
    fn new(header: &Header) -> Self {
        Self {
            runtime: tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap(),
            bundle: Bundle {
                bin: PathBuf::from(&header.bundle.bin),
                cf: PathBuf::from(&header.bundle.cf),
                pane_cf: header.bundle.pane_cf.clone(),
            },
            zone: records::zone(&header.zone)
                .unwrap_or_else(|| panic!("{}: a zone Intl does not take", header.zone)),
            probes: Rc::new(Probes::default()),
            root: PathBuf::from(&header.rust_root),
            owned: header.owned.clone(),
        }
    }

    /// Plans `case` through the switch, with the system's own services, in
    /// the Rust root. The CLIs it started are ended with it.
    fn plan(&self, case: &Case) -> Planned {
        let env = Env::from_vars(case.rust.env.iter().cloned());
        let processes = Rc::new(SystemProcesses::new(env.clone()));
        let time: Rc<dyn Time> = Rc::new(SystemTime);
        let services = Services {
            env: env.clone(),
            records: Rc::new(LocalRecords::new(env, Rc::clone(&time))),
            time,
            entropy: Rc::new(SystemEntropy),
            ports: Rc::new(LoopbackPorts),
            loopback: Rc::new(SystemLoopback),
            processes: Rc::clone(&processes) as Rc<_>,
            probes: Rc::clone(&self.probes),
            bundle: self.bundle.clone(),
            zone: self.zone.clone(),
        };
        let adapter = launch::adapter(Harness::from_kind(&case.kind).unwrap(), &services);
        let id = LaunchId::new(&case.launch.launch_id).expect("a launch id");
        let agent = case.launch.agent.as_ref().map(|agent| Agent {
            model: agent.model.as_deref(),
            effort: agent.effort.as_deref(),
            thinking: agent.thinking.as_deref(),
            designer: agent.designer,
        });
        let asked = Launch {
            id: &id,
            project: case.launch.project,
            handle: &case.launch.handle,
            role: &case.launch.role,
            directory: &case.rust.directory,
            resume: case.launch.resume.as_deref(),
            message: case.launch.message.as_deref(),
            agent,
            instructions: &case.launch.instructions,
        };

        let before = snapshot(&self.root, &self.owned);
        let started = Instant::now();
        let prepared = self.runtime.block_on(adapter.prepare(&asked));
        let took = started.elapsed();
        processes.end_all();
        let after = snapshot(&self.root, &self.owned);
        let outcome = match prepared {
            Ok(prepared) => Outcome::Plan(Plan {
                argv: prepared.argv,
                env: prepared.env,
                drop_env: prepared.drop_env,
                native_session: prepared.native_session,
            }),
            Err(refused) => Outcome::Refused(refused),
        };
        Planned {
            outcome,
            changes: changes(&before, &after),
            owned: gained(&before, &after),
            took,
        }
    }
}

#[test]
#[ignore = "plans with the real CLIs installed here: npm run parity:launch"]
fn every_launch_node_planned_here_is_planned_the_same() {
    let process = Env::from_process();
    let file = process
        .path("CF_PARITY_LAUNCH")
        .expect("Node's plans, named by npm run parity:launch");
    let text =
        fs::read_to_string(file).unwrap_or_else(|error| panic!("{}: {error}", file.display()));
    let mut lines = text
        .lines()
        .map(|line| serde_json::from_str::<Line>(line).unwrap());
    let Some(Line::Header(header)) = lines.next() else {
        panic!("Node's file begins with its header");
    };
    let machine = Machine::new(&header);
    let mut tallies: Vec<Tally> = Vec::new();
    let mut differences = Vec::new();
    for line in lines {
        let case = match line {
            Line::Header(_) => panic!("Node's file has one header"),
            Line::Skipped { kind, reason } => {
                tallies.push(Tally {
                    kind,
                    left_out: Some(reason),
                    ..Tally::default()
                });
                continue;
            }
            Line::Case(case) => case,
        };
        if tallies.last().is_none_or(|tally| tally.kind != case.kind) {
            tallies.push(Tally {
                kind: case.kind.clone(),
                ..Tally::default()
            });
        }
        let planned = machine.plan(&case);

        let given: Vec<&str> = std::iter::once(case.launch.launch_id.as_str())
            .chain(case.launch.resume.as_deref())
            .collect();
        let node = normalize(
            &Raw {
                spellings: &header.forms.node,
                env: &case.node.env,
                directory: &case.node.directory,
                outcome: &case.node.outcome,
                changes: &case.node.changes,
            },
            &given,
        );
        let rust = normalize(
            &Raw {
                spellings: &header.forms.rust,
                env: &case.rust.env,
                directory: &case.rust.directory,
                outcome: &planned.outcome,
                changes: &planned.changes,
            },
            &given,
        );
        let verdict = verdict(&case, &node, &rust);
        let said = tallies
            .last_mut()
            .unwrap()
            .count(&case, verdict, &planned, &mut differences);
        println!(
            "{:<12} {:<26} {:<8} node {:>9} rust {:>9}",
            case.kind,
            case.name,
            said,
            millis(Duration::from_secs_f64(case.node.ms / 1000.0)),
            millis(planned.took)
        );
    }
    println!("\n{}", report(&tallies));
    assert!(
        tallies.iter().any(|tally| tally.cases > 0),
        "no launch compared: none of the CLIs is installed here"
    );
    assert!(
        differences.is_empty(),
        "{} differ:\n{}",
        differences.len(),
        differences.join("\n")
    );
}
