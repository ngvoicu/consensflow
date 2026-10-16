//! What Devin's wire log says: a JSON record a line of what the stock TUI and its agent
//! tell each other, of which the agent's are in it (the config of the session, the mode it
//! is in and the one it changes to, a tool call that failed or that the user skipped) and
//! little of the TUI's own. A record is read against what the ones before it said: the
//! command a tool call is, a permission asked and not answered yet, the config and the
//! mode the session was last said to have.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use cf_e2e::live::members::squash;
use serde_json::Value;

use crate::trail::plus;
use crate::watch::{in_text, Said, EXCERPT};
use crate::words::SKIPPED;

/// What the wire's records have said so far, which the next one is read against: the
/// permission requests not answered yet (by the id of the request), what each tool call
/// is (the command of a shell's, by the id of the call), and the config and the mode the
/// session was last said to have.
#[derive(Debug, Default)]
pub struct WireState {
    asked: HashSet<String>,
    calls: HashMap<String, String>,
    config: Option<String>,
    mode: Option<String>,
    /// How many prompts the TUI put to its agent: each a turn begun.
    pub prompts: usize,
}

/// What a record of the wire says.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Wired {
    /// Words that say a call did not run, or a permission was asked or answered.
    pub said: Vec<Said>,
    /// What it says of the session's config or of its mode, where that is a change.
    pub config: Option<String>,
    /// The mode it says the session is in, whether or not that is a change.
    pub mode: Option<String>,
}

impl WireState {
    /// What a `config_option_update` says: each option as `id=value`, whatever Devin
    /// calls them, and the mode among them.
    fn configured(&mut self, update: &Value, wired: &mut Wired) {
        let options = update["configOptions"].as_array();
        let mut said = Vec::new();
        for option in options.into_iter().flatten() {
            let value = match &option["currentValue"] {
                Value::String(text) if !text.is_empty() => text.clone(),
                Value::Number(number) => number.to_string(),
                Value::Bool(flag) => flag.to_string(),
                _ => continue,
            };
            let id = option["id"].as_str().unwrap_or("?");
            if id == "mode" {
                wired.mode = Some(value.clone());
            }
            said.push(format!("{id}={value}"));
        }
        let said = said.join(" ");
        if !said.is_empty() && self.config.as_deref() != Some(said.as_str()) {
            self.config = Some(said.clone());
            wired.config = Some(said);
        }
    }

    /// What a `current_mode_update` says.
    fn moded(&mut self, update: &Value, wired: &mut Wired) {
        let Some(mode) = update["currentModeId"].as_str() else {
            return;
        };
        wired.mode = Some(mode.to_owned());
        if self.mode.as_deref() != Some(mode) {
            self.mode = Some(mode.to_owned());
            wired.config = Some(format!("mode now {mode}"));
        }
    }

    /// What a tool call is, as its records say it: the command of a shell's, which grows
    /// as the model writes it.
    fn noted(&mut self, update: &Value) {
        let (Some(id), Some(command)) = (
            update["toolCallId"].as_str(),
            update["rawInput"]["command"].as_str(),
        ) else {
            return;
        };
        if !command.is_empty() {
            self.calls.insert(id.to_owned(), command.to_owned());
        }
    }

    /// A tool call that failed: one the user skipped (Devin's `rejected` mark on it), or
    /// one that failed by itself.
    fn failed(&self, update: &Value) -> Option<Said> {
        if update["status"] != "failed" {
            return None;
        }
        let words = update["content"][0]["content"]["text"]
            .as_str()
            .unwrap_or_default();
        let tool = update["_meta"]["cognition.ai/inferenceToolName"]
            .as_str()
            .unwrap_or("a tool");
        let command = update["toolCallId"]
            .as_str()
            .and_then(|id| self.calls.get(id))
            .map_or_else(String::new, |command| format!(" {command}"));
        let saw = if update["_meta"]["cognition.ai/rejected"] == true {
            if words.contains(SKIPPED) {
                SKIPPED
            } else {
                "rejected call"
            }
        } else {
            "failed call"
        };
        Some((
            saw.to_owned(),
            squash(&format!("{tool}{command}: {words}"), EXCERPT),
        ))
    }
}

/// What a record of the wire says: the config and the mode of the session, a tool call
/// that failed or was rejected, the markers wherever it holds them, a request for a
/// permission (its method names one) and the answer to a request that was asked (a
/// record of the same id with a result or an error). A line that is no JSON is read for
/// the markers alone.
pub fn in_wire(line: &str, state: &mut WireState) -> Wired {
    let mut wired = Wired::default();
    let Ok(record) = serde_json::from_str::<Value>(line) else {
        wired.said = in_text(line);
        return wired;
    };
    let update = &record["update"];
    let mut structured = false;
    match update["sessionUpdate"].as_str() {
        Some("config_option_update") => state.configured(update, &mut wired),
        Some("current_mode_update") => state.moded(update, &mut wired),
        Some("tool_call" | "tool_call_update") => {
            state.noted(update);
            if let Some(failed) = state.failed(update) {
                wired.said.push(failed);
                structured = true;
            }
        }
        _ => {}
    }
    if !structured {
        wired.said.extend(in_text(line));
    }
    if record["method"] == "session/prompt" {
        state.prompts += 1;
    }
    let id = record.get("id").map(Value::to_string);
    let asks = record["method"]
        .as_str()
        .is_some_and(|method| method.to_ascii_lowercase().contains("permission"));
    if asks {
        state.asked.extend(id);
        wired
            .said
            .push(("permission request".to_owned(), squash(line, EXCERPT)));
    } else if let Some(id) = id.filter(|id| state.asked.contains(id)) {
        if let Some(answer) = record.get("result").or_else(|| record.get("error")) {
            state.asked.remove(&id);
            wired.said.push((
                "permission answered".to_owned(),
                squash(&answer.to_string(), EXCERPT),
            ));
        }
    }
    wired
}

/// What the wire said of the session's config and mode, as it changed, and when.
#[derive(Debug, Default)]
pub struct Modes(Vec<(Duration, String)>);

impl Modes {
    /// Notes what the wire said at `at`.
    pub fn note(&mut self, at: Duration, said: String) {
        self.0.push((at, said));
    }

    /// How many changes there are: a mark to ask what came after ([`Modes::since`]).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The changes after the first `from`, as a line, each with its time.
    pub fn since(&self, from: usize) -> String {
        let said: Vec<String> = self.0[from.min(self.0.len())..]
            .iter()
            .map(|(at, said)| format!("{said} {}", plus(*at)))
            .collect();
        if said.is_empty() {
            "no word of a config or a mode".to_owned()
        } else {
            said.join(", ")
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_wire_says_a_permission_was_asked_and_how_it_was_answered_by_the_id_of_the_request() {
        let mut state = WireState::default();
        let request = json!({
            "id": 7, "method": "session/request_permission",
            "params": { "toolCall": { "title": "run_command" },
                        "options": [{ "optionId": "allow" }, { "optionId": "reject" }] },
        })
        .to_string();
        let said = in_wire(&request, &mut state).said;
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].0, "permission request");
        assert!(said[0].1.contains("run_command"), "{said:?}");
        assert_eq!(state.asked.len(), 1);
        // Another record is no answer; the record of the same id is.
        let other = json!({ "id": 8, "result": { "ok": true } }).to_string();
        assert!(in_wire(&other, &mut state).said.is_empty());
        let answer = json!({
            "id": 7, "result": { "outcome": { "outcome": "selected", "optionId": "reject" } },
        })
        .to_string();
        let said = in_wire(&answer, &mut state).said;
        assert_eq!(said.len(), 1);
        assert_eq!(said[0].0, "permission answered");
        assert!(said[0].1.contains("reject"), "{said:?}");
        assert!(state.asked.is_empty());
        // A wire with no word of a permission in it says nothing, and a line that is
        // no JSON is read for the markers alone.
        let chunk = json!({ "update": { "sessionUpdate": "agent_message_chunk" } }).to_string();
        assert_eq!(in_wire(&chunk, &mut state), Wired::default());
        assert_eq!(
            in_wire("not json: User skipped this tool call", &mut state)
                .said
                .len(),
            1
        );
    }

    #[test]
    fn the_config_of_a_session_and_the_mode_it_is_in_are_said_as_they_change() {
        let mut state = WireState::default();
        let config = |mode: &str, model: &str| {
            json!({ "sessionId": "s", "update": {
                "sessionUpdate": "config_option_update",
                "configOptions": [
                    { "id": "mode", "currentValue": mode },
                    { "id": "model", "currentValue": model },
                    { "id": "empty", "currentValue": "" },
                    { "id": "level", "currentValue": 0 },
                    { "id": "flag", "currentValue": true },
                ],
            } })
            .to_string()
        };
        let first = in_wire(&config("normal", "swe-2-high"), &mut state);
        assert_eq!(
            first.config.as_deref(),
            Some("mode=normal model=swe-2-high level=0 flag=true")
        );
        assert_eq!(first.mode.as_deref(), Some("normal"));
        assert!(first.said.is_empty());
        // The same again is no change, though it says the mode.
        let again = in_wire(&config("normal", "swe-2-high"), &mut state);
        assert_eq!(
            (again.config, again.mode.as_deref()),
            (None, Some("normal"))
        );
        let moved = in_wire(&config("bypass", "swe-2-medium"), &mut state);
        assert_eq!(
            moved.config.as_deref(),
            Some("mode=bypass model=swe-2-medium level=0 flag=true")
        );
        // A change of mode alone is a record of its own.
        let mode = |id: &str| {
            json!({ "update": { "sessionUpdate": "current_mode_update", "currentModeId": id } })
                .to_string()
        };
        let now = in_wire(&mode("normal"), &mut state);
        assert_eq!(
            (now.config.as_deref(), now.mode.as_deref()),
            (Some("mode now normal"), Some("normal"))
        );
        let same = in_wire(&mode("normal"), &mut state);
        assert_eq!((same.config, same.mode.as_deref()), (None, Some("normal")));
        // A record with no options, or no mode, says none.
        let none = json!({ "update": { "sessionUpdate": "current_mode_update" } }).to_string();
        assert_eq!(in_wire(&none, &mut state), Wired::default());
        // A prompt the TUI puts to its agent is counted, and nothing else is.
        assert_eq!(state.prompts, 0);
        let prompt = json!({ "jsonrpc": "2.0", "id": 3, "method": "session/prompt",
            "params": { "sessionId": "s", "prompt": [] } })
        .to_string();
        in_wire(&prompt, &mut state);
        assert_eq!(state.prompts, 1);
    }

    #[test]
    fn a_call_the_user_skipped_is_said_with_its_command_and_one_that_failed_by_itself_is_not_a_skip(
    ) {
        let mut state = WireState::default();
        let call = json!({ "update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "c1",
            "rawInput": { "command": "echo ran > shell.txt" },
        } })
        .to_string();
        assert_eq!(in_wire(&call, &mut state), Wired::default());
        let skipped = json!({ "update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "failed",
            "content": [{ "type": "content", "content": { "type": "text",
                "text": "Tool execution was rejected: User skipped this tool call" } }],
            "_meta": { "cognition.ai/inferenceToolName": "exec", "cognition.ai/rejected": true },
        } })
        .to_string();
        let said = in_wire(&skipped, &mut state).said;
        assert_eq!(said.len(), 1, "one sighting, and not one for each marker");
        assert_eq!(said[0].0, SKIPPED);
        assert_eq!(
            said[0].1,
            "exec echo ran > shell.txt: Tool execution was rejected: User skipped this tool call"
        );
        // A call that failed on its own is no skip.
        let failed = json!({ "update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "c2", "status": "failed",
            "content": [{ "type": "content", "content": { "type": "text",
                "text": "No shell with id `29a429`." } }],
            "_meta": { "cognition.ai/inferenceToolName": "get_output" },
        } })
        .to_string();
        let said = in_wire(&failed, &mut state).said;
        assert_eq!(
            said,
            [(
                "failed call".to_owned(),
                "get_output: No shell with id `29a429`.".to_owned()
            )]
        );
        // A rejection with other words is a rejected call all the same.
        let other = json!({ "update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "c3", "status": "failed",
            "content": [{ "content": { "text": "denied" } }],
            "_meta": { "cognition.ai/rejected": true },
        } })
        .to_string();
        assert_eq!(in_wire(&other, &mut state).said[0].0, "rejected call");
        // A call that completed says nothing.
        let done = json!({ "update": {
            "sessionUpdate": "tool_call_update", "toolCallId": "c1", "status": "completed",
        } })
        .to_string();
        assert_eq!(in_wire(&done, &mut state), Wired::default());
    }
}
