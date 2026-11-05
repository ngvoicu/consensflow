//! The models a harness lists, on the page (`models.get`, CHF-03): the page names the
//! harness by its CLI's name, and a harness this machine does not have is refused
//! before anything is run. What a list holds, and when it is read, is
//! `crate::models`'s. Not held to a Node recording: Node's page had no such operation.

use super::reading::home_with;
use super::*;

#[tokio::test]
async fn a_harness_the_page_names_badly_or_not_installed_here_is_refused_with_why() {
    LocalSet::new()
        .run_until(async {
            let (_home, env) = home_with("[]");
            let rig = rig_over(env, None);
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let unknown = ask(&app, "models.get", json!({ "harness": "claude-code" })).await;
            assert_eq!(
                unknown,
                json!({ "ok": false, "error": "\"claude-code\" is no harness" })
            );
            let missing = ask(&app, "models.get", json!({ "harness": "claude" })).await;
            assert_eq!(
                missing,
                json!({ "ok": false, "error": "claude is not installed here" })
            );
            assert_eq!(
                rig.kit.ledger.borrow().model_listing("claude").unwrap(),
                None,
                "nothing kept"
            );
            assert_eq!(rig.kicks.get(), 0, "a read wakes nothing");
        })
        .await;
}
