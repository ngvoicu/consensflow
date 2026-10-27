//! How a window's first message goes in (`Adapter::first_message`, which the
//! engine reads to know how and when the message is handed over): Devin reads it
//! from a file of the launch's own as it opens; OpenCode's is posted to its server
//! once the window is up; Claude Code's, Pi's and Codex's are delivered once the
//! window is up, as any later message is. None is on a command line, on any
//! platform (T-432).

use cf_base::env::Env;
use cf_harness::contract::FirstMessage;
use cf_harness::launch;
use cf_harness::testing::Fakes;
use cf_proto::agents::Harness;

/// How `harness`'s adapter says its windows take their first message, for windows
/// that run with `env`.
fn says(harness: Harness, env: &Env) -> FirstMessage {
    let root = tempfile::tempdir().unwrap();
    let services = Fakes::new(env).services(env, root.path());
    launch::adapter(harness, &services).first_message()
}

#[test]
fn devin_reads_its_first_message_as_it_opens_opencode_posts_it_and_the_others_are_delivered_it() {
    for os in ["Darwin", "Windows_NT", "Linux"] {
        let env = Env::from_vars([("OS", os)]);
        for harness in Harness::ALL {
            let expected = match harness {
                Harness::Devin => FirstMessage::Opened,
                Harness::Opencode => FirstMessage::Started,
                Harness::Claude | Harness::Pi | Harness::Codex => FirstMessage::Delivered,
            };
            assert_eq!(
                says(harness, &env),
                expected,
                "{} on {os}",
                harness.as_str()
            );
        }
    }
}
