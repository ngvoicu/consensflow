//! Live: does a catalog agent's window open on its model and answer? It is
//! opt-in (`cargo xtask live agent`, `npm run live:agent`): it spends the
//! machine's quota on a window of each agent named, and needs Claude Code
//! installed and logged in, so it is ignored unless asked for and is no part of
//! `check` or of CI.
//!
//! Each Claude Code agent named opens as the app opens it: the row the launcher
//! runs for it (`cf agent list`, its model and effort as the catalog has them),
//! in a window of the pane host of a rig, on real Claude Code with no MCP
//! servers, connectors or browser (`--strict-mcp-config --no-chrome`) and
//! without `CLAUDE_CONFIG_DIR`, in a folder of its own under the user's home. It
//! is asked for a sum its message does not hold, so the answer on the screen
//! means the window took the message; Claude Code's own record of the session
//! must then name the agent's model as the one that answered, and no other.
//! `--effort` opens the agent at each level named instead of its own: the levels
//! a row of that model could take.
//!
//! ```text
//! cargo xtask live agent                          huginn, Haiku 5.5 at its own level
//! cargo xtask live agent --agent huginn --agent hermod
//! cargo xtask live agent --agent huginn --effort low --effort medium --effort high \
//!     --effort xhigh --effort max
//! ```
//!
//! They run one after another; the run fails when any window did not open, did
//! not answer, or was answered by another model.

use std::time::Duration;

use cf_e2e::live::claude::{self, Entry};
use cf_e2e::live::report::{self, Check};
use cf_e2e::live::window::{Open, Window};
use cf_e2e::live::{self, ask, Harness, Log};
use cf_e2e::rig::Rig;
use cf_e2e::{Error, Result, ScratchHome};
use serde_json::{json, Value};

/// The stand-in for Claude, which the rig puts first on the `PATH` of its windows:
/// no window of this test runs it (each is opened on the real harness's own
/// path), but a rig has one.
const FAKE_AGENT: &str = env!("CARGO_BIN_EXE_fake-agent");

/// What the check is called in the report.
const WHAT: &str = "opens and answers as its model";

/// The agent that is run when none is named.
const DEFAULT_AGENT: &str = "huginn";

/// The sum a window is asked for, which its message does not hold.
const SUM: (u32, u32) = (1234, 4321);

/// What Claude Code writes as the model of an answer it wrote itself (a refusal,
/// the notice of an interrupt): the record of no model's answer.
const SYNTHETIC: &str = "<synthetic>";

/// How long Claude Code's record is waited for to hold an answer once the screen
/// shows it, and how often it is looked at.
const RECORDED: Duration = Duration::from_secs(10);
const LOOK: Duration = Duration::from_millis(500);

/// What a window is asked, and the answer that shows it took the message.
struct Ask {
    message: String,
    answer: String,
}

impl Ask {
    fn sum((a, b): (u32, u32)) -> Self {
        Self {
            message: format!(
                "Reply with only the sum of {a} and {b}, in digits, and run no tools."
            ),
            answer: (a + b).to_string(),
        }
    }
}

/// The catalog's row for an agent, as the launcher runs it: its model and effort.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    id: String,
    model: String,
    effort: Option<String>,
}

/// The agents the human sees in a home that holds nothing of theirs (`cf agent
/// list --json`): the catalog's.
fn catalog() -> Result<Value> {
    let home = ScratchHome::new()?;
    let ran = home.cf(["agent", "list", "--json"])?;
    if ran.code != Some(0) {
        return Err(Error::Daemon(format!(
            "cf agent list did not succeed: {ran}"
        )));
    }
    Ok(ran.json()?["agents"].clone())
}

/// The row of the catalog's agent `name` (with or without the `@` that names an
/// agent in a message), which must run on Claude Code: this check opens Claude
/// Code agents.
fn row_of(agents: &Value, name: &str) -> Result<Row> {
    let wanted = name.strip_prefix('@').unwrap_or(name);
    let agent = agents
        .as_array()
        .and_then(|agents| agents.iter().find(|agent| agent["name"] == wanted))
        .ok_or_else(|| Error::Daemon(format!("{name} is no agent of the catalog")))?;
    let harness = agent["harness"].as_str().unwrap_or_default();
    if harness != "claude" {
        return Err(Error::Daemon(format!(
            "{name} runs on {harness}: this check opens Claude Code agents"
        )));
    }
    let text = |field: &str| agent[field].as_str().map(str::to_owned);
    Ok(Row {
        id: wanted.to_owned(),
        model: text("model").ok_or_else(|| Error::Daemon(format!("{name} has no model")))?,
        effort: text("effort").filter(|effort| !effort.is_empty()),
    })
}

/// One window to open: an agent, at an effort (its own, where none is named).
struct Opening<'a> {
    row: &'a Row,
    effort: Option<String>,
}

impl Opening<'_> {
    fn effort(&self) -> &str {
        self.effort.as_deref().unwrap_or("default")
    }

    /// How the window is told apart in the report: `huginn low`, `huginn default`.
    fn who(&self) -> String {
        format!("{} {}", self.row.id, self.effort())
    }

    /// The id of its pane.
    fn pane(&self) -> String {
        format!("agent-{}-{}", self.row.id, self.effort())
    }

    /// The agent the window runs: the row's kind and model, and the effort where there is one.
    fn agent(&self) -> Value {
        let mut agent = json!({ "kind": Harness::Claude.kind(), "model": self.row.model });
        if let (Some(effort), Some(fields)) = (&self.effort, agent.as_object_mut()) {
            fields.insert("effort".to_owned(), json!(effort));
        }
        agent
    }
}

/// The windows to open: for each agent in turn, one at its own effort, or one at
/// each effort asked for.
fn openings<'a>(rows: &'a [Row], efforts: &[String]) -> Vec<Opening<'a>> {
    rows.iter()
        .flat_map(|row| {
            let levels: Vec<Option<String>> = if efforts.is_empty() {
                vec![row.effort.clone()]
            } else {
                efforts.iter().cloned().map(Some).collect()
            };
            levels
                .into_iter()
                .map(move |effort| Opening { row, effort })
        })
        .collect()
}

/// The models Claude Code recorded as answering in a session, in the order they
/// first appear: those of the assistant's records, but for the answers Claude
/// wrote itself.
fn answered_by(record: &[Entry]) -> Vec<String> {
    let mut models: Vec<String> = Vec::new();
    for model in record
        .iter()
        .filter(|entry| entry.kind == "assistant")
        .filter_map(|entry| entry.model.as_deref())
        .filter(|model| *model != SYNTHETIC)
    {
        if !models.iter().any(|seen| seen == model) {
            models.push(model.to_owned());
        }
    }
    models
}

/// What the models of the record say of the answer: the agent's model, and no
/// other, answered.
fn outcome(who: &str, model: &str, models: &[String], seconds: &str) -> Check {
    let ok = models == [model];
    let detail = if ok {
        format!("answered in {seconds} s, as {model}")
    } else {
        let named = if models.is_empty() {
            "no model".to_owned()
        } else {
            models.join(", ")
        };
        format!("answered, but its record names {named}, not {model}")
    };
    Check::new(who, WHAT, ok, detail)
}

/// Has the open `window` of `opening` answer `asked`, and says whose answer
/// Claude Code's record holds it to be.
fn check_of(window: &Window, opening: &Opening, asked: &Ask) -> Result<Check> {
    let who = opening.who();
    if let Err(screen) = window.ready() {
        return Ok(Check::failed(&who, WHAT, format!("did not open: {screen}")));
    }
    let sent = window.send(&window.given(&asked.message)?, &asked.answer)?;
    if !sent.shown {
        let detail = format!("NOT ANSWERED: {}", window.tail());
        return Ok(Check::failed(&who, WHAT, detail));
    }
    let session = window.session().ok_or_else(|| {
        Error::Daemon("a Claude Code window is opened on a conversation of its own".to_owned())
    })?;
    let config = window.claude_config();
    let models = live::until(RECORDED, LOOK, || {
        let models = answered_by(&claude::record(&config, &session));
        (!models.is_empty()).then_some(models)
    })
    .unwrap_or_default();
    Ok(outcome(&who, &opening.row.model, &models, &sent.seconds))
}

#[test]
#[ignore = "opt-in: spends the machine's quota on a window of each agent named; cargo xtask live agent (npm run live:agent)"]
fn each_catalog_agent_opens_on_its_model_and_its_record_says_it_answered() -> Result {
    let log = Log::own_agents(
        "live agent",
        "name the catalog's agents with --agent, and their levels with --effort",
    )?;
    // What a window is opened on is the harness code's to say: built before anything is spent.
    ask::prepare()?;
    let named = live::list("agent");
    let named = if named.is_empty() {
        vec![DEFAULT_AGENT.to_owned()]
    } else {
        named
    };
    let agents = catalog()?;
    let rows = named
        .iter()
        .map(|name| row_of(&agents, name))
        .collect::<Result<Vec<_>>>()?;
    let claude = log.found(Harness::Claude)?;
    let rig = Rig::start(live::rig(std::slice::from_ref(&claude), FAKE_AGENT))?;
    let asked = Ask::sum(SUM);
    let mut checks = Vec::new();
    for opening in openings(&rows, &live::list("effort")) {
        let window = Open::new(&rig, &claude, "agent", &opening.pane())
            .agent(opening.agent())
            .open_telling(&log, &opening.who())?;
        checks.push(check_of(&window, &opening, &asked)?);
    }
    rig.close()?;
    report::conclude(&log, "what each check came to", &checks)
}

#[cfg(test)]
mod tests {
    use cf_e2e::live::Installed;
    use cf_e2e::rig::Config;

    use super::*;

    fn listing() -> Value {
        json!([
            { "name": "huginn", "harness": "claude", "model": "claude-haiku-5-5", "effort": "max" },
            { "name": "hermod", "harness": "claude", "model": "claude-sonnet-5-5" },
            { "name": "hyperion", "harness": "codex", "model": "gpt-6.1-sol", "effort": "max" },
        ])
    }

    fn row(id: &str, model: &str, effort: Option<&str>) -> Row {
        Row {
            id: id.to_owned(),
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
        }
    }

    fn entry(kind: &str, model: Option<&str>) -> Entry {
        Entry {
            at_ms: None,
            kind: kind.to_owned(),
            text: String::new(),
            model: model.map(str::to_owned),
            raw: String::new(),
        }
    }

    #[test]
    fn the_message_asks_for_a_sum_it_does_not_hold() {
        let ask = Ask::sum(SUM);
        assert_eq!(
            ask.message,
            "Reply with only the sum of 1234 and 4321, in digits, and run no tools."
        );
        assert_eq!(ask.answer, "5555");
        assert!(!ask.message.contains(&ask.answer));
    }

    #[test]
    fn a_row_is_the_catalogs_model_and_effort_for_an_agent_named_with_or_without_its_at_sign() {
        let agents = listing();
        let huginn = row("huginn", "claude-haiku-5-5", Some("max"));
        assert_eq!(row_of(&agents, "huginn").unwrap(), huginn);
        assert_eq!(row_of(&agents, "@huginn").unwrap(), huginn);
        // An agent the catalog gives no effort runs at the harness's own.
        assert_eq!(
            row_of(&agents, "hermod").unwrap(),
            row("hermod", "claude-sonnet-5-5", None)
        );
    }

    #[test]
    fn an_agent_that_is_not_in_the_catalog_or_does_not_run_on_claude_code_is_refused() {
        let agents = listing();
        assert_eq!(
            row_of(&agents, "@nemo").unwrap_err().to_string(),
            "@nemo is no agent of the catalog"
        );
        assert_eq!(
            row_of(&agents, "hyperion").unwrap_err().to_string(),
            "hyperion runs on codex: this check opens Claude Code agents"
        );
        assert!(row_of(&json!(null), "huginn").is_err());
    }

    #[test]
    fn the_catalog_has_huginn_on_claude_code_and_hyperion_on_codex() -> Result {
        // The real catalog, as `cf agent list --json` says it in a home of nobody's.
        let agents = catalog()?;
        let huginn = row_of(&agents, DEFAULT_AGENT)?;
        assert_eq!(huginn.model, "claude-haiku-5-5");
        assert!(huginn.effort.is_some(), "{huginn:?}");
        let refused = row_of(&agents, "hyperion").unwrap_err().to_string();
        assert!(refused.starts_with("hyperion runs on codex"), "{refused}");
        Ok(())
    }

    #[test]
    fn an_agent_opens_at_its_own_effort_or_at_each_effort_asked_for_and_each_window_is_told_apart()
    {
        let rows = [
            row("huginn", "claude-haiku-5-5", Some("max")),
            row("hermod", "claude-sonnet-5-5", None),
        ];
        let who = |efforts: &[&str]| -> Vec<(String, String)> {
            let efforts: Vec<String> = efforts.iter().map(|effort| (*effort).to_owned()).collect();
            openings(&rows, &efforts)
                .iter()
                .map(|opening| (opening.who(), opening.pane()))
                .collect()
        };
        assert_eq!(
            who(&[]),
            [
                ("huginn max".to_owned(), "agent-huginn-max".to_owned()),
                (
                    "hermod default".to_owned(),
                    "agent-hermod-default".to_owned()
                ),
            ]
        );
        // The levels named replace the agent's own, for each agent in turn.
        let named: Vec<String> = who(&["low", "high"])
            .into_iter()
            .map(|(who, _)| who)
            .collect();
        assert_eq!(
            named,
            ["huginn low", "huginn high", "hermod low", "hermod high"]
        );
    }

    #[test]
    fn the_agent_a_window_runs_is_the_rows_kind_and_model_with_an_effort_only_where_there_is_one() {
        let huginn = row("huginn", "claude-haiku-5-5", Some("max"));
        let opening = |effort: Option<&str>| Opening {
            row: &huginn,
            effort: effort.map(str::to_owned),
        };
        assert_eq!(
            opening(Some("low")).agent(),
            json!({ "kind": "claude-code", "model": "claude-haiku-5-5", "effort": "low" })
        );
        assert_eq!(
            opening(None).agent(),
            json!({ "kind": "claude-code", "model": "claude-haiku-5-5" })
        );
    }

    #[test]
    fn the_models_that_answered_are_the_assistants_apart_from_the_answers_claude_wrote_itself() {
        let record = [
            entry("user", Some("a-user-has-no-model")),
            entry("assistant", Some("claude-haiku-5-5")),
            entry("assistant", Some(SYNTHETIC)),
            entry("assistant", None),
            entry("system/turn_duration", Some("claude-fable-5-1")),
            entry("assistant", Some("claude-haiku-5-5")),
            entry("assistant", Some("claude-sonnet-5-5")),
        ];
        assert_eq!(
            answered_by(&record),
            ["claude-haiku-5-5", "claude-sonnet-5-5"]
        );
        // No answer yet: nothing, which is waited on.
        assert!(answered_by(&[entry("user", None)]).is_empty());
        assert!(answered_by(&[]).is_empty());
    }

    #[test]
    fn it_passes_when_one_model_answered_and_it_is_the_agents_and_not_for_any_other_record() {
        let said = |models: &[&str]| -> Check {
            let models: Vec<String> = models.iter().map(|model| (*model).to_owned()).collect();
            outcome("huginn low", "claude-haiku-5-5", &models, "6.4")
        };
        assert_eq!(
            said(&["claude-haiku-5-5"]),
            Check::passed("huginn low", WHAT, "answered in 6.4 s, as claude-haiku-5-5")
        );
        for (models, named) in [
            (&["claude-sonnet-5-5"][..], "claude-sonnet-5-5"),
            (
                &["claude-haiku-5-5", "claude-sonnet-5-5"],
                "claude-haiku-5-5, claude-sonnet-5-5",
            ),
            (&[], "no model"),
        ] {
            let check = said(models);
            assert!(!check.ok, "{check:?}");
            assert_eq!(
                check.detail,
                format!("answered, but its record names {named}, not claude-haiku-5-5")
            );
        }
    }

    /// What the stand-in agent answers: a message that says `Reply with exactly:
    /// <word>` with the word.
    fn stand_in_ask() -> Ask {
        Ask {
            message: "Reply with exactly: 5555".to_owned(),
            answer: "5555".to_owned(),
        }
    }

    #[test]
    fn a_window_opened_on_the_agents_model_and_effort_answers_and_its_record_is_read_for_the_model(
    ) -> Result {
        let rig = Rig::start(Config::new(FAKE_AGENT))?;
        let home = rig.var("HOME").unwrap_or_default().to_owned();
        let install = Installed::at(Harness::Claude, FAKE_AGENT, &home);
        let huginn = row("huginn", "claude-haiku-5-5", Some("max"));
        let opening = Opening {
            row: &huginn,
            effort: Some("low".to_owned()),
        };
        let window = Open::new(&rig, &install, "agent", &opening.pane())
            .agent(opening.agent())
            .without_trust()
            .open(&Log::new("live agent test"))?;
        let argv = window.argv();
        for flag in [["--model", "claude-haiku-5-5"], ["--effort", "low"]] {
            assert!(
                argv.windows(2).any(|pair| pair == flag),
                "{flag:?} is not in {argv:?}"
            );
        }
        // The stand-in answers, and writes no model in its record: which the check says.
        let check = check_of(&window, &opening, &stand_in_ask())?;
        assert_eq!(check.who, "huginn low");
        assert!(!check.ok, "{check:?}");
        assert_eq!(
            check.detail,
            "answered, but its record names no model, not claude-haiku-5-5"
        );
        drop(window);
        rig.close()
    }
}
