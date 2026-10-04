//! The reading half of the one test under `describe('the roster keeps what
//! the human chose about it')` in `tests/roster.test.mjs`. Its writing half
//! is `setPreferences`: keeping the choice in the file, refusing a value
//! that is no boolean and a key that is no preference, and `normalizeRoster`
//! keeping the choice. Here the choice is written into the file by hand.

use super::*;
use cf_catalog::Preferences;

#[test]
fn hides_claude_and_openai_models_on_pi_and_opencode_when_they_are_kept_to_their_own_harnesses() {
    let home = Home::new();
    let catalog = catalog();
    let roster = Roster::new(&catalog, home.path());
    let hidden = |name: &str| {
        let agents = roster.list().unwrap();
        let agent = agents
            .iter()
            .find(|agent| agent.name.as_deref() == Some(name))
            .unwrap();
        agent.hidden
    };
    assert_eq!(
        roster.preferences().unwrap(),
        Preferences {
            own_harness_only: false
        }
    );
    assert!(
        roster.list().unwrap().iter().all(|agent| !agent.hidden),
        "nothing hidden by default"
    );
    home.write(r#"{"schemaVersion":1,"agents":[],"preferences":{"ownHarnessOnly":true}}"#);
    assert_eq!(
        roster.preferences().unwrap(),
        Preferences {
            own_harness_only: true
        }
    );
    let names = [
        "kronos", "baldr", "phoebe", "bil", "aurora", "apollo", "diana", "ares", "gefjon",
    ];
    assert_eq!(
        names.map(hidden),
        [true, true, true, true, true, false, false, false, false],
        "Opus, Luna and Sol through Pi or OpenCode; never on Claude Code or Codex, never Grok or Muse"
    );
    home.write(r#"{"schemaVersion":1,"agents":[],"preferences":{"ownHarnessOnly":false}}"#);
    assert!(roster.list().unwrap().iter().all(|agent| !agent.hidden));
}
