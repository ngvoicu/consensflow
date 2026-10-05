use std::cell::RefCell;
use std::collections::VecDeque;

use serde_json::{json, Value};

use super::*;
use crate::testing::{FixedPorts, ScriptedEntropy};

const LAUNCH: &str = "0a1b2c3d-4e5f-4061-8a7b-9c0d1e2f3a4b";
const SETTINGS: &str = "/cf/extensions/opencode/ab12/hosts/opencode-extension/tui.json";

/// The tokens the scripted stream's first and second 24 bytes make.
const FIRST_TOKEN: &str = "AwoRGB8mLTQ7QklQV15lbHN6gYiPlp2k";
const SECOND_TOKEN: &str = "q7K5wMfO1dzj6vH4_wYNFBsiKTA3PkVM";

fn ports(ports: impl IntoIterator<Item = u16>) -> FixedPorts {
    FixedPorts(RefCell::new(VecDeque::from_iter(ports)))
}

fn launched(env: &Env, ports: &FixedPorts, entropy: &ScriptedEntropy) -> Result<Launched, String> {
    let launch = LaunchId::new(LAUNCH).unwrap();
    launch_configuration(env, ports, entropy, &launch, "/work/app", SETTINGS)
}

#[test]
fn a_window_is_opened_on_a_port_and_a_password_of_its_own_and_its_plugin_told_of_the_other() {
    let entropy = ScriptedEntropy::default();
    let launched = launched(&Env::default(), &ports([41_000, 41_001]), &entropy).unwrap();
    assert_eq!(
        launched.args,
        ["--port", "41001", "--hostname", "127.0.0.1"],
        "the second port is the window's, the first the plugin's"
    );
    let named = |name: &str| {
        let found = launched.env.iter().find(|(held, _)| held == name);
        found.map(|(_, value)| value.as_str())
    };
    assert_eq!(
        launched
            .env
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "OPENCODE_TUI_CONFIG",
            "CF_OPENCODE_SESSION_BRIDGE",
            "OPENCODE_SERVER_PASSWORD",
            "OPENCODE_SERVER_USERNAME"
        ],
        "in the order Node built them"
    );
    assert_eq!(named("OPENCODE_TUI_CONFIG"), Some(SETTINGS));
    assert_eq!(named("OPENCODE_SERVER_USERNAME"), Some("opencode"));
    assert_eq!(named("OPENCODE_SERVER_PASSWORD"), Some(SECOND_TOKEN));
    let bridge: Value = serde_json::from_str(named("CF_OPENCODE_SESSION_BRIDGE").unwrap()).unwrap();
    assert_eq!(
        bridge,
        json!({ "launchId": LAUNCH, "port": 41_000, "token": FIRST_TOKEN })
    );
    assert_eq!(
        named("CF_OPENCODE_SESSION_BRIDGE"),
        Some(&*format!(
            r#"{{"launchId":"{LAUNCH}","port":41000,"token":"{FIRST_TOKEN}"}}"#
        )),
        "its fields in the order Node wrote them"
    );
    let channel = launched.channel;
    assert_eq!(channel.launch_id, LAUNCH);
    assert_eq!(channel.endpoint, "http://127.0.0.1:41001");
    assert_eq!(channel.password, SECOND_TOKEN);
    assert_eq!(channel.bridge.endpoint, "http://127.0.0.1:41000");
    assert_eq!(channel.bridge.token, FIRST_TOKEN);
    assert_eq!(entropy.take_draws(), [24, 24]);
}

#[test]
fn two_launches_draw_two_tokens_each_and_no_token_is_drawn_twice() {
    let entropy = ScriptedEntropy::default();
    let free = ports([41_000, 41_001, 41_002, 41_003]);
    let first = launched(&Env::default(), &free, &entropy).unwrap();
    let second = launched(&Env::default(), &free, &entropy).unwrap();
    assert_ne!(first.channel.bridge.token, second.channel.bridge.token);
    assert_ne!(first.channel.password, second.channel.password);
    assert_eq!(second.channel.endpoint, "http://127.0.0.1:41003");
}

#[test]
fn a_settings_file_of_the_humans_own_is_refused_before_anything_is_drawn() {
    let env = Env::from_vars([("OPENCODE_TUI_CONFIG", "/home/me/tui.json")]);
    let entropy = ScriptedEntropy::default();
    let free = ports([41_000, 41_001]);
    assert_eq!(
        launched(&env, &free, &entropy).unwrap_err(),
        "OpenCode has a custom OPENCODE_TUI_CONFIG; its settings were preserved. Remove that launch override to enable ConsensFlow reply delivery."
    );
    assert!(entropy.take_draws().is_empty());
    assert_eq!(free.0.borrow().len(), 2, "no port taken");
    // Set to nothing is not set.
    let nothing = Env::from_vars([("OPENCODE_TUI_CONFIG", "")]);
    assert!(launched(&nothing, &free, &entropy).is_ok());
}

#[test]
fn no_workspace_is_refused_before_the_settings_are_looked_at() {
    let env = Env::from_vars([("OPENCODE_TUI_CONFIG", "/home/me/tui.json")]);
    let launch = LaunchId::new(LAUNCH).unwrap();
    let refused = launch_configuration(
        &env,
        &ports([41_000, 41_001]),
        &ScriptedEntropy::default(),
        &launch,
        "",
        SETTINGS,
    );
    assert_eq!(
        refused.unwrap_err(),
        "launch configuration needs a workspace"
    );
}

#[test]
fn a_port_the_system_will_not_give_is_a_failure_in_node_s_sentence() {
    let entropy = ScriptedEntropy::default();
    assert_eq!(
        launched(&Env::default(), &ports([41_000]), &entropy).unwrap_err(),
        "could not choose a loopback port"
    );
    assert_eq!(
        entropy.take_draws(),
        [24],
        "the plugin's token was drawn by then"
    );
}
