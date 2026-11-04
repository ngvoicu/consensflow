//! Add member and Edit member on the page (T-1143): a member is a harness, a model that
//! harness lists, an effort it lists for that model, and the tier the human chose. Each
//! harness's case keeps the listing its adapter reads of its recorded answer (CHF-03's
//! fixtures, as `app/tests/fixtures/models/` holds them: `cf-harness`'s `page_listings`
//! writes those files and fails while they differ), so what is taken and refused is what
//! the harness really lists. The member holds what it runs on itself (harness-models): the
//! model as its launch passes it, the effort and the name the listing gives the model, by
//! which it is named.

use std::path::PathBuf;

use cf_engine::testing::Made;
use cf_proto::models::ModelListing;

use super::reading::home_installed;
use super::*;

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
    _home: tempfile::TempDir,
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
        _home: home,
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

    /// What the member `handle` runs on, as it holds it: its harness, its model, its effort
    /// and the name its harness's list gives the model.
    fn runs(&self, handle: &str) -> (String, Option<String>, Option<String>, Option<String>) {
        let member = self.member(handle);
        (
            member.harness.unwrap_or_default(),
            member.model,
            member.effort,
            member.model_name,
        )
    }

    fn member(&self, handle: &str) -> cf_ledger::ParticipantView {
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

/// Claude Code: the member carries the tier the human chose and holds the model at the
/// effort, named by the model's name in Claude Code's list and the effort; a second Add of
/// the same model and effort gives that member the role more.
#[tokio::test]
async fn a_member_added_by_model_holds_it_is_named_by_it_and_carries_the_tier_the_human_chose() {
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
            assert_eq!(added["member"]["handle"], "opus-high");
            assert_eq!(added["member"]["agent"], json!(null), "no saved agent");
            assert_eq!(added["member"]["modelName"], "Opus");
            assert_eq!(added["member"]["tier"], "critical");
            assert_eq!(
                staffing.runs("opus-high"),
                (
                    "claude-code".to_owned(),
                    Some("claude-opus-5-5".into()),
                    Some("high".into()),
                    Some("Opus".into())
                )
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
            assert_eq!(again["member"]["handle"], "opus-high", "{again}");
            assert_eq!(again["member"]["roles"], json!(["worker", "reviewer"]));
            // Another effort is another member.
            let other = staffing
                .add(json!({
                    "harness": "claude-code",
                    "model": "claude-opus-5-5",
                    "effort": "max",
                    "tier": "critical",
                }))
                .await;
            assert_eq!(other["member"]["handle"], "opus-max", "{other}");
        })
        .await;
}

/// Add member is refused, in words the dialog shows, for no tier, a model the harness does
/// not list, and an effort it does not list for the model or one left out; an image
/// designer draws on Codex alone.
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
                (
                    json!({ "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "tier": "light", "roles": ["designer"] }),
                    "an image designer draws on Codex: pick one of Codex's models",
                ),
                (
                    json!({ "harness": "claude-code", "effort": "high", "tier": "light" }),
                    "pick the member's model: one its harness lists",
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
                .filter(|participant| participant.role != "chief" && participant.role != "human")
                .count();
            assert_eq!(members, 0, "nobody joined");
            assert_eq!(staffing.rig.kicks.get(), 0, "nothing succeeded, nothing is woken");
        })
        .await;
}

/// Codex: a model of its account's list at an effort Codex lists only for it (`ultra`); an
/// image designer is a Codex member like any other.
#[tokio::test]
async fn a_codex_member_runs_the_model_codex_lists_at_its_effort() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let added = staffing
                .add(json!({ "harness": "codex", "model": "gpt-6.1-sol", "effort": "ultra", "tier": "critical" }))
                .await;
            assert_eq!(added["member"]["handle"], "gpt-6-1-sol-ultra", "{added}");
            assert_eq!(added["member"]["roles"], json!(["worker"]), "a worker unless said");
            assert_eq!(
                staffing.runs("gpt-6-1-sol-ultra"),
                (
                    "codex".to_owned(),
                    Some("gpt-6.1-sol".into()),
                    Some("ultra".into()),
                    Some("GPT-6.1-Sol".into())
                )
            );
            // GPT-6 Luna's list stops at max.
            let refused = staffing
                .add(json!({ "harness": "codex", "model": "gpt-6-luna", "effort": "ultra", "tier": "light" }))
                .await;
            assert_eq!(
                refused["error"],
                "codex runs gpt-6-luna at low, medium, high, xhigh, max, not ultra"
            );
            let designer = staffing
                .add(json!({ "harness": "codex", "model": "gpt-6-astra", "effort": "high", "tier": "light", "roles": ["designer"] }))
                .await;
            assert_eq!(designer["member"]["handle"], "gpt-6-astra-high", "{designer}");
            assert_eq!(designer["member"]["designer"], true);
        })
        .await;
}

/// Pi lists only the providers it holds a login for: a model on one it lists is taken as
/// `provider/id`, an OpenRouter id keeping its own `/`; a model of a provider Pi has no
/// login for (Google's, here) is not listed, and is refused.
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
            let name = "anthropic-claude-fable-5-1-high";
            assert_eq!(added["member"]["handle"], name, "{added}");
            assert_eq!(
                staffing.runs(name),
                (
                    "pi".to_owned(),
                    Some("openrouter/anthropic/claude-fable-5.1".into()),
                    Some("high".into()),
                    Some("Anthropic: Claude Fable 5.1".into())
                )
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
/// member holds the exact `model_uid` Devin lists for that effort, never one joined
/// (`claude-opus-5-5-fast-medium`), and the effort beside it for the page to say.
#[tokio::test]
async fn a_devin_member_holds_the_exact_id_devin_lists_for_the_effort_never_one_joined() {
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
            assert_eq!(added["member"]["handle"], "claude-opus-5-5-fast-medium");
            assert_eq!(
                staffing.runs("claude-opus-5-5-fast-medium"),
                (
                    "devin".to_owned(),
                    Some("claude-opus-5-5-medium-fast".into()),
                    Some("medium".into()),
                    Some("Claude Opus 5.5 Fast".into())
                )
            );
            // A model Devin lists with no effort at all is its own id.
            let adaptive = staffing
                .add(json!({ "harness": "devin", "model": "adaptive", "tier": "light" }))
                .await;
            assert_eq!(adaptive["member"]["handle"], "adaptive", "{adaptive}");
            assert_eq!(
                staffing.runs("adaptive"),
                (
                    "devin".to_owned(),
                    Some("adaptive".into()),
                    None,
                    Some("Adaptive".into())
                )
            );
        })
        .await;
}

/// Muse Code: a model of its account's list, at an effort Muse lists for it.
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
                staffing.runs("muse-spark-1-3-minimal"),
                (
                    "muse".to_owned(),
                    Some("muse-spark-1.3".into()),
                    Some("minimal".into()),
                    Some("muse-spark-1.3".into())
                )
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
/// in the tier chosen, under its handle and in its roles.
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
                    "member": "sonnet-medium",
                    "harness": "codex",
                    "model": "gpt-6-astra",
                    "effort": "max",
                    "tier": "complex",
                }),
            )
            .await;
            assert_eq!(edited["ok"], true, "{edited}");
            assert_eq!(edited["member"]["handle"], "sonnet-medium");
            assert_eq!(edited["member"]["tier"], "complex");
            assert_eq!(edited["member"]["roles"], json!(["worker", "tester"]));
            assert_eq!(
                staffing.runs("sonnet-medium"),
                (
                    "codex".to_owned(),
                    Some("gpt-6-astra".into()),
                    Some("max".into()),
                    Some("GPT-6-Astra".into())
                )
            );
            assert_eq!(staffing.tier_of("sonnet-medium").as_deref(), Some("complex"));
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

/// An image designer draws with Codex's image tool and holds no other role, so it is a
/// member of its own beside a worker on the same model and effort, whichever is added
/// first: the second is named with its harness in front, and neither is refused.
#[tokio::test]
async fn a_worker_and_an_image_designer_on_one_model_and_effort_are_two_members_in_either_order() {
    LocalSet::new()
        .run_until(async {
            let on = |roles: Value| {
                json!({ "harness": "codex", "model": "gpt-6.1-sol", "effort": "high", "tier": "light", "roles": roles })
            };
            // A worker first, then the designer.
            let first = staffing();
            let worker = first.add(on(json!(["worker"]))).await;
            assert_eq!(worker["member"]["handle"], "gpt-6-1-sol-high", "{worker}");
            let designer = first.add(on(json!(["designer"]))).await;
            assert_eq!(designer["ok"], true, "{designer}");
            assert_eq!(designer["member"]["handle"], "codex-gpt-6-1-sol-high");
            assert_eq!(designer["member"]["designer"], true);
            assert_eq!(first.member("gpt-6-1-sol-high").roles, ["worker"]);
            assert!(!first.member("gpt-6-1-sol-high").designer);
            // The designer first, then a worker; a second designer is that designer again.
            let staffing = staffing();
            let designer = staffing.add(on(json!(["designer"]))).await;
            assert_eq!(designer["member"]["handle"], "gpt-6-1-sol-high", "{designer}");
            let worker = staffing.add(on(json!(["worker", "reviewer"]))).await;
            assert_eq!(worker["ok"], true, "{worker}");
            assert_eq!(worker["member"]["handle"], "codex-gpt-6-1-sol-high");
            assert_eq!(worker["member"]["designer"], false);
            let again = staffing.add(on(json!(["designer"]))).await;
            assert_eq!(again["member"]["handle"], "gpt-6-1-sol-high", "{again}");
            assert_eq!(staffing.member("gpt-6-1-sol-high").roles, ["designer"]);
        })
        .await;
}

/// A chief is picked as a member is (HMD-04): the harness, a model it lists and an effort it
/// lists for it, checked against the kept listing and held as the harness takes it (Devin's
/// as the exact id it lists for the effort). New project and Switch chief both keep what
/// was picked, and the chief's next launch is given it: its effort, Pi's as its thinking.
#[tokio::test]
async fn a_chief_is_held_and_launched_on_the_model_and_effort_picked_on_each_harness() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            // (harness, model as the picker sends it, effort, model held, effort held,
            // what the launch is given: model, effort, thinking)
            let picks = [
                ("claude-code", "claude-opus-5-5", "high", "claude-opus-5-5", Some("high"), None),
                ("codex", "gpt-6.1-sol", "ultra", "gpt-6.1-sol", Some("ultra"), None),
                ("pi", "anthropic/claude-fable-5", "max", "anthropic/claude-fable-5", None, Some("max")),
                ("devin", "swe-2", "max", "swe-2-max", None, None),
                ("muse", "muse-wonder-1.0", "low", "muse-wonder-1.0", Some("low"), None),
            ];
            for (harness, model, effort, held, given_effort, given_thinking) in picks {
                let opened = ask(
                    &staffing.app,
                    "project.open",
                    json!({
                        "directory": format!("/work/{harness}"),
                        "harness": harness, "model": model, "effort": effort, "staff": [],
                    }),
                )
                .await;
                assert_eq!(opened["ok"], true, "{harness}: {opened}");
                let project = opened["project"]["id"].as_i64().unwrap();
                let chief = staffing.chief_of(project);
                assert_eq!(
                    (chief.model.as_deref(), chief.effort.as_deref()),
                    (Some(held), Some(effort)),
                    "{harness}: the chief holds what was picked"
                );
                let launch = staffing.last_chief_launch(project);
                assert_eq!(
                    (
                        launch["agent"]["model"].as_str(),
                        launch["agent"]["effort"].as_str(),
                        launch["agent"]["thinking"].as_str(),
                    ),
                    (Some(held), given_effort, given_thinking),
                    "{harness}: {launch}"
                );
            }
            // Switch chief takes the same choice, Devin's at another effort: the engine is
            // handed the exact id Devin lists for it, and the effort beside it (the engine
            // keeps what it is handed, `chief_switch`).
            let asked = json!({ "project": 1, "harness": "devin", "model": "swe-2", "effort": "high", "when": "now" });
            let target = crate::page::chief::chief_asked(&staffing.rig.page, crate::page::body::Body::new(&asked), true)
                .expect("a switch");
            assert_eq!(
                (target.harness.as_str(), target.model.as_str(), target.effort.as_deref()),
                ("devin", "swe-2-high", Some("high"))
            );
            // An effort the model does not list, or none where it lists some, is refused.
            for (body, said) in [
                (json!({ "harness": "codex", "model": "gpt-6-luna", "effort": "ultra" }), "codex runs gpt-6-luna at low, medium, high, xhigh, max, not ultra"),
                (json!({ "harness": "devin", "model": "swe-2" }), "pick the effort devin runs swe-2 at: medium, high, max"),
            ] {
                let mut asked = json!({ "directory": "/work/refused", "staff": [] });
                asked.as_object_mut().unwrap().extend(body.as_object().unwrap().clone());
                let refused = ask(&staffing.app, "project.open", asked).await;
                assert_eq!(refused["error"], said, "{refused}");
            }
        })
        .await;
}

impl Staffing {
    /// The chief of `project` as the ledger holds it.
    fn chief_of(&self, project: i64) -> cf_ledger::ParticipantView {
        self.rig
            .kit
            .ledger
            .borrow()
            .project(project)
            .unwrap()
            .unwrap()
            .participants
            .into_iter()
            .find(|participant| participant.role == "chief")
            .expect("the chief")
    }

    /// The last launch the engine's adapter prepared for the chief of `project`.
    fn last_chief_launch(&self, project: i64) -> Value {
        self.rig.kit.settle();
        self.rig
            .kit
            .adapter
            .prepared()
            .into_iter()
            .rfind(|launch| launch["role"] == "chief" && launch["project"]["id"] == project)
            .unwrap_or_else(|| panic!("no launch of project {project}'s chief"))
    }
}
