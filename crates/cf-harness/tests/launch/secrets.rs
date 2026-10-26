//! No secret on a command line, for any harness: a window's board token
//! (`CONSENSFLOW_TOKEN`) is in its environment, never an argument, since every user
//! of the machine reads a command line (`ps` is setuid root on macOS). Held to both
//! platforms' recordings of every adapter's launches, and to what this platform's
//! adapters launch when each scenario is played. (Codex's supervisor once put
//! ConsensFlow's variables on its server's command line, past the adapter; its own
//! tests hold that, `cf-codex-session`'s `no_secret_on`.)

use std::fs;
use std::path::Path;

use serde_json::Value;

/// Every value a scenario gives `CONSENSFLOW_TOKEN`: in its environment, and in the
/// environment of any launch it records.
fn tokens(scenario: &Value) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(token) = scenario["env"]["CONSENSFLOW_TOKEN"].as_str() {
        found.push(token.to_owned());
    }
    for record in scenario["records"].as_array().into_iter().flatten() {
        for settled in record["settled"].as_array().into_iter().flatten() {
            for pair in settled["answer"]["env"].as_array().into_iter().flatten() {
                if pair[0] == "CONSENSFLOW_TOKEN" {
                    found.extend(pair[1].as_str().map(str::to_owned));
                }
            }
        }
    }
    found.retain(|token| !token.is_empty());
    found
}

/// That the command lines `records` launched hold no board token, nor its name, nor
/// ConsensFlow's variables set for Codex's commands; how many there were.
fn no_secret_launched(records: &[Value], secrets: &[String], case: &str) -> usize {
    let mut launches = 0;
    for record in records {
        for settled in record["settled"].as_array().into_iter().flatten() {
            let Some(argv) = settled["answer"]["argv"].as_array() else {
                continue;
            };
            launches += 1;
            for word in argv.iter().filter_map(Value::as_str) {
                assert!(
                    !word.contains("CONSENSFLOW_TOKEN"),
                    "{case}: a command line names the board's token: {word}"
                );
                assert!(
                    !word.contains("shell_environment_policy.set.CONSENSFLOW_"),
                    "{case}: a command line sets ConsensFlow's variables: {word}"
                );
                for secret in secrets {
                    assert!(
                        !word.contains(secret.as_str()),
                        "{case}: the board's token is on a command line"
                    );
                }
            }
        }
    }
    launches
}

#[test]
fn no_launch_this_platforms_adapters_make_puts_the_board_token_or_its_name_on_a_command_line() {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/goldens/launch/scenarios.{}.json",
        crate::scenarios::platform()
    ));
    let scenarios: Vec<Value> = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    let mut launches = 0;
    for scenario in &scenarios {
        let records = crate::scenarios::play(scenario);
        let case = format!("played: {}", scenario["name"]);
        launches += no_secret_launched(&records, &tokens(scenario), &case);
    }
    assert!(launches > 300, "{launches} launches played");
}

#[test]
fn no_launch_of_any_harness_on_either_platform_puts_the_board_token_or_its_name_on_a_command_line()
{
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/goldens/launch");
    for platform in ["darwin", "win32"] {
        let file = folder.join(format!("scenarios.{platform}.json"));
        let scenarios: Vec<Value> =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        let mut launches = 0;
        for scenario in &scenarios {
            let case = format!("{platform}: {}", scenario["name"]);
            let records = scenario["records"].as_array().unwrap();
            launches += no_secret_launched(records, &tokens(scenario), &case);
        }
        assert!(launches > 300, "{platform}: {launches} launches recorded");
    }
}
