//! The reading half of the first test under `describe('an agents file that
//! cannot be read')` in `tests/roster.test.mjs`: 'is said, never read as an
//! empty roster, and never saved over'. Its writing half is that
//! `normalizeRoster`, `setPreferences`, `addAgent`, `editAgent` and
//! `removeAgent` say the same and leave the file alone. The second test
//! there is `saveDocument`'s.

use super::*;
use serde_json::json;

#[test]
fn is_said_never_read_as_an_empty_roster() {
    let home = Home::new();
    let catalog = catalog();
    let roster = Roster::new(&catalog, home.path());
    // What `addAgent` saves: two-space JSON, a newline; and a hand edit leaves a trailing comma.
    let saved = json!({
        "schemaVersion": 1,
        "agents": [{
            "id": "mybuilder",
            "name": "Mybuilder",
            "kind": "claude-code",
            "createdAt": "2026-10-04T12:00:00.000Z",
            "updatedAt": "2026-10-04T12:00:00.000Z",
            "model": "claude-opus-5-5",
        }],
    });
    let saved = format!("{}\n", serde_json::to_string_pretty(&saved).unwrap());
    let broken = saved.replacen("\n  ]", ",\n  ]", 1);
    assert_ne!(broken, saved);
    home.write(&broken);
    let said = |message: &str| {
        message.starts_with("Your agents file ")
            && message.contains("agents.json is not valid JSON: fix it or move it away")
    };
    let reads = [
        roster.list().map(drop),
        roster.agent_row("mybuilder").map(drop),
        roster.preferences().map(drop),
    ];
    for read in reads {
        let refusal = read.unwrap_err();
        assert!(said(&refusal.message), "{}", refusal.message);
    }
    assert_eq!(home.text(), broken, "the file is as the human left it");
    home.write("null");
    let refusal = roster.list().unwrap_err();
    assert!(refusal.message.contains("is not an agents file"));
}
