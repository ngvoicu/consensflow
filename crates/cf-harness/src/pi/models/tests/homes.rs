//! HMD-13: Pi's list is right in every setup Pi itself can run in.
//!
//! Each case is a Pi folder of one kind of setup (`tests/fixtures/pi-homes/scenarios.json`), made
//! on disk in a throwaway folder, and the answers the installed Pi gave for it when it was read
//! both ways, with and without its extensions (`answers-*.json`, recorded by
//! `recorded_homes_answer_as_the_installed_pi_does` below, run by hand with `--ignored` on each
//! machine). The stand-in `pi` answers as the recorded Pi did, by the arguments it is started
//! with, so a read that leaves Pi's packages and extensions out reads what Pi listed with them
//! left out.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::*;
use crate::pi::paths::expand;
use crate::seams::processes::Streams;
use crate::testing::{ChildScript, Ends, ManualTime, ScriptedProcesses};

const SCENARIOS: &str = include_str!("../../../../tests/fixtures/pi-homes/scenarios.json");
const ANSWERS: &str = include_str!("../../../../tests/fixtures/pi-homes/answers-mac.json");

/// One kind of setup: what it is, the files of the Pi folder, and the variables of the
/// environment it is read in.
struct Home {
    name: String,
    says: String,
    files: BTreeMap<String, String>,
    env: Vec<(String, String)>,
}

fn homes() -> Vec<Home> {
    let all: Value = serde_json::from_str(SCENARIOS).unwrap();
    all["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|home| Home {
            name: home["name"].as_str().unwrap().to_owned(),
            says: home["says"].as_str().unwrap().to_owned(),
            files: home["files"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(file, text)| (file.clone(), text.as_str().unwrap().to_owned()))
                .collect(),
            env: home["env"]
                .as_object()
                .unwrap()
                .iter()
                .map(|(name, value)| (name.clone(), value.as_str().unwrap().to_owned()))
                .collect(),
        })
        .collect()
}

/// The folder of `home` made under `root`.
fn make(home: &Home, root: &Path) {
    for (file, text) in &home.files {
        let at = file
            .split('/')
            .fold(root.to_path_buf(), |at, part| at.join(part));
        std::fs::create_dir_all(at.parent().unwrap()).unwrap();
        std::fs::write(at, text).unwrap();
    }
}

/// Every file under `root` with what it holds: what a read must leave as it found it.
fn contents(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(at: &Path, root: &Path, into: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, into);
            } else {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                into.insert(name, std::fs::read(&path).unwrap());
            }
        }
    }
    let mut into = BTreeMap::new();
    walk(root, root, &mut into);
    into
}

/// The installed Pi's recorded answers of `name`: read with its extensions, and without.
fn recorded(name: &str) -> Value {
    let all: Value = serde_json::from_str(ANSWERS).unwrap();
    all["scenarios"][name].clone()
}

/// A `pi` that answers as the recorded one did, by whether it is started with Pi's extensions
/// left out. One that ended says what the recorded one said on its error stream.
struct RecordedPi {
    answers: Value,
    scripted: ScriptedProcesses,
}

impl RecordedPi {
    fn new(name: &str) -> Self {
        Self {
            answers: recorded(name),
            scripted: ScriptedProcesses::default(),
        }
    }

    fn answer(&self, program: &Program) -> &Value {
        let without = program.args.iter().any(|arg| arg == "--no-extensions");
        &self.answers[if without { "without" } else { "with" }]
    }
}

impl Processes for RecordedPi {
    /// Pi is started once, to be asked, and never run to its end to be asked why it failed.
    fn run(
        &self,
        program: Program,
        _limits: Limits,
    ) -> crate::contract::Work<'_, Result<String, cf_process::Failed>> {
        panic!("a second start of Pi: {}", crate::testing::named(&program))
    }

    fn spawn(
        &self,
        program: Program,
        streams: Streams,
    ) -> Result<Box<dyn crate::seams::processes::Child>, crate::seams::processes::StartRefusal>
    {
        let answer = self.answer(&program).clone();
        let script = if answer["exit"].is_null() {
            let line = json!({ "type": "response", "id": "models", "success": true,
                "data": { "models": answer["models"] } });
            ChildScript {
                lines: vec![line.to_string()],
                ends: Ends::Asked,
            }
        } else {
            // Pi ended: what it said on its error stream, and the code it exited with.
            self.scripted.child_said(
                "pi",
                answer["stderr"].as_str().unwrap_or_default(),
                answer["exit"]
                    .as_i64()
                    .and_then(|code| i32::try_from(code).ok()),
            );
            ChildScript {
                lines: Vec::new(),
                ends: Ends::Itself,
            }
        };
        self.scripted.child("pi", script);
        self.scripted.spawn(program, streams)
    }
}

/// What the human's Pi, set up as `home`, lists as the app reads it: the list, or the words of
/// the failure, and the folder as it was left (the same files with the same contents).
fn listing_of(home: &Home) -> Result<Vec<String>, String> {
    let folder = tempfile::tempdir().unwrap();
    let pi = crate::testing::fake_executable(&folder.path().join("pi"));
    let agent = folder.path().join("agent");
    std::fs::create_dir_all(&agent).unwrap();
    make(home, &agent);
    let before = contents(&agent);
    let mut vars: Vec<(std::ffi::OsString, std::ffi::OsString)> = vec![
        ("PATH".into(), pi.parent().unwrap().as_os_str().to_owned()),
        ("PI_CODING_AGENT_DIR".into(), agent.as_os_str().to_owned()),
    ];
    vars.extend(
        home.env
            .iter()
            .map(|(name, value)| (name.into(), value.into())),
    );
    let env = Env::from_vars(vars);
    let processes = RecordedPi::new(&home.name);
    let time = ManualTime::new(0);
    let listed = eventually(Box::pin(listed(&env, &processes, &time, false)));
    assert_eq!(
        contents(&agent),
        before,
        "{}: a read writes nothing",
        home.name
    );
    listed
        .map(|listing| listing.models.iter().map(ModelChoice::model).collect())
        .map_err(String::from)
}

fn providers(models: &[String]) -> Vec<String> {
    let mut providers: Vec<String> = models
        .iter()
        .map(|model| model.split('/').next().unwrap().to_owned())
        .collect();
    providers.sort();
    providers.dedup();
    providers
}

fn home(name: &str) -> Home {
    homes().into_iter().find(|home| home.name == name).unwrap()
}

/// The providers a setup of each kind lists, as the installed Pi listed them when it was read the
/// way a window starts.
#[test]
fn every_setup_pi_can_run_in_lists_its_models() {
    let expected: [(&str, &[&str]); 7] = [
        ("oauth-login", &["openai-codex"]),
        ("api-key-login", &["openrouter"]),
        ("env-key", &["openrouter"]),
        ("custom-provider", &["mylocal"]),
        ("package-provider", &["pkgprov"]),
        ("extension-provider", &["extprov"]),
        (
            "several",
            &["extprov", "mylocal", "openai-codex", "pkgprov"],
        ),
    ];
    let mut wrong = Vec::new();
    for (name, wanted) in expected {
        let home = home(name);
        match listing_of(&home) {
            Ok(listed) if providers(&listed) == wanted => {}
            Ok(listed) => wrong.push(format!(
                "{name} ({}): lists {:?}",
                home.says,
                providers(&listed)
            )),
            Err(why) => wrong.push(format!("{name} ({}): {why}", home.says)),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn a_pi_with_nothing_set_up_says_so_and_how_to_set_one_up() {
    let why = listing_of(&home("nothing")).unwrap_err();
    assert!(
        why.starts_with("Pi lists no model: no provider is set up ("),
        "{why}"
    );
    for how in ["/login", "providers.md", "models.json"] {
        assert!(why.contains(how), "{how} in {why}");
    }
}

#[test]
fn a_package_whose_provider_listed_nothing_is_not_called_no_provider() {
    let why = listing_of(&home("package-without-models")).unwrap_err();
    assert!(
        why.starts_with("Pi lists no model though it has packages npm:pi-test-provider"),
        "{why}"
    );
    assert!(!why.contains("no provider is set up"), "{why}");
}

#[test]
fn an_extension_pi_cannot_load_is_pi_s_own_error() {
    let why = listing_of(&home("extension-fails")).unwrap_err();
    assert!(why.contains("ended before it answered"), "{why}");
    assert!(
        why.contains("it said:\nError: Failed to load extension"),
        "{why}"
    );
    assert!(why.contains("this extension fails to load"), "{why}");
    assert!(!why.contains("no provider"), "{why}");
}

/// What of this process's environment a real Pi is read in by the tests below (F7 of the review
/// T-1270): where to find programs and the home, the temporary folder, and what Windows needs
/// to start one. Everything else is left out, a credential of the person running the tests
/// among it (`HF_TOKEN`, `ANTHROPIC_OAUTH_TOKEN`, a path to a credentials file), not only the
/// variables that end in `_API_KEY`.
const KEPT: [&str; 17] = [
    "PATH",
    "HOME",
    "USERPROFILE",
    "TMPDIR",
    "TEMP",
    "TMP",
    "XDG_DATA_HOME",
    "XDG_CONFIG_HOME",
    "LANG",
    "LC_ALL",
    "APPDATA",
    "LOCALAPPDATA",
    "SystemRoot",
    "SYSTEMROOT",
    "windir",
    "ComSpec",
    "PATHEXT",
];

/// The environment a real Pi is read in: `process`'s, but what [`KEPT`] names, and `agent` as
/// Pi's folder. `CF_PI` names the `pi` to run, and its folder goes first on the PATH.
fn real_env(process: &Env, agent: &Path) -> (Env, PathBuf) {
    let base = process
        .clone()
        .without_vars(|name| !KEPT.iter().any(|kept| kept.eq_ignore_ascii_case(name)));
    let pi = PathBuf::from(process.text("CF_PI").unwrap_or("pi"));
    let mut env = base.with_var("PI_CODING_AGENT_DIR", &agent.to_string_lossy());
    if let Some(bin) = pi.parent().filter(|bin| !bin.as_os_str().is_empty()) {
        let rest = env
            .search_path()
            .map(std::ffi::OsStr::to_owned)
            .unwrap_or_default();
        let joined = std::env::join_paths(
            std::iter::once(bin.to_path_buf()).chain(std::env::split_paths(&rest)),
        )
        .unwrap();
        env = env.with_var("PATH", &joined.to_string_lossy());
    }
    (env, pi)
}

/// The digest of every file under `root`, by name: what a read of a copy must leave as it
/// found it. Names are all that is ever said of it.
fn digests(root: &Path) -> BTreeMap<String, String> {
    use sha2::{Digest, Sha256};
    fn walk(at: &Path, root: &Path, into: &mut BTreeMap<String, String>) {
        for entry in std::fs::read_dir(at).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, root, into);
            } else if let Ok(bytes) = std::fs::read(&path) {
                let name = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                into.insert(name, format!("{:x}", Sha256::digest(bytes)));
            }
        }
    }
    let mut into = BTreeMap::new();
    walk(root, root, &mut into);
    into
}

fn block_on<T>(work: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(work)
}

/// The recordings are of the installed Pi: each scenario's folder is made and read by the app's
/// own read ([`read`]) through the system's processes, with Pi's extensions and without, and every
/// row kept in the recording is among what Pi lists, from the providers it lists and no others; a
/// recorded empty list is the app's words for one, a recorded refusal to start is Pi's. Run by hand
/// where Pi is installed (the Mac, zeewin2), `CF_PI` naming the `pi`:
/// `cargo test -p cf-harness --lib recorded_homes -- --ignored --nocapture`.
#[test]
#[ignore = "runs the installed Pi, offline, with no model turn"]
fn recorded_homes_answer_as_the_installed_pi_does() {
    use crate::seams::{SystemProcesses, SystemTime};
    let mut wrong = Vec::new();
    for home in homes() {
        let folder = tempfile::tempdir().unwrap();
        let agent = folder.path().join("agent");
        std::fs::create_dir_all(&agent).unwrap();
        make(&home, &agent);
        let (mut env, pi) = real_env(&Env::from_process(), &agent);
        for (name, value) in &home.env {
            env = env.with_var(name, value);
        }
        let processes = SystemProcesses::new(env.clone());
        for (way, extra) in [("with", None), ("without", Some("--no-extensions"))] {
            let mut program = program(pi.clone(), &env);
            program.cwd = Some(folder.path().to_path_buf());
            program.args.extend(extra.map(str::to_owned));
            let read = block_on(read(&processes, &SystemTime, program));
            let kept = &recorded(&home.name)[way];
            let at = format!("{} {way}", home.name);
            match (read, kept["exit"].is_null()) {
                (Ok(listing), true) => {
                    let models: Vec<String> =
                        listing.models.iter().map(ModelChoice::model).collect();
                    let among = kept["models"].as_array().unwrap().iter().all(|kept| {
                        let model = format!(
                            "{}/{}",
                            kept["provider"].as_str().unwrap(),
                            kept["id"].as_str().unwrap()
                        );
                        models.contains(&model)
                    });
                    let mut wanted: Vec<&str> = kept["providers"]
                        .as_object()
                        .map(|table| table.keys().map(String::as_str).collect())
                        .unwrap_or_default();
                    wanted.sort_unstable();
                    if !among || providers(&models) != wanted {
                        wrong.push(format!(
                            "{at}: lists {:?}, recorded {wanted:?}",
                            providers(&models)
                        ));
                    }
                }
                (Err(why), true) if kept["models"].as_array().is_some_and(Vec::is_empty) => {
                    if !why.starts_with("Pi lists no model") {
                        wrong.push(format!("{at}: {why}"));
                    }
                }
                // Pi's refusal to start: its own words, which name the extension that failed.
                (Err(why), false) => {
                    if ![
                        "ended before it answered",
                        "Failed to load extension",
                        "this extension fails to load",
                    ]
                    .iter()
                    .all(|word| why.contains(word))
                    {
                        wrong.push(format!("{at}: {why}"));
                    }
                }
                (read, _) => wrong.push(format!("{at}: {read:?}, recorded {kept}")),
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// The folders of the human's own Pi that a copy must never be: `.pi/agent` in the home, and the
/// one `PI_CODING_AGENT_DIR` names, **as Pi reads it** (the review T-1270, F7 and its second
/// reading): `~` is the home and `~/x` is under it ([`expand`], Pi's `piPath`), and a path with
/// no root is the working folder's. A folder called `~/owner` taken for the relative one named
/// `~` let the human's own folder pass for a copy, and Pi then read and wrote it.
fn owner_folders(process: &Env, cwd: &Path) -> Vec<PathBuf> {
    let mut owners = Vec::new();
    if let Ok(home) = crate::shared::paths::home(process) {
        owners.push(Path::new(&home).join(".pi").join("agent"));
    }
    if let Some(configured) = crate::shared::paths::set(process, "PI_CODING_AGENT_DIR") {
        let expanded = expand(&configured, process).unwrap_or(configured);
        // A path with a root replaces the working folder in a join.
        owners.push(cwd.join(expanded));
    }
    owners
}

/// The owner's folder that `copy` is, if it is one of `owners`: the folder Pi would read is the
/// one it is told, not the ones inside it.
fn owner_among<'a>(copy: &Path, owners: &'a [PathBuf]) -> Option<&'a PathBuf> {
    let copy = std::fs::canonicalize(copy).unwrap_or_else(|_| copy.to_path_buf());
    owners.iter().find(|owner| {
        let owner = std::fs::canonicalize(owner).unwrap_or_else(|_| (*owner).clone());
        copy == owner
    })
}

/// What the guard of the real-Pi helper says of `copy`, when the process has `home` as its home,
/// `configured` as its `PI_CODING_AGENT_DIR` and `cwd` as its working folder.
fn guard_flags(copy: &Path, home: &Path, configured: &str, cwd: &Path) -> bool {
    let process = Env::from_vars([
        ("HOME", home.as_os_str()),
        ("PI_CODING_AGENT_DIR", std::ffi::OsStr::new(configured)),
    ]);
    owner_among(copy, &owner_folders(&process, cwd)).is_some()
}

/// Pi expands the folder it is told (`piPath`): `~` is the home, `~/x` is under it, and a path
/// with no root is the working folder's. The guard that keeps the helper from reading the
/// human's own folder expands it the same way: a `~/owner` it took for a folder called `~` would
/// let a copy that is the owner's own pass, and Pi would then read, and write, the owner's.
#[test]
fn the_guard_of_the_real_pi_helper_expands_the_folder_pi_is_told_as_pi_does() {
    let folder = tempfile::tempdir().unwrap();
    let home = folder.path().join("home");
    let owner = home.join("owner");
    std::fs::create_dir_all(&owner).unwrap();
    let elsewhere = folder.path().join("a-copy");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let cwd = folder.path().join("work");
    std::fs::create_dir_all(cwd.join("rel").join("agent")).unwrap();

    assert!(
        guard_flags(&owner, &home, "~/owner", &cwd),
        "~/owner is the home's owner folder"
    );
    assert!(guard_flags(&home, &home, "~", &cwd), "~ is the home itself");
    assert!(
        guard_flags(&home, &home, "~/", &cwd),
        "~/ is the home as well"
    );
    assert!(
        guard_flags(&cwd.join("rel").join("agent"), &home, "rel/agent", &cwd),
        "a relative folder is the working folder's"
    );
    for configured in ["~/owner", "~", "~/", "rel/agent"] {
        assert!(
            !guard_flags(&elsewhere, &home, configured, &cwd),
            "a copy elsewhere is not the owner's, for {configured}"
        );
    }
    // The default folder of the home is the owner's too.
    let default = home.join(".pi").join("agent");
    std::fs::create_dir_all(&default).unwrap();
    assert!(guard_flags(&default, &home, "", &cwd));
}

/// The copy `CF_PI_HOME` names, which must be a copy and never the human's own, neither the
/// default `~/.pi/agent` nor the one this process's `PI_CODING_AGENT_DIR` names (as Pi reads it:
/// [`owner_folders`]), and the environment a real Pi is read in on it ([`real_env`]).
fn a_copy_of_the_human_s_folder(process: &Env) -> (PathBuf, Env) {
    let copy = PathBuf::from(
        process
            .text("CF_PI_HOME")
            .expect("CF_PI_HOME names a copy of a Pi folder"),
    );
    let owners = owner_folders(process, &std::env::current_dir().unwrap());
    if let Some(owner) = owner_among(&copy, &owners) {
        panic!(
            "CF_PI_HOME names a copy, never the human's own folder: {}",
            owner.display()
        );
    }
    let (env, _) = real_env(process, &copy);
    for credential in ["HF_TOKEN", "ANTHROPIC_OAUTH_TOKEN", "OPENROUTER_API_KEY"] {
        assert!(
            env.os(credential).is_none(),
            "{credential} is not Pi's to have"
        );
    }
    (copy, env)
}

/// The app's own read, through the system's processes and clock, of the Pi folder `CF_PI_HOME`
/// names, which must be a copy ([`a_copy_of_the_human_s_folder`]) (HMD-13: this Mac's, with the
/// `pi-cliproxyapi-provider` package, lists its CLIProxyAPI models; zeewin2's lists its `openai`
/// login's). `CF_PI_EXPECT` names the providers it must list, comma between, `CF_PI` the `pi`.
/// The copy is digested before and after, and the read must leave it as it found it. Nothing is
/// spent: a list is no turn. Run by hand: `CF_PI_HOME=… CF_PI_EXPECT=cliproxyapi cargo test -p
/// cf-harness --lib the_app_reads -- --ignored --nocapture`.
#[test]
#[ignore = "runs the installed Pi on a copy of a Pi folder, offline, with no model turn"]
fn the_app_reads_a_copy_of_the_human_s_pi_folder_as_a_window_would_start() {
    use crate::seams::{SystemProcesses, SystemTime};
    let process = Env::from_process();
    let (copy, env) = a_copy_of_the_human_s_folder(&process);
    let before = digests(&copy);
    let processes = SystemProcesses::new(env.clone());
    let started = std::time::Instant::now();
    let listed = block_on(listed(&env, &processes, &SystemTime, false));
    let took = started.elapsed();
    let after = digests(&copy);
    let changed: Vec<&String> = before
        .keys()
        .chain(after.keys())
        .filter(|name| before.get(*name) != after.get(*name))
        .collect();
    assert!(
        changed.is_empty(),
        "the read left the copy changed: {changed:?}"
    );
    let listing = listed.unwrap_or_else(|why| panic!("the read failed after {took:?}: {why}"));
    let models: Vec<String> = listing.models.iter().map(ModelChoice::model).collect();
    let found = providers(&models);
    println!(
        "{} models from {} providers {:?} in {took:?}; the first: {:?}; the copy ({} files) unchanged",
        models.len(),
        found.len(),
        found,
        &models[..models.len().min(3)],
        after.len()
    );
    if let Some(expect) = process.text("CF_PI_EXPECT") {
        for provider in expect.split(',').filter(|provider| !provider.is_empty()) {
            assert!(
                found.iter().any(|found| found == provider),
                "{provider} among {found:?}"
            );
        }
    }
}

/// The human's Refresh on the same copy: `pi update --models`, then one read of the list, as an
/// ordinary read is, with no hold of Pi's session (the review's second reading, F6 and F8). Its
/// time is the update's and the read's, well inside the app's 30 s bridge deadline. The update
/// writes Pi's own catalog into the copy, which is what it is for: only the files that are not a
/// catalog are held unchanged. Run by hand like the test above, with `--ignored --nocapture`.
#[test]
#[ignore = "runs the installed Pi on a copy of a Pi folder: its update fetches the catalogs, then one list read, no model turn"]
fn a_refresh_of_the_installed_pi_on_a_copy_is_its_update_and_one_read() {
    use crate::seams::{SystemProcesses, SystemTime};
    let process = Env::from_process();
    let (copy, env) = a_copy_of_the_human_s_folder(&process);
    let before = digests(&copy);
    let processes = SystemProcesses::new(env.clone());
    let started = std::time::Instant::now();
    let listed = block_on(listed(&env, &processes, &SystemTime, true));
    let took = started.elapsed();
    let after = digests(&copy);
    let changed: Vec<&String> = before
        .keys()
        .chain(after.keys())
        .filter(|name| before.get(*name) != after.get(*name))
        .collect();
    let listing = listed.unwrap_or_else(|why| panic!("the Refresh failed after {took:?}: {why}"));
    let models: Vec<String> = listing.models.iter().map(ModelChoice::model).collect();
    println!(
        "Refresh: {} models from {:?} in {took:?}; {}; {} of the copy's {} files changed: {changed:?}",
        models.len(),
        providers(&models),
        match &listing.freshness {
            cf_proto::models::Freshness::Fresh => "fresh".to_owned(),
            cf_proto::models::Freshness::Stale { why } => format!("stale: {why}"),
        },
        changed.len(),
        after.len()
    );
    assert!(
        took < std::time::Duration::from_secs(25),
        "a Refresh took {took:?}: the app's bridge gives up at 30 s"
    );
    assert_eq!(listing.how, HOW, "read once, as any read is");
    if let Some(expect) = process.text("CF_PI_EXPECT") {
        let found = providers(&models);
        for provider in expect.split(',').filter(|provider| !provider.is_empty()) {
            assert!(
                found.iter().any(|found| found == provider),
                "{provider} among {found:?}"
            );
        }
    }
}
