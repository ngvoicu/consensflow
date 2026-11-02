//! What the live tests that run Muse Code's own TUI in a pty share (`live_muse_background`,
//! `live_muse_posture`): the owner's binary run directly, on the owner's login (the
//! keychain, through the real `HOME`) with every Muse folder the run's own, a throwaway
//! project, the allowance asked, the owner's files fingerprinted, one live session at a
//! time, and the record read for a model Muse moved to by itself.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cf_e2e::live::{machine, Log};
use cf_e2e::process::Run;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::Value;

/// The model and effort of the case.
pub const MODEL: &str = "muse-spark-1.3";
pub const EFFORT: &str = "minimal";

pub type Outcome = Result<(), String>;

pub fn home() -> PathBuf {
    machine::home().unwrap_or_default()
}

/// What `program` with `args` says on its output, run in `cwd` with `env` alone.
pub fn ran(
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
pub fn binary() -> Result<PathBuf, String> {
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
pub fn owner_files() -> BTreeMap<String, u64> {
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
pub fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The Muse processes another test runs: one whose folder is a temporary one, a work
/// tree's, or a live test's in a checkout (`live-muse-…`). The owner's own, in the home,
/// is none.
pub fn other_tests_muse() -> Vec<String> {
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
            cwd.contains("/var/folders/")
                || cwd.contains("consensflow-trees")
                || cwd.contains("live-muse-")
        })
        .collect()
}

/// The run's environment: the owner's home for the keychain, every Muse folder its own.
pub fn environment(root: &Path) -> BTreeMap<String, String> {
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

pub fn write(file: &Path, text: &str) -> Outcome {
    if let Some(folder) = file.parent() {
        fs::create_dir_all(folder).map_err(|error| error.to_string())?;
    }
    fs::write(file, text).map_err(|error| format!("{}: {error}", file.display()))
}

/// The run's home: its folders, the login's metadata and the feature gates, and a
/// throwaway git project.
pub fn set_up(root: &Path) -> Outcome {
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
pub fn usage(
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

pub fn percentages(value: &Value, into: &mut Vec<f64>) {
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
pub fn session_log(root: &Path) -> Option<(String, PathBuf)> {
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
pub fn another_model(line: &str) -> Option<String> {
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
pub struct Window {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<Vec<u8>>>,
}

impl Window {
    /// Opens Muse in `root`'s project on the case's model, in `posture` (`--yolo`, or the
    /// flags of a softer one), with what a run of its own keeps off.
    pub fn open(
        binary: &Path,
        env: &BTreeMap<String, String>,
        root: &Path,
        posture: &[&str],
    ) -> Result<Self, String> {
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
        ]);
        command.args(posture);
        command.args([
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

    pub fn keys(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// Pastes `text`, bracketed, and sends it with Enter a moment after.
    pub fn paste(&mut self, text: &str) {
        self.keys(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        thread::sleep(Duration::from_millis(500));
        self.keys(b"\r");
    }

    pub fn tail(&self) -> String {
        let Ok(screen) = self.screen.lock() else {
            return String::new();
        };
        let from = screen.len().saturating_sub(1500);
        String::from_utf8_lossy(&screen[from..]).into_owned()
    }

    /// Quits as a person does at rest, two Ctrl-Cs, and kills what is left.
    pub fn close(mut self) {
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
