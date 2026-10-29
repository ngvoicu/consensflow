//! How each harness keeps its own processes, its MCP servers (`Adapter::servers`,
//! which the pane host tells a turn's work from them by). Claude Code and Pi keep
//! them in the group of their window's program and run what their agents start in
//! groups of their own; Codex opens them with its window, each in a group of its
//! own. All three say so where there are process groups. Where there are none
//! (Windows), Claude Code says it runs each command in a shell of its own (T-906); the
//! others' trees there were not probed, and none of them leaves a Monitor whose end a
//! look decides. OpenCode and Devin have not been probed, and say nothing on every
//! platform.

use cf_base::env::Env;
use cf_harness::launch;
use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;
use cf_proto::panes::Servers;

/// What `harness`'s adapter says of how it keeps its servers, for windows that
/// run with `env`.
fn says(harness: Harness, env: &Env) -> Servers {
    let root = tempfile::tempdir().unwrap();
    let services = Fakes::new(env).services(env, root.path());
    launch::adapter(harness, &services).servers()
}

#[test]
fn claude_and_pi_keep_their_servers_in_the_windows_group_and_codex_opens_them_with_the_window() {
    let unix = Env::from_vars([("OS", "Darwin")]);
    let known = |kept| {
        if cfg!(windows) {
            Servers::Unknown
        } else {
            kept
        }
    };
    let claude = if cfg!(windows) {
        Servers::ByShell
    } else {
        Servers::InGroup
    };
    assert_eq!(says(Harness::Claude, &unix), claude);
    assert_eq!(says(Harness::Pi, &unix), known(Servers::InGroup));
    assert_eq!(says(Harness::Codex, &unix), known(Servers::AtLaunch));
}

#[test]
fn on_windows_claude_says_it_runs_each_command_in_a_shell_and_no_other_harness_says_anything() {
    // T-906: Claude's adapter said `Unknown` here, and the host told none of a Claude
    // window's commands, whatever it could see.
    let windows = Env::from_vars([("OS", "Windows_NT")]);
    for harness in Harness::ALL {
        let said = if harness == Harness::Claude {
            Servers::ByShell
        } else {
            Servers::Unknown
        };
        assert_eq!(says(harness, &windows), said, "{}", harness.as_str());
    }
}

#[test]
fn a_harness_that_was_not_probed_says_nothing_and_is_told_by_when_its_processes_began() {
    let unix = Env::from_vars([("OS", "Darwin")]);
    for harness in [Harness::Opencode, Harness::Devin] {
        assert_eq!(
            says(harness, &unix),
            Servers::Unknown,
            "{}",
            harness.as_str()
        );
    }
}
