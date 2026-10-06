//! `cf agent add|list|edit|remove` (`agentVerb` and `resolveAdd`,
//! `bin/cf.mjs`): the agents of the human's own, in the roster's file, over
//! the catalog's. A catalog agent is the catalog's, never added again, edited
//! or removed; a refusal says why and writes nothing.

use std::io::Write;

use cf_base::args::{self, Opt, Parsed, Positionals};
use cf_base::env::Env;
use cf_base::js;
use cf_base::text::pad_end;
use cf_base::time::SystemClock;
use cf_catalog::{validate_work_tier, AgentView, Catalog, Roster};
use serde_json::{json, Map, Value};

use super::{bundled, roster, to_json, Done, Stop};

/// The options every action takes, for the first of them to be wrong to say so.
const OPTIONS: [Opt; 7] = [
    Opt::text("harness"),
    Opt::text("model"),
    Opt::text("effort"),
    Opt::text("work-tier"),
    Opt::text("description"),
    Opt::flag("designer"),
    Opt::flag("json"),
];

/// What JavaScript says when a table reads the width of a field an agent of
/// the file does not have (no id, no kind, no model): the table stops there.
const NOTHING_TO_PAD: &str = "Cannot read properties of undefined (reading 'padEnd')";

/// What the first word after `agent` asks for.
#[derive(Clone, Copy)]
enum Action {
    Add,
    List,
    Edit,
    Remove,
}

impl Action {
    fn of(word: Option<&str>) -> Option<Self> {
        match word? {
            "add" => Some(Self::Add),
            "list" => Some(Self::List),
            "edit" => Some(Self::Edit),
            "remove" => Some(Self::Remove),
            _ => None,
        }
    }
}

pub(super) fn run(env: &Env, words: &[String], out: &mut dyn Write) -> Done {
    // The options are read before the action is known, so the first of the
    // two to be wrong is what is said.
    let parsed = args::parse(
        words.get(1..).unwrap_or_default(),
        &OPTIONS,
        Positionals::Allowed,
    )
    .map_err(Stop::Said)?;
    let name = parsed.positionals.first().map(String::as_str);
    let Some(action) = Action::of(words.first().map(String::as_str)) else {
        return Err(Stop::Said(
            "usage: cf agent add|list|edit|remove".to_owned(),
        ));
    };
    let catalog = bundled()?;
    match action {
        Action::Add => add(env, &catalog, name, &parsed, out),
        Action::List => list(env, &catalog, &parsed, out),
        Action::Edit => edit(env, &catalog, name, &parsed, out),
        Action::Remove => remove(env, &catalog, name, out),
    }
}

/// A text as a JavaScript template prints it: `undefined` for what is not there.
fn shown(text: Option<&str>) -> &str {
    text.unwrap_or("undefined")
}

/// `name  harness  model`, what an add and an edit say of the agent.
fn said(view: &AgentView) -> String {
    format!(
        "{}  {}  {}",
        shown(view.name.as_deref()),
        shown(view.harness.as_deref()),
        shown(view.model.as_deref())
    )
}

/// An agent of the human's own: a name, a harness and a model (`resolveAdd`).
/// A catalog agent is in the roster already, so its name is refused; the rest
/// is the roster's to check, in its order.
fn add(
    env: &Env,
    catalog: &Catalog,
    name: Option<&str>,
    parsed: &Parsed,
    out: &mut dyn Write,
) -> Done {
    if let Some(name) = name.filter(|name| catalog.entry(name).is_some()) {
        return Err(Stop::Said(format!(
            "{name} is a catalog agent, in your agents already: pick another name for your own"
        )));
    }
    let (Some(harness), Some(model)) = (parsed.text("harness"), parsed.text("model")) else {
        return Err(Stop::Said(format!(
            "{} needs --harness and --model (see `cf catalog` for the ready-made ones)",
            shown(name)
        )));
    };
    let mut input = Map::new();
    if let Some(name) = name {
        input.insert("name".to_owned(), Value::from(name));
    }
    input.insert("harness".to_owned(), Value::from(harness));
    input.insert("model".to_owned(), Value::from(model));
    for key in ["effort", "description"] {
        if let Some(text) = parsed.text(key) {
            input.insert(key.to_owned(), Value::from(text));
        }
    }
    if parsed.flag("designer") {
        input.insert("designer".to_owned(), Value::Bool(true));
    }
    // `auto` is no tier: the agent is left to the tier its model suits.
    if let Some(tier) = parsed.text("work-tier").filter(|tier| *tier != "auto") {
        input.insert("workTier".to_owned(), Value::from(tier));
    }
    let added = roster(env, catalog)?.add(&input, &mut SystemClock)?;
    writeln!(out, "{}", said(&added))?;
    Ok(())
}

/// Every agent, the catalog's first, as a table (its columns as wide as
/// `padEnd` makes them, in UTF-16 units) or as JSON.
fn list(env: &Env, catalog: &Catalog, parsed: &Parsed, out: &mut dyn Write) -> Done {
    let agents = roster(env, catalog)?.list()?;
    if parsed.flag("json") {
        let text = js::stringify_indented(&json!({ "agents": to_json(&agents)? }), 2);
        writeln!(out, "{text}")?;
        return Ok(());
    }
    if agents.is_empty() {
        writeln!(
            out,
            "no agents yet — add one with `cf ui` or `cf agent add`"
        )?;
        return Ok(());
    }
    for agent in &agents {
        // A row of the file with no id, kind or model has no text to pad,
        // and stops the table after the rows before it.
        let (Some(name), Some(harness), Some(model)) = (
            agent.name.as_deref(),
            agent.harness.as_deref(),
            agent.model.as_deref(),
        ) else {
            return Err(Stop::Said(NOTHING_TO_PAD.to_owned()));
        };
        writeln!(
            out,
            "{}{}{}{}",
            pad_end(name, 14),
            pad_end(harness, 10),
            pad_end(model, 36),
            agent.effort.as_deref().unwrap_or("-")
        )?;
    }
    Ok(())
}

/// The fields the options name, changed on an agent of the human's own.
fn edit(
    env: &Env,
    catalog: &Catalog,
    name: Option<&str>,
    parsed: &Parsed,
    out: &mut dyn Write,
) -> Done {
    let mut patch = Map::new();
    for key in ["model", "effort", "description"] {
        if let Some(text) = parsed.text(key) {
            patch.insert(key.to_owned(), Value::from(text));
        }
    }
    // `auto` takes the tier off: the agent is left to the tier its model suits.
    if let Some(tier) = parsed.text("work-tier") {
        let tier = if tier == "auto" {
            Value::Null
        } else {
            Value::from(tier)
        };
        patch.insert("workTier".to_owned(), tier);
    }
    let roster = roster(env, catalog)?;
    let Some(name) = name else {
        return nobody(&roster, patch.get("workTier"));
    };
    let edited = roster.edit(name, &patch, &mut SystemClock)?;
    writeln!(out, "{}", said(&edited))?;
    Ok(())
}

/// An agent of the human's own, taken away.
fn remove(env: &Env, catalog: &Catalog, name: Option<&str>, out: &mut dyn Write) -> Done {
    let roster = roster(env, catalog)?;
    let Some(name) = name else {
        return nobody(&roster, None);
    };
    roster.remove(name)?;
    writeln!(out, "removed {name}")?;
    Ok(())
}

/// What an edit and a remove say with no name: Node looked for the agent
/// called `undefined`, which no file has, and said so, after the tier it was
/// given was looked at and the file read, as for any name. The roster is read
/// here the same way, and never searched for a name it was not given.
fn nobody(roster: &Roster<'_>, tier: Option<&Value>) -> Done {
    validate_work_tier(tier)?;
    roster.preferences()?;
    Err(Stop::Said("no agent named undefined".to_owned()))
}
