//! Where a window's first message goes in (`Adapter::first_message_once_up`,
//! which the engine reads to know when the message is handed over): OpenCode's
//! is posted to its server once the window is up; the other four take it with
//! the command line their window opens with, on every platform.

use cf_base::env::Env;
use cf_harness::launch;
use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;

/// What `harness`'s adapter says of its first message going in once the window
/// is up, for windows that run with `env`.
fn says(harness: Harness, env: &Env) -> bool {
    let root = tempfile::tempdir().unwrap();
    let services = Fakes::new(env).services(env, root.path());
    launch::adapter(harness, &services).first_message_once_up()
}

#[test]
fn opencode_takes_its_first_message_once_its_window_is_up_and_no_other_harness_does() {
    for os in ["Darwin", "Windows_NT", "Linux"] {
        let env = Env::from_vars([("OS", os)]);
        for harness in Harness::ALL {
            assert_eq!(
                says(harness, &env),
                harness == Harness::Opencode,
                "{} on {os}",
                harness.as_str()
            );
        }
    }
}
