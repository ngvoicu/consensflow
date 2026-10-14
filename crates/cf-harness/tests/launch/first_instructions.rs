//! Which harness keeps the instructions a resumed conversation began with
//! (`Adapter::keeps_first_instructions`, which the engine reads to say a changed
//! role itself, as a message): Codex does, a thread keeping the developer message
//! of its first turn and not using the one it is resumed with. The other four take
//! their role again at every launch (Claude through its role folder, OpenCode
//! through its configuration, Pi through `--skill`, Devin through its
//! SessionStart hook) and say no, on every platform.

use cf_base::env::Env;
use cf_harness::launch;
use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;

/// What `harness`'s adapter says of keeping a resumed conversation's first
/// instructions, for windows that run with `env`.
fn says(harness: Harness, env: &Env) -> bool {
    let root = tempfile::tempdir().unwrap();
    let services = Fakes::new(env).services(env, root.path());
    launch::adapter(harness, &services).keeps_first_instructions()
}

#[test]
fn codex_keeps_the_instructions_a_resumed_thread_began_with_on_every_platform() {
    for os in ["Darwin", "Windows_NT", "Linux"] {
        assert!(says(Harness::Codex, &Env::from_vars([("OS", os)])), "{os}");
    }
}

#[test]
fn no_other_harness_does_for_each_takes_its_role_again_at_every_launch() {
    for os in ["Darwin", "Windows_NT", "Linux"] {
        let env = Env::from_vars([("OS", os)]);
        for harness in Harness::ALL.into_iter().filter(|h| *h != Harness::Codex) {
            assert!(!says(harness, &env), "{} on {os}", harness.as_str());
        }
    }
}
