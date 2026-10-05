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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::LazyLock;
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
use regex::{Captures, Regex};
use serde::Deserialize;
use sha2::{Digest, Sha256};

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

/// Everything under a root a plan would change, and what the CLIs' own
/// folders hold.
struct Snapshot {
    entries: BTreeMap<String, Change>,
    counts: BTreeMap<String, i64>,
}

/// What became of a case.
enum Verdict {
    Equal,
    /// The CLI could not answer, and both sides were refused in its words.
    Skipped(String),
    Differs(Vec<String>),
}

/// What became of one harness's cases.
#[derive(Default)]
struct Tally {
    kind: String,
    cases: usize,
    equal: usize,
    differ: usize,
    /// Why a case was skipped, by case; why the harness was left out.
    skipped: Vec<(String, String)>,
    left_out: Option<String>,
    node: Duration,
    rust: Duration,
    /// How many entries the CLIs made in their folders, by side.
    owned: BTreeMap<String, (i64, i64)>,
}

impl Tally {
    /// Counts a case with how it ended: what differed is added to
    /// `differences`; what each side took, and what the CLIs made, are added
    /// up. How the case is called in the line that tells of it.
    fn count(
        &mut self,
        case: &Case,
        verdict: Verdict,
        planned: &Planned,
        differences: &mut Vec<String>,
    ) -> &'static str {
        self.cases += 1;
        let said = match verdict {
            Verdict::Equal => {
                self.equal += 1;
                "equal"
            }
            Verdict::Skipped(reason) => {
                self.skipped.push((case.name.clone(), reason));
                "skipped"
            }
            Verdict::Differs(found) => {
                self.differ += 1;
                differences.push(format!(
                    "{} {}:\n{}",
                    case.kind,
                    case.name,
                    found.join("\n")
                ));
                "DIFFERS"
            }
        };
        if said != "skipped" {
            self.node += Duration::from_secs_f64(case.node.ms / 1000.0);
            self.rust += planned.took;
        }
        for (folder, made) in &planned.owned {
            self.owned.entry(folder.clone()).or_default().1 += made;
        }
        for (folder, made) in &case.node.owned {
            self.owned.entry(folder.clone()).or_default().0 += made;
        }
        said
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

/// The ways Node's adapters say a CLI could not answer a question of a plan
/// (a version or a help it was asked, the MCP servers or the instructions of
/// Codex, the throwaway server of OpenCode): it ran out of time, would not
/// start, was too old, or said what no one can read.
static UNANSWERED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(could not ask Codex whether it has its native queue",
        r"|could not list Codex's MCP servers to switch them off",
        r"|Cannot read native Codex instructions safely",
        r"|(This Codex|Codex \S+) has no native queue",
        r"|opencode serve (failed to start|exited early)",
        r"|opencode session (timed out|transport failed|unauthorized|rejected with status|returned |failed to stop)",
        r"|Devin \S+ or newer is required",
        r"|Command failed: ",
        r"|spawn )",
    ))
    .unwrap()
});

/// How a case ended, by what both sides did.
fn verdict(case: &Case, node: &Normal, rust: &Normal) -> Verdict {
    let mut found = differences(node, rust);
    // A CLI that could not answer says nothing of the plan, nor of the
    // refusal a case was made for, which only comes once it has answered.
    if let Outcome::Refused(sentence) = &node.outcome {
        if UNANSWERED.is_match(sentence) {
            return if found.is_empty() {
                Verdict::Skipped(sentence.clone())
            } else {
                Verdict::Differs(found)
            };
        }
    }
    if let Some(begins) = &case.refuses {
        for (side, normal) in [("node", node), ("rust", rust)] {
            match &normal.outcome {
                Outcome::Refused(sentence) if sentence.starts_with(begins.as_str()) => {}
                other => found.push(format!(
                    "  {side} was to be refused with \"{begins}…\" and {}",
                    describe(other)
                )),
            }
        }
    }
    if found.is_empty() {
        Verdict::Equal
    } else {
        Verdict::Differs(found)
    }
}

/// What a side planned in and found, raw.
struct Raw<'a> {
    spellings: &'a Spellings,
    env: &'a [(String, String)],
    directory: &'a str,
    outcome: &'a Outcome,
    changes: &'a [Change],
}

/// What a side found, normalized.
struct Normal {
    /// What it planned in, its environment in order and its folder: the
    /// roots are held to one shape, so that what differs is the adapters'.
    setting: Vec<String>,
    outcome: Outcome,
    changes: Vec<Change>,
    /// The hashes Pi's bundle was published under, as they are.
    pi_hashes: Vec<String>,
}

/// Names what a plan draws by its order of appearance, each kind apart: the
/// same value is the same name each time it comes, and what a plan is given
/// has none.
struct Normalizer<'a> {
    spellings: &'a Spellings,
    given: &'a [&'a str],
    seen: BTreeMap<&'static str, Vec<String>>,
    pi_hashes: Vec<String>,
}

static BUNDLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(extensions[\\/])(opencode|pi)([\\/])([0-9a-f]{64})").unwrap());
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b").unwrap()
});
static SESSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bses_[A-Za-z0-9]+").unwrap());
/// Pi's name for a conversation of its own: `cf-<project>-<handle>-<4 bytes in hex>`.
static PI_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(cf-\d+-[A-Za-z0-9_-]+-)([0-9a-f]{8})\b").unwrap());
static PORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(127\.0\.0\.1:|"port":)([0-9]+)"#).unwrap());
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_-]+").unwrap());

impl<'a> Normalizer<'a> {
    fn new(spellings: &'a Spellings, given: &'a [&'a str]) -> Self {
        Self {
            spellings,
            given,
            seen: BTreeMap::new(),
            pi_hashes: Vec::new(),
        }
    }

    /// `value`'s name among the `kind`s seen so far: `$UUID1`, `$UUID2`…
    fn name(&mut self, kind: &'static str, value: &str) -> String {
        let seen = self.seen.entry(kind).or_default();
        let at = seen
            .iter()
            .position(|held| held == value)
            .unwrap_or_else(|| {
                seen.push(value.to_owned());
                seen.len() - 1
            });
        format!("${kind}{}", at + 1)
    }

    /// `text` with the root written `$ROOT` and what is drawn named.
    fn text(&mut self, text: &str) -> String {
        let mut text = text.replace(&self.spellings.file_url, "file://$ROOT");
        for spelling in &self.spellings.plain {
            text = text.replace(spelling.as_str(), "$ROOT");
        }
        let text = BUNDLE
            .replace_all(&text, |found: &Captures| {
                let hash = found[4].to_owned();
                if &found[2] == "pi" && !self.pi_hashes.contains(&hash) {
                    self.pi_hashes.push(hash.clone());
                }
                format!(
                    "{}{}{}{}",
                    &found[1],
                    &found[2],
                    &found[3],
                    self.name("HASH", &hash)
                )
            })
            .into_owned();
        let text = UUID
            .replace_all(&text, |found: &Captures| self.drawn("UUID", &found[0]))
            .into_owned();
        let text = SESSION
            .replace_all(&text, |found: &Captures| self.drawn("SESSION", &found[0]))
            .into_owned();
        let text = PI_NAME
            .replace_all(&text, |found: &Captures| {
                let name = found[0].to_owned();
                if self.given.contains(&name.as_str()) {
                    name
                } else {
                    format!("{}{}", &found[1], self.name("NAME", &found[2]))
                }
            })
            .into_owned();
        let text = PORT
            .replace_all(&text, |found: &Captures| {
                format!("{}{}", &found[1], self.name("PORT", &found[2]))
            })
            .into_owned();
        // A token is 24 bytes drawn and written in base64url: a word of 32.
        WORD.replace_all(&text, |found: &Captures| {
            if found[0].len() == 32 {
                self.name("TOKEN", &found[0])
            } else {
                found[0].to_owned()
            }
        })
        .into_owned()
    }

    /// `value` as it stands if a launch was given it, else its name.
    fn drawn(&mut self, kind: &'static str, value: &str) -> String {
        if self.given.contains(&value) {
            value.to_owned()
        } else {
            self.name(kind, value)
        }
    }

    /// An argument list, the port that follows `--port` named as any port.
    fn argv(&mut self, argv: &[String]) -> Vec<String> {
        let mut named = Vec::new();
        for (at, argument) in argv.iter().enumerate() {
            let is_port = at > 0
                && argv[at - 1] == "--port"
                && !argument.is_empty()
                && argument.bytes().all(|byte| byte.is_ascii_digit());
            named.push(if is_port {
                self.name("PORT", argument)
            } else {
                self.text(argument)
            });
        }
        named
    }
}

/// What a side found, as one normalizer reads it: what it planned in, its
/// plan or refusal in the order its parts come in, then each change by path.
fn normalize(raw: &Raw, given: &[&str]) -> Normal {
    let mut names = Normalizer::new(raw.spellings, given);
    let mut setting: Vec<String> = raw
        .env
        .iter()
        .map(|(name, value)| format!("{name}={}", names.text(value)))
        .collect();
    setting.push(format!("directory={}", names.text(raw.directory)));
    let outcome = match raw.outcome {
        Outcome::Plan(plan) => Outcome::Plan(Plan {
            argv: names.argv(&plan.argv),
            env: plan
                .env
                .iter()
                .map(|(name, value)| (name.clone(), names.text(value)))
                .collect(),
            drop_env: plan.drop_env.clone(),
            native_session: plan.native_session.as_deref().map(|text| names.text(text)),
        }),
        Outcome::Refused(sentence) => Outcome::Refused(names.text(sentence)),
    };
    let mut sorted = raw.changes.to_vec();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    let changes = sorted
        .into_iter()
        .map(|change| Change {
            path: names.text(&change.path),
            text: change.text.as_deref().map(|text| names.text(text)),
            target: change.target.as_deref().map(|target| names.text(target)),
            ..change
        })
        .collect();
    Normal {
        setting,
        outcome,
        changes,
        pi_hashes: names.pi_hashes,
    }
}

/// What a plan came to, in a few words.
fn describe(outcome: &Outcome) -> String {
    match outcome {
        Outcome::Plan(_) => "planned".to_owned(),
        Outcome::Refused(sentence) => format!("was refused with \"{sentence}\""),
    }
}

/// How long a text may be for a difference to show it whole.
const WHOLE: usize = 200;

/// What the two sides hold where they differ, each line after `indent`: both
/// texts where both are short, none where a side holds none; else where they
/// first differ, and a stretch of each from a little before it.
fn side_by_side(node: Option<&str>, rust: Option<&str>, indent: &str) -> String {
    let long = |text: Option<&str>| text.is_some_and(|text| text.chars().count() > WHOLE);
    if !long(node) && !long(rust) {
        let said = |text: Option<&str>| {
            text.map_or_else(|| "(none)".to_owned(), |text| format!("{text:?}"))
        };
        return format!("{indent}node: {}\n{indent}rust: {}", said(node), said(rust));
    }
    let (node, rust) = (node.unwrap_or_default(), rust.unwrap_or_default());
    let at = node
        .chars()
        .zip(rust.chars())
        .take_while(|(left, right)| left == right)
        .count();
    let stretch = |text: &str| -> String {
        let stretch: String = text.chars().skip(at.saturating_sub(40)).take(120).collect();
        format!("…{stretch:?}")
    };
    format!(
        "{indent}first differ at character {at}:\n{indent}node: {}\n{indent}rust: {}",
        stretch(node),
        stretch(rust)
    )
}

/// Where two lists differ, each place said: what the node's and the rust's
/// hold there, none where a list ended. `show` is an item as text.
fn lists<T: PartialEq>(
    found: &mut Vec<String>,
    what: &str,
    node: &[T],
    rust: &[T],
    show: impl Fn(&T) -> String,
) {
    let mut places = (0..node.len().max(rust.len()))
        .filter(|&at| node.get(at) != rust.get(at))
        .map(|at| {
            let (node, rust) = (node.get(at).map(&show), rust.get(at).map(&show));
            format!(
                "  {what}[{at}]:\n{}",
                side_by_side(node.as_deref(), rust.as_deref(), "    ")
            )
        });
    found.extend(places.by_ref().take(8));
    let more = places.count();
    if more > 0 {
        found.push(format!("  {what}: {more} more places differ"));
    }
}

/// Every place the two sides' findings differ at, said.
fn differences(node: &Normal, rust: &Normal) -> Vec<String> {
    let mut found = Vec::new();
    lists(
        &mut found,
        "setting",
        &node.setting,
        &rust.setting,
        String::clone,
    );
    match (&node.outcome, &rust.outcome) {
        (Outcome::Plan(node), Outcome::Plan(rust)) => {
            lists(&mut found, "argv", &node.argv, &rust.argv, String::clone);
            lists(&mut found, "env", &node.env, &rust.env, |(name, value)| {
                format!("{name}={value}")
            });
            lists(
                &mut found,
                "dropEnv",
                &node.drop_env,
                &rust.drop_env,
                String::clone,
            );
            if node.native_session != rust.native_session {
                found.push(format!(
                    "  nativeSession:\n{}",
                    side_by_side(
                        node.native_session.as_deref(),
                        rust.native_session.as_deref(),
                        "    "
                    )
                ));
            }
        }
        (Outcome::Refused(node), Outcome::Refused(rust)) => {
            if node != rust {
                found.push(format!(
                    "  refusal:\n{}",
                    side_by_side(Some(node), Some(rust), "    ")
                ));
            }
        }
        (node, rust) => found.push(format!(
            "  node {}, where rust {}",
            describe(node),
            describe(rust)
        )),
    }
    tree(&mut found, &node.changes, &rust.changes);
    if node.pi_hashes != rust.pi_hashes {
        found.push(format!(
            "  Pi's bundle is published under another name:\n    node: {:?}\n    rust: {:?}",
            node.pi_hashes, rust.pi_hashes
        ));
    }
    found
}

/// Where the trees the two sides changed differ, path by path.
fn tree(found: &mut Vec<String>, node: &[Change], rust: &[Change]) {
    let by_path = |changes: &'_ [Change]| -> BTreeMap<String, Change> {
        changes
            .iter()
            .map(|change| (change.path.clone(), change.clone()))
            .collect()
    };
    let (node, rust) = (by_path(node), by_path(rust));
    let paths: BTreeSet<&String> = node.keys().chain(rust.keys()).collect();
    for path in paths {
        match (node.get(path), rust.get(path)) {
            (Some(node), Some(rust)) if node != rust => {
                found.push(format!("  {path}:{}", how_they_differ(node, rust)));
            }
            (Some(_), None) => found.push(format!("  {path}: only node changed it")),
            (None, Some(_)) => found.push(format!("  {path}: only rust changed it")),
            _ => {}
        }
    }
}

/// What differs of a path both sides changed: its kind, its mode, its text.
fn how_they_differ(node: &Change, rust: &Change) -> String {
    let mut how = String::new();
    if node.kind != rust.kind {
        how += &format!("\n    kind: node {}, rust {}", node.kind, rust.kind);
    }
    if node.mode != rust.mode {
        how += &format!(
            "\n    mode: node {}, rust {}",
            octal(node.mode),
            octal(rust.mode)
        );
    }
    for (what, node, rust) in [
        ("text", &node.text, &rust.text),
        ("bytes", &node.bytes, &rust.bytes),
        ("target", &node.target, &rust.target),
    ] {
        if node != rust {
            how += &format!(
                "\n    {what}:\n{}",
                side_by_side(node.as_deref(), rust.as_deref(), "      ")
            );
        }
    }
    how
}

fn octal(mode: Option<u32>) -> String {
    mode.map_or_else(|| "none".to_owned(), |mode| format!("{mode:o}"))
}

/// Everything under `root` a plan would change (`snapshot`, `tree.mjs`), by
/// its path there with `/` between the names; `owned` folders are counted,
/// not listed.
fn snapshot(root: &Path, owned: &[String]) -> Snapshot {
    fn walk(
        folder: &Path,
        relative: &str,
        owned: &[String],
        harness_owns: bool,
        found: &mut Snapshot,
    ) {
        let names = match fs::read_dir(folder) {
            Ok(names) => names,
            Err(error) if harness_owns && error.kind() == ErrorKind::NotFound => return,
            Err(error) => panic!("{}: {error}", folder.display()),
        };
        for entry in names {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) if harness_owns && error.kind() == ErrorKind::NotFound => continue,
                Err(error) => panic!("{}: {error}", folder.display()),
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let name = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let within = owned
                .iter()
                .find(|own| name.starts_with(&format!("{own}/")));
            if let Some(within) = within {
                *found.counts.get_mut(within).unwrap() += 1;
            } else if !owned.contains(&name) {
                found
                    .entries
                    .insert(name.clone(), describe_file(&entry.path(), &name));
            }
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                let owns = within.is_some() || owned.contains(&name);
                walk(&entry.path(), &name, owned, owns, found);
            }
        }
    }
    let mut found = Snapshot {
        entries: BTreeMap::new(),
        counts: owned.iter().map(|folder| (folder.clone(), 0)).collect(),
    };
    walk(root, "", owned, false, &mut found);
    found
}

/// A file, folder or link as the comparison holds it (`describe`, `tree.mjs`).
fn describe_file(file: &Path, name: &str) -> Change {
    let found = fs::symlink_metadata(file).unwrap();
    if found.is_symlink() {
        let mut link = Change::new(name, "link");
        link.target = Some(fs::read_link(file).unwrap().to_string_lossy().into_owned());
        return link;
    }
    let mode = mode_of(&found);
    if found.is_dir() {
        let mut folder = Change::new(name, "dir");
        folder.mode = mode;
        return folder;
    }
    let mut made = Change::new(name, "file");
    made.mode = mode;
    match String::from_utf8(fs::read(file).unwrap()) {
        Ok(text) => made.text = Some(text),
        Err(invalid) => made.bytes = Some(hex(&Sha256::digest(invalid.as_bytes()))),
    }
    made
}

#[cfg(unix)]
fn mode_of(found: &fs::Metadata) -> Option<u32> {
    Some(std::os::unix::fs::PermissionsExt::mode(&found.permissions()) & 0o777)
}

#[cfg(not(unix))]
fn mode_of(_found: &fs::Metadata) -> Option<u32> {
    None
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// What a plan did to the tree: each path made or changed, as it is now, and
/// each one removed (`changes`, `tree.mjs`).
fn changes(before: &Snapshot, after: &Snapshot) -> Vec<Change> {
    let mut made: Vec<Change> = after
        .entries
        .iter()
        .filter(|(path, entry)| before.entries.get(*path) != Some(entry))
        .map(|(_, entry)| entry.clone())
        .collect();
    made.extend(
        before
            .entries
            .keys()
            .filter(|path| !after.entries.contains_key(*path))
            .map(|path| Change::new(path, "removed")),
    );
    made.sort_by(|left, right| left.path.cmp(&right.path));
    made
}

/// How many entries each owned folder gained, those that gained any.
fn gained(before: &Snapshot, after: &Snapshot) -> BTreeMap<String, i64> {
    after
        .counts
        .iter()
        .map(|(folder, count)| (folder.clone(), count - before.counts[folder]))
        .filter(|(_, made)| *made != 0)
        .collect()
}

fn millis(time: Duration) -> String {
    format!("{:.1} ms", time.as_secs_f64() * 1000.0)
}

/// What became of each harness's cases, and how long each side took.
fn report(tallies: &[Tally]) -> String {
    let mut lines = vec![format!(
        "{:<12} {:>5} {:>5} {:>6}   {:<44} {:>13} {:>13}",
        "harness", "cases", "equal", "differ", "skipped (why)", "node's time", "rust's time"
    )];
    for tally in tallies {
        let skipped = match (&tally.left_out, tally.skipped.first()) {
            (Some(reason), _) => format!("left out ({reason})"),
            (None, Some((_, reason))) => {
                format!("{} ({})", tally.skipped.len(), clipped(reason, 36))
            }
            (None, None) => "-".to_owned(),
        };
        lines.push(format!(
            "{:<12} {:>5} {:>5} {:>6}   {:<44} {:>13} {:>13}",
            tally.kind,
            tally.cases,
            tally.equal,
            tally.differ,
            clipped(&skipped, 44),
            millis(tally.node),
            millis(tally.rust)
        ));
    }
    for tally in tallies {
        for (case, reason) in &tally.skipped {
            lines.push(format!("skipped: {} {case}: {reason}", tally.kind));
        }
        for (folder, (node, rust)) in &tally.owned {
            lines.push(format!(
                "written by the CLIs, not compared: {} {folder}: {node} entries (node), {rust} (rust)",
                tally.kind
            ));
        }
    }
    lines.push(
        "Node plans first: by the time Rust plans, the files of the CLIs are in the system's cache."
            .to_owned(),
    );
    lines.join("\n")
}

/// `text` as long as `width` at most.
fn clipped(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_owned();
    }
    let kept: String = text.chars().take(width - 1).collect();
    format!("{kept}…")
}

const LAUNCH: &str = "11111111-1111-4111-8111-111111111111";

/// The root `/tmp/consensflow launch %#-X/<side>` as Node's `rootForms` spells it.
fn spellings_of(side: &str) -> Spellings {
    Spellings {
        file_url: format!("file:///tmp/consensflow%20launch%20%25%23-X/{side}"),
        plain: vec![
            format!("/tmp/consensflow launch %#-X/{side}"),
            format!("%2Ftmp%2Fconsensflow%20launch%20%25%23-X%2F{side}"),
            format!("%2Ftmp%2Fconsensflow+launch+%25%23-X%2F{side}"),
        ],
    }
}

fn spellings() -> Spellings {
    spellings_of("rust")
}

#[test]
fn a_root_is_written_in_every_spelling_it_is_read_in() {
    let spellings = spellings();
    let mut names = Normalizer::new(&spellings, &[]);
    for (text, written) in [
        (
            "/tmp/consensflow launch %#-X/rust/consensflow/a",
            "$ROOT/consensflow/a",
        ),
        (
            "file:///tmp/consensflow%20launch%20%25%23-X/rust/consensflow/a",
            "file://$ROOT/consensflow/a",
        ),
        (
            "?directory=%2Ftmp%2Fconsensflow%20launch%20%25%23-X%2Frust%2Fwork",
            "?directory=$ROOT%2Fwork",
        ),
        (
            "?directory=%2Ftmp%2Fconsensflow+launch+%25%23-X%2Frust",
            "?directory=$ROOT",
        ),
    ] {
        assert_eq!(names.text(text), written);
    }
    // Another root is no part of this one's.
    let other = "/tmp/consensflow launch %#-X/node/consensflow/a";
    assert_eq!(names.text(other), other);
}

#[test]
fn what_a_plan_draws_is_named_by_its_order_of_appearance_and_what_it_is_given_is_not() {
    let given = [LAUNCH, "ses_given0001"];
    let spellings = spellings();
    let side = |first: &str, second: &str, session: &str| {
        let mut names = Normalizer::new(&spellings, &given);
        names.text(&format!(
            "{first} {second} {first} {LAUNCH} ses_given0001 {session} {session}"
        ))
    };
    let named = "$UUID1 $UUID2 $UUID1 11111111-1111-4111-8111-111111111111 ses_given0001 $SESSION1 $SESSION1";
    // Two sides that drew other values, in the same places, are the same.
    assert_eq!(
        side(
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "ses_ef4449f98ffeVbNNlDLO94GcTN"
        ),
        named
    );
    assert_eq!(
        side(
            "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
            "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
            "ses_0123456789abAbCdEfGhIjKlMn"
        ),
        named
    );
    // A draw that is one value on a side and two on the other is not.
    let same = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let other = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    assert_ne!(
        Normalizer::new(&spellings, &given).text(&format!("{same} {same}")),
        Normalizer::new(&spellings, &given).text(&format!("{same} {other}"))
    );
}

#[test]
fn a_port_a_token_a_hash_and_a_name_of_pi_s_are_each_named_in_their_own_kind() {
    let spellings = spellings();
    let given = ["cf-7-rhea-0a1b2c3d"];
    let mut names = Normalizer::new(&spellings, &given);
    let hash = "257abc9f5e18ad4b748fcadfcbf6899c11427087aa5eeaf76c72bed9462ced0f";
    let text = format!(
        concat!(
            r#"{{"launchId":"x","port":41234,"token":"abcdefghijklmnopqrstuvwxyzABCDEF"}} "#,
            "http://127.0.0.1:41235/s cf-7-rhea-9f8e7d6c cf-7-rhea-0a1b2c3d /x/extensions/pi/{}/hosts ",
            "/x/extensions/opencode/{}/hosts"
        ),
        hash,
        "0123456789abcdef".repeat(4)
    );
    assert_eq!(
        names.text(&text),
        concat!(
            r#"{"launchId":"x","port":$PORT1,"token":"$TOKEN1"} "#,
            "http://127.0.0.1:$PORT2/s cf-7-rhea-$NAME1 cf-7-rhea-0a1b2c3d /x/extensions/pi/$HASH1/hosts ",
            "/x/extensions/opencode/$HASH2/hosts"
        )
    );
    assert_eq!(names.pi_hashes, [hash], "Pi's hashes are kept as they are");
    names.text(&format!("/y/extensions/pi/{hash}/hosts"));
    assert_eq!(names.pi_hashes, [hash], "and each is kept once");
}

#[test]
fn a_token_is_a_word_of_thirty_two_and_no_part_of_a_longer_or_shorter_one() {
    let spellings = spellings();
    let mut names = Normalizer::new(&spellings, &[]);
    for kept in [
        "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6x",
        "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d",
        "consensflow-delivery.mjs",
    ] {
        assert_eq!(names.text(kept), kept);
    }
    assert_eq!(
        names.text("x a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6 y-_z"),
        "x $TOKEN1 y-_z"
    );
    assert_eq!(
        names.text("Tn6how5MeNkKJwojV781E5ANtOo6bavs"),
        "$TOKEN2",
        "base64url's own characters are in a token"
    );
}

#[test]
fn the_port_that_follows_port_is_named_as_any_port_and_no_other_number() {
    let spellings = spellings();
    let mut names = Normalizer::new(&spellings, &[]);
    let argv: Vec<String> = ["--port", "41000", "--hostname", "127.0.0.1", "--n", "41000"]
        .map(str::to_owned)
        .to_vec();
    assert_eq!(
        names.argv(&argv),
        [
            "--port",
            "$PORT1",
            "--hostname",
            "127.0.0.1",
            "--n",
            "41000"
        ]
    );
}

/// A file with its folders made.
fn write(file: &Path, text: impl AsRef<[u8]>) {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

#[test]
fn a_plan_s_changes_are_what_it_made_changed_and_removed_but_for_the_folders_a_cli_owns() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let owned = vec!["cli".to_owned()];
    write(&root.join("a/kept.txt"), "kept");
    write(&root.join("a/changed.txt"), "was");
    write(&root.join("gone.txt"), "x");
    write(&root.join("cli/state/db"), "x");
    let before = snapshot(root, &owned);
    write(&root.join("a/changed.txt"), "is");
    write(&root.join("a/made.txt"), "new");
    fs::remove_file(root.join("gone.txt")).unwrap();
    write(&root.join("cli/state/more"), "y");
    write(&root.join("cli/other"), "z");
    let after = snapshot(root, &owned);
    let made: Vec<(String, String, Option<String>)> = changes(&before, &after)
        .into_iter()
        .map(|change| (change.path, change.kind, change.text))
        .collect();
    assert_eq!(
        made,
        [
            ("a/changed.txt", "file", Some("is")),
            ("a/made.txt", "file", Some("new")),
            ("gone.txt", "removed", None),
        ]
        .map(|(path, kind, text)| (
            path.to_owned(),
            kind.to_owned(),
            text.map(str::to_owned)
        ))
    );
    assert_eq!(
        gained(&before, &after),
        BTreeMap::from([("cli".to_owned(), 2)])
    );
    assert!(
        !after.entries.contains_key("cli"),
        "an owned folder is not listed"
    );
}

#[test]
fn a_file_is_its_text_whole_or_the_hash_of_its_bytes_and_a_link_is_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("bom"), "\u{feff}text");
    write(&root.join("binary"), [0xff, 0xfe, 0x00]);
    #[cfg(unix)]
    std::os::unix::fs::symlink("elsewhere", root.join("link")).unwrap();
    let found = snapshot(root, &[]).entries;
    assert_eq!(found["bom"].text.as_deref(), Some("\u{feff}text"));
    assert_eq!(found["binary"].text, None);
    assert_eq!(
        found["binary"].bytes.as_deref(),
        Some(hex(&Sha256::digest([0xff, 0xfe, 0x00])).as_str())
    );
    #[cfg(unix)]
    {
        assert_eq!(found["link"].kind, "link");
        assert_eq!(found["link"].target.as_deref(), Some("elsewhere"));
    }
}

#[cfg(unix)]
#[test]
fn a_mode_is_what_the_system_gave_it_and_it_is_told() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(&root.join("sealed/file"), "x");
    fs::set_permissions(root.join("sealed/file"), fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(root.join("sealed"), fs::Permissions::from_mode(0o700)).unwrap();
    let found = snapshot(root, &[]).entries;
    assert_eq!(found["sealed"].mode, Some(0o700));
    assert_eq!(found["sealed/file"].mode, Some(0o600));
}

#[test]
fn a_refusal_is_the_clis_for_a_cli_that_could_not_answer_and_never_for_a_launch_s_own() {
    for unanswered in [
        "could not ask Codex whether it has its native queue: it did not answer in time",
        "could not list Codex's MCP servers to switch them off: Command failed: codex mcp list --json",
        "Cannot read native Codex instructions safely",
        "Codex 0.150.0 has no native queue, which ConsensFlow needs to reach its window: update Codex.",
        "This Codex has no native queue, which ConsensFlow needs to reach its window: update Codex.",
        "opencode serve failed to start",
        "opencode serve exited early",
        "opencode session timed out",
        "opencode session rejected with status 500",
        "opencode session returned the wrong directory",
        "Devin 3000.10.21 or newer is required for complete worker replies. Update Devin before opening this pane.",
        "Command failed: /bin/devin --version\n",
        "spawn /bin/devin ENOENT",
    ] {
        assert!(UNANSWERED.is_match(unanswered), "{unanswered}");
    }
    for ours in [
        "the chief window needs its role text",
        "opencode session needs a working directory",
        "ConsensFlow's Pi extension could not be installed: EACCES",
        "Cannot read native Devin configuration; the original was preserved",
        "OpenCode has a custom OPENCODE_TUI_CONFIG; its settings were preserved.",
    ] {
        assert!(!UNANSWERED.is_match(ours), "{ours}");
    }
}

fn case(refuses: Option<&str>) -> Case {
    serde_json::from_value(serde_json::json!({
        "kind": "codex",
        "name": "member-fresh",
        "refuses": refuses,
        "launch": {
            "launchId": LAUNCH, "project": 7, "handle": "rhea", "role": "worker",
            "resume": null, "message": null, "agent": null, "instructions": "x",
        },
        "node": {
            "env": [], "directory": "/work",
            "outcome": { "refused": "unused" }, "changes": [], "owned": {}, "ms": 1.0,
        },
        "rust": { "env": [], "directory": "/work" },
    }))
    .unwrap()
}

fn planned(argv: &[&str]) -> Normal {
    Normal {
        setting: vec!["HOME=$ROOT/home".to_owned()],
        outcome: Outcome::Plan(Plan {
            argv: argv.iter().map(|&argument| argument.to_owned()).collect(),
            env: vec![("A".to_owned(), "1".to_owned())],
            drop_env: Vec::new(),
            native_session: None,
        }),
        changes: Vec::new(),
        pi_hashes: Vec::new(),
    }
}

fn refused(sentence: &str) -> Normal {
    Normal {
        outcome: Outcome::Refused(sentence.to_owned()),
        ..planned(&[])
    }
}

fn differs(verdict: Verdict) -> String {
    match verdict {
        Verdict::Differs(found) => found.join("\n"),
        Verdict::Equal => panic!("equal"),
        Verdict::Skipped(reason) => panic!("skipped: {reason}"),
    }
}

#[test]
fn two_roots_of_one_shape_are_one_setting_and_a_root_that_is_not_is_said() {
    let [node, rust] = ["node", "rust"].map(|side| {
        let root = format!("/tmp/consensflow launch %#-X/{side}");
        let env = vec![
            ("HOME".to_owned(), format!("{root}/home")),
            ("TMPDIR".to_owned(), format!("{root}/tmp")),
        ];
        normalize(
            &Raw {
                spellings: &spellings_of(side),
                env: &env,
                directory: &format!("{root}/work"),
                outcome: &Outcome::Refused(String::new()),
                changes: &[],
            },
            &[],
        )
    });
    assert_eq!(
        node.setting,
        [
            "HOME=$ROOT/home",
            "TMPDIR=$ROOT/tmp",
            "directory=$ROOT/work"
        ]
    );
    assert_eq!(node.setting, rust.setting);
    let odd = Normal {
        setting: vec!["HOME=$ROOT/elsewhere".to_owned()],
        ..planned(&["a"])
    };
    let said = differs(verdict(&case(None), &planned(&["a"]), &odd));
    assert!(said.contains("setting[0]"), "{said}");
}

#[test]
fn two_plans_are_equal_and_a_place_they_differ_at_is_said() {
    let plain = case(None);
    assert!(matches!(
        verdict(&plain, &planned(&["a", "b"]), &planned(&["a", "b"])),
        Verdict::Equal
    ));
    let said = differs(verdict(
        &plain,
        &planned(&["a", "b", "c"]),
        &planned(&["a", "x"]),
    ));
    assert!(
        said.contains("argv[1]:\n    node: \"b\"\n    rust: \"x\""),
        "{said}"
    );
    assert!(
        said.contains("argv[2]:\n    node: \"c\"\n    rust: (none)"),
        "{said}"
    );
    let mut moved = planned(&["a"]);
    if let Outcome::Plan(plan) = &mut moved.outcome {
        plan.env = vec![
            ("B".to_owned(), "2".to_owned()),
            ("A".to_owned(), "1".to_owned()),
        ];
    }
    let said = differs(verdict(&plain, &planned(&["a"]), &moved));
    assert!(
        said.contains("env[0]"),
        "an environment's order counts: {said}"
    );
}

#[test]
fn a_tree_is_held_to_its_paths_its_modes_and_its_texts() {
    let file = |path: &str, mode: u32, text: &str| {
        let mut change = Change::new(path, "file");
        change.mode = Some(mode);
        change.text = Some(text.to_owned());
        change
    };
    let with = |changes: Vec<Change>| Normal {
        changes,
        ..planned(&["a"])
    };
    let node = with(vec![
        file("a", 0o600, "x"),
        file("b", 0o600, "y"),
        file("c", 0o600, "z"),
    ]);
    let rust = with(vec![
        file("a", 0o644, "x"),
        file("b", 0o600, "Y"),
        file("d", 0o600, "z"),
    ]);
    let said = differs(verdict(&case(None), &node, &rust));
    assert!(said.contains("a:\n    mode: node 600, rust 644"), "{said}");
    assert!(
        said.contains("b:\n    text:\n      node: \"y\"\n      rust: \"Y\""),
        "{said}"
    );
    assert!(said.contains("c: only node changed it"), "{said}");
    assert!(said.contains("d: only rust changed it"), "{said}");
}

#[test]
fn pi_s_bundle_is_held_to_the_name_it_is_published_under() {
    let mut node = planned(&["a"]);
    let mut rust = planned(&["a"]);
    node.pi_hashes = vec!["a".repeat(64)];
    rust.pi_hashes = vec!["b".repeat(64)];
    let said = differs(verdict(&case(None), &node, &rust));
    assert!(
        said.contains("Pi's bundle is published under another name"),
        "{said}"
    );
}

#[test]
fn a_cli_that_could_not_answer_is_skipped_where_rust_could_not_either_and_a_difference_where_it_could(
) {
    let sentence = "opencode session timed out";
    assert!(matches!(
        verdict(&case(None), &refused(sentence), &refused(sentence)),
        Verdict::Skipped(reason) if reason == sentence
    ));
    let said = differs(verdict(&case(None), &refused(sentence), &planned(&["a"])));
    assert!(said.contains("node was refused"), "{said}");
    let said = differs(verdict(
        &case(None),
        &refused(sentence),
        &refused("opencode serve exited early"),
    ));
    assert!(said.contains("refusal:"), "{said}");
    // A refusal of ConsensFlow's own, in the same words, is an answer: equal.
    let ours = "the chief window needs its role text";
    assert!(matches!(
        verdict(&case(None), &refused(ours), &refused(ours)),
        Verdict::Equal
    ));
}

#[test]
fn a_case_that_is_to_be_refused_is_refused_by_both_sides_in_its_words() {
    let sentence = "Private pi integration differs from this build";
    let wanted = case(Some(sentence));
    assert!(matches!(
        verdict(&wanted, &refused(sentence), &refused(sentence)),
        Verdict::Equal
    ));
    let said = differs(verdict(&wanted, &planned(&["a"]), &planned(&["a"])));
    assert!(
        said.contains("node was to be refused") && said.contains("rust was to be refused"),
        "{said}"
    );
    let said = differs(verdict(
        &wanted,
        &refused(sentence),
        &refused("another sentence"),
    ));
    assert!(said.contains("rust was to be refused"), "{said}");
    // A CLI that could not answer is no refusal the case wanted, nor a failure of it.
    let timed_out = "opencode session timed out";
    assert!(matches!(
        verdict(&wanted, &refused(timed_out), &refused(timed_out)),
        Verdict::Skipped(reason) if reason == timed_out
    ));
    let said = differs(verdict(&wanted, &refused(timed_out), &refused(sentence)));
    assert!(said.contains("refusal:"), "{said}");
}

#[test]
fn many_places_that_differ_are_said_eight_and_counted() {
    let node: Vec<String> = (0..12).map(|at| format!("n{at}")).collect();
    let rust: Vec<String> = (0..12).map(|at| format!("r{at}")).collect();
    let mut found = Vec::new();
    lists(&mut found, "argv", &node, &rust, String::clone);
    assert_eq!(found.len(), 9);
    assert!(found[8].contains("4 more places differ"), "{found:?}");
}

#[test]
fn a_long_text_is_shown_where_it_first_differs_and_a_short_one_whole() {
    let node = format!("{}A{}", "x".repeat(500), "y".repeat(500));
    let rust = format!("{}B{}", "x".repeat(500), "y".repeat(500));
    let said = side_by_side(Some(&node), Some(&rust), "  ");
    assert!(
        said.starts_with("  first differ at character 500:\n"),
        "{said}"
    );
    assert!(
        said.contains(&format!(
            "  node: …\"{}A{}\"",
            "x".repeat(40),
            "y".repeat(79)
        )),
        "{said}"
    );
    assert!(
        said.contains(&format!("  rust: …\"{}B", "x".repeat(40))),
        "{said}"
    );
    assert_eq!(
        side_by_side(Some("a"), None, ""),
        "node: \"a\"\nrust: (none)"
    );
}

#[test]
fn the_report_has_a_row_for_each_harness_and_says_what_was_skipped_and_left_out() {
    let tallies = [
        Tally {
            kind: "codex".to_owned(),
            cases: 3,
            equal: 2,
            skipped: vec![("member-fresh".to_owned(), "could not list".to_owned())],
            node: Duration::from_millis(30),
            rust: Duration::from_millis(20),
            owned: BTreeMap::from([("codex".to_owned(), (4, 5))]),
            ..Tally::default()
        },
        Tally {
            kind: "devin".to_owned(),
            left_out: Some("devin is not installed on this machine".to_owned()),
            ..Tally::default()
        },
    ];
    let table = report(&tallies);
    assert!(table.contains("1 (could not list)"), "{table}");
    assert!(
        table.contains("left out (devin is not installed on this ma…"),
        "{table}"
    );
    assert!(
        table.contains("skipped: codex member-fresh: could not list"),
        "{table}"
    );
    assert!(
        table.contains("codex codex: 4 entries (node), 5 (rust)"),
        "{table}"
    );
    assert!(
        table.contains("30.0 ms") && table.contains("20.0 ms"),
        "{table}"
    );
}
