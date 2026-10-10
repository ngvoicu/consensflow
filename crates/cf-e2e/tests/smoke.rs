//! The packaged smoke: the REAL `.app`, not this checkout.
//!
//! Every other suite reaches into the checkout's crates and builds what it
//! runs. This one is the only place that asks whether the thing a person
//! double-clicks works: the bundle's own page, the bundle's own `cf` (it is the
//! daemon, and the command a window runs), the production Tauri commands, and a
//! real PTY child. So it resolves NOTHING of the product from the repository,
//! and every path it asserts on lives under `Contents/`. What it takes from the
//! repository is this file, the proof of the agents screens
//! ([`cf_e2e::agents_proof`]), which only speaks HTTP to the daemon the app
//! started and reads the roster that daemon wrote, and a program of its own
//! ([`PASTE_READER`]) that the stand-in harness runs. The bundle ships no Node,
//! and this runs none.
//!
//! The app is started in its self-test mode as the release workflow's signed
//! one is: `cargo xtask smoke` (`npm run smoke`) runs it on the app a build
//! leaves, and on the one `--app` or `CONSENSFLOW_SMOKE_APP` names. The case is
//! ignored by `cargo test`, which has no built app to give it, and run by that
//! command; run, a missing bundle is a failure that names the build command. A
//! smoke that quietly passes because there was nothing to test is worse than
//! none.
//!
//! The environment it reads: `CONSENSFLOW_SMOKE_APP` (the app, a relative path
//! from the checkout's root), `CONSENSFLOW_SMOKE_TIMEOUT_MS` (how long each
//! wait is given: 180000) and `CONSENSFLOW_SMOKE_KEEP=1` (keep the machine of a
//! run that passed: a failed one is always kept).
//!
//! It is a macOS bundle's: `Contents/`, `Info.plist`. The Windows app is held by
//! `app/scripts/windows-smoke.mjs`.
#![cfg(target_os = "macos")]

// The parts are in `smoke/`: cargo looks for the modules of a test's root file
// beside it, where each would be a suite of its own.
#[path = "smoke/app.rs"]
mod app;
#[path = "smoke/bundle.rs"]
mod bundle;
#[path = "smoke/extension.rs"]
mod extension;
#[path = "smoke/machine.rs"]
mod machine;

use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::time::Duration;

use cf_e2e::agents_proof::{self, Daemon as Target};
use cf_e2e::daemon_log::{self, Kind};
use cf_e2e::process::{is_alive, own_var, Run};
use cf_e2e::{files, http};
use regex::Regex;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use url::Url;

use app::App;
use machine::{parse_pids, Machine, FLOOD_LINES, FLOOD_WIDTH};

/// What a case ends with: nothing, or why it could not be run. A check that
/// does not hold is a panic, which is the case's failure.
type Outcome = Result<(), Box<dyn std::error::Error>>;

/// What the stand-in harness runs to read a large paste, built for this test.
const PASTE_READER: &str = env!("CARGO_BIN_EXE_smoke-paste-reader");

/// How long each wait is given, unless `CONSENSFLOW_SMOKE_TIMEOUT_MS` says.
const PATIENCE_MS: u64 = 180_000;

/// What the flood is by CONSTRUCTION — not what xterm still holds.
///
/// These lines wrap far past the emulator's 10 000-row scrollback, so the rows
/// on screen can never add up to the bytes sent. The size that matters is this
/// one: it is over the 1 MiB unacked-output window, so the LAST line can only
/// arrive if the page kept returning credit.
const FLOOD_BYTES: usize = FLOOD_LINES * (FLOOD_WIDTH + "CFSMOKE-FLOOD 1234 ".len() + 1);

// A flood that no longer exceeds the output window proves nothing of the acks.
const _: () = assert!(
    FLOOD_BYTES > 1024 * 1024,
    "the flood no longer exceeds the output window"
);

/// How long a wait is given, from what `CONSENSFLOW_SMOKE_TIMEOUT_MS` says if it says.
fn patience_of(said: Option<&str>) -> Result<Duration, String> {
    match said.filter(|said| !said.is_empty()) {
        None => Ok(Duration::from_millis(PATIENCE_MS)),
        Some(said) => said.parse().map(Duration::from_millis).map_err(|_| {
            format!("CONSENSFLOW_SMOKE_TIMEOUT_MS is a number of milliseconds, not {said}")
        }),
    }
}

/// `bytes` as the lower-case hex a child that hexes what it is given writes.
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The bytes `text` writes in hex, none if it is not.
fn from_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok())
        .collect()
}

#[test]
#[ignore = "opt-in, on a built app: cargo xtask smoke (npm run smoke)"]
fn the_built_app_opens_a_pane_renders_a_real_child_takes_input_and_exits_clean() -> Outcome {
    let bundle = bundle::locate(&bundle::requested(
        own_var("CONSENSFLOW_SMOKE_APP").as_deref(),
    ))?;
    bundle::assert_no_node(&bundle.app)?;
    let patience = patience_of(own_var("CONSENSFLOW_SMOKE_TIMEOUT_MS").as_deref())?;

    // A failed smoke leaves its machine behind on purpose: the harness, its pid
    // file, the state root and the app's own launchers are the evidence, and
    // rebuilding to look at them again costs minutes. A passing one tidies up
    // (`CONSENSFLOW_SMOKE_KEEP=1` has it keep the machine too).
    let machine = Machine::new(Path::new(PASTE_READER))?;
    let outcome = drive(&bundle, &machine, patience, &mut std::io::stdout());
    let keep = own_var("CONSENSFLOW_SMOKE_KEEP").as_deref() == Some("1");
    machine.say_where_kept(&mut std::io::stderr(), outcome.is_err() || keep);
    outcome?;
    if !keep {
        machine.cleanup()?;
    }
    Ok(())
}

/// What the smoke does with the app of `bundle`, started on `machine`: each
/// step of it, in the order the app and the page take them. What it learned
/// that is worth saying of a run, it says to `said` as it learns it: which
/// daemon ran, and what was proved of its agents.
fn drive(
    bundle: &bundle::Bundle,
    machine: &Machine,
    patience: Duration,
    said: &mut impl Write,
) -> Outcome {
    let mut app = App::start(
        Run::new(&bundle.binary)
            .vars(machine.vars())
            .cwd(&machine.root),
        &machine.root,
        &machine.state,
        patience,
    )?;

    // 1. The page came from the bundle, not from a dev server or the checkout.
    let boot = app.wait_for("boot")?;
    assert_eq!(
        boot["data"]["protocol"], "tauri:",
        "the page loaded over {}",
        boot["data"]["protocol"]
    );
    let assets = boot["data"]["assets"].as_array().map(Vec::as_slice);
    assert!(
        assets.is_some_and(|assets| !assets.is_empty()),
        "the page reported no assets it loaded: {}",
        boot["data"]
    );
    for asset in assets.unwrap_or_default() {
        let asset = asset.as_str().unwrap_or_default();
        assert!(
            asset.starts_with("tauri://"),
            "the page loaded {asset}, which did not come from the bundle"
        );
    }

    // 2. A project, its chief window docked beside the board, and the fake
    //    harness's own first line drawn by the real xterm — through
    //    `project.open`, the production operation.
    let opened = app.wait_for("project")?;
    assert_eq!(
        opened["data"]["ok"], true,
        "project.open refused: {}",
        opened["data"]
    );
    // The daemon the app started: its own log says what it is, the native one,
    // and the app's says which `cf` it started, the bundle's.
    let log = files::read_string(&machine.state.join("daemon.log"))?;
    let started = log.lines().next().unwrap_or_default();
    let native =
        daemon_log::start_line(started, None).is_some_and(|start| start.kind == Kind::Native);
    assert!(
        native,
        "the daemon's first line is not the native one's: {started}"
    );
    writeln!(said, "the daemon that ran: {started}")?;
    let app_log = app::app_log(&machine.state).unwrap_or_default();
    let daemons = app::daemons_started(&app_log);
    assert!(
        !daemons.is_empty(),
        "the app's log says it started no daemon: {app_log}"
    );
    let bundled = std::fs::canonicalize(&bundle.cf)?;
    for daemon in &daemons {
        assert_eq!(
            std::fs::canonicalize(daemon).ok().as_deref(),
            Some(bundled.as_path()),
            "the app started {} as its daemon, and not the bundle's own cf ({})",
            daemon.display(),
            bundle.cf.display()
        );
    }

    let rendered = app.wait_for("rendered")?;
    let banner = rendered["data"]["banner"].as_str().unwrap_or_default();
    assert!(
        banner.contains(&format!("CFSMOKE-READY {}", machine.tag)),
        "the pane drew {banner:?}, not the harness's banner"
    );
    assert!(
        rendered["data"]["rows"].as_u64().unwrap_or(0) > 0,
        "the pane rendered no rows"
    );
    // The pane's own PATH, reported by the child that has to live with it.
    assert_eq!(
        rendered["data"]["tools"], "ok",
        "the chief pane inherited a PATH with no system commands on it"
    );

    // 3. Input typed through the page's own path reached the child: it came
    //    back as hex, which only the child computes.
    let echoed = app.wait_for("echo")?;
    let typed = echoed["data"]["typed"].as_str().unwrap_or_default();
    assert_eq!(typed, format!("cfsmoke-{}", machine.tag));
    assert_eq!(
        echoed["data"]["hex"].as_str().unwrap_or_default(),
        to_hex(typed.as_bytes()),
        "the child echoed something other than what was typed"
    );

    // The board both ways: the chief's task reached a worker window, and the
    // worker's result came back into the chief's window as a paste the child
    // hexed, header first.
    let board = app.wait_for("board")?;
    assert!(
        board["data"]["result"].is_u64(),
        "no result came back from the worker"
    );
    let delivered = from_hex(board["data"]["hex"].as_str().unwrap_or_default())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let header =
        Regex::new(r"^\[ConsensFlow m-\d+ · T-1 · result from @terpsichore-[a-z]+-[a-z]+\]")?;
    assert!(
        header.is_match(&delivered),
        "the chief was given {delivered:?}"
    );
    assert_eq!(
        board["data"]["delivered"], true,
        "the daemon never confirmed the delivery from the record"
    );

    // The agents screens, each in its dialog over the board, framed at the
    // daemon's page with the UI token.
    let screens = app.wait_for("agents-screens")?["data"]["screens"].clone();
    let dialogs: Vec<Value> = screens
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .map(|screen| json!([screen["name"], screen["open"]]))
        .collect();
    assert_eq!(
        Value::Array(dialogs),
        json!([["Agents", true], ["Harnesses", true]]),
        "{screens}"
    );
    let framed = |at: usize| screens[at]["src"].as_str().unwrap_or_default().to_owned();
    assert!(
        Regex::new(r"^http://localhost:\d+/\?token=")?.is_match(&framed(0)),
        "{}",
        framed(0)
    );
    assert!(
        Regex::new(r"^http://localhost:\d+/harnesses\?token=")?.is_match(&framed(1)),
        "{}",
        framed(1)
    );

    // The agents behind those screens, asked of the daemon that serves them, the
    // app's own, over its API and through the roster it writes. The packaged
    // build's own catalog, an agent saved with its profile, the screens behind
    // the UI token, and the deletion of the agent saved; nothing of the
    // product's modules is used to ask. The address and the token are the ones
    // the frame was given.
    let served = framed(0);
    let token = Url::parse(&served)?
        .query_pairs()
        .find(|(name, _)| name == "token")
        .map(|(_, value)| value.into_owned())
        .unwrap_or_default();
    agents_proof::prove(Target {
        url: http::origin(&served),
        token: &token,
        home: &machine.state,
    })?;
    writeln!(
        said,
        "the daemon's agents: the catalog, a saved profile, the screens, a deletion"
    )?;

    let pasted = app.wait_for("large-paste")?;
    let expected = format!("\x1b[200~{}\x1b[201~", "漢字 résumé 🙂\r".repeat(30_000)).into_bytes();
    assert_eq!(
        pasted["data"]["bytes"].as_u64(),
        Some(u64::try_from(expected.len())?)
    );
    assert_eq!(
        pasted["data"]["hash"].as_str(),
        Some(format!("{:x}", Sha256::digest(&expected)).as_str())
    );

    // 4. Acks flow. The flood is ~FLOOD_BYTES bytes by construction, well over
    //    the 1 MiB unacked-output window, so its LAST line can only be on
    //    screen if the page returned credit for everything before it. The ack
    //    count is the same fact from the page's side.
    let drained = app.wait_for("drained")?;
    assert_eq!(
        drained["data"]["lastFloodLine"].as_u64(),
        Some(u64::try_from(FLOOD_LINES)?)
    );
    let acks = drained["data"]["acks"].as_u64().unwrap_or(0);
    assert!(acks > 1, "only {acks} acks for {FLOOD_BYTES} bytes");

    let written = files::read_string(&machine.pid_file)?;
    let pids = parse_pids(&written).filter(|pids| pids.len() >= 2);
    let Some(pids) = pids else {
        panic!("the chief's and the worker's fake harnesses wrote no pids: {written:?}");
    };
    assert!(
        is_alive(pids[0]),
        "the chief's fake harness was not running when it answered"
    );

    // 5. The Pi extension the packaged `cf` installs loads from its own folder
    //    alone, never from this checkout. The bundle holds no copy of the
    //    extension to load: it is inside the `cf`, which writes it where Pi is
    //    told to load it from when it finds a Pi (`cf setup`, here with a
    //    stand-in `pi` on the PATH, and in a home of its own so the app's is not
    //    touched).
    let pi_bin = machine.probe.join("pi-bin");
    files::write_executable(&pi_bin.join("pi"), "#!/bin/sh\nexit 0\n")?;
    let setup_home = machine.probe.join("setup-home");
    let setup = machine
        .command(&bundle.cf)
        .arg("setup")
        .var("CONSENSFLOW_HOME", &setup_home)
        .var("PATH", format!("{}:{}", pi_bin.display(), machine.path()))
        .run()?;
    assert_eq!(setup.code, Some(0), "the packaged cf setup failed: {setup}");
    assert!(
        Regex::new(r"(?m)^harnesses: .*\bpi\b")?.is_match(&setup.stdout),
        "cf setup found no Pi: {}",
        setup.stdout
    );
    let installed = setup_home.join("extensions").join("pi");
    let bundles: Vec<_> = std::fs::read_dir(&installed)?.collect::<Result<_, _>>()?;
    assert_eq!(
        bundles.len(),
        1,
        "cf setup wrote {} Pi bundles: {:?}",
        bundles.len(),
        bundles
            .iter()
            .map(|entry| entry.file_name())
            .collect::<Vec<_>>()
    );
    // As the system names it: this machine's temporary folder is behind a link.
    let extension_root = std::fs::canonicalize(bundles[0].path())?;
    let extension = extension::extension_in(&extension_root);
    assert!(
        extension.exists(),
        "cf setup wrote no Pi extension at {}",
        extension.display()
    );
    assert!(
        extension::exports_a_function_by_default(&files::read_string(&extension)?),
        "the installed Pi extension has no default export that is a function"
    );
    assert_eq!(
        extension::strays(&extension_root, &extension)?,
        Vec::<String>::new(),
        "the installed extension loads from outside its own folder"
    );

    // 6. A second packaged `cf ui` on the app's home is a second ConsensFlow, and
    //    the ledger's exclusive lock refuses it. Exit 1 alone could be a
    //    dependency the bundle lacks, so it is the ledger's own words that must
    //    be on stderr, and no handle line on stdout.
    let second = machine
        .command(&bundle.cf)
        .args(["ui", "--json", "--no-open"])
        .run()?;
    assert!(!second.timed_out, "the second cf ui never ended: {second}");
    assert_eq!(second.code, Some(1), "the second cf ui ended: {second}");
    assert!(
        Regex::new(r"(?m)^cf: another ConsensFlow has .*consensflow\.db open$")?
            .is_match(&second.stderr),
        "the second cf ui was refused, but not for the ledger's lock: {}",
        second.stderr
    );
    assert_eq!(
        second.stdout, "",
        "the second cf ui printed a handle line: {}",
        second.stdout
    );

    // 7. The app's own exit: stdin EOF, `RunEvent::Exit`, and nothing left.
    //    (The bundle holds no Node and no sources, and cannot run any.)
    let settled = app.wait_for("settled")?;
    assert_eq!(settled["data"]["terminalPreserved"], true);
    app.quit();
    let Some(ended) = app.exit()? else {
        panic!(
            "the app did not end within {} ms of its input ending",
            patience.as_millis()
        );
    };
    assert_eq!(
        ended.code(),
        Some(0),
        "the app exited {:?} / {:?}",
        ended.code(),
        ended.signal()
    );
    let outlived: Vec<u32> = pids.into_iter().filter(|pid| is_alive(*pid)).collect();
    assert_eq!(
        outlived,
        Vec::<u32>::new(),
        "a fake harness outlived the app"
    );
    Ok(())
}

#[test]
fn the_wait_each_report_is_given_is_180_seconds_unless_the_variable_says_otherwise() {
    assert_eq!(patience_of(None), Ok(Duration::from_secs(180)));
    assert_eq!(patience_of(Some("")), Ok(Duration::from_secs(180)));
    assert_eq!(patience_of(Some("2500")), Ok(Duration::from_millis(2500)));
    assert_eq!(
        patience_of(Some("soon")),
        Err("CONSENSFLOW_SMOKE_TIMEOUT_MS is a number of milliseconds, not soon".to_owned())
    );
}

#[test]
fn what_a_child_hexes_is_read_back_whole_and_text_that_is_not_hex_is_none() {
    let text = "cfsmoke-ok 漢 🙂";
    assert_eq!(
        from_hex(&to_hex(text.as_bytes())).as_deref(),
        Some(text.as_bytes())
    );
    assert_eq!(to_hex(b"\x00\xff"), "00ff");
    assert_eq!(from_hex(""), Some(vec![]));
    for not_hex in ["abc", "zz", "漢字", "0g"] {
        assert_eq!(from_hex(not_hex), None, "{not_hex}");
    }
}
