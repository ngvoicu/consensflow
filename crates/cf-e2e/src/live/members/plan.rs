//! What a check opens: the folder of its project, what every window of that
//! folder is told, and the agents of the roster, one of which is the chief. A
//! roster row is a real harness's agent on its cheap model ([`member`]) or the
//! stand-in agent's ([`stand_in_chief`], [`stand_in_worker`]), which is no model
//! and only notes what it is sent.

use serde_json::{json, Value};

use super::super::agents;
use super::super::machine::Harness;

/// What the folder of a check tells every window: the bench's rule (do what each
/// message asks and nothing more), but a message that names one of the window's
/// tools is asking for it. The bench's own words ("run no tools unless the
/// message tells you to run a command") had Claude decline to ask its question
/// with the tool the message named, for a tool is not a command.
pub const BRIEF: &str = "This folder is an automated ConsensFlow test. Do exactly what each message asks and nothing more. When a ConsensFlow message tells you to run a command or to use one of your tools, do it; otherwise run no tools, and reply with one line.\n";

/// What the folder of the bench (and of the approval run) tells every window: the
/// bench measures delivery, not judgment, and a chief left to guess its job
/// explores for minutes after every result, which each delivery then waits for.
pub const BENCH_BRIEF: &str = "This folder is an automated ConsensFlow bench. Do exactly what each message asks and nothing more. When a ConsensFlow message arrives, reply with one line that names it and run no tools unless the message tells you to run a command.\n";

/// The model of the agent the stand-in plays for a chief, and for a member: the
/// shim that starts Claude Code hands a window with either of them to the
/// stand-in, and every other Claude window to the real one.
pub const STAND_IN_CHIEF_MODEL: &str = "fake-chief";
pub const STAND_IN_WORKER_MODEL: &str = "fake";

/// The id of the chief that is the stand-in, and of the worker that is.
pub const STAND_IN_CHIEF: &str = "chief";
pub const STAND_IN_WORKER: &str = "worker";

/// The chief that is the stand-in agent: no model, which only notes what it is
/// sent, so that nothing answers a question or takes a task but the check, which
/// plays the chief with the chief's own token.
pub fn stand_in_chief() -> Value {
    json!({ "id": STAND_IN_CHIEF, "kind": "claude-code", "model": STAND_IN_CHIEF_MODEL })
}

/// A member that is the stand-in agent: it asks through the hook of its settings
/// when its brief says `ASK …`.
pub fn stand_in_worker() -> Value {
    json!({ "id": STAND_IN_WORKER, "kind": "claude-code", "model": STAND_IN_WORKER_MODEL })
}

/// The work tier of the member of `harness`: its own, so that a task which names
/// the tier lands on that harness. The daemon picks a member by tier alone, and a
/// check that needs one harness's question tool must land on that harness.
const fn tier(harness: Harness) -> &'static str {
    match harness {
        Harness::Claude => "complex",
        Harness::OpenCode => "light",
        Harness::Pi => "standard",
        Harness::Devin | Harness::Codex => "critical",
    }
}

/// The words of `cf task add` that name a tier: critical work names a purpose,
/// and the flag carries it.
pub fn tier_flag(tier: &str) -> Vec<String> {
    let words: &[&str] = if tier == "critical" {
        &["--tier", "critical", "--purpose", "hard-problem"]
    } else {
        &["--tier", tier]
    };
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// The member of `harness`, on its cheap model and in its tier, as the roster
/// has it.
pub fn member(harness: Harness) -> Value {
    agents::row(harness, tier(harness))
}

/// [`member`] under the id `id`: a project that has the harness twice (a worker
/// and the chief, a worker and the reviewer) tells its agents apart by it.
pub fn member_as(harness: Harness, id: &str) -> Value {
    let mut row = member(harness);
    if let Some(fields) = row.as_object_mut() {
        fields.insert("id".to_owned(), json!(id));
    }
    row
}

/// The harness a roster row runs on, when it is a real one: the stand-in agent's
/// rows (a Claude Code agent whose model is a fake one) are none.
fn harness_of(row: &Value) -> Option<Harness> {
    let kind = row["kind"].as_str()?;
    if kind == "claude-code"
        && [STAND_IN_CHIEF_MODEL, STAND_IN_WORKER_MODEL].contains(&row["model"].as_str()?)
    {
        return None;
    }
    Harness::ALL
        .into_iter()
        .find(|harness| harness.kind() == kind)
}

/// What a check opens.
#[derive(Debug, Clone)]
pub struct Plan {
    /// The name of the project's folder, below the live folders of the home the
    /// rig gives its windows (`.consensflow-candidate/live/<folder>`): a folder of
    /// its own, so that what Claude Code asks of a folder (its trust) is asked once
    /// and kept.
    pub folder: String,
    /// What the folder tells every window: [`BRIEF`], or the bench's.
    pub brief: &'static str,
    /// Files written into the folder before anything opens, each a path below it
    /// and its text: a Claude window reads the settings there.
    pub files: Vec<(String, String)>,
    /// The variables the daemon, the pane host and so every window are given over
    /// what the rig sets: a name and its value each, the stand-in agent's own
    /// settings (`CF_TEST_WIND_DOWN_MS`).
    pub vars: Vec<(String, String)>,
    /// The agents of the roster.
    pub roster: Vec<Value>,
    /// The id of the agent that is the project's chief.
    pub chief: String,
}

impl Plan {
    /// A project in the folder `folder` whose chief is the agent `chief` (a row of
    /// the roster, which it is the first of).
    pub fn new(folder: &str, chief: Value) -> Self {
        Self {
            folder: folder.to_owned(),
            brief: BRIEF,
            files: Vec::new(),
            vars: Vec::new(),
            chief: chief["id"].as_str().unwrap_or_default().to_owned(),
            roster: vec![chief],
        }
    }

    /// With `brief` as what the folder tells every window.
    #[must_use]
    pub fn told(mut self, brief: &'static str) -> Self {
        self.brief = brief;
        self
    }

    /// With the agent `row` in the roster.
    #[must_use]
    pub fn with(mut self, row: Value) -> Self {
        self.roster.push(row);
        self
    }

    /// With the file `path` (below the folder) holding `text`.
    #[must_use]
    pub fn with_file(mut self, path: &str, text: impl Into<String>) -> Self {
        self.files.push((path.to_owned(), text.into()));
        self
    }

    /// With the variable `name` set to `value` in the environment of the rig.
    #[must_use]
    pub fn with_var(mut self, name: &str, value: impl Into<String>) -> Self {
        self.vars.push((name.to_owned(), value.into()));
        self
    }

    /// The real harnesses the roster runs, each once, in the order the roster has
    /// them: what the machine must have, and what the rig starts with.
    pub fn harnesses(&self) -> Vec<Harness> {
        let mut found: Vec<Harness> = Vec::new();
        for harness in self.roster.iter().filter_map(harness_of) {
            if !found.contains(&harness) {
                found.push(harness);
            }
        }
        found
    }
}

/// What a member is told to do to ask its question: use its harness's own
/// tool, and answer with the colour it was given. None for a harness with no
/// such tool (Pi).
pub fn question_brief(harness: Harness) -> Option<String> {
    let tool = agents::question_tool(harness)?;
    Some(format!(
        "Use your {tool} to ask which colour to use, with the options red and blue. Do not pick one yourself: ask, and wait for the answer. Once it is answered, reply with exactly one line: COLOUR=<the answer>"
    ))
}

/// What the stand-in is told to ask the same question: through the hook of its
/// settings, as Claude's own tool is.
pub fn stand_in_question_brief() -> String {
    let questions = json!([{
        "question": "Which colour?",
        "header": "Colour",
        "options": [{ "label": "red" }, { "label": "blue" }],
        "multiSelect": false,
    }]);
    format!("ASK {questions}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_folder_of_a_check_tells_a_window_that_a_message_naming_one_of_its_tools_asks_for_it() {
        // The bench's own words had Claude decline to ask its question with the tool the
        // message named, for a tool is not a command.
        assert!(BRIEF.contains("tells you to run a command or to use one of your tools, do it"));
        assert!(!BENCH_BRIEF.contains("one of your tools"));
        for brief in [BRIEF, BENCH_BRIEF] {
            assert!(brief.starts_with("This folder is an automated ConsensFlow "));
            assert!(brief.contains("Do exactly what each message asks and nothing more."));
            assert!(brief.ends_with(".\n"));
        }
    }

    #[test]
    fn each_harness_has_a_work_tier_of_its_own_but_devin_and_codex_that_share_the_critical_one() {
        let tiers: Vec<&str> = Harness::ALL.into_iter().map(tier).collect();
        assert_eq!(
            tiers,
            ["complex", "light", "standard", "critical", "critical"]
        );
    }

    #[test]
    fn critical_work_names_a_purpose_and_other_work_only_its_tier() {
        assert_eq!(
            tier_flag("critical"),
            ["--tier", "critical", "--purpose", "hard-problem"]
        );
        assert_eq!(tier_flag("light"), ["--tier", "light"]);
        assert_eq!(tier_flag("complex"), ["--tier", "complex"]);
    }

    #[test]
    fn a_member_is_the_agent_of_its_harnesss_cheap_model_in_its_tier_and_may_have_another_id() {
        let devin = member(Harness::Devin);
        assert_eq!(devin["id"], "live-devin");
        assert_eq!(devin["workTier"], "critical");
        assert_eq!(devin["effort"], "medium");
        let reviewer = member_as(Harness::Devin, "bench-reviewer");
        assert_eq!(reviewer["id"], "bench-reviewer");
        assert_eq!(
            [&reviewer["kind"], &reviewer["model"]],
            [&devin["kind"], &devin["model"]]
        );
    }

    #[test]
    fn a_fake_model_is_the_stand_ins_and_no_harness_of_the_machine() {
        assert_eq!(harness_of(&stand_in_chief()), None);
        assert_eq!(harness_of(&stand_in_worker()), None);
        assert_eq!(harness_of(&member(Harness::Claude)), Some(Harness::Claude));
        assert_eq!(harness_of(&member(Harness::Pi)), Some(Harness::Pi));
        // A Claude agent on a model that only starts with fake is the real Claude's.
        let row = json!({ "id": "x", "kind": "claude-code", "model": "fake-2" });
        assert_eq!(harness_of(&row), Some(Harness::Claude));
        assert_eq!(harness_of(&json!({ "id": "y", "kind": "nothing" })), None);
    }

    #[test]
    fn a_plan_needs_each_real_harness_of_its_roster_once_and_none_for_the_stand_ins() {
        let alone = Plan::new("door-fake", stand_in_chief()).with(stand_in_worker());
        assert!(alone.harnesses().is_empty());
        assert_eq!(alone.chief, "chief");

        let bench = Plan::new("bench", member_as(Harness::OpenCode, "bench-chief"))
            .with(member(Harness::OpenCode))
            .with(member(Harness::Pi))
            .with(member_as(Harness::Pi, "bench-reviewer"))
            .with(stand_in_worker());
        assert_eq!(bench.harnesses(), [Harness::OpenCode, Harness::Pi]);
        assert_eq!(bench.chief, "bench-chief");
        assert_eq!(bench.roster[0]["id"], "bench-chief");
    }

    #[test]
    fn a_plan_has_the_brief_and_the_files_it_was_given() {
        let plan = Plan::new("p", stand_in_chief());
        assert_eq!((plan.brief, plan.files.len()), (BRIEF, 0));
        let plan = plan
            .told(BENCH_BRIEF)
            .with_file(".claude/settings.local.json", "{}");
        assert_eq!(plan.brief, BENCH_BRIEF);
        assert_eq!(
            plan.files,
            [(".claude/settings.local.json".to_owned(), "{}".to_owned())]
        );
    }

    #[test]
    fn a_plan_has_the_variables_it_was_given_for_the_rig_in_the_order_given() {
        let plan = Plan::new("p", stand_in_chief());
        assert!(plan.vars.is_empty());
        let plan = plan
            .with_var("CF_TEST_WIND_DOWN_MS", "60000")
            .with_var("CF_TEST_ANSWER_MS", String::from("5"));
        assert_eq!(
            plan.vars,
            [
                ("CF_TEST_WIND_DOWN_MS".to_owned(), "60000".to_owned()),
                ("CF_TEST_ANSWER_MS".to_owned(), "5".to_owned())
            ]
        );
    }

    #[test]
    fn a_member_with_a_tool_for_it_is_told_to_ask_with_that_tool_and_pi_has_none() {
        let claude = question_brief(Harness::Claude).unwrap();
        assert!(claude.starts_with("Use your AskUserQuestion tool to ask which colour"));
        assert!(claude.contains("Do not pick one yourself: ask, and wait for the answer."));
        assert!(claude.ends_with("reply with exactly one line: COLOUR=<the answer>"));
        for (harness, tool) in [
            (Harness::OpenCode, "question tool"),
            (Harness::Codex, "request_user_input tool"),
            (Harness::Devin, "ask_user_question tool"),
        ] {
            let brief = question_brief(harness).unwrap();
            assert!(
                brief.starts_with(&format!("Use your {tool} to ask")),
                "{brief}"
            );
        }
        assert_eq!(question_brief(Harness::Pi), None);
    }

    #[test]
    fn the_stand_in_asks_the_same_question_through_its_hook() {
        let brief = stand_in_question_brief();
        let questions: Value = serde_json::from_str(brief.strip_prefix("ASK ").unwrap()).unwrap();
        assert_eq!(questions[0]["question"], "Which colour?");
        assert_eq!(questions[0]["options"][0]["label"], "red");
        assert_eq!(questions[0]["options"][1]["label"], "blue");
        assert_eq!(questions[0]["multiSelect"], false);
    }
}
