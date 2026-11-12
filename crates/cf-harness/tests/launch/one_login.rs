//! Which harness has one login on the machine for all its windows (`Adapter::one_login`, which
//! the engine reads when the human says a quota was renewed by hand, T-1357): Claude Code,
//! Codex, Devin and Muse do, so the quota renewed is every member's on it; Pi logs into a
//! provider for each model, so two of its members may be on two accounts and the word for one
//! is not for the other. The same on every platform.

use cf_base::env::Env;
use cf_harness::launch;
use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;

/// What `harness`'s adapter says of having one login for the machine, for windows that run
/// with `env`.
fn says(harness: Harness, env: &Env) -> bool {
    let root = tempfile::tempdir().unwrap();
    let services = Fakes::new(env).services(env, root.path());
    launch::adapter(harness, &services).one_login()
}

#[test]
fn every_harness_but_pi_has_one_login_on_every_platform() {
    for os in ["Darwin", "Windows_NT", "Linux"] {
        let env = Env::from_vars([("OS", os)]);
        for harness in Harness::ALL {
            assert_eq!(
                says(harness, &env),
                harness != Harness::Pi,
                "{} on {os}",
                harness.as_str()
            );
        }
    }
}
