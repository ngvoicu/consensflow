//! `cf manages the roster`: the agents a person adds, lists, edits and removes
//! with `cf agent`, and the other things the command says of itself.

use std::ffi::OsString;
use std::path::Path;

use cf_e2e::{cf, files, ScratchHome};
use serde_json::json;
use url::Url;

use crate::{agent, Outcome};

#[test]
fn runs_the_native_cf() -> Outcome {
    let home = ScratchHome::new()?;
    // The native cf is given no Node for the catalog: that it answers says it
    // is the native cf that did, and did not hand the verb on.
    let ran = home.cf(["catalog", "--harness", "pi"])?;
    assert_eq!(ran.code, Some(0), "{ran}");
    assert!(ran.stdout.starts_with("pi:\n"), "{ran}");
    Ok(())
}

// Which cf ran is told by the processes that started, not by the selection:
// the native cf serves the catalog with no Node at all, and a selection that
// came to a cf that handed the verb to a Node process would start one.
#[test]
fn is_the_native_cf_no_node_process_ran() -> Outcome {
    let home = ScratchHome::new()?;
    let marks = home.root().join("node-runs");
    // A preload that has every Node process say it started: which cf ran is told by it.
    let spy = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join("node-spy.mjs");
    let spy = Url::from_file_path(&spy)
        .map_err(|()| format!("{} is not an absolute path", spy.display()))?;
    let ran = home.cf_with(
        [
            ("NODE_OPTIONS", OsString::from(format!("--import={spy}"))),
            ("CF_TEST_SPY", marks.clone().into_os_string()),
        ],
        ["catalog", "--harness", "pi"],
    )?;
    assert_eq!(ran.code, Some(0), "{ran}");
    let started: Vec<String> = if marks.exists() {
        let text = files::read_string(&marks)?;
        text.lines()
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        Vec::new()
    };
    assert_eq!(started, Vec::<String>::new(), "a Node process ran");
    Ok(())
}

#[test]
fn adds_lists_edits_and_removes_an_agent() -> Outcome {
    let home = ScratchHome::new()?;
    let added = home.cf([
        "agent",
        "add",
        "mine",
        "--harness",
        "claude",
        "--model",
        "claude-opus-5",
        "--effort",
        "max",
    ])?;
    assert_eq!(added.code, Some(0), "{added}");

    let listed = home.cf(["agent", "list"])?;
    assert!(listed.stdout.contains("mine"), "{listed}");
    assert!(listed.stdout.contains("claude-opus-5"), "{listed}");

    let as_json = home.cf(["agent", "list", "--json"])?.json()?;
    assert_eq!(agent(&as_json, "mine")["effort"], "max", "{as_json}");

    let edited = home.cf(["agent", "edit", "mine", "--model", "claude-fable-5-1"])?;
    assert_eq!(edited.code, Some(0), "{edited}");

    let removed = home.cf(["agent", "remove", "mine"])?;
    assert_eq!(removed.code, Some(0), "{removed}");
    let after = home.cf(["agent", "list"])?;
    assert!(!after.stdout.contains("mine"), "{after}");
    Ok(())
}

#[test]
fn adds_an_image_agent_a_codex_agent_that_designs_on_no_other_harness() -> Outcome {
    let home = ScratchHome::new()?;
    let added = home.cf([
        "agent",
        "add",
        "my-draw",
        "--harness",
        "codex",
        "--model",
        "codex-image",
        "--designer",
    ])?;
    assert_eq!(added.code, Some(0), "{}", added.stderr);
    let listed = home.cf(["agent", "list", "--json"])?.json()?;
    let draw = agent(&listed, "my-draw");
    assert_eq!(
        json!([
            draw["harness"],
            draw["designer"],
            draw["profile"]["modelLabel"]
        ]),
        json!(["codex", true, "Codex Images"]),
        "{listed}"
    );
    let elsewhere = home.cf([
        "agent",
        "add",
        "pi-draw",
        "--harness",
        "pi",
        "--model",
        "x",
        "--designer",
    ])?;
    assert_eq!(elsewhere.code, Some(1), "{elsewhere}");
    assert!(
        elsewhere.stderr.contains("an image agent is a Codex agent"),
        "{elsewhere}"
    );
    // `image` is no harness of its own any more.
    let old = home.cf([
        "agent",
        "add",
        "old-draw",
        "--harness",
        "image",
        "--model",
        "x",
    ])?;
    assert!(old.stderr.contains("unknown harness \"image\""), "{old}");
    home.cf(["agent", "remove", "my-draw"])?;
    Ok(())
}

#[test]
fn refuses_a_flag_it_would_ignore_and_writes_nothing_for_it() -> Outcome {
    let home = ScratchHome::new()?;
    for flag in [&["--dry-run"][..], &["--from", "x"], &["--presets", "x"]] {
        let ran = home.cf([
            "agent",
            "add",
            "trial",
            "--harness",
            "codex",
            "--model",
            "gpt-6-astra",
        ]
        .into_iter()
        .chain(flag.iter().copied()))?;
        assert_ne!(ran.code, Some(0), "{}: {ran}", flag[0]);
        assert!(
            ran.stderr
                .contains(&format!("Unknown option '{}'", flag[0])),
            "{ran}"
        );
    }
    let listed = home.cf(["agent", "list"])?;
    assert!(!listed.stdout.contains("trial"), "{listed}");
    Ok(())
}

#[test]
fn fails_an_unknown_verb_loudly() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf(["frobnicate"])?;
    assert_ne!(ran.code, Some(0), "{ran}");
    Ok(())
}

#[test]
fn prints_its_version() -> Outcome {
    let home = ScratchHome::new()?;
    let ran = home.cf(["--version"])?;
    assert!(ran.stdout.contains("3.0.0"), "{ran}");
    Ok(())
}

#[test]
#[cfg_attr(windows, ignore = "a POSIX shell pipeline")]
fn survives_its_output_pipe_closing_early_like_cf_head() -> Outcome {
    let home = ScratchHome::new()?;
    // `false` never reads: the pipe is closed before cf writes anything, so
    // every write EPIPEs. PIPESTATUS surfaces cf's own exit code.
    let script = format!(
        r#""{}" help | false; exit ${{PIPESTATUS[0]}}"#,
        cf::binary()?.display()
    );
    let path = format!("{}:/usr/bin:/bin", home.path_dir().display());
    let ran = home
        .command("/bin/bash")
        .args(["-c", &script])
        .var("PATH", path)
        .run()?;
    assert!(!ran.stderr.contains("EPIPE"), "{ran}");
    assert_eq!(ran.code, Some(0), "{ran}");
    Ok(())
}
