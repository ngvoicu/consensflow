use std::rc::Rc;

use cf_process::Failed;
use serde_json::json;

use super::*;
use crate::pi::setup::Setup;
use crate::testing::{finished, ChildScript, Driver, Ends, ManualTime, ScriptedProcesses};
use cf_proto::models::WORDS_WITHHELD;

mod failures;
mod homes;
mod off_thread;
mod refresh;
mod standin;

/// When a test's list is read.
const AT: i64 = 1_793_000_000_000;

/// The first work `driver` settles, run until it does for up to ten seconds of real time (see
/// [`eventually`]).
pub(super) fn settled<T>(driver: &mut Driver<T>) -> T {
    let began = std::time::Instant::now();
    loop {
        if let Some((_, done)) = driver.run().into_iter().next() {
            return done;
        }
        assert!(
            began.elapsed() < std::time::Duration::from_secs(10),
            "work that waited on nothing it was given"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// `work`, polled until it is done, for up to ten seconds of real time: a read that finds no model
/// has Pi's folder read on a thread of its own (`setup::inspect`), which answers a moment later and
/// has no waker to wake a test that waits on nothing else.
pub(super) fn eventually<T>(mut work: crate::contract::Work<'_, T>) -> T {
    let began = std::time::Instant::now();
    loop {
        let polled = work
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop()));
        if let std::task::Poll::Ready(done) = polled {
            return done;
        }
        assert!(
            began.elapsed() < std::time::Duration::from_secs(10),
            "work that waited on nothing it was given"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// What Pi 1.4.0 answered `get_available_models` with on 2026-10-28, offline, a few rows of
/// each provider kept (`tools/probes/chief-harness/models.py pi`), and with no login.
const RECORDED: &str = include_str!("../../../tests/fixtures/models/pi-get-available-models.json");
const SIGNED_OUT: &str =
    include_str!("../../../tests/fixtures/models/pi-get-available-models-signed-out.json");

fn recorded() -> ModelListing {
    parse(&serde_json::from_str(RECORDED).unwrap(), AT).unwrap()
}

fn efforts_of(models: &ModelListing, model: &str) -> Vec<String> {
    models.model(model).expect(model).efforts.clone()
}

#[test]
fn each_model_keeps_its_provider_apart_and_launches_as_both_joined() {
    let models = recorded();
    let fable = models.model("openrouter/anthropic/claude-fable-5").unwrap();
    assert_eq!(fable.provider.as_deref(), Some("openrouter"));
    assert_eq!(fable.id, "anthropic/claude-fable-5");
    assert_eq!(
        fable.launch_model(Some("high")),
        "openrouter/anthropic/claude-fable-5"
    );
    assert_eq!(models.models.len(), 20);
    assert!(!models.is_stale());
    assert_eq!(models.how, HOW);
}

#[test]
fn the_levels_are_pi_s_own_rule_of_reasoning_and_the_thinking_map() {
    let models = recorded();
    assert_eq!(efforts_of(&models, "openai/gpt-4"), ["off"], "no reasoning");
    assert_eq!(
        efforts_of(&models, "anthropic/claude-haiku-4-5"),
        ["off", "minimal", "low", "medium", "high"],
        "reasoning and no map: no xhigh or max unless named"
    );
    assert_eq!(
        efforts_of(&models, "kimi-coding/k3"),
        ["low", "high", "max"],
        "a level mapped to null is not taken"
    );
    assert_eq!(
        efforts_of(&models, "anthropic/claude-fable-5"),
        ["minimal", "low", "medium", "high", "xhigh", "max"],
        "off is mapped to null; xhigh and max are named"
    );
    assert_eq!(
        efforts_of(&models, "meta/muse-spark-1.2"),
        ["minimal", "low", "medium", "high", "xhigh"]
    );
}

#[test]
fn a_contributor_model_pi_lists_is_marked_and_never_the_default() {
    let models = recorded();
    let contributor = models.model("meta/muse-spark-1.2-contributor").unwrap();
    assert!(contributor.contributor);
    let default = models.default_model().unwrap();
    assert!(!default.contributor);
}

#[test]
fn pi_with_no_login_lists_no_model_and_says_why() {
    let signed_out = parse(&serde_json::from_str(SIGNED_OUT).unwrap(), AT);
    assert_eq!(
        signed_out,
        Err(Unlisted::NoModel),
        "an empty list is told from the folder of Pi, which is read only then"
    );
    assert!(Setup::default()
        .unlisted()
        .starts_with("Pi lists no model: no provider is set up ("));
    let refused = parse(&json!({ "success": false, "error": "bad" }), AT);
    // What Pi gives as its reason is Pi's words, after the mark, whole.
    assert_eq!(
        refused,
        Err(Unlisted::Said(
            "pi refused get_available_models; it said:\nbad".to_owned()
        ))
    );
    let multiline = parse(
        &json!({ "success": false, "error": "line one\nline two\n" }),
        AT,
    );
    assert_eq!(
        multiline,
        Err(Unlisted::Said(
            "pi refused get_available_models; it said:\nline one\nline two".to_owned()
        )),
        "not cut to a line"
    );
    for none in [
        json!({ "success": false }),
        json!({ "success": false, "error": "  " }),
    ] {
        assert_eq!(
            parse(&none, AT),
            Err(Unlisted::Said(
                "pi refused get_available_models: no reason given".to_owned()
            )),
            "no words of Pi's, no mark"
        );
    }
}

#[test]
fn the_list_is_read_offline_with_the_packages_and_extensions_a_window_starts_with() {
    let program = program(PathBuf::from("pi"), &Env::default());
    assert_eq!(
        program.args,
        ["--mode", "rpc", "--no-session", "--no-mcp", "--offline"]
    );
    assert!(
        !program
            .args
            .iter()
            .any(|arg| arg == "--no-extensions" || arg == "-ne"),
        "Pi's --no-extensions leaves out settings.json's packages: a provider such as \
         pi-cliproxyapi-provider lists nothing (HMD-13)"
    );
    assert_eq!(program.env.text("PI_OFFLINE"), Some("1"));
}

/// A `pi` on the PATH of a throwaway folder, and what Pi's list and its update answer.
fn listed_with(
    update: Option<Result<String, Failed>>,
    fetch: bool,
) -> (Result<ModelListing, String>, Vec<String>) {
    let folder = tempfile::tempdir().unwrap();
    let pi = crate::testing::fake_executable(&folder.path().join("pi"));
    let env = Env::from_vars([("PATH", pi.parent().unwrap().as_os_str())]);
    let processes = Rc::new(ScriptedProcesses::default());
    if let Some(update) = update {
        processes.run_answer("pi update --models", update);
    }
    let answer = |id: &str| {
        json!({ "type": "response", "id": id, "success": true,
            "data": { "models": [{ "provider": "meta", "id": "spark", "name": "Spark" }] } })
        .to_string()
    };
    let lines = vec![answer("models")];
    processes.child(
        "pi",
        ChildScript {
            lines,
            ends: Ends::Asked,
        },
    );
    let time = Rc::new(ManualTime::new(0));
    let mut driver = Driver::default();
    let (asked_env, asked_processes, asked_time) =
        (env.clone(), Rc::clone(&processes), Rc::clone(&time));
    driver.begin(0, async move {
        listed(&asked_env, &*asked_processes, &*asked_time, fetch)
            .await
            .map_err(String::from)
    });
    let listed = driver.run().remove(0).1;
    let ran = processes
        .take_ran()
        .iter()
        .map(|(program, _)| crate::testing::named(program))
        .collect();
    (listed, ran)
}

#[test]
fn only_the_human_s_refresh_fetches_pi_s_catalogs_and_a_failed_fetch_leaves_a_stale_list() {
    let (listed, ran) = listed_with(None, false);
    assert!(!listed.unwrap().is_stale());
    assert!(ran.is_empty(), "nothing fetched: {ran:?}");

    let (listed, ran) = listed_with(Some(Ok(String::new())), true);
    assert!(!listed.unwrap().is_stale());
    assert_eq!(ran, ["pi update --models"]);

    let failed = Failed {
        message: "Command failed: pi update --models\noffline".to_owned(),
        code: Some(1),
        killed: false,
        stdout: String::new(),
    };
    let (listed, _) = listed_with(Some(Err(failed)), true);
    let listed = listed.unwrap();
    assert_eq!(
        listed.freshness,
        cf_proto::models::Freshness::Stale {
            why: "Pi's own catalog: pi update --models failed (exit 1); it said:\noffline"
                .to_owned()
        }
    );
    assert_eq!(listed.models.len(), 1, "the list Pi keeps");
}

fn update_failed(message: &str, code: Option<i32>) -> String {
    update_failure(&Failed {
        message: message.to_owned(),
        code,
        killed: false,
        stdout: String::new(),
    })
}

/// Why `pi update --models` failed: its exit code, and after the mark everything its error
/// stream held, whole (the third reading, F2: the last line alone left a label behind it).
#[test]
fn why_a_pi_update_failed_is_its_code_and_all_it_said_after_the_mark() {
    assert_eq!(
        update_failed(
            "Command failed: pi update --models\nfirst\nsecond\nthird\n",
            Some(2)
        ),
        "failed (exit 2); it said:\nfirst\nsecond\nthird"
    );
    // A program that never started has the system's words only, which are not Pi's.
    assert_eq!(
        update_failed("spawn pi ENOENT", None),
        "failed: spawn pi ENOENT"
    );
    // One that said nothing has no mark.
    assert_eq!(
        update_failed("Command failed: pi update --models\n", Some(1)),
        "failed (exit 1)"
    );
    // One that said as much as is kept said it whole; one that said more had its front let go
    // (T-1276: the label of a value can be in it), and none of what it said is handed on.
    let whole = "é".repeat(UPDATE_SAID_BYTES / 2);
    assert_eq!(
        update_failed(
            &format!("Command failed: pi update --models\n{whole}"),
            None
        ),
        format!("failed{IT_SAID}{whole}")
    );
    let many = "é".repeat(UPDATE_SAID_BYTES);
    let cut = update_failed(&format!("Command failed: pi update --models\n{many}"), None);
    assert_eq!(cut, format!("failed{IT_SAID}{WORDS_WITHHELD}"));
}

/// T-1276, `long-password-continuation` for `pi update --models`: the label stands before the
/// last `UPDATE_SAID_BYTES` of what it said, the words of its value after it.
#[test]
fn a_pi_update_whose_front_was_let_go_hands_on_none_of_its_words() {
    let label = "password: to\nred\nbird\ncedar\nflower\norchard\n";
    let rest = "to\nred\nbird\ncedar\nflower\norchard\n".repeat(UPDATE_SAID_BYTES / 20);
    let tail = "remainingviolet\nremainingwillow";
    let failed = update_failed(
        &format!("Command failed: pi update --models\n{label}{rest}{tail}"),
        Some(1),
    );
    assert_eq!(failed, format!("failed (exit 1){IT_SAID}{WORDS_WITHHELD}"));
}

/// Pi's default model, from a folder of its own whose `settings.json` says `settings`, a
/// `pi` on the PATH whose list offers `listed`.
fn default_with(
    settings: Option<Value>,
    listed: &[(&str, &str)],
) -> Result<Option<String>, crate::contract::NoDefault> {
    let folder = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(folder.path().join("bin")).unwrap();
    let pi = crate::testing::fake_executable(&folder.path().join("bin").join("pi"));
    let agent = folder.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    if let Some(settings) = settings {
        std::fs::write(agent.join("settings.json"), settings.to_string()).unwrap();
    }
    let env = Env::from_vars([
        ("PATH", pi.parent().unwrap().as_os_str()),
        ("PI_CODING_AGENT_DIR", agent.as_os_str()),
    ]);
    let processes = Rc::new(ScriptedProcesses::default());
    let models: Vec<Value> = listed
        .iter()
        .map(|(provider, id)| json!({ "provider": provider, "id": id, "name": id }))
        .collect();
    let answer = json!({ "type": "response", "id": "models", "success": true,
        "data": { "models": models } });
    processes.child(
        "pi",
        ChildScript {
            lines: vec![answer.to_string()],
            ends: Ends::Asked,
        },
    );
    let time = ManualTime::new(0);
    finished(Box::pin(default_model(&env, &*processes, &time)))
}

#[test]
fn pi_starts_on_its_settings_default_and_never_on_a_contributor_model() {
    let settings = |provider: &str, model: &str| {
        Some(json!({ "defaultProvider": provider, "defaultModel": model }))
    };
    let listed = [
        ("meta", "muse-spark-1.3-contributor"),
        ("openai", "gpt-6-luna"),
    ];
    assert_eq!(
        default_with(settings("openrouter", "anthropic/claude-fable-5"), &listed),
        Ok(Some("openrouter/anthropic/claude-fable-5".to_owned()))
    );
    // A contributor default is never the one: the first its list offers that is not.
    assert_eq!(
        default_with(settings("meta", "muse-spark-1.3-contributor"), &listed),
        Ok(Some("openai/gpt-6-luna".to_owned()))
    );
    // No default said: the same.
    assert_eq!(
        default_with(None, &listed),
        Ok(Some("openai/gpt-6-luna".to_owned()))
    );
    // Nothing but contributor models: none, and why.
    assert_eq!(
        default_with(None, &[("meta", "muse-spark-1.3-contributor")]),
        Err(crate::contract::NoDefault::NoneOffered(
            "Pi offers no model that is not a contributor model"
        ))
    );
}

#[test]
fn a_model_pi_does_not_have_is_said_in_its_providers_words_and_its_catalogs_warning_is_none() {
    // Pi's own warning of an id its offline catalog does not know: it runs it all the same,
    // and a model the catalog has not caught up with yet works (Calliope's P1, T-955).
    assert!(!gone(
        "Model 'gpt-gone-9' not found for provider 'openai'. Using custom model id.",
        "openai/gpt-gone-9"
    ));
    // The provider's refusal, as OpenAI said it on CHF-02's probe.
    assert!(gone(
        r#"400 {"error":{"code":"model_not_found","message":"The model `gpt-gone-9` does not exist"}}"#,
        "openai/gpt-gone-9"
    ));
    assert!(!gone(
        "429 rate limited: try again later",
        "openai/gpt-gone-9"
    ));
}

/// A Pi chief's model is `provider/id`, and the provider names the id alone: the words are
/// held to the id after the first slash, so an OpenRouter id keeps its own slash, and
/// Anthropic's `not_found_error` is a refusal as OpenAI's `model_not_found` is (Calliope's
/// P1, T-994).
#[test]
fn a_pi_refusal_names_the_id_after_its_provider_split_once_at_the_first_slash() {
    let anthropic = r#"404 {"type":"error","error":{"type":"not_found_error","message":"model: claude-fable-5"}}"#;
    assert!(gone(anthropic, "anthropic/claude-fable-5"));
    assert!(
        !gone(anthropic, "anthropic/claude-opus-5-5"),
        "another model"
    );
    // OpenRouter's id holds a slash of its own, which its refusal names whole.
    let openrouter = r#"404 {"error":{"code":"model_not_found","message":"No endpoints found for anthropic/claude-fable-5"}}"#;
    assert!(gone(openrouter, "openrouter/anthropic/claude-fable-5"));
    assert!(
        !gone(
            r#"404 {"error":{"code":"model_not_found","message":"No endpoints found for claude-fable-5"}}"#,
            "openrouter/anthropic/claude-fable-5"
        ),
        "split once: OpenRouter's id is not its last segment"
    );
    // A model with no provider is its own id.
    assert!(gone(anthropic, "claude-fable-5"));
}

/// Pi 1.4.0's windows on a model their provider lacks stay up, and the first turn fails in
/// the record's `errorMessage` (`gone_launch.py pi`, 2026-10-31): the `openai-codex`
/// provider in Codex's own words, with no `model_not_found`, and Anthropic's 404
/// `not_found_error`. Both refuse the model asked for, by its id alone.
#[test]
fn a_pi_window_on_a_model_its_provider_lacks_fails_in_that_providers_words() {
    let codex = "Codex error: The 'gpt-gone-9' model is not supported when using Codex with a ChatGPT account.";
    assert!(gone(codex, "openai-codex/gpt-gone-9"));
    assert!(!gone(codex, "openai-codex/gpt-6-luna"));
    let anthropic = r#"404 {"type":"error","error":{"type":"not_found_error","message":"model: claude-gone-9"},"request_id":"req_0"}"#;
    assert!(gone(anthropic, "anthropic/claude-gone-9"));
}

/// T-1328, the second reading of T-1322: Pi's own installer's shim at a path with a key's look and
/// blanks in it (`sk-abc defg hijk lmno\pi.cmd`), its launcher there and no Node anywhere. The
/// lookup of the default model for a chief whose model is gone failed with the shim's path in its
/// words, and the note to the human quoted them. The kind of failure rides beside the words
/// (`Said`, `NoDefault`): the page keeps the words and the chief's note has the kind alone.
#[test]
fn a_pi_shim_with_no_node_says_its_kind_to_the_chiefs_note_and_its_path_to_the_page_alone() {
    let folder = tempfile::tempdir().unwrap();
    let bin = folder.path().join("sk-abc defg hijk lmno");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("pi.cmd"),
        "@echo off\r\nnode \"%~dp0pi-launcher.js\" %*\r\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        // A file the machine would start, as the detection looks for one.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("pi.cmd"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    std::fs::write(bin.join("pi-launcher.js"), "").unwrap();
    let agent = folder.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let env = Env::from_vars([
        ("OS", "Windows_NT"),
        ("PATHEXT", ".COM;.EXE;.BAT;.CMD"),
        ("PATH", bin.to_str().unwrap()),
        ("PI_CODING_AGENT_DIR", agent.to_str().unwrap()),
    ]);
    let processes = crate::seams::processes::SystemProcesses::new(env.clone());
    let time = ManualTime::new(0);

    let said = finished(Box::pin(listed(&env, &processes, &time, false))).unwrap_err();
    assert!(
        said.words.contains("sk-abc defg hijk lmno") && said.words.contains("runs on Node"),
        "the page keeps the person's words: {said}"
    );
    let kind = cf_base::startup::StartFailure::of(cf_base::startup::StartKind::NoNodeForPi);
    assert_eq!(said.failure, Some(kind));

    let why = finished(Box::pin(default_model(&env, &processes, &time))).unwrap_err();
    assert_eq!(why, crate::contract::NoDefault::Unread(Some(kind)));
    let told = format!("{why:?} {}", kind.words("Pi"));
    assert!(!told.contains("sk-abc") && !told.contains("lmno"), "{told}");
    assert_eq!(
        kind.words("Pi"),
        "Node.js was not found for Pi; install Node 22.19 or newer, or run Pi's installer again"
    );
}

/// A Pi that is not there says `ENOENT` to the chief's note and not the file: the system's name for
/// it is a kind and a code, from a fixed table (T-1328: `spawn pi hunter22.cmd ENOENT` lost the
/// code with the path).
#[test]
fn a_pi_the_system_cannot_start_says_its_code_to_the_chiefs_note_and_not_its_file() {
    let folder = tempfile::tempdir().unwrap();
    let bin = folder.path().join("hunter22");
    std::fs::create_dir_all(&bin).unwrap();
    // A program of that name whose file the system cannot run: not an executable.
    std::fs::write(bin.join("pi"), "").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(bin.join("pi"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let agent = folder.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    let env = Env::from_vars([
        ("PATH", bin.to_str().unwrap()),
        ("PI_CODING_AGENT_DIR", agent.to_str().unwrap()),
    ]);
    let processes = crate::seams::processes::SystemProcesses::new(env.clone());
    let time = ManualTime::new(0);
    // The empty file is no program the system can run (ENOEXEC): whichever code the system names,
    // the note has the kind and the code and no word of the folder.
    // The system starts a child under a runtime of Tokio's.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let why = runtime
        .block_on(default_model(&env, &processes, &time))
        .unwrap_err();
    let crate::contract::NoDefault::Unread(Some(failure)) = why else {
        panic!("a failure typed by its producer: {why:?}");
    };
    let told = failure.words("Pi");
    assert!(!told.contains("hunter22") && !told.contains('/'), "{told}");
    assert!(told.starts_with("Pi's program "), "{told}");
}
