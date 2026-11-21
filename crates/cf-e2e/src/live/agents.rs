//! The agents of a live test: one for each harness, on the model the run is asked
//! for, and the one place that says which model that is ([`model`]). A live test
//! spends real quota, so a harness runs on the cheapest model its human's rules
//! allow unless the run names another: a machine whose harness has other logins
//! than the cheap one needs (zeewin's Pi has a ChatGPT login and no OpenRouter
//! key) names the one it has, with `--models` ([`super::MODELS`]):
//!
//! ```text
//! cargo xtask live <suite> --models pi=openai/gpt-6-luna,codex=gpt-6.1-sol:high,devin=swe-2:medium
//! ```
//!
//! Each piece is a harness, `=`, its model, and where it has one `:` and an
//! effort the harness takes ([`parse`]); a harness the flag does not name keeps
//! its cheap one. `--claude-model M` ([`super::CLAUDE_MODEL`], which runs had before
//! `--models`) says the same as `--models claude=M`, and `--models` wins where
//! both name Claude. Every suite, the bench among them, reads the model here, and
//! begins with [`super::Log::begin`], which refuses a flag that is wrong before
//! anything is spent and says what each harness runs on.
//!
//! An agent is a row of the roster of the run's home ([`row`], [`write_roster`])
//! where the daemon opens a member's window, and the `agent` a window is opened
//! on without a daemon ([`agent`], which the test binary `harness-ask` is asked
//! the window's command line by).

use std::collections::BTreeMap;
use std::fmt;

use serde_json::{json, Value};

use super::machine::Harness;
use super::{requested, Log, CLAUDE_MODEL, MODELS};
use crate::rig::Rig;
use crate::{Error, Result};

/// The image designer: a Codex member whose image tool draws.
pub const DESIGNER: &str = "pygmalion";

/// The model of the agent the stand-in plays for a chief, and for a member: the
/// shim that starts Claude Code hands a window with either of them to the
/// stand-in, and every other Claude window to the real one.
pub const STAND_IN_CHIEF_MODEL: &str = "fake-chief";
pub const STAND_IN_WORKER_MODEL: &str = "fake";

/// The member a fresh rig seats on the stand-in (`rig::install`'s stand-ins): a member is
/// asked for by its model, and one on a stand-in model by its own name, which is its
/// handle (`rig::member_body`), so a shim hands a window asked for on this model to the
/// stand-in as well (spec harness-models).
pub const STAND_IN_MEMBER_MODEL: &str = "worker";

/// The harness an agent (a roster row, or what a window is asked for) runs on,
/// when it is a real one: the stand-in agent's (a Claude Code agent whose model is
/// a fake one) are none.
pub fn harness_of(agent: &Value) -> Option<Harness> {
    let kind = agent["kind"].as_str()?;
    // A fake model is the rig's stand-in, of whichever kind it is shaped as (T-1207).
    if [STAND_IN_CHIEF_MODEL, STAND_IN_WORKER_MODEL].contains(&agent["model"].as_str()?) {
        return None;
    }
    Harness::ALL
        .into_iter()
        .find(|harness| harness.kind() == kind)
}

/// What a window launched on `agent` runs on: its harness, the model and the
/// effort it is given (Pi's is read under `thinking`, as Pi reads it); none for
/// the stand-in agent's, and for a kind that is no harness.
pub fn launched_on(agent: &Value) -> Option<Model> {
    let harness = harness_of(agent)?;
    let text = |key: &str| agent[key].as_str().filter(|text| !text.is_empty());
    let effort = text(effort_key(harness));
    Some(Model {
        harness,
        name: text("model").unwrap_or("its own default").to_owned(),
        effort: effort.map(str::to_owned),
    })
}

/// The field an agent holds its effort in, where its harness reads it: Pi's is its thinking.
fn effort_key(harness: Harness) -> &'static str {
    if harness == Harness::Pi {
        "thinking"
    } else {
        "effort"
    }
}

/// The id of the agent that runs `harness`.
pub fn id(harness: Harness) -> String {
    format!("live-{}", harness.name())
}

/// What the human's rules call the cheap model of each harness, as the app's
/// catalog and the harness's own list of models both have it (checked on
/// 2026-10-11): Claude Code on Sonnet 5.5, Pi on Muse Spark 1.3 through
/// OpenRouter, which is where the catalog has it, Devin on SWE-2 at medium, Codex
/// on GPT-5.6 Luna. The model, and the effort where it has one: what a harness
/// runs on where the run names no other ([`model`]). An effort given here is only the one that
/// is not the harness's to say; a model whose harness lists efforts and a row that names none
/// is given the least one the harness lists when the roster is written ([`with_efforts`]),
/// as the daemon refuses a member or a chief that names none (A4-28, T-1369).
fn cheap(harness: Harness) -> (&'static str, Option<&'static str>) {
    match harness {
        Harness::Claude => ("claude-sonnet-5-5", None),
        Harness::Pi => ("openrouter/meta/muse-spark-1.3", None),
        Harness::Devin => ("swe-2", Some("medium")),
        Harness::Codex => ("gpt-5.6-luna", None),
        // A preferred inexpensive non-contributor model. The roster picks it (or the older
        // Spark) only from Muse's fresh account listing before it opens a window.
        Harness::Muse => ("muse-spark-1.3", Some("minimal")),
    }
}

/// The effort levels `harness` takes, as the product's catalog has them
/// (as each CLI's own
/// help; a test holds the two to one list).
fn efforts(harness: Harness) -> &'static [&'static str] {
    match harness {
        Harness::Claude => &["low", "medium", "high", "xhigh", "max"],
        Harness::Codex => &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"],
        Harness::Pi => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        Harness::Devin => &["low", "medium", "high", "xhigh", "max"],
        Harness::Muse => &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"],
    }
}

/// The model a harness runs on in a live run, and its effort where it has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub harness: Harness,
    pub name: String,
    pub effort: Option<String>,
}

impl Model {
    /// The cheap model of `harness` ([`cheap`]).
    fn cheap(harness: Harness) -> Self {
        let (name, effort) = cheap(harness);
        Self {
            harness,
            name: name.to_owned(),
            effort: effort.map(str::to_owned),
        }
    }
}

impl fmt::Display for Model {
    /// `codex gpt-6.1-sol, effort high`; `pi openai/gpt-6-luna`.
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(out, "{} {}", self.harness.name(), self.name)?;
        match &self.effort {
            Some(effort) => write!(out, ", effort {effort}"),
            None => Ok(()),
        }
    }
}

/// The model of every harness in a run, in the order of [`Harness::ALL`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Models {
    models: [Model; 5],
    /// The harnesses the flags named, in the order they did.
    named: Vec<Harness>,
    /// What the run says besides, of a model it chose for a harness the flags did not name
    /// ([`Models::on_machine`]).
    notes: Vec<String>,
}

impl Models {
    /// The model `harness` runs on.
    pub fn of(&self, harness: Harness) -> &Model {
        &self.models[slot(harness)]
    }

    /// The harnesses `--models` and `--claude-model` named, in the order they were.
    pub fn named(&self) -> &[Harness] {
        &self.named
    }

    /// What a run says of them in its first lines: what the flags give each
    /// harness, which a suite's own flag may override for its windows (the report
    /// says what each window was launched on: [`super::Log::launched`]).
    pub fn line(&self) -> String {
        let each: Vec<String> = self.models.iter().map(Model::to_string).collect();
        let mut line = format!(
            "the models --models and the cheap ones give: {}",
            each.join("; ")
        );
        for note in &self.notes {
            line.push_str(&format!(". {note}"));
        }
        line
    }

    /// These models on the machine `machine` (none on the owner's Mac, which the runner
    /// signals no test machine for), whose Pi has the logins `pi_providers` (their names;
    /// none where the file could not be read): a Pi the flags did not name, on a machine
    /// that has no login for the cheap model's provider (OpenRouter) and has one the kit
    /// knows a model for ([`PI_ON`]), runs on that model and its effort, and the run says
    /// which provider and model, and why. Nothing else is chosen: not for a login the kit
    /// has no model for, not for a machine it has no row for, not where Pi's logins are not
    /// known. Those runs keep the cheap model, which the kit's isolation then refuses
    /// before anything is spent, naming the logins Pi has.
    #[must_use]
    pub fn on_machine(mut self, machine: Option<&str>, pi_providers: Option<&[String]>) -> Self {
        let Some(providers) = pi_providers else {
            return self;
        };
        let cheap_provider = cheap(Harness::Pi).0.split('/').next().unwrap_or_default();
        if self.named.contains(&Harness::Pi) || providers.iter().any(|name| name == cheap_provider)
        {
            return self;
        }
        let Some(row) = PI_ON.iter().find(|row| {
            row.machine == machine && providers.iter().any(|name| name == row.provider)
        }) else {
            return self;
        };
        let pi = &mut self.models[slot(Harness::Pi)];
        pi.name = row.model.to_owned();
        pi.effort = row.effort.map(str::to_owned);
        let effort = row
            .effort
            .map(|effort| format!(", effort {effort}"))
            .unwrap_or_default();
        self.notes.push(format!(
            "pi runs on {}{effort} because {}'s Pi has a `{}` login ({}) and none for {cheap_provider}, the cheap model's provider; --models pi=… names another",
            row.model,
            machine.unwrap_or("the owner's Mac"),
            row.provider,
            row.cheap,
        ));
        self
    }
}

/// The model of Pi that a machine gets when its Pi has no login for the cheap model's
/// provider: the machine (`None` is the owner's Mac, which the runner signals no test
/// machine for), the provider of the login it has, the model the kit names for it, its
/// effort, and why it is cheap — a subscription's allowance or the person's own proxy,
/// never a key that is charged by use.
struct PiOn {
    /// The test machine's name; none for the owner's Mac.
    machine: Option<&'static str>,
    /// The provider the machine's Pi has a login for.
    provider: &'static str,
    /// The model the kit names for it.
    model: &'static str,
    /// The effort it runs at, where Pi takes one.
    effort: Option<&'static str>,
    /// Why the model is cheap, said in the run's log.
    cheap: &'static str,
}

/// What a machine's Pi runs on when the flags did not name it and the machine has no login
/// for the cheap model's provider: zeewin2's and zeewin's Pi 1.4.0 hold a ChatGPT login
/// under `openai` and no OpenRouter entry, and `openai/gpt-5.6-luna` is the cheap Codex
/// model on that login and ran Pi 6 of 6 there (T-1136). The owner's Mac's Pi 1.5.0 holds
/// one `cliproxyapi` login, the local CLIProxyAPI proxy's, and no other (T-1172; the owner,
/// 2026-11-03: "Pi on this Mac can be used with the models pi has now"); its model is the proxy's
/// `devin/swe-2`, a free pool, at `medium`, the lowest of the efforts Pi's own
/// list in the kit's folder gives it (`medium`, `high`, `max`; T-1755). The contributor model the
/// kit ran before is no longer served there. A machine
/// this has no row for keeps the cheap model, and the kit's isolation refuses the run
/// before anything is spent, naming the logins Pi has.
const PI_ON: [PiOn; 3] = [
    PiOn {
        machine: Some("zeewin2"),
        provider: "openai",
        model: "openai/gpt-5.6-luna",
        effort: None,
        cheap: "a ChatGPT subscription's allowance, no usage credits",
    },
    PiOn {
        machine: Some("zeewin"),
        provider: "openai",
        model: "openai/gpt-5.6-luna",
        effort: None,
        cheap: "a ChatGPT subscription's allowance, no usage credits",
    },
    PiOn {
        machine: None,
        provider: "cliproxyapi",
        model: "cliproxyapi/devin/swe-2",
        effort: Some("medium"),
        cheap: "the person's own CLIProxyAPI proxy's swe-2, a free pool, at the lowest effort Pi lists for it",
    },
];

/// What `--models` and `--claude-model` say (the variables' values, none where
/// they are not set): the model of each harness, its cheap one where neither
/// names another; or why no window could be asked for what they say.
///
/// `--models` is pieces between commas (a flag given twice has its values joined
/// so), each `harness=model` or `harness=model:effort`, with spaces around any of
/// them let be; a piece with nothing in it is none. An effort is the last `:` and
/// what follows it, where that is an effort some harness takes, and it must be
/// one the piece's harness takes: what follows a `:` that names no effort is the
/// model's (OpenRouter's free models end in `:free`). Refused: a piece with no
/// `=`, a harness that is none of the five, one named twice, and a model that is
/// empty. `--claude-model` is the model of a piece `claude=…`, read the same way;
/// where `--models` names Claude as well, `--models` wins.
pub fn parse(
    models: Option<&str>,
    claude_model: Option<&str>,
) -> std::result::Result<Models, String> {
    let mut chosen = Harness::ALL.map(Model::cheap);
    let mut named: Vec<Harness> = Vec::new();
    for piece in models.unwrap_or_default().split(',').map(str::trim) {
        if piece.is_empty() {
            continue;
        }
        let split = piece
            .split_once('=')
            .map(|(harness, model)| (harness.trim(), model));
        let Some((harness, model)) = split.filter(|(harness, _)| !harness.is_empty()) else {
            return Err(format!(
                "--models takes harness=model or harness=model:effort, not {piece}"
            ));
        };
        let Some(harness) = Harness::from_name(harness) else {
            let names: Vec<&str> = Harness::ALL.into_iter().map(Harness::name).collect();
            return Err(format!(
                "--models names no harness called {harness} ({}): {piece}",
                names.join(", ")
            ));
        };
        if named.contains(&harness) {
            return Err(format!(
                "--models names {} twice: say one model for it",
                harness.name()
            ));
        }
        named.push(harness);
        chosen[slot(harness)] = model_of(harness, model, "--models")?;
    }
    if let Some(model) = claude_model.filter(|_| !named.contains(&Harness::Claude)) {
        chosen[slot(Harness::Claude)] = model_of(Harness::Claude, model, "--claude-model")?;
        named.push(Harness::Claude);
    }
    Ok(Models {
        models: chosen,
        named,
        notes: Vec::new(),
    })
}

/// Where `harness` is in [`Harness::ALL`].
fn slot(harness: Harness) -> usize {
    Harness::ALL
        .iter()
        .position(|each| *each == harness)
        .unwrap_or_default()
}

/// The model of `harness` that `given` names (`model` or `model:effort`), as the
/// flag `flag` gave it.
fn model_of(harness: Harness, given: &str, flag: &str) -> std::result::Result<Model, String> {
    let given = given.trim();
    let split = given
        .rsplit_once(':')
        .map(|(name, effort)| (name.trim(), effort.trim()));
    let (name, effort) = match split {
        Some((name, effort))
            if Harness::ALL
                .iter()
                .any(|each| efforts(*each).contains(&effort)) =>
        {
            if !efforts(harness).contains(&effort) {
                return Err(format!(
                    "{flag}: {} takes no effort {effort} (it takes {})",
                    harness.name(),
                    efforts(harness).join(", ")
                ));
            }
            (name, Some(effort.to_owned()))
        }
        _ => (given, None),
    };
    if name.is_empty() {
        return Err(format!(
            "{flag} names no model for {}: {given:?}",
            harness.name()
        ));
    }
    Ok(Model {
        harness,
        name: name.to_owned(),
        effort,
    })
}

/// The models this run is asked for ([`parse`] of what `--models` and
/// `--claude-model` set), or why no window could be asked for them, before
/// anything is spent.
pub fn asked() -> Result<Models> {
    asked_with(requested, None, super::keychain::live_case(), pi_providers)
}

/// Resolve Pi's machine-specific model only when a case is seating Pi.
#[allow(clippy::print_stderr)] // The selected provider and model are part of the run's report.
pub fn asked_for(harness: Harness) -> Result<Models> {
    let models = asked_with(
        requested,
        Some(harness),
        super::keychain::live_case(),
        pi_providers,
    )?;
    if harness == Harness::Pi && !models.notes.is_empty() {
        eprintln!("{}", models.line());
    }
    Ok(models)
}

fn asked_with(
    read: impl Fn(&str) -> Option<String>,
    seated: Option<Harness>,
    live: bool,
    providers: impl FnOnce() -> Option<Vec<String>>,
) -> Result<Models> {
    let models = asked_of(read)?;
    if !live || seated != Some(Harness::Pi) || models.named.contains(&Harness::Pi) {
        return Ok(models);
    }
    let machine = super::renew::TestMachine::detect();
    Ok(models.on_machine(
        machine.as_ref().map(super::renew::TestMachine::name),
        providers().as_deref(),
    ))
}

/// A count of provider lookups by a unit test, never their contents.
#[cfg(test)]
static PI_PROVIDER_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn pi_providers() -> Option<Vec<String>> {
    assert!(
        super::keychain::live_case(),
        "stand-in run attempted Pi provider lookup"
    );
    let folder = crate::process::own_var("PI_CODING_AGENT_DIR")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            super::machine::home()
                .ok()
                .map(|home| home.join(".pi").join("agent"))
        })?;
    #[cfg(test)]
    PI_PROVIDER_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    static NAMES: std::sync::OnceLock<Option<Vec<String>>> = std::sync::OnceLock::new();
    NAMES
        .get_or_init(|| super::isolation::pi_provider_names(&folder.join("auth.json")).ok())
        .clone()
}

/// [`asked`], of the variables as `read` has them.
fn asked_of(read: impl Fn(&str) -> Option<String>) -> Result<Models> {
    parse(read(MODELS).as_deref(), read(CLAUDE_MODEL).as_deref()).map_err(Error::Daemon)
}

/// The model `harness` runs on in this run: the one the run named for it, or its
/// cheap one. A run reads the flags with [`super::Log::begin`] before anything
/// else, which refuses them when they are wrong; here they can only be read.
pub fn model(harness: Harness) -> Model {
    match asked_for(harness) {
        Ok(models) => models.of(harness).clone(),
        Err(refused) => panic!("{refused} (a live run begins with Log::begin, which refuses it)"),
    }
}

/// The agent that runs `harness` on its model ([`model`]), as a window is asked
/// for (`kind`, `model` and, where it has one, the effort: under `thinking` for
/// Pi, which reads it there, and `effort` for the others).
pub fn agent(harness: Harness) -> Value {
    agent_on(&model(harness))
}

/// The agent that runs `model`'s harness on it.
pub fn agent_on(model: &Model) -> Value {
    let mut agent = json!({ "kind": model.harness.kind(), "model": model.name });
    if let (Some(effort), Some(fields)) = (&model.effort, agent.as_object_mut()) {
        fields.insert(effort_key(model.harness).to_owned(), json!(effort));
    }
    agent
}

/// The roster row of the agent that runs `harness`, of the work tier `tier`
/// (the daemon picks a member for a task by its tier: a task that names one
/// lands on the harness that has it).
pub fn row(harness: Harness, tier: &str) -> Value {
    row_on(&model(harness), tier)
}

/// [`row`] of the agent that runs `model`'s harness on it.
pub fn row_on(model: &Model, tier: &str) -> Value {
    let mut row = agent_on(model);
    if let Some(fields) = row.as_object_mut() {
        fields.insert("id".to_owned(), json!(id(model.harness)));
        fields.insert("workTier".to_owned(), json!(tier));
    }
    row
}

/// Writes the rig's stand-ins: the agents `rows`, over the ones the rig started with, each row
/// of a real harness given an effort where it needs one ([`with_efforts`]). A member added from
/// a row runs on the row's harness, model and effort (`rig::member_body`), so each row of a
/// real harness is what the `log`'s report says it was launched on.
pub fn write_roster(rig: &Rig, rows: &[Value], log: &Log) -> Result {
    let rows = with_efforts(rig, rows, |note| log.say(note));
    if rows.iter().any(|row| {
        row["kind"] == "muse" && row["designer"] != true && row["model"].as_str().is_none()
    }) {
        return Err(Error::Daemon(
            "Muse's model list gave the run no usable Spark model; choose one with --models muse=…"
                .to_owned(),
        ));
    }
    validate_rows(&rows, |harness| listed_by(rig, harness))?;
    let roster = json!({ "agents": rows });
    rig.stand_ins(&roster)?;
    for row in &rows {
        log.launched(row);
    }
    Ok(())
}

/// Refuse a named model that this run's isolated harness does not list before
/// opening a chief or member window, including rows that already name an effort.
fn validate_rows(
    rows: &[Value],
    mut read: impl FnMut(Harness) -> std::result::Result<Vec<Listed>, String>,
) -> Result {
    let mut lists: Vec<(Harness, Vec<Listed>)> = Vec::new();
    for row in rows {
        let named = row["model"].as_str().is_some_and(|name| !name.is_empty());
        let Some(model) = launched_on(row).filter(|_| named && row["designer"] != true) else {
            continue;
        };
        if !lists.iter().any(|(harness, _)| *harness == model.harness) {
            let list = read(model.harness).map_err(|reason| {
                Error::Daemon(format!(
                    "cannot check {} model {} against this run's list: {reason}",
                    model.harness.name(),
                    model.name
                ))
            })?;
            lists.push((model.harness, list));
        }
        let list = &lists
            .iter()
            .find(|(harness, _)| *harness == model.harness)
            .ok_or_else(|| Error::Daemon("model list was not kept".to_owned()))?
            .1;
        if list
            .iter()
            .any(|item| item.name == model.name && !item.disabled)
        {
            continue;
        }
        let mut nearest: Vec<_> = list.iter().collect();
        nearest.sort_by_key(|item| (distance(&model.name, &item.name), item.name.as_str()));
        let names = nearest
            .iter()
            .take(5)
            .map(|item| {
                if item.disabled {
                    format!("{} (disabled)", item.name)
                } else {
                    item.name.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(Error::Daemon(format!(
            "{} model {} is not enabled in this run's own list; closest listed ids: {}",
            model.harness.name(),
            model.name,
            if names.is_empty() { "none" } else { &names }
        )));
    }
    Ok(())
}

/// Admit a directly opened window against the list its rig's daemon reads.
pub fn validate_window(rig: &Rig, agent: &Value) -> Result {
    validate_rows(std::slice::from_ref(agent), |harness| {
        listed_by(rig, harness)
    })
}

/// Admit a model for a direct, non-rig probe, using only a run-owned harness
/// home. The listing operation opens no window and spends no model turn.
pub fn validate_in_env(model: &Model, home: &[(String, String)]) -> Result {
    let env = isolated_env(model.harness, home)?;
    validate_rows(&[agent_on(model)], |harness| listed_in_env(harness, &env))
}

/// What `harness` lists, asked of the real program with the isolated environment `env`
/// ([`isolated_env`]): no window, no model turn, and never the rig's stand-in.
fn listed_in_env(
    harness: Harness,
    env: &BTreeMap<String, String>,
) -> std::result::Result<Vec<Listed>, String> {
    let answer = super::ask::question(&json!({
        "op": "models", "kind": harness.kind(), "env": env,
    }))
    .map_err(|why| why.to_string())?;
    listing(&answer["listing"], harness)
}

/// Where a rig's list of `harness`'s models is read: by the rig's daemon (`models.get`), or, for
/// Claude Code whose rig starts it through the stand-in's shim, directly from the real program
/// on the run's own config folder `config`. The shim hands the daemon's read of the list to the
/// stand-in, which lists fake models only: a real model asked against it is refused before any
/// window opens (T-1835, T-1857).
fn list_route(
    harness: Harness,
    config: Option<&str>,
) -> std::result::Result<Option<BTreeMap<String, String>>, String> {
    match (harness, config) {
        (Harness::Claude, Some(config)) => isolated_env(
            Harness::Claude,
            &[("CLAUDE_CONFIG_DIR".to_owned(), config.to_owned())],
        )
        .map(Some)
        .map_err(|why| why.to_string()),
        _ => Ok(None),
    }
}

/// The environment a direct read of `harness`'s models is given: only what `home` says of the
/// run-owned folders its isolation names, never a folder that is not the run's.
fn isolated_env(harness: Harness, home: &[(String, String)]) -> Result<BTreeMap<String, String>> {
    let names: &[&str] = match harness {
        Harness::Claude => &["CLAUDE_CONFIG_DIR"],
        Harness::Codex => &["CODEX_HOME"],
        Harness::Pi => &["PI_CODING_AGENT_DIR", "PI_CODING_AGENT_SESSION_DIR"],
        Harness::Devin if cfg!(windows) => &["USERPROFILE", "APPDATA", "LOCALAPPDATA"],
        Harness::Devin | Harness::Muse => &[
            "XDG_CONFIG_HOME",
            "XDG_DATA_HOME",
            "XDG_CACHE_HOME",
            "XDG_STATE_HOME",
        ],
    };
    let value = |name: &str| {
        home.iter()
            .rev()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    };
    // agy's home is its kept home, which the kit keeps and no run makes; and it is run from the
    // kit's own copy: a search path the caller gives is refused, the copy's folder is the only
    // one a direct read may run it from (the owner's launcher updates itself).
    let kept = (harness == Harness::Agy)
        .then(|| {
            super::machine::home()
                .ok()
                .and_then(|user| super::agy_home::kept(&user).ok())
        })
        .flatten();
    if harness == Harness::Agy {
        let copy_only = value("PATH").is_some_and(|path| {
            std::env::split_paths(path).all(|folder| crate::door::is_made_or_below(&folder))
                && !path.is_empty()
        });
        if !copy_only {
            return Err(Error::Daemon(
                "cannot check agy models: its search path is not the kit's own copy's folder alone"
                    .to_owned(),
            ));
        }
    }
    for name in names {
        let safe = value(name).is_some_and(|value| {
            let path = std::path::Path::new(value);
            path.is_absolute()
                && (crate::door::is_made_or_below(path) || kept.as_deref() == Some(path))
        });
        if !safe {
            return Err(Error::Daemon(format!(
                "cannot check {} models: {name} is not a run-owned folder",
                harness.name()
            )));
        }
    }
    let mut env: BTreeMap<String, String> = [
        "PATH",
        "PATHEXT",
        "USER",
        "LANG",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SHELL",
        "SystemRoot",
        "ComSpec",
    ]
    .into_iter()
    .filter_map(|key| crate::process::own_var(key).map(|value| (key.to_owned(), value)))
    .collect();
    if harness == Harness::Agy {
        if let Some(path) = value("PATH") {
            env.insert("PATH".to_owned(), path.to_owned());
        }
    }
    for name in names {
        env.insert(
            (*name).to_owned(),
            value(name).unwrap_or_default().to_owned(),
        );
    }
    env.extend(
        super::launcher::updates_off().map(|(name, value)| (name.to_owned(), value.to_owned())),
    );
    if harness == Harness::Pi {
        env.insert("PI_OFFLINE".to_owned(), "1".to_owned());
    }
    if let Some(git) = value("GIT_CONFIG_GLOBAL") {
        let path = std::path::Path::new(git);
        if !path.is_absolute() || !crate::door::is_made_or_below(path) {
            return Err(Error::Daemon(
                "the run's Git config is not its own".to_owned(),
            ));
        }
        env.insert("GIT_CONFIG_GLOBAL".to_owned(), git.to_owned());
    }
    // Do not let any omitted home variable fall back to the owner's home.
    let owned_home = names
        .first()
        .and_then(|name| value(name))
        .ok_or_else(|| Error::Daemon("the run has no harness home".to_owned()))?;
    env.insert("HOME".to_owned(), owned_home.to_owned());
    if !env.contains_key("USERPROFILE") {
        env.insert("USERPROFILE".to_owned(), owned_home.to_owned());
    }
    Ok(env)
}

fn distance(a: &str, b: &str) -> usize {
    let mut previous: Vec<usize> = (0..=b.chars().count()).collect();
    for (i, left) in a.chars().enumerate() {
        let mut next = vec![i + 1];
        for (j, right) in b.chars().enumerate() {
            next.push(
                (previous[j + 1] + 1)
                    .min(next[j] + 1)
                    .min(previous[j] + usize::from(left != right)),
            );
        }
        previous = next;
    }
    *previous.last().unwrap_or(&0)
}

/// A model a harness lists, as the daemon's `models.get` answers it: the name `--model` takes
/// for it (Pi's `provider/id`), the efforts it takes from the lowest as the harness orders
/// them, and whether the harness will not run it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Listed {
    name: String,
    efforts: Vec<String>,
    disabled: bool,
}

/// The account's preferred enabled Spark, used for both roster and direct probes.
fn preferred_muse(listed: &[Listed]) -> Option<Model> {
    let chosen = ["muse-spark-1.3", "muse-spark-1.2"]
        .iter()
        .find_map(|name| {
            listed
                .iter()
                .find(|item| !item.disabled && item.name == *name)
        })?;
    Some(Model {
        harness: Harness::Muse,
        name: chosen.name.clone(),
        effort: chosen.efforts.first().cloned(),
    })
}

/// The account's model from the direct `harness-ask` answer. A bundled or missing list
/// is not evidence of the account's models and must not start a paid turn.
fn muse_from_answer(answer: &Value) -> Result<Model> {
    if answer["stale"] != false {
        return Err(Error::Daemon(
            "Muse did not return a fresh account model list".to_owned(),
        ));
    }
    let models = answer["models"]
        .as_array()
        .ok_or_else(|| Error::Daemon("Muse's account model list is missing".to_owned()))?;
    let models = models
        .iter()
        .filter_map(|item| {
            Some(Listed {
                name: item["id"].as_str()?.to_owned(),
                efforts: item["efforts"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str().map(str::to_owned))
                    .collect(),
                disabled: item["contributor"] != false || item["disabled"] == true,
            })
        })
        .collect::<Vec<_>>();
    preferred_muse(&models).ok_or_else(|| {
        Error::Daemon("Muse's account lists no enabled Spark model the kit knows".to_owned())
    })
}

/// Ask Muse's host for its account list with the caller's already isolated environment.
/// This opens no window and spends no model turn.
pub fn muse_in_env(env: &BTreeMap<String, String>) -> Result<Model> {
    let answer = super::ask::question(&json!({ "op": "muse_models", "env": env }))?;
    muse_from_answer(&answer)
}

/// What `models.get` is asked: the harness by its CLI's name (`claude`), which the daemon reads
/// the list of, and not by its kind (`claude-code`), which a roster row and a member name.
fn models_asked(harness: Harness) -> Value {
    json!({ "harness": harness.name() })
}

/// What `harness` lists now, read by the rig's daemon as the page's picker reads it
/// (`models.get`: no window, no model turn); why it could not be read, where it could not.
fn listed_by(rig: &Rig, harness: Harness) -> std::result::Result<Vec<Listed>, String> {
    if let Some(env) = list_route(harness, rig.var("CLAUDE_CONFIG_DIR"))? {
        return listed_in_env(harness, &env);
    }
    let answer = rig
        .page("models.get", models_asked(harness))
        .map_err(|why| why.to_string())?;
    if answer["ok"] != true {
        let said = answer["error"].as_str().unwrap_or("no reason given");
        return Err(said.to_owned());
    }
    listing(&answer["listing"], harness)
}

fn listing(listing: &Value, harness: Harness) -> std::result::Result<Vec<Listed>, String> {
    if harness == Harness::Muse && listing["freshness"]["state"] != "fresh" {
        return Err("Muse did not return a fresh account model list".to_owned());
    }
    let models = listing["models"].as_array();
    let Some(models) = models.filter(|models| !models.is_empty()) else {
        return Err("it lists no model".to_owned());
    };
    Ok(models
        .iter()
        .filter_map(|model| {
            let id = model["id"].as_str()?;
            let name = match model["provider"].as_str() {
                Some(provider) => format!("{provider}/{id}"),
                None => id.to_owned(),
            };
            let efforts = model["efforts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|effort| effort.as_str().map(str::to_owned))
                .collect();
            Some(Listed {
                name,
                efforts,
                disabled: model["disabled"] == true,
            })
        })
        .collect())
}

/// `rows` with the least effort each row of a real harness is to run at, which the daemon requires
/// of any member or chief on a model that lists efforts (A4-28: "pick the effort codex runs
/// gpt-5.6-luna at: low, medium, …"): the first its harness lists for the model, which it orders
/// from the lowest. A row that names an effort keeps it, and so does a row whose harness could not
/// be read, whose model it does not list or will not run, or lists no effort for (the daemon's own
/// check says why at `member.add`); an image designer and the stand-in agent's rows have none to
/// give. `say` is told why a harness could not be read.
pub fn with_efforts(rig: &Rig, rows: &[Value], say: impl FnMut(String)) -> Vec<Value> {
    with_least_efforts(rows, |harness| listed_by(rig, harness), say)
}

/// [`with_efforts`], of the lists `read` gives (each harness is read once, and only for a row
/// that needs it).
fn with_least_efforts(
    rows: &[Value],
    mut read: impl FnMut(Harness) -> std::result::Result<Vec<Listed>, String>,
    mut say: impl FnMut(String),
) -> Vec<Value> {
    let mut lists: Vec<(Harness, Vec<Listed>)> = Vec::new();
    let named_muse = asked_of(requested).is_ok_and(|models| models.named.contains(&Harness::Muse));
    rows.iter()
        .map(|row| {
            let mut row = row.clone();
            let named = row["model"].as_str().is_some_and(|model| !model.is_empty());
            let Some(model) = launched_on(&row).filter(|_| named && row["designer"] != true) else {
                return row;
            };
            let choose_muse = model.harness == Harness::Muse
                && model.name == cheap(Harness::Muse).0
                && !named_muse;
            if model.effort.is_some() && !choose_muse {
                return row;
            }
            if !lists.iter().any(|(harness, _)| *harness == model.harness) {
                let listed = read(model.harness).unwrap_or_else(|why| {
                    say(format!(
                        "{}'s models could not be read, so its row {} names no effort: {why}",
                        model.harness.name(),
                        model.name
                    ));
                    Vec::new()
                });
                lists.push((model.harness, listed));
            }
            if choose_muse {
                let chosen = lists.iter().find(|(harness, _)| *harness == Harness::Muse)
                    .and_then(|(_, listed)| preferred_muse(listed));
                if let Some(fields) = row.as_object_mut() {
                    match chosen {
                        Some(listed) => {
                            fields.insert("model".to_owned(), json!(listed.name));
                            if let Some(effort) = listed.effort {
                                fields.insert("effort".to_owned(), json!(effort));
                            } else {
                                fields.remove("effort");
                            }
                        }
                        None => {
                            fields.remove("model");
                            fields.remove("effort");
                            say("Muse's account lists no enabled Spark model the kit knows; choose one with --models muse=…".to_owned());
                        }
                    }
                }
                return row;
            }
            let least = lists
                .iter()
                .filter(|(harness, _)| *harness == model.harness)
                .flat_map(|(_, listed)| listed)
                .find(|listed| listed.name == model.name && !listed.disabled)
                .and_then(|listed| listed.efforts.first().cloned());
            if let (Some(effort), Some(fields)) = (least, row.as_object_mut()) {
                fields.insert(effort_key(model.harness).to_owned(), json!(effort));
            }
            row
        })
        .collect()
}

/// What each harness calls its tool for asking the person it works for a
/// question, as a brief names it: Pi has none, and a Muse member is launched
/// without its own (`--disable-request-user-input`, T-790). Both ask with `cf ask`.
pub fn question_tool(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::Claude => Some("AskUserQuestion tool"),
        Harness::Codex => Some("request_user_input tool"),
        Harness::Devin => Some("ask_user_question tool"),
        Harness::Muse | Harness::Pi => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t1778_direct_model_admission_never_reads_a_foreign_or_missing_home() {
        let model = Model {
            harness: Harness::Codex,
            name: "example".to_owned(),
            effort: None,
        };
        let empty = validate_in_env(&model, &[]).unwrap_err().to_string();
        assert!(empty.contains("CODEX_HOME") && empty.contains("run-owned"));
        let outside = if cfg!(windows) {
            std::path::PathBuf::from(r"C:\not-a-home-of-this-run")
        } else {
            std::path::PathBuf::from("/not-a-home-of-this-run")
        };
        let foreign = validate_in_env(
            &model,
            &[(
                "CODEX_HOME".to_owned(),
                outside.to_string_lossy().into_owned(),
            )],
        )
        .unwrap_err()
        .to_string();
        assert!(foreign.contains("CODEX_HOME") && foreign.contains("run-owned"));
    }

    /// T-1835, T-1857: the model list a rig's roster is checked against is the real Claude's, on
    /// the run's own config folder, never the shim's stand-in's; every other harness's is the
    /// daemon's.
    #[test]
    fn a_real_claude_model_is_checked_against_the_real_list_not_the_stand_ins() {
        let config = crate::door::made("cf-list-route-").unwrap();
        let folder = config.path().to_string_lossy().into_owned();
        let env = list_route(Harness::Claude, Some(&folder)).unwrap().unwrap();
        assert_eq!(env["CLAUDE_CONFIG_DIR"], folder);
        assert_eq!(env["HOME"], folder, "the owner's home is not a fallback");
        // The shim's folder is no part of the environment: the caller's search path finds the
        // real program, and the stand-in is a program of the rig's alone.
        assert!(list_route(Harness::Claude, None).unwrap().is_none());
        for other in [Harness::Codex, Harness::Pi, Harness::Devin, Harness::Muse] {
            assert!(list_route(other, Some(&folder)).unwrap().is_none(), "{other:?}");
        }
        // A config folder that is not the run's own is refused, not read.
        assert!(list_route(Harness::Claude, Some("/not-a-home-of-this-run")).is_err());
    }

    /// agy's direct read is run from the kit's own copy alone, with the updater switch, on the
    /// kept home: the caller's search path, which finds the owner's launcher, is refused.
    #[test]
    fn a_direct_read_of_agy_runs_from_the_kits_copy_alone_with_the_updater_switch() {
        let copy = crate::door::made("cf-agy-copy-").unwrap();
        let home = crate::door::made("cf-agy-home-").unwrap();
        let var = |name: &str, value: &std::path::Path| {
            (name.to_owned(), value.to_string_lossy().into_owned())
        };
        let env = isolated_env(
            Harness::Agy,
            &[var("HOME", home.path()), var("PATH", copy.path())],
        )
        .unwrap();
        assert_eq!(env["PATH"], copy.path().to_string_lossy());
        assert_eq!(env["HOME"], home.path().to_string_lossy());
        assert_eq!(env["AGY_CLI_DISABLE_AUTO_UPDATE"], "1");
        for refused in [
            vec![var("HOME", home.path())],
            vec![
                var("HOME", home.path()),
                ("PATH".to_owned(), "/usr/local/bin:/usr/bin".to_owned()),
            ],
        ] {
            let said = isolated_env(Harness::Agy, &refused).unwrap_err().to_string();
            assert!(said.contains("kit's own copy"), "{said}");
        }
    }

    #[test]
    fn t1778_both_model_list_doors_preserve_the_provider_and_reject_stale_muse() {
        let supplied = json!({
            "freshness": {"state": "fresh"},
            "models": [{"provider": "cliproxyapi", "id": "devin/swe-2", "efforts": ["medium"]}]
        });
        let rows = super::listing(&supplied, Harness::Pi).unwrap();
        assert_eq!(rows[0].name, "cliproxyapi/devin/swe-2");
        let mut stale = supplied;
        stale["freshness"]["state"] = json!("stale");
        assert!(super::listing(&stale, Harness::Muse).is_err());
    }

    #[test]
    fn t1778_owner_proxy_uses_its_current_model_id() {
        let models = parse(None, None)
            .unwrap()
            .on_machine(None, Some(&["cliproxyapi".to_owned()]));
        assert_eq!(models.of(Harness::Pi).name, "cliproxyapi/devin/swe-2");
    }

    #[test]
    fn t1778_a_named_effort_does_not_hide_an_unlisted_model() {
        let rows = [row_on(
            &Model {
                harness: Harness::Pi,
                name: "cliproxyapi/swe-2".to_owned(),
                effort: Some("medium".to_owned()),
            },
            "standard",
        )];
        let error = validate_rows(&rows, |_| {
            Ok(listing(&[(
                "cliproxyapi/devin/swe-2",
                &["medium", "high", "max"],
            )]))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("cliproxyapi/swe-2"), "{error}");
        assert!(error.contains("cliproxyapi/devin/swe-2"), "{error}");
        assert!(validate_rows(&rows, |_| Err("not available".to_owned())).is_err());
        let matching = [row_on(
            &Model {
                harness: Harness::Pi,
                name: "cliproxyapi/devin/swe-2".to_owned(),
                effort: Some("medium".to_owned()),
            },
            "standard",
        )];
        assert!(validate_rows(&matching, |_| Ok(listing(&[(
            "cliproxyapi/devin/swe-2",
            &["medium"]
        )])))
        .is_ok());
        let disabled = vec![Listed {
            name: "cliproxyapi/devin/swe-2".to_owned(),
            efforts: vec![],
            disabled: true,
        }];
        assert!(validate_rows(&matching, |_| Ok(disabled.clone()))
            .unwrap_err()
            .to_string()
            .contains("disabled"));
    }

    #[test]
    fn t1744_reading_the_models_for_a_non_pi_live_run_never_reads_pi_providers() {
        let read = std::cell::Cell::new(false);
        let models = asked_with(
            |_| None,
            None,
            true,
            || {
                read.set(true);
                Some(vec!["openai".to_owned()])
            },
        )
        .unwrap();
        assert_eq!(models.of(Harness::Codex).name, cheap(Harness::Codex).0);
        assert!(
            !read.get(),
            "asking for the run's models read Pi's providers"
        );
    }

    #[test]
    fn t1744_pi_seating_selects_from_provider_names_and_an_explicit_model_needs_no_lookup() {
        let read = std::cell::Cell::new(0);
        let lookup = || {
            read.set(read.get() + 1);
            Some(vec!["openai".to_owned()])
        };
        let models = asked_with(|_| None, Some(Harness::Pi), true, lookup).unwrap();
        assert_eq!(read.get(), 1);
        assert_eq!(models.of(Harness::Codex).name, cheap(Harness::Codex).0);
        let explicit = asked_with(
            |key| (key == MODELS).then(|| "pi=openai/example".to_owned()),
            Some(Harness::Pi),
            true,
            lookup,
        )
        .unwrap();
        assert_eq!(explicit.of(Harness::Pi).name, "openai/example");
        assert_eq!(read.get(), 1);
    }

    #[test]
    fn standin_isolation_log_begin_never_reads_a_decoy_pi_auth() {
        const CHILD: &str = "CF_T1647_PI_CHILD";
        const OUTSIDE: &str = "CF_T1647_PI_OUTSIDE";
        if crate::process::own_var(CHILD).is_some() {
            let decoy =
                std::path::PathBuf::from(crate::process::own_var("PI_CODING_AGENT_DIR").unwrap());
            let inherited = std::path::PathBuf::from(crate::process::own_var(OUTSIDE).unwrap());
            assert_ne!(decoy, inherited.join(".pi/agent"));
            crate::door::make_dir(&decoy).unwrap();
            crate::door::write(decoy.join("auth.json"), "{\"cliproxyapi\":{}}\n").unwrap();
            super::super::Log::begin("stand-in decoy").unwrap();
            assert_eq!(
                PI_PROVIDER_READS.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "Log::begin read the decoy Pi auth file"
            );
            return;
        }
        // This is the child's inherited decoy home, outside the home that its
        // own bootstrap makes. With no PI_CODING_AGENT_DIR, the old lookup
        // would have read this file through HOME.
        let inherited = crate::door::made("pi-inherited-home-").unwrap();
        let agent = inherited.path().join(".pi/agent");
        crate::door::make_dir(&agent).unwrap();
        crate::door::write(agent.join("auth.json"), "{\"outside-provider\":{}}\n").unwrap();
        let output = crate::process::case_of(
            std::env::current_exe().unwrap(),
            "live::agents::tests::standin_isolation_log_begin_never_reads_a_decoy_pi_auth",
            std::time::Duration::from_secs(120),
        )
        .inheriting_env()
        .arg("--nocapture")
        .var(CHILD, "1")
        .var(OUTSIDE, inherited.path())
        .var("HOME", inherited.path())
        .var("USERPROFILE", inherited.path())
        .var("CF_E2E_STAND_IN", "1")
        .without("PI_CODING_AGENT_DIR")
        .without("PI_CODING_AGENT_SESSION_DIR")
        .without("CONSENSFLOW_TEST_MACHINE")
        .run()
        .unwrap();
        assert!(output.code == Some(0), "{}{}", output.stdout, output.stderr);
    }

    /// T-931 P2-2: the harnesses `--models` and `--claude-model` name are asked for: a
    /// machine that lacks one is the run's error, not a harness left out.
    #[test]
    fn the_harnesses_the_model_flags_name_are_the_ones_asked_for() {
        let models = parse(
            Some("muse=muse-spark-1.3:minimal, pi=openai/gpt-6-luna"),
            None,
        )
        .unwrap();
        assert_eq!(models.named(), [Harness::Muse, Harness::Pi]);
        let claude = parse(None, Some("claude-sonnet-5-5")).unwrap();
        assert_eq!(claude.named(), [Harness::Claude]);
        assert!(parse(None, None).unwrap().named().is_empty());
    }

    #[test]
    fn every_harness_has_one_agent_of_its_own_kind_on_its_cheap_model() {
        let rows: Vec<Value> = Harness::ALL
            .into_iter()
            .map(|harness| row_on(parse(None, None).unwrap().of(harness), "standard"))
            .collect();
        let ids: Vec<&str> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            [
                "live-claude",
                "live-pi",
                "live-devin",
                "live-codex",
                "live-muse"
            ]
        );
        let models: Vec<&str> = rows
            .iter()
            .map(|row| row["model"].as_str().unwrap())
            .collect();
        assert_eq!(
            models,
            [
                "claude-sonnet-5-5",
                "openrouter/meta/muse-spark-1.3",
                "swe-2",
                "gpt-5.6-luna",
                "muse-spark-1.3"
            ]
        );
        assert!(rows.iter().all(|row| row["workTier"] == "standard"));
        assert_eq!(rows[0]["kind"], "claude-code");
        assert_eq!(rows[2]["effort"], "medium");
        assert_eq!(rows[4]["effort"], "minimal");
        assert!(rows[0].get("effort").is_none());
    }

    #[test]
    fn a_window_is_asked_for_on_the_kind_and_model_of_the_row_and_none_of_its_roster_fields() {
        for harness in Harness::ALL {
            let agent = agent(harness);
            let row = row(harness, "light");
            assert_eq!(agent["kind"], row["kind"], "{harness:?}");
            assert_eq!(agent["model"], row["model"], "{harness:?}");
            assert_eq!(agent["effort"], row["effort"], "{harness:?}");
            assert!(agent.get("id").is_none() && agent.get("workTier").is_none());
            assert_eq!(row["workTier"], "light");
        }
    }

    /// What `--models` (and `--claude-model`) give each harness: its model and
    /// effort, in the order of [`Harness::ALL`].
    fn given(models: Option<&str>, claude: Option<&str>) -> Vec<(String, Option<String>)> {
        parse(models, claude)
            .unwrap()
            .models
            .into_iter()
            .map(|model| (model.name, model.effort))
            .collect()
    }

    fn on(name: &str, effort: Option<&str>) -> (String, Option<String>) {
        (name.to_owned(), effort.map(str::to_owned))
    }

    /// The cheap models, as [`given`] has them.
    fn cheap_ones() -> Vec<(String, Option<String>)> {
        Harness::ALL
            .map(|harness| {
                let (name, effort) = cheap(harness);
                on(name, effort)
            })
            .to_vec()
    }

    fn refused(models: &str, claude: Option<&str>) -> String {
        parse(Some(models), claude).unwrap_err()
    }

    #[test]
    fn a_harness_the_flags_do_not_name_runs_on_its_cheap_model_and_none_name_any_when_absent() {
        assert_eq!(given(None, None), cheap_ones());
        // A flag given nothing, or commas alone, names nothing.
        assert_eq!(given(Some(""), None), cheap_ones());
        assert_eq!(given(Some(" , ,"), None), cheap_ones());
        // The run that sets no variable has them too, and its agents are on them.
        // Where it runs: on this machine, a test machine's Pi may be on the model the kit names
        // for it ([`Models::on_machine`]), which the gate's own run on zeewin2 is.
        let here = parse(None, None).unwrap();
        assert_eq!(asked().unwrap(), here);
        for harness in Harness::ALL {
            let model = model(harness);
            if harness == Harness::Pi && here.of(harness) != &Model::cheap(harness) {
                assert!(PI_ON.iter().any(|row| row.model == model.name), "{model}");
            } else {
                assert_eq!(model, Model::cheap(harness), "{harness:?}");
            }
        }
        let mut pi_only = cheap_ones();
        pi_only[1] = on("openai/gpt-6-luna", None);
        assert_eq!(given(Some("pi=openai/gpt-6-luna"), None), pi_only);
    }

    #[test]
    fn the_flag_names_several_harnesses_each_with_its_model_and_an_effort_where_it_has_one() {
        assert_eq!(
            given(
                Some("pi=openai/gpt-6-luna,codex=gpt-6.1-sol:high,devin=swe-2:max"),
                None
            ),
            [
                on("claude-sonnet-5-5", None),
                on("openai/gpt-6-luna", None),
                on("swe-2", Some("max")),
                on("gpt-6.1-sol", Some("high")),
                on("muse-spark-1.3", Some("minimal")),
            ]
        );
        // Spaces around a piece, a harness, a model or an effort are let be, and so is
        // an empty piece (a flag given twice is its values with a comma between them).
        assert_eq!(
            given(
                Some(" pi = openai/gpt-6-luna : low ,, codex =gpt-6.1-sol ,"),
                None
            ),
            given(Some("pi=openai/gpt-6-luna:low,codex=gpt-6.1-sol"), None)
        );
        // The order of the pieces is not the order of the harnesses.
        assert_eq!(
            given(Some("codex=b,claude=a"), None),
            given(Some("claude=a,codex=b"), None)
        );
    }

    #[test]
    fn what_follows_a_colon_is_the_models_own_unless_it_is_an_effort() {
        let pi = |given_to_pi: &str| given(Some(&format!("pi={given_to_pi}")), None)[1].clone();
        // OpenRouter's free models end in `:free`, which is no effort.
        assert_eq!(
            pi("openrouter/poolside/laguna-s-2.1:free"),
            on("openrouter/poolside/laguna-s-2.1:free", None)
        );
        assert_eq!(
            pi("openrouter/poolside/laguna-s-2.1:free:high"),
            on("openrouter/poolside/laguna-s-2.1:free", Some("high"))
        );
        // An `=` after the first is the model's.
        assert_eq!(pi("openrouter/x/y=z"), on("openrouter/x/y=z", None));
    }

    #[test]
    fn a_flag_no_window_could_be_asked_for_is_refused_in_words_naming_the_harness_and_the_model() {
        assert_eq!(
            refused("gemini=gemini-3", None),
            "--models names no harness called gemini (claude, pi, devin, codex, muse): gemini=gemini-3"
        );
        assert_eq!(
            refused("=gpt-6", None),
            "--models takes harness=model or harness=model:effort, not =gpt-6"
        );
        assert_eq!(
            refused("pi", None),
            "--models takes harness=model or harness=model:effort, not pi"
        );
        assert_eq!(refused("pi=", None), "--models names no model for pi: \"\"");
        assert_eq!(
            refused("codex=gpt-6.1-sol,pi= :high", None),
            "--models names no model for pi: \":high\""
        );
        assert_eq!(
            refused(
                "pi=openai/gpt-6-luna,pi=openrouter/meta/muse-spark-1.3",
                None
            ),
            "--models names pi twice: say one model for it"
        );
        assert_eq!(
            refused("devin=swe-2:off", None),
            "--models: devin takes no effort off (it takes low, medium, high, xhigh, max)"
        );
        assert_eq!(
            refused("claude=claude-opus-5-5:ultra", None),
            "--models: claude takes no effort ultra (it takes low, medium, high, xhigh, max)"
        );
        // Codex takes one Claude does not.
        assert_eq!(
            given(Some("codex=gpt-6.1-sol:ultra"), None)[3],
            on("gpt-6.1-sol", Some("ultra"))
        );
    }

    #[test]
    fn the_claude_model_flag_is_a_claude_piece_of_models_and_models_wins_where_both_name_claude() {
        let mut opus = cheap_ones();
        opus[0] = on("claude-opus-5-5", None);
        assert_eq!(given(None, Some("claude-opus-5-5")), opus);
        assert_eq!(given(Some("claude=claude-opus-5-5"), None), opus);
        // It has an effort as a piece does.
        assert_eq!(
            given(None, Some(" claude-opus-5-5:max "))[0],
            on("claude-opus-5-5", Some("max"))
        );
        // With --models naming others, each has its own.
        let both = given(Some("pi=openai/gpt-6-luna"), Some("claude-opus-5-5"));
        assert_eq!(both[0], on("claude-opus-5-5", None));
        assert_eq!(both[1], on("openai/gpt-6-luna", None));
        // Where --models names Claude too, --models wins, and the other is not read.
        assert_eq!(
            given(Some("claude=claude-haiku-5-5"), Some("claude-opus-5-5"))[0],
            on("claude-haiku-5-5", None)
        );
        assert_eq!(
            given(Some("claude=claude-haiku-5-5"), Some(":max"))[0],
            on("claude-haiku-5-5", None)
        );
        assert_eq!(
            parse(None, Some(":max")).unwrap_err(),
            "--claude-model names no model for claude: \":max\""
        );
    }

    #[test]
    fn a_run_reads_the_variables_of_both_flags_and_says_what_is_wrong_with_them_as_its_error() {
        let read = |models: &'static str, claude: &'static str| {
            move |name: &str| match name {
                MODELS => Some(models.to_owned()),
                CLAUDE_MODEL => Some(claude.to_owned()),
                _ => None,
            }
        };
        assert_eq!(
            asked_of(read("pi=openai/gpt-6-luna", "claude-opus-5-5:max")).unwrap(),
            parse(Some("pi=openai/gpt-6-luna"), Some("claude-opus-5-5:max")).unwrap()
        );
        assert_eq!(
            asked_of(read("pi=", "x")).unwrap_err().to_string(),
            parse(Some("pi="), None).unwrap_err()
        );
        assert_eq!(
            asked_of(|_: &str| None).unwrap(),
            parse(None, None).unwrap()
        );
    }

    #[test]
    fn a_run_says_each_harness_with_its_model_and_its_effort_where_it_has_one() {
        let models = parse(Some("pi=openai/gpt-6-luna,codex=gpt-6.1-sol:high"), None).unwrap();
        assert_eq!(
            models.line(),
            "the models --models and the cheap ones give: claude claude-sonnet-5-5; \
             pi openai/gpt-6-luna; devin swe-2, effort medium; codex gpt-6.1-sol, effort high; \
             muse muse-spark-1.3, effort minimal"
        );
        // What a window launched on an agent runs on is the agent's: Pi's effort is its
        // thinking, and the stand-in agent's windows run on nothing of a harness.
        for harness in Harness::ALL {
            let model = models.of(harness);
            assert_eq!(launched_on(&agent_on(model)).as_ref(), Some(model));
        }
        assert_eq!(
            launched_on(&json!({ "kind": "pi", "model": "p", "effort": "high" })),
            Some(Model {
                harness: Harness::Pi,
                name: "p".to_owned(),
                effort: None
            })
        );
        for model in [STAND_IN_CHIEF_MODEL, STAND_IN_WORKER_MODEL] {
            assert_eq!(
                launched_on(&json!({ "kind": "claude-code", "model": model })),
                None
            );
        }
        assert_eq!(launched_on(&json!({ "kind": "kimi", "model": "k" })), None);
        assert_eq!(models.of(Harness::Codex).effort.as_deref(), Some("high"));
        assert_eq!(models.of(Harness::Pi).name, "openai/gpt-6-luna");
    }

    #[test]
    fn an_agents_effort_is_where_its_harness_reads_it_thinking_for_pi_and_effort_for_the_others() {
        let models = parse(
            Some("pi=openai/gpt-6-luna:high,codex=gpt-6.1-sol:high"),
            None,
        )
        .unwrap();
        assert_eq!(
            agent_on(models.of(Harness::Pi)),
            json!({ "kind": "pi", "model": "openai/gpt-6-luna", "thinking": "high" })
        );
        assert_eq!(
            agent_on(models.of(Harness::Codex)),
            json!({ "kind": "codex", "model": "gpt-6.1-sol", "effort": "high" })
        );
        assert_eq!(
            agent_on(models.of(Harness::Devin)),
            json!({ "kind": "devin", "model": "swe-2", "effort": "medium" })
        );
    }

    #[test]
    fn the_list_of_a_harness_is_asked_by_its_clis_name_and_not_by_its_kind() {
        assert_eq!(
            models_asked(Harness::Claude),
            json!({ "harness": "claude" })
        );
        assert_eq!(models_asked(Harness::Muse), json!({ "harness": "muse" }));
        assert_ne!(Harness::Claude.kind(), Harness::Claude.name());
    }

    fn listing(models: &[(&str, &[&str])]) -> Vec<Listed> {
        models
            .iter()
            .map(|(name, efforts)| Listed {
                name: (*name).to_owned(),
                efforts: efforts.iter().map(|effort| (*effort).to_owned()).collect(),
                disabled: false,
            })
            .collect()
    }

    #[test]
    fn t1744_muse_default_comes_from_its_listing_and_refuses_missing_or_disabled_spark() {
        let rows = [row_on(&Model::cheap(Harness::Muse), "standard")];
        let chosen = with_least_efforts(
            &rows,
            |_| {
                Ok(listing(&[
                    ("muse-spark-1.2", &["low", "high"]),
                    ("muse-wonder-1.0", &["minimal"]),
                ]))
            },
            |_| {},
        );
        assert_eq!(chosen[0]["model"], "muse-spark-1.2");
        assert_eq!(chosen[0]["effort"], "low");
        let mut notes = Vec::new();
        let no_choice = with_least_efforts(
            &rows,
            |_| {
                Ok(vec![Listed {
                    name: "muse-spark-1.3".to_owned(),
                    efforts: vec!["minimal".to_owned()],
                    disabled: true,
                }])
            },
            |note| notes.push(note),
        );
        assert!(no_choice[0].get("model").is_none());
        assert!(no_choice[0].get("effort").is_none());
        assert!(notes.iter().any(|note| note.contains("--models muse=")));
    }

    #[test]
    fn t1744_direct_muse_probes_choose_a_fresh_account_list_and_refuse_unusable_lists() {
        let answer = json!({ "stale": false, "models": [
            { "id": "muse-spark-1.3-contributor", "contributor": true, "efforts": ["minimal"] },
            { "id": "muse-spark-1.2", "contributor": false, "efforts": ["low", "high"] }
        ] });
        let chosen = muse_from_answer(&answer).unwrap();
        assert_eq!(chosen.name, "muse-spark-1.2");
        assert_eq!(chosen.effort.as_deref(), Some("low"));
        let mut stale = answer.clone();
        stale["stale"] = json!(true);
        assert!(muse_from_answer(&stale).is_err());
        let mut missing = answer.clone();
        missing["models"] = json!([]);
        assert!(muse_from_answer(&missing).is_err());
        let mut disabled = answer;
        disabled["models"][1]["disabled"] = json!(true);
        assert!(muse_from_answer(&disabled).is_err());
    }

    /// T-1369: a row of a real harness that names no effort is given the least one its harness
    /// lists for the model, where the daemon would refuse it for naming none; Pi's goes under its
    /// thinking, as Pi reads it.
    #[test]
    fn a_row_with_no_effort_is_given_the_first_effort_its_harness_lists_for_its_model() {
        let rows = [
            row_on(&Model::cheap(Harness::Codex), "standard"),
            row_on(&Model::cheap(Harness::Claude), "standard"),
            row_on(
                &parse(Some("pi=openrouter/meta/muse-spark-1.3"), None)
                    .unwrap()
                    .of(Harness::Pi)
                    .clone(),
                "standard",
            ),
        ];
        let filled = with_least_efforts(
            &rows,
            |harness| {
                Ok(match harness {
                    Harness::Codex => listing(&[
                        ("gpt-5.6-sol", &["low", "ultra"]),
                        ("gpt-5.6-luna", &["low", "medium", "high"]),
                    ]),
                    Harness::Claude => listing(&[("claude-sonnet-5-5", &["low", "medium"])]),
                    _ => vec![Listed {
                        name: "openrouter/meta/muse-spark-1.3".to_owned(),
                        efforts: vec!["off".to_owned(), "high".to_owned()],
                        disabled: false,
                    }],
                })
            },
            |note| panic!("{note}"),
        );
        assert_eq!(filled[0]["effort"], "low");
        assert_eq!(filled[1]["effort"], "low");
        assert_eq!(filled[2]["thinking"], "off");
        assert!(filled[2].get("effort").is_none());
        // What a window is asked for on a row is what the row holds.
        let launched = launched_on(&filled[2]).unwrap();
        assert_eq!(launched.effort.as_deref(), Some("off"));
    }

    #[test]
    fn a_row_that_needs_no_effort_or_cannot_be_given_one_is_left_as_it_is() {
        let devin = row_on(&Model::cheap(Harness::Devin), "standard");
        let named = row_on(
            &parse(Some("codex=gpt-6.1-sol:max"), None)
                .unwrap()
                .of(Harness::Codex)
                .clone(),
            "standard",
        );
        // An image designer draws on Codex's own model: the daemon refuses any it is asked for.
        let designer =
            json!({ "id": "pygmalion", "kind": "codex", "model": "gpt-5.6-sol", "designer": true });
        let stand_in =
            json!({ "id": "chief", "kind": "claude-code", "model": STAND_IN_CHIEF_MODEL });
        let unlisted = row_on(&Model::cheap(Harness::Codex), "standard");
        let none = row_on(&Model::cheap(Harness::Claude), "standard");
        let rows = [devin, named, designer, stand_in, unlisted, none];
        let mut read = Vec::new();
        let kept = with_least_efforts(
            &rows,
            |harness| {
                read.push(harness);
                match harness {
                    // Codex lists other models than the row's; Claude's lists none of its own.
                    Harness::Codex => Ok(listing(&[("gpt-5.6-sol", &["low"])])),
                    _ => Err("it did not answer".to_owned()),
                }
            },
            |_| {},
        );
        assert_eq!(kept, rows);
        // Only the rows that needed a list read one, and each harness was read once.
        assert_eq!(read, [Harness::Codex, Harness::Claude]);
    }

    #[test]
    fn a_model_the_harness_will_not_run_or_lists_no_effort_for_is_left_for_the_daemon_to_say() {
        let rows = [
            row_on(&Model::cheap(Harness::Codex), "standard"),
            row_on(&Model::cheap(Harness::Claude), "standard"),
        ];
        let kept = with_least_efforts(
            &rows,
            |harness| {
                Ok(match harness {
                    Harness::Codex => vec![Listed {
                        name: "gpt-5.6-luna".to_owned(),
                        efforts: vec!["low".to_owned()],
                        disabled: true,
                    }],
                    _ => listing(&[("claude-sonnet-5-5", &[])]),
                })
            },
            |_| {},
        );
        assert_eq!(kept, rows);
    }

    #[test]
    fn a_harness_that_could_not_be_read_is_told_once_with_the_row_it_leaves() {
        let rows = [
            row_on(&Model::cheap(Harness::Codex), "standard"),
            row_on(
                &parse(Some("codex=gpt-6.1-sol"), None)
                    .unwrap()
                    .of(Harness::Codex)
                    .clone(),
                "light",
            ),
        ];
        let mut said = Vec::new();
        let kept = with_least_efforts(
            &rows,
            |_| Err("login expired".to_owned()),
            |note| said.push(note),
        );
        assert_eq!(kept, rows);
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("codex's models could not be read"),
            "{said:?}"
        );
        assert!(said[0].ends_with("login expired"), "{said:?}");
    }

    /// T-790: a Muse member is launched without its question tools
    /// (`--disable-request-user-input`), so no brief tells it to use one.
    #[test]
    fn only_pi_and_muse_members_have_no_tool_to_ask_a_question_with() {
        for harness in Harness::ALL {
            assert_eq!(
                question_tool(harness).is_none(),
                matches!(harness, Harness::Pi | Harness::Muse),
                "{harness:?}"
            );
        }
    }

    /// The owner's Mac's Pi holds one login, `cliproxyapi` — the local CLIProxyAPI proxy's — and
    /// none for the cheap model's provider, OpenRouter, so it runs on the model that login serves:
    /// the proxy's free pool `swe-2` at `medium` effort, and the run says why (T-1172; the
    /// owner, 2026-11-03: "Pi on this Mac can be used with the models pi has now"; the proxy's
    /// free pool was named on 2026-11-09).
    #[test]
    fn the_owners_mac_whose_pi_has_a_cliproxyapi_login_runs_pi_on_the_model_it_serves_and_says_so()
    {
        let have = |providers: &[&str]| {
            providers
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        let models = parse(None, None)
            .unwrap()
            .on_machine(None, Some(&have(&["cliproxyapi"])));
        let pi = models.of(Harness::Pi);
        assert_eq!(pi.name, "cliproxyapi/devin/swe-2");
        assert_eq!(pi.effort.as_deref(), Some("medium"));
        let said = models.line();
        assert!(
            said.contains("pi cliproxyapi/devin/swe-2, effort medium"),
            "{said}"
        );
        assert!(
            said.contains("`cliproxyapi`") && said.contains("none for openrouter"),
            "{said}"
        );
        assert!(said.contains("free pool"), "{said}");
    }

    /// A test machine whose Pi has an `openai` login (a ChatGPT subscription's) and none for the
    /// cheap model's provider, OpenRouter, runs Pi on the model the kit names for that machine, and
    /// the run says which provider and model, and that it spends no usage credits (T-1136: zeewin2's
    /// Pi has no OpenRouter entry).
    #[test]
    fn a_test_machine_whose_pi_has_no_openrouter_login_runs_pi_on_the_model_the_kit_names_for_it_and_says_so(
    ) {
        let have = |providers: &[&str]| {
            providers
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        let models = parse(None, None)
            .unwrap()
            .on_machine(Some("zeewin2"), Some(&have(&["openai"])));
        let pi = models.of(Harness::Pi);
        assert_eq!(pi.name, "openai/gpt-5.6-luna");
        let said = models.line();
        assert!(said.contains("pi openai/gpt-5.6-luna"), "{said}");
        assert!(
            said.contains("`openai`") && said.contains("none for openrouter"),
            "{said}"
        );
        assert!(said.contains("no usage credits"), "{said}");
        // The other harnesses keep their cheap models.
        for harness in [
            Harness::Claude,
            Harness::Codex,
            Harness::Devin,
            Harness::Muse,
        ] {
            let (name, _) = cheap(harness);
            assert_eq!(models.of(harness).name, name);
        }
    }

    /// Nothing else is chosen for Pi: not on the owner's Mac (no test machine), not where a flag
    /// named a model, not where an OpenRouter login is there, not on a machine the kit has no
    /// entry for, and not for a login the kit has no model for.
    #[test]
    fn pi_is_never_given_another_model_than_the_cheap_one_or_the_named_one_by_chance() {
        let have = |providers: &[&str]| {
            providers
                .iter()
                .map(|name| (*name).to_owned())
                .collect::<Vec<_>>()
        };
        let cheap_pi = cheap(Harness::Pi).0;
        let run = |models: Option<&str>, machine: Option<&str>, providers: &[&str]| {
            parse(models, None)
                .unwrap()
                .on_machine(machine, Some(&have(providers)))
                .of(Harness::Pi)
                .name
                .clone()
        };
        assert_eq!(run(None, None, &["openai"]), cheap_pi, "the owner's Mac");
        assert_eq!(
            run(Some("pi=openrouter/x/y"), Some("zeewin2"), &["openai"]),
            "openrouter/x/y",
            "a model the run named"
        );
        assert_eq!(
            run(Some("pi=openrouter/x/y"), None, &["cliproxyapi"]),
            "openrouter/x/y",
            "a model the run named, on the owner's Mac too"
        );
        assert_eq!(
            run(None, None, &["cliproxyapi", "openrouter"]),
            cheap_pi,
            "the cheap provider's login wins over a machine's entry"
        );
        assert_eq!(
            run(None, Some("zeewin2"), &["openai", "openrouter"]),
            cheap_pi
        );
        assert_eq!(run(None, Some("elsewhere"), &["openai"]), cheap_pi);
        assert_eq!(run(None, Some("zeewin2"), &["cliproxyapi"]), cheap_pi);
        assert_eq!(run(None, Some("zeewin2"), &[]), cheap_pi);
        // An unreadable login file is no reason to choose.
        let unread = parse(None, None).unwrap().on_machine(Some("zeewin2"), None);
        assert_eq!(unread.of(Harness::Pi).name, cheap_pi);
        assert!(!unread.line().contains("usage credits"));
    }
}
