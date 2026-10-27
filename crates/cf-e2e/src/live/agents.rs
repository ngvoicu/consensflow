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

use std::fmt;

use serde_json::{json, Value};

use super::machine::Harness;
use super::{requested, Log, CLAUDE_MODEL, MODELS};
use crate::rig::Rig;
use crate::{files, Error, Result};

/// The image designer, the catalog's own: a Codex agent whose image tool draws.
pub const DESIGNER: &str = "pygmalion";

/// The model of the agent the stand-in plays for a chief, and for a member: the
/// shim that starts Claude Code hands a window with either of them to the
/// stand-in, and every other Claude window to the real one.
pub const STAND_IN_CHIEF_MODEL: &str = "fake-chief";
pub const STAND_IN_WORKER_MODEL: &str = "fake";

/// The harness an agent (a roster row, or what a window is asked for) runs on,
/// when it is a real one: the stand-in agent's (a Claude Code agent whose model is
/// a fake one) are none.
pub fn harness_of(agent: &Value) -> Option<Harness> {
    let kind = agent["kind"].as_str()?;
    if kind == "claude-code"
        && [STAND_IN_CHIEF_MODEL, STAND_IN_WORKER_MODEL].contains(&agent["model"].as_str()?)
    {
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
    let effort = if harness == Harness::Pi {
        text("thinking")
    } else {
        text("effort")
    };
    Some(Model {
        harness,
        name: text("model").unwrap_or("its own default").to_owned(),
        effort: effort.map(str::to_owned),
    })
}

/// The id of the agent that runs `harness`.
pub fn id(harness: Harness) -> String {
    format!("live-{}", harness.name())
}

/// What the human's rules call the cheap model of each harness, as the app's
/// catalog and the harness's own list of models both have it (checked on
/// 2026-10-11): Claude Code on Sonnet 5.5, OpenCode and Pi on Muse Spark 1.3
/// (OpenCode's free one, Pi's through OpenRouter, which is where the catalog
/// has it: OpenCode Go is out of ConsensFlow), Devin on SWE-2 at medium, Codex
/// on GPT-5.6 Luna. The model, and the effort where it has one: what a harness
/// runs on where the run names no other ([`model`]).
fn cheap(harness: Harness) -> (&'static str, Option<&'static str>) {
    match harness {
        Harness::Claude => ("claude-sonnet-5-5", None),
        Harness::OpenCode => ("opencode/muse-spark-1.3-contributor-free", None),
        Harness::Pi => ("openrouter/meta/muse-spark-1.3", None),
        Harness::Devin => ("swe-2", Some("medium")),
        Harness::Codex => ("gpt-5.6-luna", None),
        // Gabriel's testing model of 2026-10-26: not a contributor model, at its least
        // effort (the contributor model measured no saving, T-718).
        Harness::Muse => ("muse-spark-1.3", Some("minimal")),
    }
}

/// The effort levels `harness` takes, as the product's catalog has them
/// (`efforts` of `crates/cf-catalog/src/catalog.rs`, which quotes each CLI's own
/// help; a test holds the two to one list).
fn efforts(harness: Harness) -> &'static [&'static str] {
    match harness {
        Harness::Claude => &["low", "medium", "high", "xhigh", "max"],
        Harness::Codex => &["minimal", "low", "medium", "high", "xhigh", "max", "ultra"],
        Harness::Pi => &["off", "minimal", "low", "medium", "high", "xhigh", "max"],
        Harness::OpenCode => &["minimal", "low", "medium", "high", "xhigh", "max"],
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
pub struct Models([Model; 6]);

impl Models {
    /// The model `harness` runs on.
    pub fn of(&self, harness: Harness) -> &Model {
        &self.0[slot(harness)]
    }

    /// What a run says of them in its first lines: what the flags give each
    /// harness, which a suite's own flag may override for its windows (the report
    /// says what each window was launched on: [`super::Log::launched`]).
    pub fn line(&self) -> String {
        let each: Vec<String> = self.0.iter().map(Model::to_string).collect();
        format!(
            "the models --models and the cheap ones give: {}",
            each.join("; ")
        )
    }
}

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
    }
    Ok(Models(chosen))
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
    asked_of(requested)
}

/// [`asked`], of the variables as `read` has them.
fn asked_of(read: impl Fn(&str) -> Option<String>) -> Result<Models> {
    parse(read(MODELS).as_deref(), read(CLAUDE_MODEL).as_deref()).map_err(Error::Daemon)
}

/// The model `harness` runs on in this run: the one the run named for it, or its
/// cheap one. A run reads the flags with [`super::Log::begin`] before anything
/// else, which refuses them when they are wrong; here they can only be read.
pub fn model(harness: Harness) -> Model {
    match asked() {
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
        let key = if model.harness == Harness::Pi {
            "thinking"
        } else {
            "effort"
        };
        fields.insert(key.to_owned(), json!(effort));
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

/// Writes the roster of the rig's home: the agents `rows`, over the one the rig
/// started with. The daemon opens a member's window on its row, so each row of a
/// real harness is what the `log`'s report says it was launched on.
pub fn write_roster(rig: &Rig, rows: &[Value], log: &Log) -> Result {
    let roster = json!({ "schemaVersion": 1, "agents": rows });
    files::write(&rig.home().join("agents.json"), format!("{roster}\n"))?;
    for row in rows {
        log.launched(row);
    }
    Ok(())
}

/// What each harness calls its tool for asking the person it works for a
/// question, as a brief names it; Pi has none.
pub fn question_tool(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::Claude => Some("AskUserQuestion tool"),
        Harness::OpenCode => Some("question tool"),
        Harness::Codex => Some("request_user_input tool"),
        Harness::Devin => Some("ask_user_question tool"),
        Harness::Muse => Some("request_user_input tool"),
        Harness::Pi => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_harness_has_one_agent_of_its_own_kind_on_its_cheap_model() {
        let rows: Vec<Value> = Harness::ALL
            .into_iter()
            .map(|harness| row(harness, "standard"))
            .collect();
        let ids: Vec<&str> = rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            [
                "live-claude",
                "live-opencode",
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
                "opencode/muse-spark-1.3-contributor-free",
                "openrouter/meta/muse-spark-1.3",
                "swe-2",
                "gpt-5.6-luna",
                "muse-spark-1.3"
            ]
        );
        assert!(rows.iter().all(|row| row["workTier"] == "standard"));
        assert_eq!(rows[0]["kind"], "claude-code");
        assert_eq!(rows[3]["effort"], "medium");
        assert_eq!(rows[5]["effort"], "minimal");
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
            .0
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
        assert_eq!(asked().unwrap(), parse(None, None).unwrap());
        for harness in Harness::ALL {
            assert_eq!(model(harness), Model::cheap(harness), "{harness:?}");
        }
        let mut pi_only = cheap_ones();
        pi_only[2] = on("openai/gpt-6-luna", None);
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
                on("opencode/muse-spark-1.3-contributor-free", None),
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
        let pi = |given_to_pi: &str| given(Some(&format!("pi={given_to_pi}")), None)[2].clone();
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
            "--models names no harness called gemini (claude, opencode, pi, devin, codex, muse): gemini=gemini-3"
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
            given(Some("codex=gpt-6.1-sol:ultra"), None)[4],
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
        assert_eq!(both[2], on("openai/gpt-6-luna", None));
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
             opencode opencode/muse-spark-1.3-contributor-free; pi openai/gpt-6-luna; \
             devin swe-2, effort medium; codex gpt-6.1-sol, effort high; \
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
            Some("pi=openai/gpt-6-luna:high,codex=gpt-6.1-sol:high,opencode=o/m"),
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
            agent_on(models.of(Harness::OpenCode)),
            json!({ "kind": "opencode", "model": "o/m" })
        );
        assert_eq!(
            agent_on(models.of(Harness::Devin)),
            json!({ "kind": "devin", "model": "swe-2", "effort": "medium" })
        );
    }

    /// A run can name any agent of the product's catalog by its model and effort:
    /// Buri, OpenCode Zen's free Nemotron 3.5 Super VL at xhigh, among them.
    #[test]
    fn every_agent_of_the_catalog_can_be_named_by_its_model_and_effort() {
        let presets: Value = serde_json::from_str(
            &std::fs::read_to_string(crate::checkout::path("crates/cf-catalog/data/presets.json"))
                .unwrap(),
        )
        .unwrap();
        let presets = presets["presets"].as_array().unwrap();
        assert!(presets.len() > 100, "{}", presets.len());
        for preset in presets {
            let kind = preset["kind"].as_str().unwrap();
            let harness = Harness::ALL
                .into_iter()
                .find(|harness| harness.kind() == kind)
                .unwrap_or_else(|| panic!("no harness runs {kind}"));
            let model = preset["model"].as_str().unwrap();
            let effort = preset["effort"].as_str().or(preset["thinking"].as_str());
            let piece = match effort {
                Some(effort) => format!("{}={model}:{effort}", harness.name()),
                None => format!("{}={model}", harness.name()),
            };
            let named = parse(Some(&piece), None).unwrap_or_else(|refused| panic!("{refused}"));
            assert_eq!(
                (
                    named.of(harness).name.as_str(),
                    named.of(harness).effort.as_deref()
                ),
                (model, effort),
                "{piece}"
            );
        }
        let buri = parse(
            Some("opencode=opencode/nemotron-3.5-super-vl-free:xhigh"),
            None,
        )
        .unwrap();
        assert_eq!(
            agent_on(buri.of(Harness::OpenCode)),
            json!({ "kind": "opencode", "model": "opencode/nemotron-3.5-super-vl-free", "effort": "xhigh" })
        );
    }

    #[test]
    fn the_efforts_a_harness_takes_are_the_ones_the_products_catalog_has() {
        let catalog =
            std::fs::read_to_string(crate::checkout::path("crates/cf-catalog/src/catalog.rs"))
                .unwrap();
        for harness in Harness::ALL {
            // The catalog's own name of the harness: OpenCode is `Opencode` there.
            let variant = match harness {
                Harness::OpenCode => "Opencode".to_owned(),
                other => {
                    let name = other.name();
                    format!("{}{}", name[..1].to_uppercase(), &name[1..])
                }
            };
            let quoted: Vec<String> = efforts(harness)
                .iter()
                .map(|effort| format!("\"{effort}\""))
                .collect();
            let line = format!("Harness::{variant} => &[{}],", quoted.join(", "));
            assert!(catalog.contains(&line), "the catalog has no {line}");
        }
    }

    #[test]
    fn only_pi_has_no_tool_to_ask_a_question_with() {
        for harness in Harness::ALL {
            assert_eq!(
                question_tool(harness).is_none(),
                harness == Harness::Pi,
                "{harness:?}"
            );
        }
    }
}
