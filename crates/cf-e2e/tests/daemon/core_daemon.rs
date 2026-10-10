//! The cases that go through a process, run against the native daemon
//! (`npm run test:daemons`). What was a test of Node's own modules (the pass
//! loop, the API's doors, a Node preload) is held where the daemon is: the stop
//! and pass tests of `crates/cf-daemon/src/{stop,pass,errors}`, and the
//! recorded traces (`core-daemon-001`) it plays. What its start line says is
//! `rust 3.0.0`.

use std::path::Path;
use std::time::Instant;

use cf_e2e::cf;
use cf_e2e::daemon::{Daemon, Home};
use cf_e2e::daemon_log::lines_of;
use cf_e2e::files;
use cf_e2e::http::Http;
use regex::Regex;
use serde_json::{json, Value};

use crate::over_bridge::OverBridge;
use crate::{secs, Outcome};

/// A daemon already running on `home`, which holds its ledger: the other
/// ConsensFlow a second start is refused for. It has said it is ready.
fn running_daemon(home: &Home) -> cf_e2e::Result<Daemon> {
    let mut daemon = home.daemon()?;
    daemon.handle(secs(60))?;
    Ok(daemon)
}

/// What a second daemon on the same home says and does: it is refused. Waits
/// for it to end. Its error output, its code and its process id.
fn refused_second_start(home: &Home) -> cf_e2e::Result<(String, Option<i32>, u32)> {
    let mut second = home.daemon()?;
    let code = second.exit_code(secs(60))?;
    Ok((second.errors(), code, second.id()))
}

/// The daemon writes down what is worth knowing afterwards: its start, its
/// stop and why.
fn starts_its_log_with_its_pid_and_ends_it_with_why_it_stopped(
    end: impl Fn(&Daemon),
    reason: &str,
) -> Outcome {
    let home = Home::new()?;
    let mut daemon = home.daemon()?;
    daemon.handle(secs(60))?;
    end(&daemon);
    let code = daemon.exit_code(secs(60))?;
    assert_eq!(code, Some(0), "{}", daemon.errors());
    let log = home.log();
    // The runtime the start line names is the daemon's own: the native one's.
    let whole = Regex::new(&format!(
        r"^\S+ info start pid {} rust \S+ home \S+\n\S+ info stop: {reason}; rss \d+ MB\n\S+ info exit 0\n$",
        daemon.id()
    ))?;
    assert!(whole.is_match(&log), "{log}");
    Ok(())
}

#[test]
fn starts_its_log_with_its_pid_and_ends_it_with_why_it_stopped_its_input_ending() -> Outcome {
    starts_its_log_with_its_pid_and_ends_it_with_why_it_stopped(
        |daemon| daemon.end_input(),
        "stdin ended",
    )
}

#[test]
#[cfg_attr(windows, ignore = "Node on Windows never delivers SIGTERM")]
fn starts_its_log_with_its_pid_and_ends_it_with_why_it_stopped_sigterm() -> Outcome {
    starts_its_log_with_its_pid_and_ends_it_with_why_it_stopped(
        |daemon| daemon.terminate(),
        "SIGTERM",
    )
}

#[test]
fn touches_no_window_of_the_running_daemon_when_a_second_start_is_refused_its_ledger() -> Outcome {
    let home = Home::new()?;
    let launch = home
        .consensflow()
        .join("integrations")
        .join("claude")
        .join("0b9f2c1e-5d4a-4c3b-9a8f-7e6d5c4b3a21");
    // A daemon is running on the home, and a window of its has its files.
    let mut running = running_daemon(&home)?;
    files::write(&launch.join("settings.json"), "{}\n")?;
    let (errors, _, _) = refused_second_start(&home)?;
    let said = format!("{errors}{}", home.log());
    assert!(
        Regex::new(r"another ConsensFlow has .*consensflow\.db open")?.is_match(&said),
        "the second start is refused: {said}"
    );
    assert_eq!(
        files::read_string(&launch.join("settings.json"))?,
        "{}\n",
        "the running daemon's window keeps its files"
    );
    running.stop()?;
    Ok(())
}

#[test]
fn ends_cf_ui_with_1_when_a_second_start_is_refused_its_ledger_and_its_log_says_so() -> Outcome {
    let home = Home::new()?;
    let mut running = running_daemon(&home)?;
    let (errors, code, pid) = refused_second_start(&home)?;
    assert_eq!(code, Some(1), "{errors}");
    assert!(
        Regex::new(r"^cf: another ConsensFlow has .*consensflow\.db open\n$")?.is_match(&errors),
        "{errors}"
    );
    // What the process wrote, the log being the running daemon's too: the start
    // line, and right after it the exit logger's, which writes the code the
    // process ends with.
    let wrote = lines_of(&home.log(), pid);
    assert_eq!(wrote.len(), 2, "{}", wrote.join("\n"));
    assert!(
        Regex::new(&format!(r"^\S+ info start pid {pid} rust \S+ home \S+$"))?.is_match(&wrote[0]),
        "{}",
        wrote[0]
    );
    assert!(
        Regex::new(r"^\S+ info exit 1$")?.is_match(&wrote[1]),
        "{}",
        wrote[1]
    );
    running.stop()?;
    Ok(())
}

#[test]
fn starts_though_its_agents_file_cannot_be_used_and_says_why_in_its_log() -> Outcome {
    let home = Home::new()?;
    let file = home.consensflow().join("agents.json");
    // A hand edit's trailing comma: the roster refuses the file and saves nothing over it.
    let broken = "{\"schemaVersion\": 1, \"agents\": [{\"id\": \"mine\", \"kind\": \"codex\"},]}\n";
    files::write(&file, broken)?;
    let mut daemon = home.daemon()?;
    daemon
        .handle(secs(60))
        .map_err(|failed| format!("it did not start: {failed}"))?;
    daemon.end_input();
    assert_eq!(daemon.exit_code(secs(60))?, Some(0), "{}", daemon.errors());
    // The roster's words are under the line.
    let log = home.log();
    assert!(
        Regex::new(
            r"\n\S+ error the agents file could not be used\n {4}Your agents file .* is not valid JSON"
        )?
        .is_match(&log),
        "{log}"
    );
    assert_eq!(
        files::read_string(&file)?,
        broken,
        "the file is left as the human wrote it"
    );
    Ok(())
}

/// A saved agent of the human's own, on Claude Code.
fn mybuilder() -> Value {
    json!({
        "id": "mybuilder",
        "name": "Mybuilder",
        "kind": "claude-code",
        "model": "fake",
        "workTier": "standard",
    })
}

/// Whether `folder` holds the `cf` under test: the daemon's bundle, first on
/// every window's `PATH`. The daemon finds its folder from its own path as the
/// system gives it, and macOS gives a program's path by any link that last
/// reached its file (the gate's exported trees link to this build folder), so
/// the file is compared, not the words.
fn holds_the_cf_under_test(folder: &Path) -> cf_e2e::Result<bool> {
    let resolve = |path: &Path| {
        std::fs::canonicalize(path).map_err(|source| cf_e2e::Error::File {
            action: "resolve",
            path: path.to_path_buf(),
            source,
        })
    };
    let named = folder.join(format!("cf{}", std::env::consts::EXE_SUFFIX));
    Ok(resolve(&named)? == resolve(cf::binary()?)?)
}

#[test]
fn opens_a_window_with_the_agents_api_its_project_and_participant_and_the_bundled_cf_first_on_path_and_names_it_no_node(
) -> Outcome {
    let d = OverBridge::start(&[mybuilder()])?;
    let opened = d.request(
        "project.open",
        json!({
            "directory": d.home.workspace(),
            "agent": "mybuilder",
            "staff": [{ "agent": "mybuilder", "roles": ["worker", "reviewer"] }],
        }),
    )?;
    assert_eq!(opened["ok"], true, "{opened}");
    let open = d.until("opened the chief", || {
        d.frames()
            .into_iter()
            .find(|frame| frame["kind"] == "req" && frame["op"] == "pane.open")
    })?;
    let project = opened["project"]["id"].clone();
    assert_eq!(open["body"]["id"], format!("p{project}-chief").as_str());
    let env = &open["body"]["env"];
    let url = d.handle["url"]
        .as_str()
        .unwrap_or_default()
        .trim_end_matches('/');
    assert_eq!(env["CONSENSFLOW_URL"], url);
    assert_eq!(env["CONSENSFLOW_PROJECT"], project.to_string().as_str());
    assert_eq!(env["CONSENSFLOW_PARTICIPANT"], "chief");
    // The app bundles no Node, and the daemon names none to a window.
    assert!(env.get("CONSENSFLOW_NODE").is_none(), "{env}");
    let path_delimiter = if cfg!(windows) { ";" } else { ":" };
    let path = env["PATH"].as_str().unwrap_or_default();
    let (bundle, rest) = path.split_once(path_delimiter).unwrap_or((path, ""));
    let bundle = Path::new(bundle);
    assert!(holds_the_cf_under_test(bundle)?, "{path}");
    assert_eq!(rest, d.home.path_dir().display().to_string(), "{path}");
    assert!(
        Regex::new(r"^\S+$")?.is_match(env["CONSENSFLOW_TOKEN"].as_str().unwrap_or_default()),
        "{env}"
    );
    // The chief's role text: its staff with their roles and tiers, and the cf of this window.
    let argv: Vec<&str> = open["body"]["argv"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let at = argv
        .iter()
        .position(|word| *word == "--append-system-prompt-file");
    let file = at
        .and_then(|at| argv.get(at + 1))
        .copied()
        .unwrap_or_default();
    let role = files::read_string(Path::new(file))?;
    assert!(
        Regex::new(r"(?m)^\| mybuilder \| worker, reviewer \| Standard work \|$")?.is_match(&role),
        "{role}"
    );
    // On Windows, cf.exe, its path in forward slashes: Git Bash drops backslashes.
    let cf_path = if cfg!(windows) {
        format!("{}/cf.exe", bundle.display().to_string().replace('\\', "/"))
    } else {
        bundle.join("cf").display().to_string()
    };
    assert!(
        role.contains(&format!("Here `cf` is {cf_path}.")),
        "{}",
        &role[role.len().saturating_sub(600)..]
    );
    Ok(())
}

#[test]
fn tells_the_page_when_the_board_changes_and_when_a_change_to_the_agents_moves_a_members_tier(
) -> Outcome {
    let d = OverBridge::start(&[mybuilder()])?;
    d.request(
        "project.open",
        json!({
            "directory": d.home.workspace(),
            "agent": "mybuilder",
            "staff": [{ "agent": "mybuilder", "roles": ["worker"] }],
        }),
    )?;
    d.until("told the page of the new project", || {
        (!d.told("core").is_empty()).then_some(())
    })?;
    // The agents screens, as the app opens them with its token.
    let http = Http::new();
    let token = d.handle["token"].as_str().unwrap_or_default();
    let url = d.handle["url"].as_str().unwrap_or_default();
    // A new agent moves nobody's tier; a member's agent given another does.
    let added = http.send(
        "POST",
        &format!("{url}api/agents"),
        Some(token),
        Some(&json!({ "name": "myhelper", "harness": "claude", "model": "fake" })),
    )?;
    assert_eq!(added.status, 201);
    let edited = http.send(
        "PATCH",
        &format!("{url}api/agents/mybuilder"),
        Some(token),
        Some(&json!({ "workTier": "complex" })),
    )?;
    assert_eq!(edited.status, 200);
    // A round trip on the bridge: every event sent before its answer has been read.
    assert_eq!(d.request("ping", json!({}))?, json!({ "ok": true }));
    assert_eq!(d.told("roster").len(), 1);
    Ok(())
}

#[test]
fn stops_itself_when_the_apps_end_of_the_bridge_breaks() -> Outcome {
    let mut d = OverBridge::start(&[])?;
    // Nobody reads the daemon's output any more, and its next write finds out.
    d.hang_up()?;
    d.daemon
        .send(&cf_e2e::wire::request("r-1", "ping", &json!({})));
    let started = Instant::now();
    let code = d.daemon.exit_code(secs(10))?;
    assert!(code.is_some(), "still waiting after 10000 ms");
    assert!(started.elapsed() < secs(10));
    assert_eq!(code, Some(0), "{}", d.daemon.errors());
    let log = d.home.log();
    // The bridge's own words for what broke are under the line.
    assert!(
        Regex::new(r"\n\S+ error the bridge failed\n {4}bridge I/O error: ")?.is_match(&log),
        "{log}"
    );
    assert!(
        Regex::new(r"\n\S+ info stop: the bridge failed; rss \d+ MB\n\S+ info exit 0\n$")?
            .is_match(&log),
        "{log}"
    );
    Ok(())
}
