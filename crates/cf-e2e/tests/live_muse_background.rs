//! **Live: Muse Code's background work holds its turn's task, and lets it go when it
//! ends** (MUS-07, T-789; Gabriel, 2026-10-26: echo and fixtures are never the proof).
//!
//!     cargo xtask live muse-background [--keep]
//!
//! One real turn on `muse-spark-1.3` at `minimal` (contributor mode off), in Muse's own
//! TUI in a pty, as a member's window is: the model is asked to start a command that
//! sleeps 40 s in the background and end its turn at once. The case reads Muse's own
//! log of the session as ConsensFlow does (the harness code's own reader, asked through
//! `harness-ask`: `cf_e2e::live::ask`), every half second, and passes when it saw what
//! holds a task and then lets it go:
//! - the turn settled with the command running: `settled`, a job pending;
//! - later, the command's end taken into the run it woke, and that run settled with
//!   nothing pending.
//!
//! No board task is opened: ConsensFlow opens no Muse window yet (MUS-08). What this
//! proves is the reading the engine holds a task by (`Pending`), on a real model's
//! record; the hold itself is the engine's, proven for every harness by its own tests.
//!
//! The rules it keeps: the binary is run directly (`muse-bin-<.muse-version>`, with
//! `MUSE_NO_AUTO_UPDATE=1`), never the launcher; on the owner's login (the keychain,
//! through the real `HOME`) with config, data, cache, runtime and temporary folders of
//! its own (a copy of `auth.json`, which holds no secret, and of the feature gates);
//! personal rules and skills, telemetry, session messaging and reminders off; a
//! throwaway project. It refuses to start while another test runs a Muse (one live
//! session at a time; the owner's own Muse in the home is no test's), or when
//! `muse usage --json` says 90 % of a window is spent, and stops at once on any model
//! but the one it launched (Muse moves a session to a contributor model by itself after
//! a refusal: T-705). The owner's `settings.json`, `trust.json` and install files are
//! fingerprinted before and after and must not change. `--keep` leaves the run's home
//! where it is, for `cargo xtask scrub-muse-session` to make a fixture of its log.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::live::{ask, machine, Log};
use cf_e2e::process::Run;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::{json, Value};

/// The model and effort of the case.
const MODEL: &str = "muse-spark-1.3";
const EFFORT: &str = "minimal";

/// The command the model starts: one that outlives its turn by far.
const COMMAND: &str = "python3 -c \"import time; time.sleep(40); print('LIVE_BG_DONE')\"";

/// How long the case waits for the whole of it.
const LIMIT: Duration = Duration::from_secs(240);

type Outcome = Result<(), String>;

fn home() -> PathBuf {
    machine::home().unwrap_or_default()
}

/// What `program` with `args` says on its output, run in `cwd` with `env` alone.
fn ran(
    program: &Path,
    args: &[&str],
    env: &BTreeMap<String, String>,
    cwd: &Path,
) -> Result<String, String> {
    let ran = Run::new(program)
        .args(args)
        .vars(env.iter())
        .cwd(cwd)
        .run()
        .map_err(|failed| failed.to_string())?;
    if ran.code != Some(0) {
        return Err(format!(
            "{} {args:?} ended with {:?}: {}",
            program.display(),
            ran.code,
            ran.stderr
        ));
    }
    Ok(ran.stdout)
}

/// The binary the owner's launcher runs.
fn binary() -> Result<PathBuf, String> {
    let bin = home().join(".local").join("bin");
    let version = fs::read_to_string(bin.join(".muse-version"))
        .map_err(|error| format!("no Muse here (.muse-version: {error})"))?;
    let binary = bin.join(format!("muse-bin-{}", version.trim()));
    if binary.is_file() {
        Ok(binary)
    } else {
        Err(format!("{} is not there", binary.display()))
    }
}

/// The owner's files a run must leave as they were, each by a fingerprint.
fn owner_files() -> BTreeMap<String, u64> {
    let config = home().join(".config").join("muse");
    let bin = home().join(".local").join("bin");
    [
        config.join("settings.json"),
        config.join("trust.json"),
        bin.join(".muse-version"),
        bin.join("muse"),
    ]
    .into_iter()
    .map(|file| {
        let print = fs::read(&file).map_or(0, |bytes| fingerprint(&bytes));
        (file.display().to_string(), print)
    })
    .collect()
}

/// A fingerprint of `bytes` (FNV-1a, 64 bits): enough to see a change.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The Muse processes another test runs: one whose folder is a temporary one or a
/// work tree's. The owner's own, in the home, is none.
fn other_tests_muse() -> Vec<String> {
    let (none, root) = (BTreeMap::new(), PathBuf::from("/"));
    let Ok(listed) = ran(Path::new("/bin/ps"), &["-axo", "pid=,comm="], &none, &root) else {
        return Vec::new();
    };
    listed
        .lines()
        .filter(|line| line.contains("muse-bin"))
        .filter_map(|line| line.split_whitespace().next().map(str::to_owned))
        .filter(|pid| {
            let cwd = ran(
                Path::new("/usr/sbin/lsof"),
                &["-a", "-p", pid, "-d", "cwd", "-Fn"],
                &none,
                &root,
            )
            .unwrap_or_default();
            cwd.contains("/var/folders/") || cwd.contains("consensflow-trees")
        })
        .collect()
}

/// The run's environment: the owner's home for the keychain, every Muse folder its own.
fn environment(root: &Path) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for (name, value) in [
        ("HOME", home().display().to_string()),
        (
            "PATH",
            "/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin".to_owned(),
        ),
        ("SHELL", "/bin/zsh".to_owned()),
        ("LANG", "en_US.UTF-8".to_owned()),
        ("TERM", "xterm-256color".to_owned()),
        ("MUSE_NO_AUTO_UPDATE", "1".to_owned()),
    ] {
        env.insert(name.to_owned(), value);
    }
    for (name, folder) in [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_RUNTIME_DIR", "runtime"),
        ("TMPDIR", "tmp"),
    ] {
        env.insert(name.to_owned(), root.join(folder).display().to_string());
    }
    env
}

fn write(file: &Path, text: &str) -> Outcome {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    fs::write(file, text).map_err(|error| format!("{}: {error}", file.display()))
}

/// The run's home: its folders, the login's metadata and the feature gates, and a
/// throwaway git project.
fn set_up(root: &Path) -> Outcome {
    for folder in [
        "config/muse",
        "data/muse",
        "cache",
        "runtime",
        "tmp",
        "work",
    ] {
        fs::create_dir_all(root.join(folder)).map_err(|error| error.to_string())?;
    }
    let owner = home();
    fs::copy(
        owner.join(".config/muse/auth.json"),
        root.join("config/muse/auth.json"),
    )
    .map_err(|error| format!("auth.json: {error}"))?;
    write(
        &root.join("config/muse/settings.json"),
        r#"{"schema_version":1,"context":{"foreign_personal_rules":false,"foreign_personal_skills":false}}"#,
    )?;
    let gates = owner.join(".local/share/muse/feature-config");
    if gates.is_dir() {
        let into = root.join("data/muse/feature-config");
        fs::create_dir_all(&into).map_err(|error| error.to_string())?;
        for entry in fs::read_dir(&gates).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            fs::copy(entry.path(), into.join(entry.file_name()))
                .map_err(|error| error.to_string())?;
        }
    }
    write(&root.join("work/README.md"), "A throwaway project.\n")?;
    let git = ["/opt/homebrew/bin/git", "/usr/bin/git"]
        .into_iter()
        .find(|git| Path::new(git).is_file())
        .ok_or("no git")?;
    ran(
        Path::new(git),
        &["init", "-q"],
        &BTreeMap::new(),
        &root.join("work"),
    )
    .map(|_| ())
}

/// What `muse usage --json` says is spent of each window, in percent.
fn usage(
    binary: &Path,
    env: &BTreeMap<String, String>,
    root: &Path,
    log: &Log,
) -> Result<Vec<f64>, String> {
    let said = ran(binary, &["usage", "--json"], env, &root.join("work"))?;
    let value: Value = serde_json::from_str(&said)
        .map_err(|error| format!("muse usage --json said no JSON: {error}"))?;
    let mut spent = Vec::new();
    percentages(&value, &mut spent);
    log.say(format!("usage: {value}"));
    // Muse says so of a window no turn has used yet (1.4.13): nothing of it is spent.
    if spent.is_empty() && value["reason"] == "server_sent_no_usage" {
        spent.push(0.0);
    }
    Ok(spent)
}

fn percentages(value: &Value, into: &mut Vec<f64>) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                match value.as_f64() {
                    Some(spent) if name == "usedPercent" => into.push(spent),
                    _ => percentages(value, into),
                }
            }
        }
        Value::Array(values) => values.iter().for_each(|value| percentages(value, into)),
        _ => {}
    }
}

/// The session's log under the run's data folder, and the session's id: the one log that
/// is no child's.
fn session_log(root: &Path) -> Option<(String, PathBuf)> {
    let mut found = Vec::new();
    let mut folders = vec![root.join("data/muse/sessions")];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.file_name().is_some_and(|name| name != "subagent") {
                folders.push(path);
            } else if path.file_name().is_some_and(|name| name == "session.jsonl") {
                found.push(path);
            }
        }
    }
    let log = found.into_iter().next()?;
    let id = log.parent()?.file_name()?.to_string_lossy().into_owned();
    Some((id, log))
}

/// The model the record names, where it names one other than the launched model: the
/// switch Muse makes by itself (`runtime.model_reconfigure.completed`), or a run or the
/// session configured on another.
fn another_model(line: &str) -> Option<String> {
    let record: Value = serde_json::from_str(line).ok()?;
    let found = match record["payload_type"].as_str()? {
        "runtime.model_reconfigure.completed" => {
            record["payload"]["record"]["effective"]["model_id"].as_str()
        }
        "runtime.session.metadata" | "run.model.configured" => {
            record["payload"]["record"]["model_id"].as_str()
        }
        _ => None,
    }?;
    (found != MODEL).then(|| found.to_owned())
}

/// A window of Muse in a pty: its screen's bytes kept, its keys written.
struct Window {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<Vec<u8>>>,
}

impl Window {
    fn open(binary: &Path, env: &BTreeMap<String, String>, root: &Path) -> Result<Self, String> {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: 50,
                cols: 150,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| error.to_string())?;
        let mut command = CommandBuilder::new(binary);
        command.args([
            "--trust-workspace",
            "--model",
            MODEL,
            "--reasoning-effort",
            EFFORT,
            "--contributor-mode",
            "off",
            "--yolo",
            "-c",
            "context.foreign_personal_rules=false",
            "-c",
            "context.foreign_personal_skills=false",
            "-c",
            "telemetry.enabled=false",
            "-c",
            "local_session_messaging.enabled=false",
            "-c",
            r#"run.reminder_roster={"agents":[]}"#,
        ]);
        command.env_clear();
        for (name, value) in env {
            command.env(name, value);
        }
        command.cwd(root.join("work"));
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|error| error.to_string())?;
        let mut reader = pair
            .master
            .try_clone_reader()
            .map_err(|error| error.to_string())?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|error| error.to_string())?;
        let screen = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&screen);
        thread::spawn(move || {
            let mut chunk = [0_u8; 8192];
            while let Ok(read) = reader.read(&mut chunk) {
                if read == 0 {
                    break;
                }
                let Ok(mut screen) = kept.lock() else {
                    break;
                };
                screen.extend_from_slice(&chunk[..read]);
                let len = screen.len();
                if len > 200_000 {
                    screen.drain(..len - 200_000);
                }
            }
        });
        Ok(Self {
            child,
            writer,
            screen,
        })
    }

    fn keys(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Pastes `text`, bracketed, and sends it with Enter a moment after.
    fn paste(&mut self, text: &str) {
        self.keys(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        thread::sleep(Duration::from_millis(500));
        self.keys(b"\r");
    }

    fn tail(&self) -> String {
        let Ok(screen) = self.screen.lock() else {
            return String::new();
        };
        let from = screen.len().saturating_sub(1500);
        String::from_utf8_lossy(&screen[from..]).into_owned()
    }

    /// Quits as a person does at rest, two Ctrl-Cs, and kills what is left.
    fn close(mut self) {
        self.keys(b"\x03");
        thread::sleep(Duration::from_secs(1));
        self.keys(b"\x03");
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            if matches!(self.child.try_wait(), Ok(Some(_))) {
                return;
            }
            thread::sleep(Duration::from_millis(200));
        }
        let _ = self.child.kill();
    }
}

/// What one look read: the turn's state, the work pending, and how many turns had ended.
#[derive(Debug, Clone, PartialEq)]
struct Seen {
    state: String,
    jobs: u64,
    agents: bool,
    ended: usize,
}

impl Seen {
    fn settled(&self) -> bool {
        self.state == "settled"
    }
}

/// What the harness code's reader reads of the session `id` now, in the run's folders.
fn look(id: &str, env: &BTreeMap<String, String>, ended: usize) -> Result<Option<Seen>, String> {
    let answer =
        ask::question(&json!({ "op": "records", "kind": "muse", "session": id, "env": env }))
            .map_err(|failed| failed.to_string())?;
    let reading = &answer["reading"];
    if reading.get("unknown").is_some() {
        return Ok(None);
    }
    Ok(Some(Seen {
        state: reading["settlement"]["state"]
            .as_str()
            .unwrap_or_default()
            .to_owned(),
        jobs: reading["jobsPending"].as_u64().unwrap_or(0),
        agents: reading["agentsPending"] == true,
        ended,
    }))
}

#[test]
#[ignore = "live: one real Muse turn on the owner's login (cargo xtask live muse-background)"]
fn a_command_a_muse_turn_leaves_running_holds_the_task_until_it_ends() -> Outcome {
    // Its one window runs the model the owner chose for Muse's live runs, never the
    // shared models of `--models`, which no Muse window is asked for yet.
    let log = Log::own_agents(
        "live muse-background",
        "it runs Muse on muse-spark-1.3 at minimal",
    )
    .map_err(|failed| failed.to_string())?;
    let binary = binary()?;
    // The harness code is built before anything is spent.
    ask::prepare().map_err(|failed| failed.to_string())?;
    let others = other_tests_muse();
    if !others.is_empty() {
        return Err(format!(
            "another test runs Muse now (pids {others:?}): one live session at a time"
        ));
    }
    let root = tempfile::Builder::new()
        .prefix("muse-rec-")
        .tempdir()
        .map_err(|error| error.to_string())?;
    let root_path = root.path().to_path_buf();
    set_up(&root_path)?;
    let env = environment(&root_path);
    let spent = usage(&binary, &env, &root_path, &log)?;
    if spent.is_empty() {
        return Err(
            "muse usage --json said nothing spent: the 90 % stop cannot be kept".to_owned(),
        );
    }
    if spent.iter().any(|spent| *spent >= 90.0) {
        return Err(format!("90 % of a window is spent ({spent:?}): stopped"));
    }
    let owner_before = owner_files();
    let mut window = Window::open(&binary, &env, &root_path)?;
    let outcome = watch(&mut window, &root_path, &env, &log);
    let tail = window.tail();
    window.close();
    let after = usage(&binary, &env, &root_path, &log);
    let owner_after = owner_files();
    if cf_e2e::live::keep_requested() {
        let kept = root.keep();
        log.say(format!("kept the run's home: {}", kept.display()));
    }
    if owner_before != owner_after {
        return Err(format!(
            "the owner's Muse files changed: {owner_before:?} → {owner_after:?}"
        ));
    }
    log.say(format!("usage after: {after:?}"));
    outcome.map_err(|failed| format!("{failed}\nthe window's last bytes:\n{tail}"))
}

/// Pastes the turn's words and reads the record until what holds the task and lets it
/// go has been seen, or the limit passes, or the record names another model.
fn watch(window: &mut Window, root: &Path, env: &BTreeMap<String, String>, log: &Log) -> Outcome {
    let started = Instant::now();
    // The window is up once its session's log says so.
    let id = loop {
        if started.elapsed() > Duration::from_secs(40) {
            return Err("the window wrote no session log in 40 s".to_owned());
        }
        if let Some((id, file)) = session_log(root) {
            if fs::read_to_string(&file).is_ok_and(|text| text.contains("\"session.started\"")) {
                break id;
            }
        }
        thread::sleep(Duration::from_millis(250));
    };
    thread::sleep(Duration::from_secs(2));
    window.paste(&format!(
        "Use the bash tool once to start this exact command so that it keeps running in the \
         background, and do not wait for it: {COMMAND} . Then reply with the single word \
         STARTED and end your turn at once. Do not use bash_input, do not poll it and do not \
         sleep. When you are told the command finished, reply with the single word FINISHED."
    ));
    let mut seen: Vec<(f64, Seen)> = Vec::new();
    let mut held = false;
    while started.elapsed() < LIMIT {
        thread::sleep(Duration::from_millis(500));
        let Some((_, file)) = session_log(root) else {
            continue;
        };
        let text = fs::read_to_string(&file).unwrap_or_default();
        if let Some(model) = text.lines().find_map(another_model) {
            return Err(format!("the record names another model, {model}: stopped"));
        }
        let ended = text.matches("\"kind\":\"terminal\",\"terminal\"").count();
        let Some(now) = look(&id, env, ended)? else {
            continue;
        };
        if seen.last().is_none_or(|(_, last)| *last != now) {
            let at = started.elapsed().as_secs_f64();
            log.say(format!("{at:6.1}s {now:?}"));
            seen.push((at, now.clone()));
        }
        if now.settled() && now.jobs > 0 {
            held = true;
        }
        if held && now.settled() && now.jobs == 0 && !now.agents && now.ended >= 2 {
            log.say(format!("held for {:.1}s, then let go", held_for(&seen)));
            return Ok(());
        }
    }
    if !held {
        return Err(format!(
            "no reading of a settled turn with its command pending in {LIMIT:?}: the model may \
             have waited on it within its turn, which proves nothing; seen: {seen:?}"
        ));
    }
    Err(format!(
        "the command's end never let the task go in {LIMIT:?}; seen: {seen:?}"
    ))
}

/// How long the readings held: from the first settled one with a job pending to the next
/// with none.
fn held_for(seen: &[(f64, Seen)]) -> f64 {
    let Some((first, _)) = seen
        .iter()
        .find(|(_, seen)| seen.settled() && seen.jobs > 0)
    else {
        return 0.0;
    };
    let after = seen
        .iter()
        .find(|(at, seen)| at > first && seen.jobs == 0)
        .map_or(*first, |(at, _)| *at);
    after - first
}
