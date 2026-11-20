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
            assert_eq!(rig.kicks.get(), 0, "a read wakes nothing");
        })
        .await;
}

/// A silent Pi's own timeout must reach the page through the same deadline
/// selected for the app bridge. The two clocks let the bridge pass 30 s
/// before the adapter is allowed to report its own 30 s timeout.
#[tokio::test]
async fn a_silent_pi_s_timeout_advice_reaches_the_page_bridge() {
    use cf_engine::seams::Adapters;
    use cf_harness::contract::Adapter;
    use cf_harness::pi::PiAdapter;
    use cf_harness::testing::{fake_executable, ChildScript, Ends, Fakes};

    struct RealPi(Rc<dyn Adapter>);
    impl Adapters for RealPi {
        fn adapter(&self, kind: &str) -> Option<Rc<dyn Adapter>> {
            (kind == "pi").then(|| Rc::clone(&self.0))
        }
    }

    LocalSet::new()
        .run_until(async {
            tokio::time::pause();
            let home = tempfile::tempdir().unwrap();
            let bin = home.path().join("bin");
            let agent = home.path().join("agent");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::create_dir_all(&agent).unwrap();
            fake_executable(&bin.join("pi"));
            let env = Env::from_vars([
                ("PATH", bin.as_os_str()),
                ("PI_CODING_AGENT_DIR", agent.as_os_str()),
            ]);
            let fakes = Fakes::new(&env);
            fakes.processes.child(
                "pi",
                ChildScript {
                    lines: Vec::new(),
                    ends: Ends::Asked,
                },
            );
            let services = fakes.services(&env, home.path());
            let adapter: Rc<dyn Adapter> = Rc::new(PiAdapter::new(&services));
            let rig = rig_listing(
                env,
                None,
                Specs::opening_with(Rc::new(|_, _| Box::pin(async { Err("unused".into()) }))),
                Context::new(),
                Rc::new(RealPi(adapter)),
            );
            let (daemon, app) = bridge_pair_over(&rig.spawn);
            register(&daemon, &rig.page, &rig.spawn);
            let operation = "models.get";
            let deadline =
                cf_proto::page::request_deadline_ms(operation).map(Duration::from_millis);
            let answer = tokio::task::spawn_local(app.request(
                operation,
                json!({ "harness": "pi" }),
                deadline,
            ));
            let mut written = Vec::new();
            for _ in 0..1000 {
                tokio::task::yield_now().await;
                written.extend(fakes.processes.take_written());
                if !written.is_empty() {
                    break;
                }
            }
            assert!(
                written
                    .iter()
                    .any(|line| line.contains("get_available_models")),
                "the stand-in was asked: {written:?}"
            );
            tokio::time::advance(Duration::from_secs(31)).await;
            tokio::task::yield_now().await;
            assert!(fakes.time.fire_next(i64::MAX), "Pi's 30 s timer was armed");
            let until = std::time::Instant::now() + Duration::from_secs(5);
            while !answer.is_finished() && std::time::Instant::now() < until {
                tokio::task::yield_now().await;
            }
            assert!(answer.is_finished(), "the page did not receive Pi's answer");
            let result = answer.await.unwrap().unwrap();
            assert_eq!(result["ok"], true, "{result}");
            let why = result["listing"]["freshness"]["why"]
                .as_str()
                .unwrap_or_default();
            assert!(why.contains("did not answer within 30 s"), "{result}");
            assert!(
                why.contains("package or extension of Pi's may be stuck"),
                "{result}"
            );
        })
        .await;
}
