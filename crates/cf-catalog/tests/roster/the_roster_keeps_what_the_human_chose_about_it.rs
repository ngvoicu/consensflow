//! The test under `describe('the roster keeps what the human chose about
//! it')` in `tests/roster.test.mjs`.

use super::*;
use serde_json::json;

#[test]
fn hides_claude_and_openai_models_on_pi_and_opencode_when_they_are_kept_to_their_own_harnesses() {
    let home = Home::new();
    let catalog = catalog();
    let roster = home.roster(&catalog);
    assert!(!roster.preferences().unwrap().own_harness_only);
    assert!(
        !roster.list().unwrap().iter().any(|agent| agent.hidden),
        "nothing hidden by default"
    );
    assert!(
        roster
            .set_preferences(Some(&json!({ "ownHarnessOnly": true })))
            .unwrap()
            .own_harness_only
    );
    let hidden = |name: &str| {
        roster
            .list()
            .unwrap()
            .iter()
            .find(|agent| agent.name.as_deref() == Some(name))
            .unwrap()
            .hidden
    };
    assert_eq!(
        ["kronos", "baldr", "phoebe", "bil", "aurora", "apollo", "diana", "ares", "gefjon"].map(hidden),
        [true, true, true, true, true, false, false, false, false],
        "Opus, Luna and Sol through Pi or OpenCode; never on Claude Code or Codex, never Grok or Muse"
    );
    assert_eq!(
        home.raw()["preferences"],
        json!({ "ownHarnessOnly": true }),
        "kept in the file"
    );
    roster.normalize().unwrap();
    assert!(
        roster.preferences().unwrap().own_harness_only,
        "a fold at start keeps it"
    );
    assert!(roster
        .set_preferences(Some(&json!({ "ownHarnessOnly": "yes" })))
        .unwrap_err()
        .message
        .contains("is on or off"));
    assert!(roster
        .set_preferences(Some(&json!({ "colour": true })))
        .unwrap_err()
        .message
        .contains("no preference named colour"));
    roster
        .set_preferences(Some(&json!({ "ownHarnessOnly": false })))
        .unwrap();
    assert!(!roster.list().unwrap().iter().any(|agent| agent.hidden));
}
