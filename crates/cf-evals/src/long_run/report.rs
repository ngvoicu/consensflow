//! What a long run keeps and says of itself: the report, as JSON and as Markdown,
//! kept beside the evals' (`evals/reports/long/`, where `eval summary` does not look:
//! a long run is no scenario), and the verdict on the console. The run fails (`FAIL`)
//! when an invariant broke (a disruption that had no effect is a break, of the tenth),
//! when a disruption it was asked to play was not played, when its time ran out before
//! its end, or when it could not be run to its end; the verdict says which. A run that
//! did not fail but holds what the watch could not judge (an ended notice the log ties
//! to no result, a window's tree it could not see) is `UNRESOLVED`, each said: missing
//! evidence never passes. Only a run with neither is `PASS`. A break the list of known
//! breaks names, by its invariant and its shape ([`super::known`]), is kept with its
//! issue and fails nothing: a run whose only breaks are known is `PASS with N known`,
//! and its status is 0, as a pass's. What it keeps and says has
//! what looks like a key or a token hidden: the breaks are kept so
//! ([`super::invariants::Keeper`]), the timeline is told so, and what a disruption
//! said is hidden here; and it is written where every text leaves the run
//! ([`super::edge::Edge::keep`]), which hides it again.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::disruptions::{Schedule, State};
use super::invariants::{Break, Filled, Unjudged, INVARIANTS};
use super::known::{self, Known, KNOWN};
use super::options::Options;
use super::staff::{Found, LeftOut};
use super::stalls::Stalls;
use super::timeline::Timeline;
use crate::clock;
use crate::plan::Staff;

/// How a run whose budget was spent ended, as its report says it.
pub const TIME_UP: &str = "time ran out";

/// The chief of the run, as it ran.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChiefKept {
    pub harness: String,
    pub model: String,
    pub effort: Option<String>,
}

/// A long run, to be kept.
pub struct Kept<'a> {
    pub options: &'a Options,
    pub chief: ChiefKept,
    pub found: &'a [Found],
    pub left: &'a [LeftOut],
    pub staff: &'a Staff,
    /// The daemon the run was on, as its start line said it.
    pub daemon: String,
    /// Where the home of the run was kept, with its ledger.
    pub home: Option<PathBuf>,
    pub started: i64,
    pub ended: i64,
    pub schedule: &'a Schedule,
    pub breaks: &'a [Break],
    /// What the watch could not judge.
    pub unresolved: &'a [Unjudged],
    /// What it could not judge at first that a later look's evidence judged: kept, and
    /// counted against nothing.
    pub filled: &'a [Filled],
    pub timeline: &'a Timeline,
    /// What the run kept of a window that stalled, and of every window at an end the
    /// project did not reach: evidence, which fails nothing.
    pub stalls: &'a Stalls,
    /// How the run ended: `the project is done`, `time ran out`…
    pub ending: String,
    /// What stopped the run before its end, if anything did.
    pub error: Option<String>,
    /// What the run did to the owner's Claude setup, or to the copy of the login its Claude Code
    /// windows ran on (refreshed: the owner's may be stale), if it did anything.
    pub login: Option<String>,
    pub log: Value,
}

impl Kept<'_> {
    /// Why the run did not pass, as the report keeps it: why it failed, then each thing
    /// the watch could not judge; nothing for a run that passed.
    pub fn why(&self) -> Vec<String> {
        let mut why = self.failures(true);
        why.extend(
            self.unresolved
                .iter()
                .map(|open| format!("unresolved ({}): {}", open.invariant, open.said)),
        );
        why
    }

    /// The entry of the list of known breaks that `found` is, if it is one.
    fn known(found: &Break) -> Option<&'static Known> {
        known::of(found, KNOWN.as_deref().unwrap_or(&[]))
    }

    /// The breaks that are none of the known ones: what fails the run.
    fn unknown(&self) -> impl Iterator<Item = &Break> {
        self.breaks
            .iter()
            .filter(|found| Self::known(found).is_none())
    }

    /// How many of the breaks are known ones.
    fn known_count(&self) -> usize {
        self.breaks.len() - self.unknown().count()
    }

    /// Why the run did not pass, as its console says it ([`Kept::failures`] without their
    /// words), then the key of each thing it could not judge. The report keeps the words
    /// ([`Kept::why`]), which may be a window's or a diagnostic's.
    fn said_why(&self) -> Vec<String> {
        let mut why = self.failures(false);
        why.extend(
            self.unresolved
                .iter()
                .map(|open| format!("unresolved ({}): {}", open.invariant, open.key)),
        );
        why
    }

    /// Why the run failed: each invariant that broke other than in a known way, a list of
    /// known breaks that could not be read, each disruption not played or played without
    /// the field's shape it is for, its time running out, and what stopped it; nothing
    /// for a run that did not fail. With `words`, as the report keeps it: why the list
    /// could not be read, why each disruption was not played, and what stopped the run
    /// (with what looks like a key or a token in it hidden); without, as the console
    /// says it.
    fn failures(&self, words: bool) -> Vec<String> {
        let mut why: Vec<String> = INVARIANTS
            .iter()
            .filter_map(|(number, name, shape)| {
                let count = self
                    .unknown()
                    .filter(|found| found.invariant == *number)
                    .count();
                (count > 0).then(|| format!("invariant {number} broke ({shape}, {name}): {count}"))
            })
            .collect();
        if let Err(why_not) = KNOWN.as_ref() {
            let list = "the list of known breaks (evals/known-breaks.json) could not be read";
            why.push(if words {
                format!("{list}: {why_not}")
            } else {
                list.to_owned()
            });
        }
        why.extend(self.schedule.not_played().iter().map(|planned| {
            let flag = planned.disruption.flag();
            match &planned.state {
                State::NotPlayed(because) if words => format!("{flag} was not played: {because}"),
                _ => format!("{flag} was not played"),
            }
        }));
        why.extend(self.schedule.without_their_shape().iter().map(|planned| {
            format!(
                "{} was played without {}",
                planned.disruption.flag(),
                planned.lacked.as_deref().unwrap_or_default()
            )
        }));
        if self.ending == TIME_UP {
            why.push(format!(
                "time ran out: the run did not reach its end within its budget of {} min",
                self.options.budget.as_secs() / 60
            ));
        }
        if let Some(login) = &self.login {
            why.push(if words {
                format!("the owner's Claude setup or login was not left as it was: {login}")
            } else {
                "the owner's Claude setup or login was not left as it was (what changed is in the report)"
                    .to_owned()
            });
        }
        if let Some(error) = self.error() {
            why.push(if words {
                format!("the run could not be run to its end: {error}")
            } else {
                "the run could not be run to its end (what stopped it is in the report)".to_owned()
            });
        }
        why
    }

    /// What stopped the run, as it is kept and said: what looks like a key or a token
    /// in it hidden (a rig that timed out says what the daemon and the host wrote).
    fn error(&self) -> Option<String> {
        self.error
            .as_deref()
            .map(|error| self.timeline.masked(error))
    }

    /// Whether the run passed: nothing failed, and nothing was left unjudged; its
    /// only breaks, if any, known ones.
    pub fn held(&self) -> bool {
        self.verdict().starts_with("PASS")
    }

    /// `FAIL` where anything failed; else `UNRESOLVED` where the watch could not
    /// judge something; else `PASS`, or `PASS with N known` where its only breaks are
    /// N known ones.
    pub fn verdict(&self) -> String {
        if !self.failures(false).is_empty() {
            "FAIL".to_owned()
        } else if !self.unresolved.is_empty() {
            "UNRESOLVED".to_owned()
        } else {
            match self.known_count() {
                0 => "PASS".to_owned(),
                known => format!("PASS with {known} known"),
            }
        }
    }

    /// The harnesses of the run with their versions, and each member's model.
    fn harnesses(&self) -> Value {
        Value::Array(
            self.found
                .iter()
                .map(|found| {
                    let members: Vec<Value> = self
                        .staff
                        .agents
                        .iter()
                        .zip(&self.staff.members)
                        .filter(|(agent, _)| agent.id.starts_with(&format!("long-{}-", found.name)))
                        .map(|(agent, member)| {
                            json!({ "agent": agent.id, "role": member.role, "model": agent.model, "tier": agent.tier })
                        })
                        .collect();
                    json!({
                        "harness": found.name,
                        "version": found.version,
                        "command": found.command,
                        "chief": found.name == self.chief.harness,
                        "members": members,
                    })
                })
                .collect(),
        )
    }

    fn disruptions(&self) -> Value {
        Value::Array(
            self.schedule
                .planned
                .iter()
                .map(|planned| {
                    let (state, said, at) = match &planned.state {
                        State::Waiting => ("waiting", String::new(), None),
                        State::Played { at, said } if planned.lacked.is_some() => (
                            "played without its shape",
                            format!("{said} ({})", planned.lacked.as_deref().unwrap_or_default()),
                            Some(at.as_secs()),
                        ),
                        State::Played { at, said } => ("played", said.clone(), Some(at.as_secs())),
                        State::Failed { at, said } => ("failed", said.clone(), Some(at.as_secs())),
                        State::Impossible(why) => ("impossible", why.clone(), None),
                        State::NotPlayed(why) => ("not played", why.clone(), None),
                    };
                    json!({
                        "disruption": planned.disruption.flag(),
                        "about": planned.disruption.about(),
                        "due": planned.due.as_secs(),
                        "state": state,
                        "playedAt": at,
                        "said": self.timeline.masked(&said),
                    })
                })
                .collect(),
        )
    }

    /// The report as it is kept.
    pub fn value(&self) -> Value {
        let options = self.options;
        json!({
            "kind": "long-run",
            "verdict": self.verdict(),
            "why": self.why(),
            "started": super::invariants::iso(self.started),
            "ended": super::invariants::iso(self.ended),
            "seconds": (self.ended - self.started).max(0) / 1000,
            "ending": self.ending,
            "error": self.error(),
            "login": self.login,
            "daemon": self.daemon,
            "home": self.home,
            "ask": options.ask.name(),
            "fixture": super::ask::FIXTURE,
            "budgetSeconds": options.budget.as_secs(),
            "everySeconds": options.every.as_secs(),
            "lookSeconds": options.look.as_secs_f64(),
            "idleMinutes": options.settings.idle / 60_000,
            "dialogMinutes": options.settings.dialog / 60_000,
            "chief": {
                "harness": self.chief.harness,
                "model": self.chief.model,
                "effort": self.chief.effort,
            },
            "staffAsked": options.staff,
            "harnesses": self.harnesses(),
            "leftOut": self.left.iter().map(|left| json!({ "harness": left.name, "why": left.why })).collect::<Vec<_>>(),
            "invariants": INVARIANTS.iter().map(|(number, name, shape)| {
                let breaks = self.unknown().filter(|found| found.invariant == *number).count();
                let known = self.breaks.iter().filter(|found| found.invariant == *number).count() - breaks;
                let unresolved = self.unresolved.iter().filter(|open| open.invariant == *number).count();
                json!({ "number": number, "name": name, "shape": shape, "held": breaks == 0 && unresolved == 0, "breaks": breaks, "known": known, "unresolved": unresolved })
            }).collect::<Vec<_>>(),
            "disruptions": self.disruptions(),
            "breaks": self.breaks.iter().map(|found| {
                let mut kept = found.to_value();
                kept["known"] = json!(Self::known(found).map(|known| &known.issue));
                kept
            }).collect::<Vec<_>>(),
            "unresolved": self.unresolved.iter().map(|open| json!({
                "invariant": open.invariant,
                "key": open.key,
                "at": super::invariants::iso(open.at),
                "said": open.said,
            })).collect::<Vec<_>>(),
            "resolved": self.filled.iter().map(|filled| json!({
                "invariant": filled.open.invariant,
                "key": filled.open.key,
                "at": super::invariants::iso(filled.open.at),
                "said": filled.open.said,
                "resolvedAt": super::invariants::iso(filled.at),
                "broke": filled.broke,
            })).collect::<Vec<_>>(),
            "stalls": self.stalls.to_value(self.timeline),
            "timeline": self.timeline.to_value(),
            "log": masked(&self.log, self.timeline),
        })
    }

    /// The report as Markdown: the verdict and why, the harnesses, the invariants,
    /// the disruptions, every break with its evidence, the stalls, and the timeline.
    pub fn markdown(&self) -> String {
        let mut text = format!(
            "# Long run of {}: {}\n\n",
            super::invariants::iso(self.started),
            self.verdict()
        );
        text.push_str(&format!(
            "{} · chief {} ({}{}) · ask {} · budget {} min, ran {} min · {}\n\n",
            self.daemon,
            self.chief.harness,
            self.chief.model,
            self.chief
                .effort
                .as_deref()
                .map(|effort| format!(", effort {effort}"))
                .unwrap_or_default(),
            self.options.ask.name(),
            self.options.budget.as_secs() / 60,
            (self.ended - self.started).max(0) / 60_000,
            self.ending
        ));
        for why in self.why() {
            text.push_str(&format!("- {why}\n"));
        }
        text.push_str("\n## Harnesses\n\n| harness | version | members |\n|---|---|---|\n");
        for harness in self.harnesses().as_array().into_iter().flatten() {
            let members: Vec<String> = harness["members"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|member| {
                    format!(
                        "{} ({}, {}, {})",
                        text_of(&member["agent"]),
                        text_of(&member["role"]),
                        text_of(&member["model"]),
                        text_of(&member["tier"])
                    )
                })
                .collect();
            text.push_str(&format!(
                "| {}{} | {} | {} |\n",
                text_of(&harness["harness"]),
                if harness["chief"] == true {
                    " (chief)"
                } else {
                    ""
                },
                text_of(&harness["version"]),
                members.join("<br>")
            ));
        }
        for left in self.left {
            text.push_str(&format!("\nLeft out: {} ({})\n", left.name, left.why));
        }
        text.push_str(
            "\n## Invariants\n\n| # | invariant | shape | held | breaks |\n|---|---|---|---|---|\n",
        );
        for (number, name, shape) in INVARIANTS {
            let breaks = self
                .unknown()
                .filter(|found| found.invariant == number)
                .count();
            let known = self
                .breaks
                .iter()
                .filter(|found| found.invariant == number)
                .count()
                - breaks;
            let unresolved = self
                .unresolved
                .iter()
                .filter(|open| open.invariant == number)
                .count();
            text.push_str(&format!(
                "| {number} | {name} | {shape} | {} | {} |\n",
                match (breaks, unresolved, known) {
                    (0, 0, 0) => "yes".to_owned(),
                    (0, 0, known) => format!("known ({known})"),
                    (0, unresolved, _) => format!("**unresolved** ({unresolved})"),
                    _ => "**no**".to_owned(),
                },
                breaks + known
            ));
        }
        text.push_str(
            "\n## Disruptions\n\n| disruption | due | what came of it |\n|---|---|---|\n",
        );
        for planned in self.disruptions().as_array().into_iter().flatten() {
            text.push_str(&format!(
                "| {} | +{} min | {}{} |\n",
                text_of(&planned["disruption"]),
                planned["due"].as_u64().unwrap_or_default() / 60,
                text_of(&planned["state"]),
                match text_of(&planned["said"]) {
                    said if said.is_empty() => String::new(),
                    said => format!(": {said}"),
                }
            ));
        }
        text.push_str("\n## Breaks\n");
        if self.breaks.is_empty() {
            text.push_str("\nNone.\n");
        }
        for found in self.breaks {
            text.push_str(&format!(
                "\n### {} · {} ({}){}\n\n{}\n",
                found.invariant,
                found.name(),
                self.timeline.offset(found.at),
                Self::known(found)
                    .map(|known| format!(" · KNOWN: {}", known.issue))
                    .unwrap_or_default(),
                found.said
            ));
            for (title, lines) in [
                ("rows", &found.evidence.rows),
                ("record", &found.evidence.record),
                ("screen", &found.evidence.screen),
            ] {
                if !lines.is_empty() {
                    text.push_str(&format!("\n{title}:\n\n```\n{}\n```\n", lines.join("\n")));
                }
            }
        }
        if !self.unresolved.is_empty() {
            text.push_str("\n## Unresolved\n\n");
            for open in self.unresolved {
                text.push_str(&format!(
                    "- {} · {} ({})\n",
                    open.invariant,
                    open.said,
                    self.timeline.offset(open.at)
                ));
            }
        }
        text.push_str(&self.stalls.markdown(self.timeline));
        text.push_str("\n## Timeline\n\n");
        text.push_str(&self.timeline.markdown());
        text
    }

    /// The verdict, as the console says it: a line of what ran and the verdict, the
    /// harnesses left out, why it did not pass ([`Kept::said_why`]), the known breaks by
    /// their keys and issues, and where the report is. Nothing of a window, a record, a
    /// message or a diagnostic: the console leaves the machine with whatever ran the
    /// run, and the report keeps those.
    pub fn say(&self, json: &Path, markdown: &Path) -> Vec<String> {
        let staff: Vec<&str> = self.found.iter().map(|found| found.name.as_str()).collect();
        let mut lines = vec![format!(
            "long run · chief {} ({}) · staff {} · {} min · {}",
            self.chief.harness,
            self.chief.model,
            staff.join(","),
            (self.ended - self.started).max(0) / 60_000,
            self.verdict()
        )];
        lines.extend(self.left.iter().map(|left| format!("  {}", left.said())));
        lines.extend(
            self.said_why()
                .into_iter()
                .map(|why| format!("  why: {why}")),
        );
        lines.extend(self.breaks.iter().filter_map(|found| {
            Self::known(found).map(|known| format!("  known: {} ({})", found.key, known.issue))
        }));
        lines.push(format!("  report: {}", json.display()));
        lines.push(format!("  timeline: {}", markdown.display()));
        lines
    }
}

/// `value` with what looks like a key or a token hidden in each of its texts, the
/// run's own names (the `timeline`'s) shown: the run's log, which says what a
/// disruption did and what the owner typed.
fn masked(value: &Value, timeline: &Timeline) -> Value {
    match value {
        Value::String(text) => Value::String(timeline.masked(text)),
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| masked(item, timeline)).collect())
        }
        Value::Object(fields) => Value::Object(
            fields
                .iter()
                .map(|(name, field)| (name.clone(), masked(field, timeline)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// A text value as it reads, or nothing.
fn text_of(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// What a long run's report is named by: when it began, its chief and its staff.
pub fn file_name(started: i64, chief: &str, staff: &[String]) -> String {
    let at = jiff::Timestamp::from_millisecond(started).unwrap_or(jiff::Timestamp::UNIX_EPOCH);
    format!(
        "{}-long-chief-{chief}-staff-{}",
        clock::stamp(at),
        staff.join("-")
    )
}

#[cfg(test)]
mod tests;
