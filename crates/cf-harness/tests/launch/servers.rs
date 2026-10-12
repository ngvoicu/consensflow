//! How each harness keeps its own processes, its MCP servers (`Adapter::servers`,
//! which the pane host tells a turn's work from them by). Claude Code and Pi keep
//! them in the group of their window's program and run what their agents start in
//! groups of their own; Codex opens them with its window, each in a group of its
//! own. All three say so where there are process groups. OpenCode and Devin have
//! not been probed, and say nothing on every platform.

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
    assert_eq!(says(Harness::Claude, &unix), known(Servers::InGroup));
    assert_eq!(says(Harness::Pi, &unix), known(Servers::InGroup));
    assert_eq!(says(Harness::Codex, &unix), known(Servers::AtLaunch));
}

#[test]
fn on_windows_no_harness_says_so_whose_job_object_has_no_groups() {
    let windows = Env::from_vars([("OS", "Windows_NT")]);
    for harness in Harness::ALL {
        assert_eq!(
            says(harness, &windows),
            Servers::Unknown,
            "{}",
            harness.as_str()
        );
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
