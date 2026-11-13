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
    _daemon: Bridge,
    app: Bridge,
    rig: Rig,
    project: i64,
    /// What each harness lists, by its kind, as the page reads it: the recorded listings.
    lists: Vec<(&'static str, Rc<crate::models::tests::Listing>)>,
    _home: tempfile::TempDir,
}

fn staffing() -> Staffing {
    let (home, env) = home_installed("[]");
    let kit = Context::made(Made {
        harnesses: vec!["claude-code", "codex", "pi", "devin", "muse"],
        ..Made::default()
    });
    let opens_nothing: crate::page::Opener =
        Rc::new(|_, _| Box::pin(async { Err("a test opens no file".to_owned()) }));
    let lists: Vec<(&'static str, Rc<crate::models::tests::Listing>)> = [
        ("claude-code", "claude"),
        ("codex", "codex"),
        ("pi", "pi"),
        ("devin", "devin"),
        ("muse", "muse"),
    ]
    .into_iter()
    .map(|(kind, name)| {
        (
            kind,
            crate::models::tests::Listing::repeating(recorded(name)),
        )
    })
    .collect();
    let adapters = Rc::new(crate::models::tests::ByKind(lists.clone()));
    let rig = rig_listing(env, None, Specs::opening_with(opens_nothing), kit, adapters);
    let (daemon, app) = bridge_pair_over(&rig.spawn);
    register(&daemon, &rig.page, &rig.spawn);
    let project = rig.kit.with_staff(&[]).id;
    Staffing {
        _home: home,
        _daemon: daemon,
        app,
        rig,
        project,
        lists,
    }
}

impl Staffing {
    /// The listing the harness of `kind` answers with, which a test can change.
    fn lists_of(&self, kind: &str) -> &crate::models::tests::Listing {
        &self.lists.iter().find(|(of, _)| *of == kind).unwrap().1
    }

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
            assert_eq!(added["member"]["handle"], "claude-opus-5-5-high");
            assert_eq!(added["member"]["agent"], json!(null), "no saved agent");
            assert_eq!(added["member"]["modelName"], "claude-opus-5-5");
            assert_eq!(added["member"]["tier"], "critical");
            assert_eq!(
                staffing.runs("claude-opus-5-5-high"),
                (
                    "claude-code".to_owned(),
                    Some("claude-opus-5-5".into()),
                    Some("high".into()),
                    Some("claude-opus-5-5".into())
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
            assert_eq!(again["member"]["handle"], "claude-opus-5-5-high", "{again}");
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
            assert_eq!(other["member"]["handle"], "claude-opus-5-5-max", "{other}");
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
                    "an image designer draws on Codex",
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
                    Some("gpt-6.1-sol".into())
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
        })
        .await;
}

/// An image designer draws with Codex's image tool: its window is launched on Codex's own
/// default model, at no effort, whatever its row holds, and a design task has no tier. So
/// Add member takes none of them for it (designer P2, T-1280): it is added on Codex alone,
/// holds no model, and is named by the default it runs on; it is kept in the lowest tier,
/// which decides nothing; and Edit member has nothing to change in it.
#[tokio::test]
async fn an_image_designer_is_added_on_codex_with_no_model_effort_or_tier() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let added = staffing
                .add(json!({ "harness": "codex", "roles": ["designer"] }))
                .await;
            assert_eq!(added["ok"], true, "{added}");
            assert_eq!(added["member"]["handle"], "codex-default", "{added}");
            assert_eq!(added["member"]["designer"], true);
            assert_eq!(added["member"]["roles"], json!(["designer"]));
            assert_eq!(
                staffing.runs("codex-default"),
                ("codex".to_owned(), None, None, None),
                "it runs on Codex's own default"
            );
            assert_eq!(
                staffing.member("codex-default").tier.as_deref(),
                Some("light")
            );
            // A second one is that designer again.
            let again = staffing
                .add(json!({ "harness": "codex", "roles": ["designer"] }))
                .await;
            assert_eq!(again["member"]["handle"], "codex-default", "{again}");
            // Another harness does not draw.
            let refused = staffing
                .add(json!({ "harness": "claude-code", "roles": ["designer"] }))
                .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "an image designer draws on Codex" })
            );
        })
        .await;
}

/// A model, an effort or a tier asked for an image designer is refused, in words: what it is
/// kept as would not be what it runs on.
#[tokio::test]
async fn an_image_designer_asked_for_a_model_an_effort_or_a_tier_is_refused() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            for fields in [
                json!({ "harness": "codex", "model": "gpt-6.1-sol", "roles": ["designer"] }),
                json!({ "harness": "codex", "effort": "high", "roles": ["designer"] }),
                json!({ "harness": "codex", "tier": "light", "roles": ["designer"] }),
            ] {
                let answer = staffing.add(fields.clone()).await;
                assert_eq!(
                    answer,
                    json!({ "ok": false, "error": "an image designer draws on Codex's own model: it takes no model, effort or tier" }),
                    "{fields}"
                );
            }
            let project = staffing.rig.kit.ledger.borrow().project(staffing.project).unwrap();
            let members = project
                .unwrap()
                .participants
                .into_iter()
                .filter(|participant| participant.role != "chief" && participant.role != "human")
                .count();
            assert_eq!(members, 0, "nobody joined");
            // And what it holds is nothing to edit.
            let added = staffing
                .add(json!({ "harness": "codex", "roles": ["designer"] }))
                .await;
            let edited = ask(
                &staffing.app,
                "member.edit",
                json!({ "project": staffing.project, "member": added["member"]["handle"], "harness": "codex" }),
            )
            .await;
            assert_eq!(
                edited,
                json!({ "ok": false, "error": "an image designer has nothing to edit: it draws on Codex's own model" })
            );
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
                    Some("openrouter/anthropic/claude-fable-5.1".into())
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

/// A model id may be 256 characters (`provider/id`, Pi's), and a member's `model_name` is that id
/// (HMD-12): Add member and New project take a listed model of 129 and of 256 characters, which
/// the old 128 of a display name refused, and name the member within 40.
#[tokio::test]
async fn a_member_on_a_listed_model_with_a_long_id_is_added_and_starts_a_project() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let long = |len: usize| format!("openrouter/{}", "a".repeat(len - "openrouter/".len()));
            let mut listing = recorded("pi");
            for len in [129, 256] {
                let mut model = cf_proto::models::ModelChoice::new(
                    long(len).strip_prefix("openrouter/").unwrap(),
                    "A long one",
                );
                model.provider = Some("openrouter".to_owned());
                model.efforts = vec!["high".to_owned()];
                listing.models.push(model);
            }
            staffing.lists_of("pi").set(listing);
            for len in [129, 256] {
                let added = staffing
                    .add(json!({ "harness": "pi", "model": long(len), "effort": "high", "tier": "complex" }))
                    .await;
                assert_eq!(added["ok"], true, "add at {len}: {added}");
                assert_eq!(added["member"]["modelName"], long(len).as_str());
                let handle = added["member"]["handle"].as_str().unwrap();
                assert!(handle.len() <= 40, "{handle}");
                let opened = ask(
                    &staffing.app,
                    "project.open",
                    json!({
                        "directory": format!("/work/long-{len}"),
                        "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high",
                        "staff": [{ "harness": "pi", "model": long(len), "effort": "high", "tier": "light", "roles": ["worker"] }],
                    }),
                )
                .await;
                assert_eq!(opened["ok"], true, "project.open at {len}: {opened}");
            }
        })
        .await;
}

/// New project starts with an image designer as the page picks one: Codex and the role, no
/// model, effort, tier or designer flag. The project opens with a designer on Codex's own
/// model, named by it, in the tier a designer is kept in, and a model, an effort or a tier
/// asked for it refuses the start in the words Add member uses (designer P2, T-1280).
#[tokio::test]
async fn a_project_starts_with_an_image_designer_as_the_page_picks_one() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let opening = |designer: Value| {
                json!({
                    "directory": "/work/drawn",
                    "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high",
                    "staff": [
                        { "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "tier": "standard", "roles": ["worker"] },
                        designer,
                    ],
                })
            };
            let opened = ask(
                &staffing.app,
                "project.open",
                opening(json!({ "harness": "codex", "roles": ["designer"] })),
            )
            .await;
            assert_eq!(opened["ok"], true, "{opened}");
            let project = staffing
                .rig
                .kit
                .ledger
                .borrow()
                .project(opened["project"]["id"].as_i64().unwrap())
                .unwrap()
                .unwrap();
            let designer = project
                .participants
                .iter()
                .find(|participant| participant.designer)
                .expect("the designer");
            assert_eq!(designer.handle, "codex-default");
            assert_eq!(designer.roles, ["designer"]);
            assert_eq!(designer.model, None);
            assert_eq!(designer.tier.as_deref(), Some("light"));
            let refused = ask(
                &staffing.app,
                "project.open",
                opening(json!({ "harness": "codex", "model": "gpt-6.1-sol", "tier": "light", "roles": ["designer"] })),
            )
            .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "an image designer draws on Codex's own model: it takes no model, effort or tier" })
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
                    Some("claude-opus-5-5-fast".into())
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
                    Some("adaptive".into())
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
/// in the tier chosen and in its roles, and is named by the model it runs on now.
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
            // The name holds the model it runs on: edited, the member takes the new one's, and
            // the name it leaves is kept for History.
            assert_eq!(edited["member"]["handle"], "gpt-6-astra-max");
            assert_eq!(edited["member"]["formerHandles"], json!(["claude-sonnet-5-5-medium"]));
            assert_eq!(edited["member"]["tier"], "complex");
            assert_eq!(edited["member"]["roles"], json!(["worker", "tester"]));
            assert_eq!(
                staffing.runs("gpt-6-astra-max"),
                (
                    "codex".to_owned(),
                    Some("gpt-6-astra".into()),
                    Some("max".into()),
                    Some("gpt-6-astra".into())
                )
            );
            assert_eq!(staffing.tier_of("gpt-6-astra-max").as_deref(), Some("complex"));
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
/// member of its own beside a worker, whichever is added first; a second designer is that
/// designer again, and holds no model for a worker on Codex's model to be named like.
#[tokio::test]
async fn a_worker_and_an_image_designer_are_two_members_in_either_order() {
    LocalSet::new()
        .run_until(async {
            let worker = json!({ "harness": "codex", "model": "gpt-6.1-sol", "effort": "high", "tier": "light", "roles": ["worker"] });
            let designer = json!({ "harness": "codex", "roles": ["designer"] });
            // A worker first, then the designer.
            let first = staffing();
            let added = first.add(worker.clone()).await;
            assert_eq!(added["member"]["handle"], "gpt-6-1-sol-high", "{added}");
            let drawn = first.add(designer.clone()).await;
            assert_eq!(drawn["ok"], true, "{drawn}");
            assert_eq!(drawn["member"]["handle"], "codex-default");
            assert_eq!(first.member("gpt-6-1-sol-high").roles, ["worker"]);
            assert!(!first.member("gpt-6-1-sol-high").designer);
            // The designer first, then a worker.
            let staffing = staffing();
            let drawn = staffing.add(designer.clone()).await;
            assert_eq!(drawn["member"]["handle"], "codex-default", "{drawn}");
            let added = staffing.add(worker).await;
            assert_eq!(added["ok"], true, "{added}");
            assert_eq!(added["member"]["handle"], "gpt-6-1-sol-high");
            assert_eq!(added["member"]["designer"], false);
            let again = staffing.add(designer).await;
            assert_eq!(again["member"]["handle"], "codex-default", "{again}");
            assert_eq!(staffing.member("codex-default").roles, ["designer"]);
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
                .await
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

/// The chiefs picked in the tests below: (harness, model as the picker sends it, effort, model
/// held, the id the harness lists for it).
const LISTED_PICKS: [(&str, &str, &str, &str, &str); 5] = [
    ("devin", "swe-2", "max", "swe-2-max", "swe-2"),
    (
        "claude-code",
        "claude-opus-5-5",
        "high",
        "claude-opus-5-5",
        "claude-opus-5-5",
    ),
    (
        "codex",
        "gpt-6.1-sol",
        "ultra",
        "gpt-6.1-sol",
        "gpt-6.1-sol",
    ),
    (
        "pi",
        "anthropic/claude-fable-5",
        "max",
        "anthropic/claude-fable-5",
        "anthropic/claude-fable-5",
    ),
    (
        "muse",
        "muse-wonder-1.0",
        "low",
        "muse-wonder-1.0",
        "muse-wonder-1.0",
    ),
];

/// A chief is held with the id its harness lists for its model beside the id its launch is
/// given, as a member is (T-1345, the issue T-1335 found): picked in New project, a Devin
/// chief's `model` is the exact id its launch is given (`swe-2-max`) and its `model_name` the
/// id Devin lists (`swe-2`), which its card says; every other harness lists the id it takes.
#[tokio::test]
async fn a_chief_picked_in_new_project_keeps_the_id_its_harness_lists_beside_its_launch_id() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            for (harness, model, effort, held, listed) in LISTED_PICKS {
                let opened = ask(
                    &staffing.app,
                    "project.open",
                    json!({
                        "directory": format!("/work/listed-{harness}"),
                        "harness": harness, "model": model, "effort": effort, "staff": [],
                    }),
                )
                .await;
                assert_eq!(opened["ok"], true, "{harness}: {opened}");
                let chief = staffing.chief_of(opened["project"]["id"].as_i64().unwrap());
                assert_eq!(
                    (chief.model.as_deref(), chief.model_name.as_deref()),
                    (Some(held), Some(listed)),
                    "{harness}"
                );
            }
        })
        .await;
}

/// The same for Switch chief, in one project from one pick to the next: each switch replaces
/// the name with the new pick's, so none outlives its model.
#[tokio::test]
async fn a_chief_switched_to_keeps_the_id_its_harness_lists_beside_its_launch_id() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            for (harness, model, effort, held, listed) in LISTED_PICKS {
                let switched = staffing
                    .driven(
                        "chief.switch",
                        json!({
                            "project": staffing.project,
                            "harness": harness, "model": model, "effort": effort, "when": "now",
                        }),
                    )
                    .await;
                assert_eq!(switched["ok"], true, "{harness}: {switched}");
                let chief = staffing.chief_of(staffing.project);
                assert_eq!(
                    (chief.model.as_deref(), chief.model_name.as_deref()),
                    (Some(held), Some(listed)),
                    "{harness}"
                );
            }
        })
        .await;
}

impl Staffing {
    /// What the app asks and gets back, the engine's own work run meanwhile as the daemon's
    /// executor runs it (the kit's is run by whoever drives the test): a switch waits on it.
    async fn driven(&self, operation: &str, body: Value) -> Value {
        let answer = ask(&self.app, operation, body);
        tokio::pin!(answer);
        loop {
            tokio::select! {
                answer = &mut answer => return answer,
                () = tokio::task::yield_now() => self.rig.kit.settle(),
            }
        }
    }

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

// --- a submission is checked against the list its picker was shown (A4-28, T-1321) ---

impl Staffing {
    /// `models.get` of `kind`'s harness (by its program's name, `devin`): the answer, which names
    /// the read the page's picker shows.
    async fn reads(&self, harness: &str) -> Value {
        ask(&self.app, "models.get", json!({ "harness": harness })).await
    }

    /// How many members of the project have `word` in their handle.
    fn members_named_after(&self, word: &str) -> usize {
        self.rig
            .kit
            .ledger
            .borrow()
            .project(self.project)
            .unwrap()
            .unwrap()
            .participants
            .iter()
            .filter(|participant| participant.handle.contains(word))
            .count()
    }

    /// How many times the harness of `kind` was asked for its models.
    fn asked_of(&self, kind: &str) -> usize {
        self.lists_of(kind).asked.borrow().len()
    }
}

/// Devin's model and effort as the recorded listing has them: `claude-opus-5-5-fast` at `medium`
/// is launched as `claude-opus-5-5-medium-fast`.
fn devin_pick(extra: Value) -> Value {
    let mut body = json!({
        "harness": "devin",
        "model": "claude-opus-5-5-fast",
        "effort": "medium",
        "tier": "standard",
    });
    body.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    body
}

const DEVIN_RUNS: (&str, &str, &str, &str) = (
    "devin",
    "claude-opus-5-5-medium-fast",
    "medium",
    "claude-opus-5-5-fast",
);

fn runs_of(staffing: &Staffing, handle: &str) -> (String, String, String, String) {
    let (harness, model, effort, name) = staffing.runs(handle);
    (
        harness,
        model.unwrap_or_default(),
        effort.unwrap_or_default(),
        name.unwrap_or_default(),
    )
}

/// The token of a read of another daemon's life, whose count is the one this daemon's first read
/// has: a dialog that was open as the daemon restarted holds one (T-1329, A).
const FOREIGN_READ: &str = "0000000000000000.1";

#[tokio::test]
async fn a_submission_is_checked_against_the_read_its_picker_was_shown_not_the_last_one() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let first = staffing.reads("devin").await;
            assert_eq!(first["ok"], true, "{first}");
            let shown = first["read"].as_str().expect("a read token").to_owned();
            // Another dialog reads Devin after it changed what it lists.
            let mut later = recorded("devin");
            later.models.retain(|model| model.id == "adaptive");
            staffing.lists_of("devin").set(later);
            let second = staffing.reads("devin").await;
            let last = second["read"].as_str().expect("a read token").to_owned();
            assert_ne!(shown, last, "each read has an id of its own");

            // The pick made from the first list is checked against it, and holds Devin's exact id.
            let added = staffing.add(devin_pick(json!({ "read": shown }))).await;
            assert_eq!(added["ok"], true, "{added}");
            let handle = added["member"]["handle"].as_str().unwrap().to_owned();
            let (harness, model, effort, name) = DEVIN_RUNS;
            assert_eq!(
                runs_of(&staffing, &handle),
                (harness.into(), model.into(), effort.into(), name.into())
            );
            // The same pick from the list read last is refused: it lists no such model.
            let refused = staffing.add(devin_pick(json!({ "read": last }))).await;
            assert_eq!(
                refused["error"],
                "devin does not list claude-opus-5-5-fast: pick one of its models",
                "{refused}"
            );
        })
        .await;
}

#[tokio::test]
async fn a_submission_naming_no_read_the_daemon_keeps_reads_the_harness_again_and_is_checked_against_that(
) {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            assert_eq!(staffing.asked_of("devin"), 0);
            // No read named: the harness is read for the submission.
            let added = staffing.add(devin_pick(json!({}))).await;
            assert_eq!(added["ok"], true, "{added}");
            assert_eq!(
                staffing.asked_of("devin"),
                1,
                "a read made for the submission"
            );
            // A read this daemon does not keep (or never made) is no read at all: a token of another
            // daemon's life is one, whatever count it carries (a dialog open as the daemon restarted),
            // and the count of this daemon's first read, "1", is no exception (T-1329, A).
            let again = staffing
                .add(devin_pick(json!({ "read": FOREIGN_READ, "tier": "light" })))
                .await;
            assert_eq!(again["ok"], true, "{again}");
            assert_eq!(
                staffing.asked_of("devin"),
                2,
                "read again, not taken at the page's word"
            );
            // And what that read does not list is refused.
            let mut changed = recorded("devin");
            changed.models.retain(|model| model.id == "adaptive");
            staffing.lists_of("devin").set(changed);
            let refused = staffing
                .add(devin_pick(json!({ "read": FOREIGN_READ })))
                .await;
            assert_eq!(
                refused["error"],
                "devin does not list claude-opus-5-5-fast: pick one of its models",
                "{refused}"
            );
        })
        .await;
}

#[tokio::test]
async fn a_failed_read_is_no_list_to_check_against_the_harness_is_read_again_and_devins_mapping_holds(
) {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            // The picker's read failed: it showed no model, and its read names no list.
            staffing.lists_of("devin").fail("Not logged in");
            let failed = staffing.reads("devin").await;
            assert_eq!(failed["listing"]["models"], json!([]), "{failed}");
            let failed_read = failed["read"].as_str().unwrap().to_owned();

            // Devin can be read again: the pick is checked against that read, and is Devin's
            // exact id for the effort, never `swe-2` and `high` taken verbatim.
            staffing.lists_of("devin").set(recorded("devin"));
            let added = staffing
                .add(devin_pick(json!({ "read": failed_read })))
                .await;
            assert_eq!(added["ok"], true, "{added}");
            let handle = added["member"]["handle"].as_str().unwrap().to_owned();
            let (harness, model, effort, name) = DEVIN_RUNS;
            assert_eq!(
                runs_of(&staffing, &handle),
                (harness.into(), model.into(), effort.into(), name.into()),
                "the mapping holds on the failed-read path"
            );

            // Devin still cannot be read: nothing is accepted unchecked, and no member is made.
            staffing.lists_of("devin").fail("Not logged in");
            let before = staffing.members_named_after("claude-opus-5-5");
            let refused = staffing
                .add(devin_pick(json!({ "read": failed_read, "tier": "light" })))
                .await;
            assert_eq!(
                refused["error"],
                "devin's models could not be read, so claude-opus-5-5-fast cannot be checked: Not logged in",
                "{refused}"
            );
            assert_eq!(staffing.members_named_after("claude-opus-5-5"), before);
        })
        .await;
}

#[tokio::test]
async fn an_edit_that_leaves_the_model_and_effort_as_they_are_saves_the_stored_choice_with_no_list()
{
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let added = staffing.add(devin_pick(json!({}))).await;
            let handle = added["member"]["handle"].as_str().unwrap().to_owned();
            staffing.lists_of("devin").fail("Not logged in");
            let edit = |extra: Value| {
                let mut body = devin_pick(json!({ "project": staffing.project, "member": handle, "tier": "light" }));
                body.as_object_mut()
                    .unwrap()
                    .extend(extra.as_object().unwrap().clone());
                ask(&staffing.app, "member.edit", body)
            };
            // Unchanged: the member's own, already resolved launch choice is saved; only the tier moves.
            let saved = edit(json!({ "unchanged": true })).await;
            assert_eq!(saved["ok"], true, "{saved}");
            let (harness, model, effort, name) = DEVIN_RUNS;
            assert_eq!(
                runs_of(&staffing, saved["member"]["handle"].as_str().unwrap()),
                (harness.into(), model.into(), effort.into(), name.into())
            );
            assert_eq!(
                staffing.tier_of(saved["member"]["handle"].as_str().unwrap()),
                Some("light".to_owned())
            );
            // Not said to be unchanged, the same words are a pick, which nothing can check.
            let unchecked = edit(json!({})).await;
            assert_eq!(
                unchecked["error"],
                "devin's models could not be read, so claude-opus-5-5-fast cannot be checked: Not logged in",
                "{unchecked}"
            );
            // And unchanged names the member's own harness, no other.
            let other = edit(json!({ "unchanged": true, "harness": "codex" })).await;
            assert_eq!(
                other["error"],
                "an edit that leaves the model unchanged keeps the member's own harness",
                "{other}"
            );
        })
        .await;
}

#[tokio::test]
async fn a_read_that_is_no_token_is_refused() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            for read in [json!(5), json!(""), json!(["a.1"]), json!(true)] {
                let refused = staffing.add(devin_pick(json!({ "read": read }))).await;
                assert_eq!(
                    refused["error"], "read is the token models.get answered with",
                    "{read}: {refused}"
                );
            }
        })
        .await;
}

/// Devin's `swe-2` at `high` is launched as the exact id it lists, `swe-2-high`, on every path a
/// pick takes and whatever read id it names: the read its picker was shown, an id the daemon
/// never made (another daemon's, FOREIGN_READ), the read of a harness that failed, and none
/// (T-1329, C: the reviewer's four states). No variable makes a daemon take the pair verbatim.
#[tokio::test]
async fn swe_2_at_high_is_checked_and_launched_as_swe_2_high_on_every_pick_path_and_read_state() {
    LocalSet::new()
        .run_until(async {
            for state in ["known", "unknown", "failed", "none"] {
                let s = staffing();
                if state == "failed" {
                    s.lists_of("devin").fail("Not logged in");
                }
                let read = match state {
                    "known" | "failed" => s.reads("devin").await["read"].clone(),
                    "unknown" => json!(FOREIGN_READ),
                    _ => Value::Null,
                };
                s.lists_of("devin").set(recorded("devin"));
                let choice = json!({
                    "harness": "devin", "model": "swe-2", "effort": "high",
                    "tier": "standard", "read": read,
                });
                // Add member.
                let added = s.add(choice.clone()).await;
                assert_eq!(added["ok"], true, "{state}: {added}");
                let handle = added["member"]["handle"].as_str().unwrap().to_owned();
                assert_eq!(s.runs(&handle).1.as_deref(), Some("swe-2-high"), "{state}");
                // Edit member and Project settings: the same pick, changed in its tier.
                let mut edit = choice.clone();
                edit["project"] = json!(s.project);
                edit["member"] = json!(handle);
                edit["tier"] = json!("light");
                let edited = ask(&s.app, "member.edit", edit).await;
                assert_eq!(edited["ok"], true, "{state}: {edited}");
                let edited_handle = edited["member"]["handle"].as_str().unwrap().to_owned();
                assert_eq!(
                    s.runs(&edited_handle).1.as_deref(),
                    Some("swe-2-high"),
                    "{state}"
                );
                // New project: the chief and a staff member.
                let mut member = choice.clone();
                member["roles"] = json!(["worker"]);
                let mut opening = choice.clone();
                opening["directory"] = json!(format!("/work/four-states-{state}"));
                opening["staff"] = json!([member]);
                let opened = ask(&s.app, "project.open", opening).await;
                assert_eq!(opened["ok"], true, "{state}: {opened}");
                let project = opened["project"]["id"].as_i64().unwrap();
                assert_eq!(s.chief_of(project).model.as_deref(), Some("swe-2-high"));
                let staff = s
                    .rig
                    .kit
                    .ledger
                    .borrow()
                    .project(project)
                    .unwrap()
                    .unwrap()
                    .participants;
                assert!(
                    staff
                        .iter()
                        .any(|p| p.role == "worker" && p.model.as_deref() == Some("swe-2-high")),
                    "{state}"
                );
                // Switch chief.
                let mut switch = choice.clone();
                switch["project"] = json!(s.project);
                switch["when"] = json!("now");
                let target = crate::page::chief::chief_asked(
                    &s.rig.page,
                    crate::page::body::Body::new(&switch),
                    true,
                )
                .await
                .unwrap();
                assert_eq!(target.model, "swe-2-high", "{state}");
            }
        })
        .await;
}

impl Staffing {
    /// The scout of `cf_ledger::roles::seam` in the table while the guard lives, and the
    /// ledger's CHECKs off, which name the five roles of today alone.
    fn with_a_scout(&self) -> cf_ledger::roles::seam::Put {
        let put = cf_ledger::roles::seam::put(&cf_ledger::roles::seam::WITH_A_SCOUT);
        self.rig
            .kit
            .ledger
            .borrow()
            .sqlite()
            .execute_batch("PRAGMA ignore_check_constraints = ON")
            .unwrap();
        put
    }
}

/// A member whose roles have no tier (a role of the table alone, the scout, which is no image
/// agent) is added on the model picked with no tier, and kept in the lowest, as an image
/// designer is; a tier asked for it is refused, and Edit member moves it with none.
#[tokio::test]
async fn a_member_whose_roles_have_no_tier_is_added_and_edited_with_none() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let _scout = staffing.with_a_scout();
            let added = staffing
                .add(json!({ "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "roles": ["scout"] }))
                .await;
            assert_eq!(added["ok"], true, "{added}");
            assert_eq!(added["member"]["roles"], json!(["scout"]));
            assert_eq!(staffing.member("claude-opus-5-5-high").tier.as_deref(), Some("light"));
            let refused = staffing
                .add(json!({ "harness": "claude-code", "model": "claude-sonnet-5-5", "effort": "medium", "tier": "complex", "roles": ["scout"] }))
                .await;
            assert_eq!(
                refused,
                json!({ "ok": false, "error": "a field scout takes no tier: its tasks have none" })
            );
            let edited = ask(
                &staffing.app,
                "member.edit",
                json!({
                    "project": staffing.project,
                    "member": "claude-opus-5-5-high",
                    "harness": "codex",
                    "model": "gpt-6-astra",
                    "effort": "max",
                }),
            )
            .await;
            assert_eq!(edited["ok"], true, "{edited}");
            assert_eq!(edited["member"]["handle"], "gpt-6-astra-max");
            assert_eq!(staffing.member("gpt-6-astra-max").tier.as_deref(), Some("light"));
            // An edit that keeps its model and effort is asked no tier either.
            let kept = ask(
                &staffing.app,
                "member.edit",
                json!({
                    "project": staffing.project,
                    "member": "gpt-6-astra-max",
                    "harness": "codex",
                    "unchanged": true,
                }),
            )
            .await;
            assert_eq!(kept["ok"], true, "{kept}");
            assert_eq!(staffing.member("gpt-6-astra-max").tier.as_deref(), Some("light"));
        })
        .await;
}

/// A member that holds a role with tiers and one without keeps the tier it has for the role
/// with tiers: a scout's role added to a worker on its model leaves its tier as it was, and
/// such a member added at once is asked for a tier.
#[tokio::test]
async fn a_member_holding_a_role_with_tiers_and_one_without_keeps_its_tier() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let _scout = staffing.with_a_scout();
            let worker = staffing
                .add(json!({ "harness": "claude-code", "model": "claude-sonnet-5-5", "effort": "medium", "tier": "complex", "roles": ["worker"] }))
                .await;
            assert_eq!(worker["ok"], true, "{worker}");
            let scout = staffing
                .add(json!({ "harness": "claude-code", "model": "claude-sonnet-5-5", "effort": "medium", "roles": ["scout"] }))
                .await;
            assert_eq!(scout["ok"], true, "{scout}");
            let member = staffing.member("claude-sonnet-5-5-medium");
            assert_eq!(
                (member.roles, member.tier.as_deref()),
                (vec!["worker".to_owned(), "scout".to_owned()], Some("complex"))
            );
            let both = staffing
                .add(json!({ "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "roles": ["scout", "worker"] }))
                .await;
            assert_eq!(
                both,
                json!({ "ok": false, "error": "pick the member's tier: critical, complex, standard or light" })
            );
        })
        .await;
}

/// A member whose roles have no tier was given none (spec new-roles, FD-04): a worker's role it
/// gains comes with the tier the human chose. Add member onto an integrator on the same model
/// takes the tier of the add; `member.roles` takes the `tier` it is sent, refuses the change
/// without one in the ledger's words, and leaves a member that drops its role with tiers in
/// the lowest.
#[tokio::test]
async fn an_integrator_gains_a_workers_role_only_with_the_tier_the_human_chose() {
    LocalSet::new()
        .run_until(async {
            let staffing = staffing();
            let integrator = staffing
                .add(json!({ "harness": "claude-code", "model": "claude-sonnet-5-5", "effort": "medium", "roles": ["integrator"] }))
                .await;
            assert_eq!(integrator["ok"], true, "{integrator}");
            let worker = staffing
                .add(json!({ "harness": "claude-code", "model": "claude-sonnet-5-5", "effort": "medium", "tier": "standard", "roles": ["worker"] }))
                .await;
            assert_eq!(worker["ok"], true, "{worker}");
            let member = staffing.member("claude-sonnet-5-5-medium");
            assert_eq!(
                (member.roles, member.tier.as_deref()),
                (vec!["integrator".to_owned(), "worker".to_owned()], Some("standard"))
            );

            let alone = ask(
                &staffing.app,
                "member.roles",
                json!({ "project": staffing.project, "agent": "claude-sonnet-5-5-medium", "roles": ["integrator"] }),
            )
            .await;
            assert_eq!(alone["ok"], true, "{alone}");
            assert_eq!(staffing.member("claude-sonnet-5-5-medium").tier.as_deref(), Some("light"));

            let untold = ask(
                &staffing.app,
                "member.roles",
                json!({ "project": staffing.project, "agent": "claude-sonnet-5-5-medium", "roles": ["integrator", "worker"] }),
            )
            .await;
            assert_eq!(
                untold,
                json!({ "ok": false, "error": "claude-sonnet-5-5-medium has no tier: a worker's tasks have one, so its tier comes with the role" })
            );
            let told = ask(
                &staffing.app,
                "member.roles",
                json!({ "project": staffing.project, "agent": "claude-sonnet-5-5-medium", "roles": ["integrator", "worker"], "tier": "complex" }),
            )
            .await;
            assert_eq!(told["ok"], true, "{told}");
            let member = staffing.member("claude-sonnet-5-5-medium");
            assert_eq!(
                (member.roles, member.tier.as_deref()),
                (vec!["integrator".to_owned(), "worker".to_owned()], Some("complex"))
            );
        })
        .await;
}

// BKV-04: page operations preserve a backup as a separate row and carry its link
// through New project. These tests use the recorded harness catalogs above.
async fn bkv_page_owner(s: &Staffing) -> String {
    let added = s
        .add(json!({
            "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high",
            "tier": "standard", "roles": ["worker"]
        }))
        .await;
    assert_eq!(added["ok"], true, "{added}");
    added["member"]["handle"].as_str().unwrap().to_owned()
}

fn bkv_page_backup(s: &Staffing, owner: &str) -> String {
    let member = cf_ledger::NewMember::from_json(&json!({
        "harness": "codex", "model": "gpt-6.1-sol", "modelName": "gpt-6.1-sol",
        "effort": "high", "tier": "standard", "roles": ["worker"]
    }))
    .unwrap();
    s.rig
        .kit
        .ledger
        .borrow_mut()
        .add_backup(s.project, owner, &member)
        .unwrap()
        .handle
}

fn bkv_page_link(s: &Staffing, project: i64) -> Vec<Value> {
    let board = s.rig.kit.ledger.borrow().board(project).unwrap();
    serde_json::to_value(board).unwrap()["lanes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|lane| lane["participant"].clone())
        .collect()
}

#[tokio::test]
async fn bkv_page_add_backup_and_refuse_a_second_or_a_chain() {
    LocalSet::new().run_until(async {
        let s = staffing();
        let owner = bkv_page_owner(&s).await;
        let body = json!({"project": s.project, "member": owner, "harness": "codex", "model": "gpt-6.1-sol", "effort": "high"});
        let added = ask(&s.app, "backup.add", body.clone()).await;
        assert_eq!(added["ok"], true, "{added}");
        let backup = added["member"]["handle"].as_str().unwrap();
        let lanes = bkv_page_link(&s, s.project);
        let owner_id = lanes.iter().find(|p| p["handle"] == owner).unwrap()["id"].clone();
        let backup_row = lanes.iter().find(|p| p["handle"] == backup).unwrap();
        assert_eq!(backup_row["backupOf"], owner_id);
        assert_eq!(backup_row["roles"], json!(["worker"]));
        assert_eq!(backup_row["tier"], "standard");
        let again = ask(&s.app, "backup.add", body.clone()).await;
        assert_eq!(again["ok"], false, "{again}");
        assert!(again["error"].as_str().unwrap().contains("already has a backup"), "{again}");
        let mut chain = body;
        chain["member"] = json!(backup);
        let refused = ask(&s.app, "backup.add", chain).await;
        assert_eq!(refused["ok"], false, "{refused}");
        assert!(refused["error"].as_str().unwrap().contains("backup"), "{refused}");
    }).await;
}

#[tokio::test]
async fn bkv_page_change_backup_replaces_its_row_and_keeps_the_owner() {
    LocalSet::new().run_until(async {
        let s = staffing();
        let owner = bkv_page_owner(&s).await;
        let old = bkv_page_backup(&s, &owner);
        let changed = ask(&s.app, "backup.change", json!({
            "project": s.project, "member": owner, "harness": "muse", "model": "muse-wonder-1.0", "effort": "high"
        })).await;
        assert_eq!(changed["ok"], true, "{changed}");
        let lanes = bkv_page_link(&s, s.project);
        let owner_id = lanes.iter().find(|p| p["handle"] == owner).unwrap()["id"].clone();
        assert!(!lanes.iter().any(|p| p["handle"] == old));
        assert_eq!(lanes.iter().filter(|p| p["backupOf"] == owner_id).count(), 1);
        assert_eq!(lanes.iter().find(|p| p["backupOf"] == owner_id).unwrap()["harness"], "muse");
    }).await;
}

#[tokio::test]
async fn bkv_page_remove_backup_and_refuse_when_none() {
    LocalSet::new()
        .run_until(async {
            let s = staffing();
            let owner = bkv_page_owner(&s).await;
            let backup = bkv_page_backup(&s, &owner);
            let body = json!({"project": s.project, "member": owner});
            let removed = ask(&s.app, "backup.remove", body.clone()).await;
            assert_eq!(removed["ok"], true, "{removed}");
            assert!(!bkv_page_link(&s, s.project)
                .iter()
                .any(|p| p["handle"] == backup));
            let again = ask(&s.app, "backup.remove", body).await;
            assert_eq!(again["ok"], false, "{again}");
            assert!(
                again["error"].as_str().unwrap().contains("has no backup"),
                "{again}"
            );
        })
        .await;
}

#[tokio::test]
async fn bkv_page_open_from_last_staff_preserves_the_backup_link() {
    LocalSet::new().run_until(async {
        let s = staffing();
        let owner = bkv_page_owner(&s).await;
        bkv_page_backup(&s, &owner);
        let last = ask(&s.app, "staff.last", json!({})).await;
        assert_eq!(last["ok"], true, "{last}");
        let rows = last["staff"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1]["backupOf"], rows[0]["id"]);
        let opened = ask(&s.app, "project.open", json!({
            "directory": "/work/bkv-copy", "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high",
            "staff": rows,
        })).await;
        assert_eq!(opened["ok"], true, "{opened}");
        let project = opened["project"]["id"].as_i64().unwrap();
        let lanes = bkv_page_link(&s, project);
        let prim = lanes.iter().find(|p| p["model"] == "claude-opus-5-5" && p["role"] == "worker").unwrap();
        let backup = lanes.iter().find(|p| p["model"] == "gpt-6.1-sol").unwrap();
        assert_eq!(backup["backupOf"], prim["id"], "{lanes:?}");
        assert_ne!(prim["id"], rows[0]["id"]);
    }).await;
}

#[tokio::test]
async fn bkv_page_adding_a_primary_does_not_merge_into_the_same_model_backup() {
    LocalSet::new().run_until(async {
        let s = staffing();
        let owner = bkv_page_owner(&s).await;
        let backup = bkv_page_backup(&s, &owner);
        let added = s.add(json!({"harness": "codex", "model": "gpt-6.1-sol", "effort": "high", "tier": "standard", "roles": ["worker"]})).await;
        assert_eq!(added["ok"], true, "{added}");
        assert_ne!(added["member"]["handle"], backup);
        let lanes = bkv_page_link(&s, s.project);
        assert_eq!(lanes.iter().filter(|p| p["model"] == "gpt-6.1-sol").count(), 2);
        assert_eq!(lanes.iter().filter(|p| p["model"] == "gpt-6.1-sol" && !p["backupOf"].is_null()).count(), 1);
    }).await;
}

#[tokio::test]
async fn bkv_page_every_harness_can_back_every_other_harness() {
    LocalSet::new()
        .run_until(async {
            let choices = [
                ("claude-code", "claude-opus-5-5", "high"),
                ("codex", "gpt-6.1-sol", "high"),
                ("pi", "anthropic/claude-fable-5", "high"),
                ("devin", "swe-2", "high"),
                ("muse", "muse-wonder-1.0", "high"),
            ];
            for (primary_harness, primary_model, primary_effort) in choices {
                for (backup_harness, backup_model, backup_effort) in choices {
                    let s = staffing();
                    let added = s
                        .add(json!({"harness": primary_harness, "model": primary_model,
                    "effort": primary_effort, "tier": "complex", "roles": ["reviewer"]}))
                        .await;
                    assert_eq!(added["ok"], true, "{primary_harness}: {added}");
                    let owner = added["member"]["handle"].as_str().unwrap();
                    let backup = ask(
                        &s.app,
                        "backup.add",
                        json!({"project": s.project, "member": owner,
                    "harness": backup_harness, "model": backup_model, "effort": backup_effort}),
                    )
                    .await;
                    assert_eq!(
                        backup["ok"], true,
                        "{primary_harness} with {backup_harness}: {backup}"
                    );
                    let lanes = bkv_page_link(&s, s.project);
                    let owner = lanes.iter().find(|p| p["handle"] == owner).unwrap();
                    let backup = lanes.iter().find(|p| p["backupOf"] == owner["id"]).unwrap();
                    assert_eq!(backup["harness"], backup_harness);
                    assert_eq!(backup["roles"], json!(["reviewer"]));
                    assert_eq!(backup["tier"], "complex");
                }
            }
        })
        .await;
}

#[tokio::test]
async fn bkv_page_open_without_staff_copies_link_and_vacation_from_last_staff() {
    LocalSet::new().run_until(async {
        let s = staffing();
        let owner = bkv_page_owner(&s).await;
        let backup = bkv_page_backup(&s, &owner);
        s.rig.kit.ledger.borrow_mut().send_on_vacation(s.project, &owner, &[]).unwrap();
        s.rig.kit.ledger.borrow_mut().send_on_vacation(s.project, &backup, &[]).unwrap();
        let opened = ask(&s.app, "project.open", json!({"directory": "/work/bkv-last", "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high"})).await;
        assert_eq!(opened["ok"], true, "{opened}");
        let lanes = bkv_page_link(&s, opened["project"]["id"].as_i64().unwrap());
        let owner = lanes.iter().find(|p| p["role"] == "worker" && p["backupOf"].is_null()).unwrap();
        let backup = lanes.iter().find(|p| p["backupOf"] == owner["id"]).unwrap();
        assert!(owner["vacationSince"].is_string());
        assert!(backup["vacationSince"].is_string());
    }).await;
}

#[tokio::test]
async fn bkv_page_bad_copy_link_refuses_the_whole_project() {
    LocalSet::new().run_until(async {
        let s = staffing();
        let before = s.rig.kit.ledger.borrow().projects().unwrap().len();
        let opened = ask(&s.app, "project.open", json!({
            "directory": "/work/bkv-invalid", "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high",
            "staff": [{"id": 40, "backupOf": 99, "harness": "codex", "model": "gpt-6.1-sol", "effort": "high", "roles": ["worker"], "tier": "standard"}]
        })).await;
        assert_eq!(opened["ok"], false, "{opened}");
        assert_eq!(s.rig.kit.ledger.borrow().projects().unwrap().len(), before);
    }).await;
}

#[tokio::test]
async fn bkv_page_open_without_staff_copies_a_designers_backup_on_another_harness() {
    LocalSet::new().run_until(async {
        let s = staffing();
        let added = s.add(json!({"harness": "codex", "roles": ["designer"]})).await;
        assert_eq!(added["ok"], true, "{added}");
        let owner = added["member"]["handle"].as_str().unwrap();
        let backup = cf_ledger::NewMember::from_json(&json!({"harness": "claude-code", "model": "claude-opus-5-5", "effort": "high", "tier": "light", "roles": ["worker"]})).unwrap();
        s.rig.kit.ledger.borrow_mut().add_backup(s.project, owner, &backup).unwrap();
        let opened = ask(&s.app, "project.open", json!({"directory": "/work/bkv-designer", "harness": "claude-code", "model": "claude-opus-5-5", "effort": "high"})).await;
        assert_eq!(opened["ok"], true, "{opened}");
        let lanes = bkv_page_link(&s, opened["project"]["id"].as_i64().unwrap());
        let owner = lanes.iter().find(|p| p["designer"] == true && p["backupOf"].is_null()).unwrap();
        let backup = lanes.iter().find(|p| p["backupOf"] == owner["id"]).unwrap();
        assert_eq!(backup["harness"], "claude-code");
        assert_eq!(backup["roles"], json!(["designer"]));
    }).await;
}

#[tokio::test]
async fn bkv_page_the_chief_has_no_backup_and_bad_changes_keep_the_old_one() {
    LocalSet::new().run_until(async {
        let s = staffing();
        let chief = ask(&s.app, "backup.add", json!({"project": s.project, "member": "chief", "harness": "codex", "model": "gpt-6.1-sol", "effort": "high"})).await;
        assert_eq!(chief["ok"], false, "{chief}");
        let owner = bkv_page_owner(&s).await;
        let backup = bkv_page_backup(&s, &owner);
        let refused = ask(&s.app, "backup.change", json!({"project": s.project, "member": owner, "harness": "codex", "model": "not-listed", "effort": "high"})).await;
        assert_eq!(refused["ok"], false, "{refused}");
        assert!(refused["error"].as_str().unwrap().contains("does not list"), "{refused}");
        assert!(bkv_page_link(&s, s.project).iter().any(|p| p["handle"] == backup && !p["backupOf"].is_null()));
    }).await;
}

#[tokio::test]
async fn bkv_page_last_staff_carries_each_rows_out_kind_and_vacation() {
    LocalSet::new()
        .run_until(async {
            let s = staffing();
            let owner = bkv_page_owner(&s).await;
            let backup = bkv_page_backup(&s, &owner);
            let owner_id = s.member(&owner).id;
            let backup_id = s.member(&backup).id;
            {
                let mut ledger = s.rig.kit.ledger.borrow_mut();
                ledger
                    .mark_out_with_kind(
                        owner_id,
                        "2099-01-01T00:00:00.000Z",
                        "quota",
                        cf_ledger::HoldKind::Quota,
                    )
                    .unwrap();
                ledger
                    .mark_out_with_kind(
                        backup_id,
                        "2099-01-01T00:00:00.000Z",
                        "login",
                        cf_ledger::HoldKind::Account,
                    )
                    .unwrap();
                ledger.send_on_vacation(s.project, &backup, &[]).unwrap();
            }
            let last = ask(&s.app, "staff.last", json!({})).await;
            assert_eq!(last["ok"], true, "{last}");
            let rows = last["staff"].as_array().unwrap();
            let owner = rows.iter().find(|row| row["id"] == owner_id).unwrap();
            let backup = rows.iter().find(|row| row["backupOf"] == owner_id).unwrap();
            assert_eq!(owner["outKind"], "quota", "{owner}");
            assert_eq!(backup["outKind"], "account", "{backup}");
            assert!(backup["vacationSince"].is_string(), "{backup}");
        })
        .await;
}
