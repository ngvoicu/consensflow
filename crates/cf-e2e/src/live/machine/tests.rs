//! What the machine's harnesses are held to: where each logs in, which folders
//! its windows need, and the rig that gives them to the windows.

use std::fs;
use std::time::Duration;

use super::*;

#[test]
fn codex_logs_in_at_its_home_when_named_and_in_the_dot_codex_of_the_users_otherwise() {
    let home = |codex: Option<&str>, user: Option<&str>| {
        home_of(
            Harness::Codex,
            codex.map(str::to_owned),
            user.map(str::to_owned),
        )
    };
    let given = std::env::temp_dir().join("elsewhere");
    let user = std::env::temp_dir().join("user");
    let (given_text, user_text) = (given.to_string_lossy(), user.to_string_lossy());
    assert_eq!(home(Some(&given_text), Some(&user_text)), Some(given));
    assert_eq!(home(None, Some(&user_text)), Some(user.join(".codex")));
    // A variable that is set to nothing is not set.
    assert_eq!(home(Some(""), Some(&user_text)), Some(user.join(".codex")));
    assert_eq!(home(None, None), None);
    assert_eq!(home(None, Some("")), None);
}

#[test]
fn every_other_harness_logs_in_at_the_users_home_whatever_codex_home_says() {
    let user = std::env::temp_dir().join("user");
    let user_text = user.to_string_lossy().into_owned();
    for harness in Harness::ALL.into_iter().filter(|h| *h != Harness::Codex) {
        let home = |codex: Option<&str>, user: Option<&str>| {
            home_of(harness, codex.map(str::to_owned), user.map(str::to_owned))
        };
        assert_eq!(
            home(Some("elsewhere"), Some(&user_text)),
            Some(user.clone())
        );
        assert_eq!(home(None, Some(&user_text)), Some(user.clone()));
        assert_eq!(home(Some("elsewhere"), None), None, "{harness:?}");
        assert_eq!(home(None, Some("")), None, "{harness:?}");
    }
}

#[test]
fn a_home_named_by_a_relative_path_is_made_whole_against_the_folder_of_the_run() {
    let home = home_of(Harness::Codex, Some("login".to_owned()), None).unwrap();
    assert!(home.is_absolute(), "{}", home.display());
    assert!(home.ends_with("login"));
}

#[test]
fn an_installed_harness_is_its_command_and_the_home_it_logs_in_at() {
    let command = Path::new("bin").join(if cfg!(windows) { "codex.cmd" } else { "codex" });
    let codex = Installed::at(Harness::Codex, &command, "login");
    assert_eq!(codex.harness(), Harness::Codex);
    assert_eq!(codex.command(), command);
    assert_eq!(codex.home(), Path::new("login"));
}

#[cfg(unix)]
#[test]
fn a_harness_says_its_version_by_the_word_it_is_asked_with_and_unknown_when_it_says_none() {
    let pi = Installed::at(Harness::Pi, "/bin/echo", "home");
    assert_eq!(pi.version(), "--version");
    // Devin's is a command of its own, not a flag.
    let devin = Installed::at(Harness::Devin, "/bin/echo", "home");
    assert_eq!(devin.version(), "version");
    let failing = Installed::at(Harness::Pi, "/usr/bin/false", "home");
    assert_eq!(failing.version(), "unknown");
    let gone = Installed::at(Harness::Pi, "/no/such/program", "home");
    assert_eq!(gone.version(), "unknown");
}

#[cfg(unix)]
#[test]
fn a_harness_whose_version_leaves_a_process_holding_its_output_says_it_and_holds_nothing() {
    use std::os::unix::fs::PermissionsExt;
    // A wrapper that says its version and leaves an updater in the background, which
    // holds its output for twenty seconds.
    let folder = tempfile::tempdir().unwrap();
    let wrapper = folder.path().join("pi");
    std::fs::write(&wrapper, "#!/bin/sh\necho 0.71.2\nsleep 20 &\n").unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let started = std::time::Instant::now();
    let said = Installed::at(Harness::Pi, &wrapper, "home").version();
    assert!(started.elapsed() < std::time::Duration::from_secs(15));
    assert_eq!(said, "0.71.2 (a process it left held its output open)");
}

#[test]
fn the_harnesses_are_named_as_a_command_line_and_the_roster_name_them() {
    let names: Vec<_> = Harness::ALL.iter().map(|harness| harness.name()).collect();
    assert_eq!(names, ["claude", "opencode", "pi", "devin", "codex"]);
    for harness in Harness::ALL {
        assert_eq!(Harness::from_name(harness.name()), Some(harness));
    }
    assert_eq!(Harness::from_name("Claude"), None);
    assert_eq!(Harness::from_name("claude-code"), None);
    assert_eq!(Harness::from_name(""), None);
    assert_eq!(Harness::Claude.kind(), "claude-code");
    assert_eq!(Harness::Pi.kind(), "pi");
    assert_eq!(Harness::Codex.kind(), "codex");
}

/// Whether `harness` has a login in `home`, on a machine that is Windows or is not,
/// whose `%APPDATA%` is `appdata`.
fn held(harness: Harness, home: &Path, appdata: Option<&str>, windows: bool) -> bool {
    has_login(&login_places(harness, home, appdata, windows))
}

#[test]
fn a_home_holds_a_login_when_it_holds_the_place_the_harness_keeps_it() {
    let home = tempfile::tempdir().unwrap();
    let held = |harness, home: &Path| held(harness, home, None, false);
    for harness in Harness::ALL.into_iter().filter(|h| *h != Harness::Codex) {
        assert!(!held(harness, home.path()), "{harness:?}");
    }
    fs::write(home.path().join(".claude.json"), "{}").unwrap();
    assert!(held(Harness::Claude, home.path()));
    assert!(!held(Harness::Pi, home.path()));
    fs::create_dir_all(home.path().join(".config").join("opencode")).unwrap();
    assert!(held(Harness::OpenCode, home.path()));
    // Codex's home is the folder itself: any folder that is there.
    assert!(held(Harness::Codex, home.path()));
    assert!(!held(Harness::Codex, &home.path().join("missing")));
}

#[test]
fn devin_keeps_its_login_in_devin_under_appdata_on_windows_and_in_the_xdg_places_elsewhere() {
    // A Windows profile as zeewin has it: Devin's login, config and sessions are in
    // `AppData\Roaming\devin`, and `.config` and `.local` are not there at all.
    let windows = tempfile::tempdir().unwrap();
    fs::create_dir_all(windows.path().join("AppData/Roaming/devin/cli")).unwrap();
    assert!(held(Harness::Devin, windows.path(), None, true));
    assert!(!held(Harness::Devin, windows.path(), None, false));
    // A Mac's or Linux's: the XDG places, either of them.
    for place in [".config/devin", ".local/share/devin"] {
        let unix = tempfile::tempdir().unwrap();
        assert!(!held(Harness::Devin, unix.path(), None, false), "{place}");
        fs::create_dir_all(unix.path().join(place)).unwrap();
        assert!(held(Harness::Devin, unix.path(), None, false), "{place}");
        assert!(!held(Harness::Devin, unix.path(), None, true), "{place}");
    }
}

#[test]
fn where_windows_names_its_appdata_devin_is_looked_for_there_and_not_in_the_profile() {
    let profile = tempfile::tempdir().unwrap();
    let redirected = tempfile::tempdir().unwrap();
    let appdata = redirected.path().to_string_lossy().into_owned();
    let own = profile.path().join("AppData").join("Roaming").join("devin");
    fs::create_dir_all(&own).unwrap();
    let places =
        |appdata: Option<&str>| login_places(Harness::Devin, profile.path(), appdata, true);
    assert_eq!(places(Some(&appdata)), [redirected.path().join("devin")]);
    assert!(!held(Harness::Devin, profile.path(), Some(&appdata), true));
    fs::create_dir_all(redirected.path().join("devin")).unwrap();
    assert!(held(Harness::Devin, profile.path(), Some(&appdata), true));
    // A machine that names none, or names it empty, has it in the profile's own.
    assert_eq!(places(None), std::slice::from_ref(&own));
    assert_eq!(places(Some("")), std::slice::from_ref(&own));
}

#[test]
fn every_other_harness_keeps_its_login_in_the_same_places_on_windows_as_anywhere() {
    let home = Path::new("home");
    for harness in Harness::ALL.into_iter().filter(|h| *h != Harness::Devin) {
        assert_eq!(
            login_places(harness, home, Some("appdata"), true),
            login_places(harness, home, None, false),
            "{harness:?}"
        );
    }
}

#[test]
fn a_machine_with_no_login_is_told_where_the_harness_keeps_it_and_what_to_do() {
    let home = Path::new("profile");
    let said = |harness, windows| {
        no_login(harness, &login_places(harness, home, None, windows)).to_string()
    };
    // The folder Devin keeps its login in, and not the profile that holds it.
    let roaming = home.join("AppData").join("Roaming").join("devin");
    assert_eq!(
        said(Harness::Devin, true),
        format!(
            "could not find {}: devin keeps its login there: log in with `devin auth login`",
            roaming.display()
        )
    );
    // Where there are two places, the first is named, and the other after it.
    assert_eq!(
        said(Harness::Claude, false),
        format!(
            "could not find {}: claude keeps its login there (or in {}): log in by running `claude` once",
            home.join(".claude.json").display(),
            home.join(".claude").display()
        )
    );
}

#[test]
fn a_window_reads_codexs_login_by_codex_home_and_every_others_by_the_machines_home() {
    let nothing = |_: &str| None;
    let codex = Installed::at(Harness::Codex, "bin/codex", "login");
    assert_eq!(
        codex.login(&nothing),
        [("CODEX_HOME".to_owned(), "login".to_owned())]
    );
    let pi = Installed::at(Harness::Pi, "bin/pi", "machine");
    let vars = pi.login(&nothing);
    let var = |name: &str| {
        vars.iter()
            .rev()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(var("HOME").as_deref(), Some("machine"));
    let config = Path::new("machine").join(".config");
    assert_eq!(
        var("XDG_CONFIG_HOME").as_deref(),
        Some(config.to_string_lossy().as_ref())
    );
    assert_eq!(var("XDG_DATA_HOME"), None, "the default is below the home");
    assert_eq!(var("CODEX_HOME"), None);
    // What the machine names for itself is what its windows read.
    let machine = |name: &str| match name {
        "XDG_CONFIG_HOME" => Some("/x/config".to_owned()),
        "XDG_DATA_HOME" => Some("/x/data".to_owned()),
        "XDG_CACHE_HOME" => Some(String::new()),
        _ => None,
    };
    let vars = pi.login(&machine);
    let var = |name: &str| {
        vars.iter()
            .rev()
            .find(|(given, _)| given == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(var("XDG_CONFIG_HOME").as_deref(), Some("/x/config"));
    assert_eq!(var("XDG_DATA_HOME").as_deref(), Some("/x/data"));
    assert_eq!(
        var("XDG_CACHE_HOME"),
        None,
        "a variable set to nothing is not set"
    );
}

#[test]
fn a_harness_needs_its_commands_folder_and_nodes_where_that_is_another() {
    let pi = Installed::at(Harness::Pi, "/opt/bin/pi", "home");
    assert_eq!(pi.folders(), [Path::new("/opt/bin")]);
    let with_node = pi.clone().with_node("/node/bin");
    assert_eq!(
        with_node.folders(),
        [Path::new("/opt/bin"), Path::new("/node/bin")]
    );
    let together = pi.with_node("/opt/bin");
    assert_eq!(together.folders(), [Path::new("/opt/bin")]);
}

#[test]
fn a_rig_given_a_claude_config_of_its_own_keeps_its_claude_windows_there() {
    let claude = Installed::at(Harness::Claude, "/opt/bin/claude", "/machine");
    let debug = format!(
        "{:?}",
        rig_keeping_claude_in(
            &[claude],
            "/the/stand-in",
            Some("/scratch/throwaway".to_owned())
        )
    );
    assert!(
        debug.contains("(\"CLAUDE_CONFIG_DIR\", \"/scratch/throwaway\")"),
        "{debug}"
    );
    assert!(!debug.contains("unset: [\"CLAUDE_CONFIG_DIR\"]"), "{debug}");
}

#[test]
fn every_harness_found_runs_on_a_home_of_the_runs_own_but_for_the_two_that_are_not_ours_to_give() {
    // The windows of Codex, Pi and Devin (where it is known how) run on a home with a copy
    // of the login: not a case's to remember to make.
    let mut isolated = vec![Harness::Codex, Harness::Pi];
    if cfg!(unix) {
        isolated.push(Harness::Devin);
    }
    for harness in isolated {
        let found = with_homes(Installed::at(harness, "/opt/bin/h", "/home/me")).unwrap();
        assert!(found.isolation().is_some(), "{harness:?}");
    }
    // Claude Code's is a config folder ([`Login`]), and OpenCode keeps the person's.
    for harness in [Harness::Claude, Harness::OpenCode] {
        let found = with_homes(Installed::at(harness, "/opt/bin/h", "/home/me")).unwrap();
        assert!(found.isolation().is_none(), "{harness:?}");
    }
}

#[test]
fn an_install_on_a_home_of_the_runs_own_is_given_the_persons_home_and_none_of_the_variables_it_reads_a_login_by(
) {
    let codex = Installed::at(Harness::Codex, "/codex/bin/codex", "/home/me/.codex")
        .with_isolation(Arc::new(Isolation::codex(
            Path::new("/home/me"),
            Path::new("/home/me/.codex"),
        )));
    let pi = Installed::at(Harness::Pi, "/opt/bin/pi", "/home/me").with_isolation(Arc::new(
        Isolation::pi(
            Path::new("/home/me"),
            Path::new("/home/me/.pi/agent"),
            "openrouter/x",
            Duration::from_secs(60),
        ),
    ));
    let debug = format!("{:?}", rig(&[codex, pi], "/the/stand-in"));
    // None of the variables the person's login is read by: not Codex's, the XDG folder or Pi's.
    for owners in ["CODEX_HOME", "XDG_CONFIG_HOME", "PI_CODING_AGENT_DIR"] {
        assert!(
            !debug.contains(&format!("(\"{owners}\"")),
            "{owners}: {debug}"
        );
    }
    // Their home is the windows' all the same: the folders of the project are below it (Devin's
    // window, opened below a folder of the rig's own, which macOS spells through the `/var` link
    // of its temporary folder, showed no answer: 2026-10-27).
    assert!(debug.contains("(\"HOME\", \"/home/me\")"), "{debug}");
    // What the rig has instead of the rest: a home for each, asked for its variables when it starts.
    assert_eq!(debug.matches("Isolation {").count(), 2, "{debug}");
}

#[test]
fn a_rig_for_several_harnesses_has_each_ones_folders_once_and_claude_without_its_config_dir() {
    let claude = Installed::at(Harness::Claude, "/opt/bin/claude", "/machine");
    let pi = Installed::at(Harness::Pi, "/opt/bin/pi", "/machine").with_node("/node/bin");
    let codex = Installed::at(Harness::Codex, "/codex/bin/codex", "/machine/.codex");
    let debug = format!("{:?}", rig(&[claude, pi, codex], "/the/stand-in"));
    // The folders the rig puts on the PATH, in order, none twice.
    let on_path = debug
        .split("path: [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .unwrap();
    assert_eq!(
        on_path.replace(['"', ' '], ""),
        "/opt/bin,/node/bin,/codex/bin"
    );
    assert!(debug.contains("unset: [\"CLAUDE_CONFIG_DIR\"]"), "{debug}");
    assert!(
        debug.contains("(\"CODEX_HOME\", \"/machine/.codex\")"),
        "{debug}"
    );
    // A locale that draws the windows' screens, which no harness's own setup gives them.
    assert!(debug.contains("(\"LANG\", \"en_US.UTF-8\")"), "{debug}");
    // A rig that has no Claude leaves the variable the stand-in uses.
    let plain = format!(
        "{:?}",
        rig(
            &[Installed::at(Harness::Pi, "/p/pi", "/m")],
            "/the/stand-in"
        )
    );
    assert!(plain.contains("unset: []"), "{plain}");
    assert!(plain.contains("claude_config: None"), "{plain}");
}

/// A Claude config folder a case keeps for the real Claude Code's windows.
#[derive(Debug)]
struct Kept;

impl ClaudeConfig for Kept {
    fn folder(&self) -> Result<PathBuf> {
        Ok(PathBuf::from("/kept/claude"))
    }

    fn described(&self) -> String {
        "the machine".to_owned()
    }

    fn finish(&self) -> Result {
        Ok(())
    }
}

#[test]
fn the_real_claudes_windows_run_on_the_config_folder_they_are_given_and_the_variable_is_not_cleared(
) {
    let claude = Installed::at(Harness::Claude, "/opt/bin/claude", "/machine")
        .with_claude_config(Arc::new(Kept));
    let debug = format!("{:?}", rig(&[claude], "/the/stand-in"));
    // The rig starts the daemon and the windows on that folder, and not on the person's own: the
    // variable is not unset (a rig that clears it leaves Claude on `~/.claude`).
    assert!(debug.contains("claude_config: Some(Kept)"), "{debug}");
    assert!(debug.contains("unset: []"), "{debug}");
}

#[test]
fn a_claude_config_the_run_was_given_wins_over_the_kits_own_copy_of_the_login() {
    // The merge of live-keychain with the long run's `--claude-config`: a folder the run's
    // caller made, the login copied into it, is where the windows keep their config, and
    // the kit makes no copy of its own beside it.
    let claude = Installed::at(Harness::Claude, "/opt/bin/claude", "/machine")
        .with_claude_config(Arc::new(Kept));
    let debug = format!(
        "{:?}",
        rig_keeping_claude_in(
            &[claude],
            "/the/stand-in",
            Some("/scratch/throwaway".to_owned())
        )
    );
    assert!(
        debug.contains("(\"CLAUDE_CONFIG_DIR\", \"/scratch/throwaway\")"),
        "{debug}"
    );
    assert!(debug.contains("claude_config: None"), "{debug}");
    assert!(!debug.contains("unset: [\"CLAUDE_CONFIG_DIR\"]"), "{debug}");
}

#[test]
fn the_user_whose_keychain_a_harness_asks_is_the_user_of_the_machine() {
    // The names are the machine's own, read where this run reads its variables.
    let expected: Vec<(String, String)> = ["USER", "LOGNAME"]
        .into_iter()
        .filter_map(|name| own_var(name).map(|value| (name.to_owned(), value)))
        .collect();
    let debug = format!("{:?}", rig(&[], "/the/stand-in"));
    for (name, value) in expected {
        assert!(debug.contains(&format!("({name:?}, {value:?})")), "{debug}");
    }
}

#[test]
fn a_windows_window_has_the_machines_folders_for_settings_and_programs_that_it_has_and_no_others() {
    // Devin keeps its settings and sessions under APPDATA: a window without it has none.
    let machine = |name: &str| match name {
        "APPDATA" => Some(r"C:\Users\ngvoi\AppData\Roaming".to_owned()),
        "OS" => Some("Windows_NT".to_owned()),
        "ProgramFiles(x86)" => Some(r"C:\Program Files (x86)".to_owned()),
        "HOME" => Some("not a Windows variable".to_owned()),
        _ => None,
    };
    // In the order they are listed, and only the ones the machine has.
    assert_eq!(
        windows_environment(&machine, true),
        [
            (
                "APPDATA".to_owned(),
                r"C:\Users\ngvoi\AppData\Roaming".to_owned()
            ),
            (
                "ProgramFiles(x86)".to_owned(),
                r"C:\Program Files (x86)".to_owned()
            ),
            ("OS".to_owned(), "Windows_NT".to_owned()),
        ]
    );
    assert!(windows_environment(&|_: &str| None, true).is_empty());
    // Another machine's windows have none of them, whatever its environment holds.
    assert!(windows_environment(&machine, false).is_empty());
    // What the rig and `process` pass every program is not said again here.
    for passed in [
        "SystemRoot",
        "ComSpec",
        "PATHEXT",
        "USERPROFILE",
        "USERNAME",
        "TEMP",
        "TMP",
    ] {
        assert!(!WINDOWS.contains(&passed), "{passed}");
    }
    // And a rig has them from the machine's own environment on Windows only.
    let debug = format!("{:?}", rig(&[], "/the/stand-in"));
    assert_eq!(
        debug.contains("(\"APPDATA\", "),
        cfg!(windows) && own_var("APPDATA").is_some(),
        "{debug}"
    );
}

#[test]
fn a_rig_of_stand_ins_and_the_real_claude_starts_claude_through_the_shim_and_has_them_write_where_the_real_one_does(
) {
    let folder = tempfile::tempdir().unwrap();
    let claude = Installed::at(Harness::Claude, "/opt/bin/claude", "/machine")
        .with_claude_config(Arc::new(Kept));
    let made = rig_with_stand_ins(
        std::slice::from_ref(&claude),
        folder.path(),
        Path::new("/the/fake-agent"),
    );
    if cfg!(windows) {
        // A shim is a shell script: on Windows there is none to start.
        assert!(made.unwrap_err().to_string().contains("macOS and Linux"));
        return;
    }
    let debug = format!("{:?}", made.unwrap());
    let shim = folder.path().join("claude-shimmed");
    assert!(
        debug.contains(&format!("stand_in: {:?}", shim)),
        "the rig's `claude` is the shim: {debug}"
    );
    // The stand-ins write where the real Claude's windows do, in the config folder the rig gave
    // them all, and not in the machine's own.
    assert!(debug.contains("claude_config: Some(Kept)"), "{debug}");
    assert!(!debug.contains("/machine/.claude"), "{debug}");
    let script = fs::read_to_string(&shim).unwrap();
    assert!(script.contains("'/the/fake-agent'"), "{script}");
    assert!(script.contains("'/opt/bin/claude'"), "{script}");
}

#[test]
fn a_rig_of_stand_ins_and_the_real_claude_needs_the_real_claude() {
    let folder = tempfile::tempdir().unwrap();
    let pi = Installed::at(Harness::Pi, "/opt/bin/pi", "/machine");
    let failed =
        rig_with_stand_ins(&[pi], folder.path(), Path::new("/the/fake-agent")).unwrap_err();
    assert!(
        failed.to_string().contains("needs Claude Code installed"),
        "{failed}"
    );
    assert!(!folder.path().join("claude-shimmed").exists());
}

#[test]
fn the_machines_home_is_the_one_this_run_has_and_a_run_that_has_none_is_an_error_that_says_so() {
    let named = own_var("HOME")
        .or_else(|| own_var("USERPROFILE"))
        .filter(|home| !home.is_empty());
    match (home(), named) {
        (Ok(home), Some(named)) => assert_eq!(home, Path::new(&named)),
        (Err(failed), None) => assert!(failed.to_string().contains("neither HOME nor USERPROFILE")),
        (found, named) => panic!("{found:?} is not the home this run has: {named:?}"),
    }
}

#[test]
fn a_live_test_has_a_folder_of_its_own_below_the_candidates_in_the_home_made_when_asked_for() {
    let home = tempfile::tempdir().unwrap();
    let folder = live_folder_in(home.path(), "paste").unwrap();
    assert_eq!(
        folder,
        home.path()
            .join(".consensflow-candidate")
            .join("live")
            .join("paste")
    );
    assert!(folder.is_dir());
    // Asked for again, it is the same folder and what was in it stays.
    fs::write(folder.join("README.md"), "kept").unwrap();
    assert_eq!(live_folder_in(home.path(), "paste").unwrap(), folder);
    assert_eq!(
        fs::read_to_string(folder.join("README.md")).unwrap(),
        "kept"
    );
}
