//! **Live: the posture a Muse member's window runs in** (MUS-16, T-790; Gabriel,
//! 2026-10-26: echo and fixtures are never the proof).
//!
//!     cargo xtask live muse-posture [--keep]
//!
//! ConsensFlow opens each member's window in its harness's full-permission mode, Muse's
//! in `--yolo`. This asks whether a softer mode of Muse's would do: one real turn on
//! `muse-spark-1.3` at `minimal` (contributor mode off) in each posture, in Muse's own
//! TUI in a pty as `live_muse_background` runs it (`live_muse/tui.rs`), each in a project
//! and folders of its own, outside every temporary folder (in the checkout's `target`).
//! The model runs one script that does what a member must be able to do:
//! - reach ConsensFlow's daemon with its `cf`: this checkout's `cf`, its `CONSENSFLOW_URL`
//!   a listener of the test's on the loopback, as a member's is the daemon's
//!   (`http://127.0.0.1:<port>`). `curl` is asked too, which goes through the proxy Muse's
//!   sandbox names in the environment where `cf` connects itself: it is said, not judged;
//! - commit in its project, a git work tree whose repository is outside it, as a
//!   member's tree's is;
//! - write a file outside its project, as a build's target or a tool's cache is.
//!
//! What came of each is read from the listener, the repository and the folder, never
//! from the model's words; a turn held past its limit (on a prompt nobody answers) is
//! said, and what it did by then is read the same way. Each window trusts its project
//! for the run (`--trust-workspace`, as `--yolo` does). The postures:
//! - `yolo`: `--yolo`, what ConsensFlow launches: no approval, no sandbox;
//! - `sandbox`: `--disable-approval`: no approval, Muse's sandbox with its default
//!   network (`proxy-only`);
//! - `network`: the same with `--sandbox-network enabled`.
//!
//! It passes while `yolo` does all three and each softer posture fails one at least,
//! which it says: once a softer posture does all three, the member's posture is due
//! another look. Its rules are `live_muse_background`'s: one live session at a time, a
//! stop at 90 % of a window, at once on a model Muse moved to by itself, and the owner's
//! Muse files unchanged.

#[path = "live_muse/tui.rs"]
mod tui;

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::live::{ask, Log};
use cf_e2e::process::Run;
use cf_e2e::{cf, checkout};

use tui::{
    another_model, binary, environment, other_tests_muse, owner_files, session_log, set_up, usage,
    Outcome, Window,
};

/// The postures, by name, with the flags each opens Muse with.
const POSTURES: [(&str, &[&str]); 3] = [
    ("yolo", &["--yolo"]),
    ("sandbox", &["--disable-approval"]),
    (
        "network",
        &["--disable-approval", "--sandbox-network", "enabled"],
    ),
];

/// How long a posture's turn may take.
const LIMIT: Duration = Duration::from_secs(180);

/// The path `curl` asks the listener for, which tells its requests from `cf`'s.
const CURL: &str = "/by-curl";

/// What one posture let the member do.
struct Done {
    /// The listener was reached by `cf`: a member's `cf` reaches the daemon.
    reached: bool,
    /// The listener was reached by `curl`, through the sandbox's proxy where there is one.
    curl: bool,
    committed: bool,
    wrote_outside: bool,
    /// What the script printed, as the tool's result in the log says (or why not).
    printed: String,
}

impl Done {
    fn all(&self) -> bool {
        self.reached && self.committed && self.wrote_outside
    }

    /// What it let the member do, in words.
    fn said(&self) -> String {
        format!(
            "cf reached the daemon's port {} (curl {}), committed in its work tree {}, wrote outside its project {}; the script printed: {}",
            self.reached, self.curl, self.committed, self.wrote_outside, self.printed
        )
    }
}

/// The request lines a listener was sent.
type Heard = Arc<Mutex<Vec<String>>>;

/// A listener on the loopback that answers each request and keeps its first line.
fn listen() -> Result<(u16, Heard), String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    let heard: Heard = Arc::default();
    let kept = Arc::clone(&heard);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let mut request = [0_u8; 2048];
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let read = stream.read(&mut request).unwrap_or(0);
            let line = String::from_utf8_lossy(&request[..read])
                .lines()
                .next()
                .unwrap_or("(nothing read)")
                .to_owned();
            if let Ok(mut heard) = kept.lock() {
                heard.push(line);
            }
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
        }
    });
    Ok((port, heard))
}

/// The script the model runs, which says how each of its steps ended.
fn script(port: u16, cf: &Path, outside: &Path) -> String {
    format!(
        "#!/bin/sh\n\
         CONSENSFLOW_URL=http://127.0.0.1:{port} CONSENSFLOW_TOKEN=probe CONSENSFLOW_PROJECT=1 CONSENSFLOW_PARTICIPANT=probe '{}' inbox > cf.out 2>&1; echo \"CF=$? $(head -c 160 cf.out)\"\n\
         curl -s -m 10 -o /dev/null http://127.0.0.1:{port}{CURL}; echo \"CURL=$?\"\n\
         echo probe > probe.txt\n\
         git add probe.txt && git -c user.name=probe -c user.email=probe@example.invalid commit -q -m probe; echo \"COMMIT=$?\"\n\
         echo outside > '{}'; echo \"OUTSIDE=$?\"\n",
        cf.display(),
        outside.join("written.txt").display()
    )
}

/// Makes `work` a git work tree of a repository beside it, `main`, with one commit.
fn work_tree(root: &Path) -> Result<(), String> {
    let (main, work) = (root.join("main"), root.join("work"));
    fs::remove_dir_all(&work).map_err(|error| error.to_string())?;
    fs::create_dir_all(&main).map_err(|error| error.to_string())?;
    fs::write(main.join("README.md"), "A throwaway project.\n")
        .map_err(|error| error.to_string())?;
    let who = [
        "-c",
        "user.name=probe",
        "-c",
        "user.email=probe@example.invalid",
    ];
    let steps: [Vec<&str>; 4] = [
        vec!["init", "-q"],
        vec!["add", "README.md"],
        [&who[..], &["commit", "-q", "-m", "start"]].concat(),
        vec!["worktree", "add", "-q", "-b", "probe", "../work"],
    ];
    for step in steps {
        let ran = Run::new("/usr/bin/git")
            .args(&step)
            .cwd(&main)
            .run()
            .map_err(|failed| failed.to_string())?;
        if ran.code != Some(0) {
            return Err(format!("git {step:?}: {}", ran.stderr));
        }
    }
    Ok(())
}

/// How many commits the project's branch has.
fn commits(work: &Path) -> usize {
    Run::new("/usr/bin/git")
        .args(["rev-list", "--count", "HEAD"])
        .cwd(work)
        .run()
        .ok()
        .filter(|ran| ran.code == Some(0))
        .and_then(|ran| ran.stdout.trim().parse().ok())
        .unwrap_or(0)
}

/// What the tool's result in the log says the script printed.
fn printed(log: &str) -> String {
    log.lines()
        .filter(|line| line.contains("tool_result_batch_committed") && line.contains("CF="))
        .map(|line| {
            let at = line.find("CF=").unwrap_or(0);
            line[at..].chars().take(600).collect::<String>()
        })
        .next_back()
        .unwrap_or_else(|| "no result of the script in the log".to_owned())
}

/// Runs one posture's turn in a home of its own under `under`, and reads what came of it.
fn run(name: &str, posture: &[&str], under: &Path, log: &Log) -> Result<Done, String> {
    let binary = binary()?;
    let root = tempfile::Builder::new()
        .prefix(&format!("live-muse-posture-{name}-"))
        .tempdir_in(under)
        .map_err(|error| error.to_string())?;
    let root_path = root.path().to_path_buf();
    set_up(&root_path)?;
    work_tree(&root_path)?;
    let outside = root_path.join("outside");
    fs::create_dir_all(&outside).map_err(|error| error.to_string())?;
    let (port, heard) = listen()?;
    let work = root_path.join("work");
    let cf = cf::binary().map_err(|failed| failed.to_string())?;
    fs::write(work.join("probe.sh"), script(port, cf, &outside))
        .map_err(|error| error.to_string())?;
    let env = environment(&root_path);
    let spent = usage(&binary, &env, &root_path, log)?;
    if spent.is_empty() || spent.iter().any(|spent| *spent >= 90.0) {
        return Err(format!("the allowance says {spent:?}: stopped"));
    }
    let owner_before = owner_files();
    let mut window = Window::open(&binary, &env, &root_path, posture)?;
    let turn = watch(&mut window, &root_path);
    let tail = window.tail();
    window.close();
    // A window that never came up, or moved to another model, proves nothing of its posture.
    let ended = turn.map_err(|why| format!("{name}: {why}; the window's last bytes: {tail}"))?;
    let after = usage(&binary, &env, &root_path, log);
    log.say(format!("{name}: usage after: {after:?}"));
    if owner_files() != owner_before {
        return Err(format!("{name}: the owner's Muse files changed"));
    }
    let text = session_log(&root_path)
        .and_then(|(_, file)| fs::read_to_string(file).ok())
        .unwrap_or_default();
    let heard = heard.lock().map(|heard| heard.clone()).unwrap_or_default();
    log.say(format!("{name}: the listener heard {heard:?}"));
    let done = Done {
        reached: heard.iter().any(|line| !line.contains(CURL)),
        curl: heard.iter().any(|line| line.contains(CURL)),
        committed: commits(&work) > 1,
        wrote_outside: outside.join("written.txt").is_file(),
        printed: if ended {
            printed(&text)
        } else {
            format!(
                "the turn did not end in {LIMIT:?} ({}); the window's last bytes: {tail}",
                printed(&text)
            )
        },
    };
    if cf_e2e::live::keep_requested() {
        log.say(format!("kept: {}", root.keep().display()));
    }
    Ok(done)
}

/// Pastes the turn's words once the window is up, and waits for its turn to end: whether it
/// ended in the limit. A window that wrote no log, or a record naming another model, is
/// the error.
fn watch(window: &mut Window, root: &Path) -> Result<bool, String> {
    let started = Instant::now();
    loop {
        if started.elapsed() > Duration::from_secs(40) {
            return Err("the window wrote no session log in 40 s".to_owned());
        }
        if session_log(root).is_some_and(|(_, file)| {
            fs::read_to_string(file).is_ok_and(|text| text.contains("\"session.started\""))
        }) {
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    thread::sleep(Duration::from_secs(2));
    window.paste(
        "Use the bash tool once to run exactly this command: sh probe.sh . Then reply with \
         exactly what it printed. Do not run anything else.",
    );
    while started.elapsed() < LIMIT {
        thread::sleep(Duration::from_millis(500));
        let Some((_, file)) = session_log(root) else {
            continue;
        };
        let text = fs::read_to_string(&file).unwrap_or_default();
        if let Some(model) = text.lines().find_map(another_model) {
            return Err(format!("the record names another model, {model}: stopped"));
        }
        if text.contains("\"kind\":\"terminal\",\"terminal\"") {
            return Ok(true);
        }
    }
    Ok(false)
}

#[test]
#[ignore = "live: one real Muse turn per posture on the owner's login (cargo xtask live muse-posture)"]
fn only_full_permission_lets_a_muse_member_reach_the_daemon_commit_and_write_outside_its_project(
) -> Outcome {
    tui::on_a_mac()?;
    let log = Log::own_agents(
        "live muse-posture",
        "it runs Muse on muse-spark-1.3 at minimal",
    )
    .map_err(|failed| failed.to_string())?;
    ask::prepare().map_err(|failed| failed.to_string())?;
    let under = checkout::path("app/src-tauri/target");
    let mut seen: BTreeMap<&str, Done> = BTreeMap::new();
    for (name, posture) in POSTURES {
        let others = other_tests_muse();
        if !others.is_empty() {
            return Err(format!(
                "another test runs Muse now (pids {others:?}): one live session at a time"
            ));
        }
        let done = run(name, posture, &under, &log)?;
        log.say(format!("{name} ({}): {}", posture.join(" "), done.said()));
        seen.insert(name, done);
    }
    let said = seen
        .iter()
        .map(|(name, done)| format!("{name}: {}", done.said()))
        .collect::<Vec<_>>()
        .join("\n");
    let yolo = seen.get("yolo").is_some_and(Done::all);
    let softer = seen
        .iter()
        .filter(|(name, _)| **name != "yolo")
        .all(|(_, done)| !done.all());
    if !yolo {
        return Err(format!(
            "`--yolo` did not let the member do all three: {said}"
        ));
    }
    if !softer {
        return Err(format!(
            "a softer posture did all three: the member's posture is due another look: {said}"
        ));
    }
    Ok(())
}
