//! The pure part of an eval run: which agents make up the staff for a set of
//! harnesses, what environment gives a chief its model where the harness takes
//! one from the environment, and how the scripted human answers a question.
//! Everything here is tested without spending a token.
//!
//! What the evals share with the live tests of `crates/cf-e2e` lives there once
//! (`cf_e2e::live`): the machine's harnesses and the environment their windows
//! get, the program Claude Code is started through, and the flags that shut
//! Codex's MCP servers out ([`cf_e2e::live::codex::isolation`], with the URL a
//! server reached by URL is given: a command on it kills Codex 0.160.1). The
//! cheap model of each harness here is the evals' own (the staff's and the
//! chief's), not the live tests' ([`cf_e2e::live::agents`]); and the name Claude
//! Code gives a folder it keeps a project's memory under is the rig's
//! ([`cf_e2e::rig::project_name`]).

use serde_json::{json, Value};

use crate::measure::question_sentences;
use crate::scenarios::Scenario;
use crate::{Error, Result};

/// A harness an eval can run: the name it goes by on a command line, the kind its
/// roster agent is, and the cheap model (brain: operations/test-models.md) it runs
/// the staff on and, on OpenCode, the chief. OpenCode's free Muse Spark died
/// mid-run twice out of two (a dead turn), so OpenCode runs a paid Go model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Harness {
    pub name: &'static str,
    pub kind: &'static str,
    pub model: &'static str,
}

/// The harnesses of the evals, in the order they are listed.
pub const HARNESSES: [Harness; 5] = [
    Harness {
        name: "claude",
        kind: "claude-code",
        model: "claude-haiku-4-5-20251001",
    },
    Harness {
        name: "codex",
        kind: "codex",
        model: "gpt-5.6-luna",
    },
    // OpenRouter only since 2026-09-28: OpenCode Go is out of ConsensFlow.
    Harness {
        name: "pi",
        kind: "pi",
        model: "openrouter/deepseek/deepseek-v4.1-flash",
    },
    Harness {
        name: "opencode",
        kind: "opencode",
        model: "openrouter/deepseek/deepseek-v4.1-flash",
    },
    // SWE-1.6 Slow, the free plan's, is not on Devin Pro (2026-10-03); SWE-2 is free there.
    Harness {
        name: "devin",
        kind: "devin",
        model: "swe-2-medium",
    },
];

/// The harness called `name`, or the refusal that says there is none.
pub fn harness(name: &str) -> Result<Harness> {
    HARNESSES
        .into_iter()
        .find(|harness| harness.name == name)
        .ok_or_else(|| Error::Refused(format!("no such eval harness: {name}")))
}

/// The roles a staff harness fills: two workers, so parallel work has somewhere to run.
const ROLES: [(&str, &str); 4] = [
    ("worker", "worker"),
    ("worker-2", "worker"),
    ("advisor", "advisor"),
    ("reviewer", "reviewer"),
];

/// An agent of the roster: what a member or the chief runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub id: String,
    pub kind: &'static str,
    pub model: String,
    /// The reasoning level, with the word its harness calls it by: `effort`, or
    /// `thinking` for Pi. Devin has none.
    pub effort: Option<(&'static str, String)>,
    /// The work tier the daemon gives it tasks by: `standard` in an eval, so that
    /// any harness may get any task; a long run gives each harness its own.
    pub tier: &'static str,
}

impl Agent {
    /// The agent `id` on `harness`, on `model` and, where the harness has one, at
    /// the reasoning level `effort`.
    pub fn new(id: String, harness: Harness, model: &str, effort: Option<&str>) -> Self {
        Self {
            id,
            kind: harness.kind,
            model: model.to_owned(),
            effort: effort.filter(|_| harness.name != "devin").map(|level| {
                (
                    if harness.name == "pi" {
                        "thinking"
                    } else {
                        "effort"
                    },
                    level.to_owned(),
                )
            }),
            tier: "standard",
        }
    }

    /// The agent, of the work tier `tier`.
    #[must_use]
    pub fn with_tier(mut self, tier: &'static str) -> Self {
        self.tier = tier;
        self
    }

    /// The row of the roster: of an eval, every member is standard tier, so the
    /// daemon picks among them by its own rule and any harness may get any task.
    pub fn to_value(&self) -> Value {
        let mut row = json!({
            "id": self.id,
            "kind": self.kind,
            "model": self.model,
            "workTier": self.tier,
        });
        if let (Some((word, level)), Some(fields)) = (&self.effort, row.as_object_mut()) {
            fields.insert((*word).to_owned(), json!(level));
        }
        row
    }
}

/// A member of the project's staff: an agent and its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub agent: String,
    pub role: &'static str,
}

/// The roster rows and the project staff of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staff {
    pub agents: Vec<Agent>,
    pub members: Vec<Member>,
}

impl Staff {
    /// The staff as `project.open` takes it.
    pub fn to_value(&self) -> Value {
        Value::Array(
            self.members
                .iter()
                .map(|member| json!({ "agent": member.agent, "roles": [member.role] }))
                .collect(),
        )
    }
}

/// The roster rows and the project staff for these harnesses: for each, two
/// workers, an advisor and a reviewer on its cheap model (`models` overrides a
/// harness's model, by its name). `effort` is each member's reasoning level (Pi
/// calls it thinking); Devin has none.
pub fn staff_for<S: AsRef<str>>(
    harnesses: &[S],
    models: &[(&str, &str)],
    effort: Option<&str>,
) -> Result<Staff> {
    let mut staff = Staff {
        agents: Vec::new(),
        members: Vec::new(),
    };
    for name in harnesses.iter().map(AsRef::as_ref) {
        let harness = harness(name)?;
        let model = models
            .iter()
            .find(|(given, _)| *given == name)
            .map_or(harness.model, |(_, model)| *model);
        for (suffix, role) in ROLES {
            let id = format!("eval-{name}-{suffix}");
            staff.members.push(Member {
                agent: id.clone(),
                role,
            });
            staff.agents.push(Agent::new(id, harness, model, effort));
        }
    }
    Ok(staff)
}

/// A Pi or OpenCode chief's model: a step above the staff's DeepSeek Flash and
/// well below sol's price (2026-09-28, the owner's choice), the same for both so the
/// two harnesses compare as harnesses.
pub const CHIEF_MODEL: &str = "openrouter/deepseek/deepseek-v4-pro-0813";

/// The model a Claude Code chief runs on unless it is told another.
pub const CLAUDE_CHIEF_MODEL: &str = "claude-opus-5";

/// How a chief gets its model: the variables of the environment that give it, where
/// its harness takes it from there, and the model it is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiefSetup {
    pub env: Vec<(String, String)>,
    pub model: String,
}

/// The chief has no model of its own in the roster: it runs its harness's default.
/// Claude Code and OpenCode take one from the environment (Opus, and DeepSeek V4
/// Pro, unless `model` says otherwise), Codex and Pi from the eval's wrappers
/// (Codex's cheap model, DeepSeek V4 Pro); Devin runs the model its own
/// configuration names, so `model` is ignored there.
pub fn chief_environment(chief: &str, model: Option<&str>) -> Result<ChiefSetup> {
    let harness = harness(chief)?;
    Ok(match chief {
        "claude" => {
            let chosen = model.unwrap_or(CLAUDE_CHIEF_MODEL);
            ChiefSetup {
                env: vec![("ANTHROPIC_MODEL".to_owned(), chosen.to_owned())],
                model: chosen.to_owned(),
            }
        }
        "opencode" => {
            let chosen = model.unwrap_or(CHIEF_MODEL);
            ChiefSetup {
                env: vec![(
                    "OPENCODE_CONFIG_CONTENT".to_owned(),
                    json!({ "model": chosen }).to_string(),
                )],
                model: chosen.to_owned(),
            }
        }
        // Pi takes it through the eval's Pi wrapper (`--model`).
        "pi" => ChiefSetup {
            env: Vec::new(),
            model: model.unwrap_or(CHIEF_MODEL).to_owned(),
        },
        // Codex takes it through the eval's Codex wrapper (`-c model=…`).
        "codex" => ChiefSetup {
            env: Vec::new(),
            model: model.unwrap_or(harness.model).to_owned(),
        },
        // Devin runs its staff's model, through the chief's agent.
        _ => ChiefSetup {
            env: Vec::new(),
            model: harness.model.to_owned(),
        },
    })
}

/// The last `count` non-empty lines a window printed, control sequences already
/// stripped: near enough its screen, for a report to show why a chief said
/// nothing (a login page, a quota wall, a model that does not exist).
pub fn last_lines(text: &str, count: usize) -> Vec<String> {
    let lines: Vec<String> = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|line| line.trim_end_matches(crate::js::is_space).to_owned())
        .filter(|line| !crate::js::trim(line).is_empty())
        .collect();
    lines[lines.len().saturating_sub(count)..].to_vec()
}

/// The owner's reply to questions a chief left in its terminal: for each question
/// sentence, the scenario's answer on its subject or its fallback, each answer
/// once, in the order asked.
pub fn terminal_answer(scenario: &Scenario, text: &str) -> String {
    let mut replies: Vec<&str> = Vec::new();
    let sentences = question_sentences(text);
    for question in &sentences {
        let reply = scenario
            .answers
            .iter()
            .find(|answer| answer.matches.is_match(question))
            .map_or(scenario.fallback.as_str(), |answer| answer.text.as_str());
        if !replies.contains(&reply) {
            replies.push(reply);
        }
    }
    replies.join(" ")
}

/// A question of a Devin chief's own dialog: what it asks, under what heading,
/// and how many options it offers before "Other (type your own)".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogQuestion {
    pub question: String,
    pub header: Option<String>,
    pub options: usize,
}

impl DialogQuestion {
    /// The question a dialog holds, as Devin's store keeps it (`question`,
    /// `header`, `options`).
    pub fn of(question: &Value) -> Self {
        Self {
            question: question["question"].as_str().unwrap_or_default().to_owned(),
            header: question["header"].as_str().map(str::to_owned),
            options: question["options"].as_array().map_or(0, Vec::len),
        }
    }
}

/// How the owner answers one question of the dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogAnswer {
    pub question: String,
    /// What the scenario has to say, if it has anything on the subject.
    pub answer: Option<String>,
    /// The keys that give it, one press at a time.
    pub keys: Vec<Vec<u8>>,
}

const DOWN: [u8; 3] = [27, 91, 66];
const ENTER: u8 = 13;

/// The owner's answers to a Devin chief's own question dialog, a question at a
/// time, each with the keys that give it: a question the scenario has an answer for
/// gets the answer typed into "Other (type your own)", which comes after the
/// question's options (Down once per option from the first; one more starts the
/// list over), and any other takes Enter, its first option. `spell` is how the
/// window takes typed text: Devin on Windows, console text. Devin 3000.11.3, probed
/// 2026-10-03.
pub fn dialog_answers(
    scenario: &Scenario,
    questions: &[DialogQuestion],
    spell: &dyn Fn(&str) -> String,
) -> Vec<DialogAnswer> {
    questions
        .iter()
        .map(|asked| {
            let subject = format!(
                "{} {}",
                asked.header.as_deref().unwrap_or_default(),
                asked.question
            );
            let answer = scenario
                .answers
                .iter()
                .find(|answer| answer.matches.is_match(&subject))
                .map(|answer| answer.text.clone());
            let keys = match &answer {
                None => vec![vec![ENTER]],
                Some(text) => {
                    let mut keys: Vec<Vec<u8>> =
                        (0..asked.options).map(|_| DOWN.to_vec()).collect();
                    keys.push(vec![ENTER]);
                    keys.push(spell(text).into_bytes());
                    keys.push(vec![ENTER]);
                    keys
                }
            };
            DialogAnswer {
                question: asked.question.clone(),
                answer,
                keys,
            }
        })
        .collect()
}
