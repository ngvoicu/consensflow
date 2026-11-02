//! Add member and Edit member on the page (T-1143): a member is a harness, a model that
//! harness lists, an effort it lists for that model, and the tier the human chose. Each
//! harness's case keeps the listing its adapter reads of its recorded answer (CHF-03's
//! fixtures, as `app/tests/fixtures/models/` holds them: `cf-harness`'s `page_listings`
//! writes those files and fails while they differ), so what is taken and refused is what
//! the harness really lists. The member runs on a saved agent of the human's for that
//! model and effort, which the launch reads as it reads every member's.

use std::path::PathBuf;

use cf_engine::seams::Roster as _;
use cf_engine::testing::Made;
use cf_proto::models::ModelListing;

use super::reading::home_installed;
use super::*;
use crate::roster::{member_agent_name, Agents};

/// What `harness` lists, as its adapter reads its recorded answer.
fn recorded(harness: &str) -> ModelListing {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../app/tests/fixtures/models")
        .join(format!("{harness}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A page over a project with no staff, whose kit opens windows of every harness, and
/// whose home keeps the listing each harness was recorded giving.
struct Staffing {
    home: tempfile::TempDir,
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    project: i64,
}

fn staffing() -> Staffing {
    let (home, env) = home_installed("[]");
    let kit = Context::made(Made {
        harnesses: vec!["claude-code", "codex", "pi", "devin", "muse"],
        ..Made::default()
    });
    let opens_nothing: crate::page::Opener =
        Rc::new(|_, _| Box::pin(async { Err("a test opens no file".to_owned()) }));
    let rig = rig_of(env, None, Specs::opening_with(opens_nothing), kit);
    for harness in ["claude", "codex", "pi", "devin", "muse"] {
        rig.kit
            .ledger
            .borrow_mut()
            .keep_model_listing(harness, &recorded(harness))
            .unwrap();
    }
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let project = rig.kit.with_staff(&[]).id;
    Staffing {
        home,
        _daemon: daemon,
        app,
        rig,
        project,
    }
}

impl Staffing {
    async fn add(&self, fields: Value) -> Value {
        let mut body = json!({ "project": self.project });
        body.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        ask(&self.app, "member.add", body).await
    }

    /// The saved agent the launch reads for `name`: its model, effort and Pi's thinking.
    fn launches(&self, name: &str) -> (Option<String>, Option<String>, Option<String>) {
        let saved = self.agents().agent(name).unwrap().expect("a saved agent");
        (saved.model, saved.effort, saved.thinking)
    }

    fn agents(&self) -> Agents {
        Agents::new(
            cf_catalog::Catalog::bundled().unwrap(),
            self.home.path().join("agents.json"),
        )
    }

    fn tier_of(&self, handle: &str) -> Option<String> {
        self.rig
            .kit
            .ledger
            .borrow()
            .project(self.project)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .find(|participant| participant.handle == handle)
            .expect("the member")
            .tier
    }
}

/// Claude Code: the member carries the tier the human chose, though the catalog's rules say
/// another of Opus 5.5 at high, and keeps it when the daemon's start follows the agents'
/// tiers. Its saved agent runs the model at the effort; a second Add of the same model and
/// effort gives that member the role more.
#[tokio::test]
async fn a_member_added_by_model_carries_the_tier_the_human_chose_and_keeps_it() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let added = staffing
                .add(json!({
                    "harness": "claude-code",
                    "model": "claude-opus-5-5",
                    "effort": "high",
                    "tier": "critical",
                    "roles": ["worker"],
                }))
                .await;
            assert_eq!(added["ok"], true, "{added}");
            assert_eq!(added["member"]["handle"], "claude-opus-5-5-high");
            assert_eq!(added["member"]["agent"], "claude-opus-5-5-high");
            assert_eq!(added["member"]["harness"], "claude-code");
            assert_eq!(added["member"]["tier"], "critical");
            assert_eq!(
                staffing.launches("claude-opus-5-5-high"),
                (Some("claude-opus-5-5".into()), Some("high".into()), None)
            );
            // The presets' rules call Opus 5.5 at high complex work: the start follows them for
            // every member but this one.
            let changed = staffing
                .agents()
                .follow(&mut staffing.rig.kit.ledger.borrow_mut())
                .unwrap();
            assert!(changed.is_empty(), "{changed:?}");
            assert_eq!(
                staffing.tier_of("claude-opus-5-5-high").as_deref(),
                Some("critical")
            );
            // The same model at the same effort is the same member: the role is one more.
            let again = staffing
                .add(json!({
                    "harness": "claude-code",
                    "model": "claude-opus-5-5",
                    "effort": "high",
                    "tier": "critical",
                    "roles": ["reviewer"],
                }))
                .await;
            assert_eq!(again["member"]["handle"], "claude-opus-5-5-high", "{again}");
            assert_eq!(again["member"]["roles"], json!(["worker", "reviewer"]));
        })
        .await;
}

/// Add member is refused, in words the dialog shows, for no tier, a model the harness does
/// not list, and an effort it does not list for the model or one left out.
#[tokio::test]
async fn add_member_is_refused_without_a_tier_or_on_what_the_harness_does_not_list() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            for (fields, said) in [
                (
                    json!({ "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high" }),
                    "pick the member's tier: critical, complex, standard or light",
                ),
                (
                    json!({ "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "tier": "huge" }),
                    "Work tier must be critical, complex, standard or light",
                ),
                (
                    json!({ "harness": "claude-code", "model": "claude-mythos-9", "effort": "high", "tier": "light" }),
                    "claude-code does not list claude-mythos-9: pick one of its models",
                ),
                (
                    json!({ "harness": "claude-code", "model": "claude-opus-5-5", "tier": "light" }),
                    "pick the effort claude-code runs claude-opus-5-5 at: low, medium, high, xhigh, max",
                ),
                (
                    json!({ "harness": "claude-code", "model": "claude-opus-5-5", "effort": "ultra", "tier": "light" }),
                    "claude-code runs claude-opus-5-5 at low, medium, high, xhigh, max, not ultra",
                ),
                (
                    json!({ "harness": "devin", "model": "adaptive", "effort": "high", "tier": "light" }),
                    "devin takes no effort for adaptive",
                ),
            ] {
                let answer = staffing.add(fields.clone()).await;
                assert_eq!(answer, json!({ "ok": false, "error": said }), "{fields}");
            }
            let project = staffing.rig.kit.ledger.borrow().project(staffing.project).unwrap();
            let members = project
                .unwrap()
                .participants
                .into_iter()
                .filter(|participant| participant.agent.is_some())
                .count();
            assert_eq!(members, 0, "nobody joined");
            assert_eq!(staffing.rig.kicks.get(), 0, "nothing succeeded, nothing is woken");
        })
        .await;
}

/// Codex: a model of its account's list at an effort Codex lists only for it (`ultra`).
#[tokio::test]
async fn a_codex_member_runs_the_model_codex_lists_at_its_effort() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let added = staffing
                .add(json!({ "harness": "codex", "model": "gpt-6.1-sol", "effort": "ultra", "tier": "critical" }))
                .await;
            assert_eq!(added["member"]["handle"], "codex-gpt-6-1-sol-ultra", "{added}");
            assert_eq!(added["member"]["roles"], json!(["worker"]), "a worker unless said");
            assert_eq!(
                staffing.launches("codex-gpt-6-1-sol-ultra"),
                (Some("gpt-6.1-sol".into()), Some("ultra".into()), None)
            );
            // GPT-6 Luna's list stops at max.
            let refused = staffing
                .add(json!({ "harness": "codex", "model": "gpt-6-luna", "effort": "ultra", "tier": "light" }))
                .await;
            assert_eq!(
                refused["error"],
                "codex runs gpt-6-luna at low, medium, high, xhigh, max, not ultra"
            );
        })
        .await;
}

/// Pi lists only the providers it holds a login for: a model on one it lists is taken as
/// `provider/id`, an OpenRouter id keeping its own `/`, its effort as Pi's thinking; a model
/// of a provider Pi has no login for (Google's, here) is not listed, and is refused.
#[tokio::test]
async fn a_pi_member_is_on_a_provider_pi_holds_a_login_for() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let providers: std::collections::BTreeSet<String> = recorded("pi")
                .models
                .into_iter()
                .filter_map(|model| model.provider)
                .collect();
            assert!(!providers.contains("google"), "{providers:?}");
            let added = staffing
                .add(json!({
                    "harness": "pi",
                    "model": "openrouter/anthropic/claude-fable-5.1",
                    "effort": "high",
                    "tier": "complex",
                }))
                .await;
            let name = "pi-openrouter-anthropic-claude-fable-5-1-high";
            assert_eq!(added["member"]["handle"], name, "{added}");
            assert_eq!(added["member"]["harness"], "pi");
            assert_eq!(
                staffing.launches(name),
                (
                    Some("openrouter/anthropic/claude-fable-5.1".into()),
                    None,
                    Some("high".into())
                ),
                "Pi's effort is its thinking"
            );
            let refused = staffing
                .add(json!({
                    "harness": "pi",
                    "model": "google/gemini-4-argon",
                    "effort": "high",
                    "tier": "complex",
                }))
                .await;
            assert_eq!(
                refused["error"],
                "pi does not list google/gemini-4-argon: pick one of its models"
            );
        })
        .await;
}

/// Devin writes the effort into the id, before a suffix (`claude-opus-5-5-medium-fast`): the
/// member runs on the exact `model_uid` Devin lists for that effort, with no effort beside it
/// for the launch to join again; never `claude-opus-5-5-fast-medium`.
#[tokio::test]
async fn a_devin_member_runs_the_exact_id_devin_lists_for_the_effort_never_one_joined() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let added = staffing
                .add(json!({
                    "harness": "devin",
                    "model": "claude-opus-5-5-fast",
                    "effort": "medium",
                    "tier": "standard",
                }))
                .await;
            assert_eq!(added["ok"], true, "{added}");
            let name = added["member"]["agent"].as_str().unwrap().to_owned();
            assert_eq!(name, "devin-claude-opus-5-5-medium-fast");
            assert_eq!(
                staffing.launches(&name),
                (Some("claude-opus-5-5-medium-fast".into()), None, None)
            );
            // A model Devin lists with no effort at all is its own id.
            let adaptive = staffing
                .add(json!({ "harness": "devin", "model": "adaptive", "tier": "light" }))
                .await;
            assert_eq!(adaptive["member"]["agent"], "devin-adaptive", "{adaptive}");
            assert_eq!(
                staffing.launches("devin-adaptive"),
                (Some("adaptive".into()), None, None)
            );
        })
        .await;
}

/// Muse Code: a model of its account's list, a contributor one too (the dialog marks it), at
/// an effort Muse lists for it.
#[tokio::test]
async fn a_muse_member_runs_a_model_muse_lists_at_its_effort() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let added = staffing
                .add(json!({ "harness": "muse", "model": "muse-spark-1.3", "effort": "minimal", "tier": "light" }))
                .await;
            assert_eq!(added["member"]["handle"], "muse-spark-1-3-minimal", "{added}");
            assert_eq!(
                staffing.launches("muse-spark-1-3-minimal"),
                (Some("muse-spark-1.3".into()), Some("minimal".into()), None)
            );
            // Muse Spark 1.2 stops at xhigh.
            let refused = staffing
                .add(json!({ "harness": "muse", "model": "muse-spark-1.2", "effort": "max", "tier": "light" }))
                .await;
            assert_eq!(
                refused["error"],
                "muse runs muse-spark-1.2 at minimal, low, medium, high, xhigh, not max"
            );
        })
        .await;
}

/// Edit member: the member runs on another model, harness and effort from its next window,
/// in the tier chosen, under its handle and in its roles; the tier stays the human's.
#[tokio::test]
async fn edit_member_moves_a_member_to_another_model_in_the_tier_chosen() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            staffing
                .add(json!({ "harness": "claude-code", "model": "claude-sonnet-5-5", "effort": "medium", "tier": "standard", "roles": ["worker", "tester"] }))
                .await;
            let edited = ask(
                &staffing.app,
                "member.edit",
                json!({
                    "project": staffing.project,
                    "member": "claude-sonnet-5-5-medium",
                    "harness": "codex",
                    "model": "gpt-6-astra",
                    "effort": "max",
                    "tier": "complex",
                }),
            )
            .await;
            assert_eq!(edited["ok"], true, "{edited}");
            assert_eq!(edited["member"]["handle"], "claude-sonnet-5-5-medium");
            assert_eq!(edited["member"]["agent"], "codex-gpt-6-astra-max");
            assert_eq!(edited["member"]["harness"], "codex");
            assert_eq!(edited["member"]["tier"], "complex");
            assert_eq!(edited["member"]["roles"], json!(["worker", "tester"]));
            assert_eq!(
                staffing.launches("codex-gpt-6-astra-max"),
                (Some("gpt-6-astra".into()), Some("max".into()), None)
            );
            // GPT-6 Astra at max is critical work by the presets' rules; the human said complex.
            let changed = staffing
                .agents()
                .follow(&mut staffing.rig.kit.ledger.borrow_mut())
                .unwrap();
            assert!(changed.is_empty(), "{changed:?}");
            let refused = ask(
                &staffing.app,
                "member.edit",
                json!({
                    "project": staffing.project,
                    "member": "nobody",
                    "harness": "codex",
                    "model": "gpt-6-astra",
                    "effort": "max",
                    "tier": "complex",
                }),
            )
            .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "no member @nobody on this project's staff" })
            );
        })
        .await;
}

/// The tier the dialog's select starts on: the presets' rules for a model a preset runs on
/// that harness, at each effort it lists; nothing of a model no preset runs there.
#[test]
fn the_preset_tiers_are_said_of_the_models_the_presets_know_at_each_effort() {
    let catalog = cf_catalog::Catalog::bundled().unwrap();
    let claude = crate::page::models::preset_tiers(
        &catalog,
        cf_proto::agents::Harness::Claude,
        &recorded("claude"),
    );
    assert_eq!(
        claude["claude-opus-5-5"],
        json!({ "low": "light", "medium": "standard", "high": "complex", "xhigh": "complex", "max": "critical" })
    );
    let muse = crate::page::models::preset_tiers(
        &catalog,
        cf_proto::agents::Harness::Muse,
        &recorded("muse"),
    );
    assert_eq!(muse, json!({}), "no preset runs on Muse");
}

/// A member's saved agent is named after its harness, model and effort, in the words an
/// agent's name takes, the harness not said twice, and short enough for a handle.
#[test]
fn a_member_agent_s_name_is_its_harness_model_and_effort() {
    use cf_proto::agents::Harness;
    assert_eq!(
        member_agent_name(Harness::Claude, "claude-opus-5-5", Some("high")),
        "claude-opus-5-5-high"
    );
    assert_eq!(
        member_agent_name(Harness::Codex, "gpt-6.1-sol", Some("ultra")),
        "codex-gpt-6-1-sol-ultra"
    );
    assert_eq!(
        member_agent_name(Harness::Pi, "openrouter/~unbiased/pareto-latest", Some("off")),
        "pi-openrouter-unbiased-pareto-latest-off"
    );
    let long = member_agent_name(Harness::Pi, &"a-very-long-model-name/".repeat(6), None);
    assert!(long.len() <= 60 && !long.ends_with('-'), "{long}");
}
