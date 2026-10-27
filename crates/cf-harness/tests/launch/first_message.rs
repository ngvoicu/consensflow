//! Where a window's first message goes in (`Adapter::first_message_once_up`,
//! which the engine reads to know when the message is handed over): OpenCode's
//! is posted to its server once the window is up; a resumed Muse window's is
//! pasted once it is up, as `muse resume` takes no prompt; every other window
//! takes it with the command line it opens with, on every platform.

use cf_base::env::Env;
use cf_harness::contract::{Launch, LaunchId};
use cf_harness::launch;
use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;

/// What `harness`'s adapter says of the first message of a window opened on
/// `resume` (afresh where none) going in once the window is up, for windows that
/// run with `env`.
fn says(harness: Harness, env: &Env, resume: Option<&str>) -> bool {
    let root = tempfile::tempdir().unwrap();
    let services = Fakes::new(env).services(env, root.path());
    let id = LaunchId::new("8b0c4f1e-6a3d-4f52-9e7b-2c1d0a9f8e7d").unwrap();
    let launch = Launch {
        id: &id,
        project: 1,
        handle: "zeus",
        role: "worker",
        directory: "/work/app",
        resume,
        previous_gone: true,
        message: Some("The task."),
        agent: None,
        instructions: "# ConsensFlow worker",
        specs: None,
        first_message_ms: 60_000,
        starts_given_up: 0,
    };
    launch::adapter(harness, &services).first_message_once_up(&launch)
}

#[test]
fn opencode_and_a_resumed_muse_take_their_first_message_once_the_window_is_up_and_no_other_does() {
    for os in ["Darwin", "Windows_NT", "Linux"] {
        let env = Env::from_vars([("OS", os)]);
        for harness in Harness::ALL {
            let name = harness.as_str();
            assert_eq!(
                says(harness, &env, None),
                harness == Harness::Opencode,
                "{name} afresh on {os}"
            );
            assert_eq!(
                says(harness, &env, Some("01a17bc9-e758-77f3-ac8d-a725edf373ae")),
                matches!(harness, Harness::Opencode | Harness::Muse),
                "{name} resumed on {os}"
            );
        }
    }
}
