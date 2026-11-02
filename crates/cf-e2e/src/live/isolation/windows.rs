// What the kit gives Devin and Muse Code on Windows (T-1084), tested on every machine: the plans
// are path arithmetic over a profile laid out as Windows lays it, so a Mac holds them too. The
// zeewin2 run of a live suite is what shows the harness reads them where the plans say.
use super::*;

/// What the owner's Devin files hold: never said anywhere, and never in the run's home but
/// where a login is copied.
const SENTINEL: &str = "SENTINEL-the-owners-own-0002";

const NOW: i64 = 1_800_000_000_000;

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

/// Every file below `folder`, relative to it, sorted.
fn files_below(folder: &Path) -> Vec<String> {
    fn walk(folder: &Path, base: &Path, into: &mut Vec<String>) {
        for entry in fs::read_dir(folder).unwrap().flatten() {
            if entry.file_type().unwrap().is_dir() {
                walk(&entry.path(), base, into);
            } else {
                let relative = entry.path().strip_prefix(base).unwrap().to_path_buf();
                let parts: Vec<String> = relative
                    .components()
                    .map(|part| part.as_os_str().to_string_lossy().into_owned())
                    .collect();
                into.push(parts.join("/"));
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, folder, &mut found);
    found.sort();
    found
}

/// The owner's profile on Windows: `AppData\Roaming` and `AppData\Local` below it, Devin's files
/// in both, another program's beside them, and the scratch folder the run's home is made in.
struct Owner {
    profile: tempfile::TempDir,
    parent: tempfile::TempDir,
}

impl Owner {
    fn new() -> Self {
        let owner = Self {
            profile: tempfile::tempdir().unwrap(),
            parent: tempfile::tempdir().unwrap(),
        };
        for file in [
            "AppData/Roaming/devin/config.json",
            "AppData/Roaming/devin/credentials.toml",
            "AppData/Roaming/devin/cli/trusted_workspaces.json",
            "AppData/Roaming/devin/cli/sessions.db",
            "AppData/Roaming/devin/cli/logs/devin_20261102-080317_48960.log",
            "AppData/Roaming/devin/skills/mine/SKILL.md",
            "AppData/Roaming/other-tool/settings.json",
            "AppData/Local/devin/cli/_versions/3000.11.234/bin/devin.exe",
            "AppData/Local/devin/telemetry_state.json",
        ] {
            write(&owner.path(file), SENTINEL);
        }
        owner
    }

    fn path(&self, below: &str) -> PathBuf {
        self.profile.path().join(below)
    }

    /// Devin's, as the machine's `APPDATA` names the Roaming folder.
    fn devin(&self) -> Isolation {
        let mut made =
            Isolation::devin_on_windows(self.profile.path(), &self.path("AppData/Roaming"));
        made.parent = self.parent.path().to_path_buf();
        made
    }

    /// What is in the temporary folder the homes are made in.
    fn made(&self) -> usize {
        fs::read_dir(self.parent.path()).unwrap().count()
    }
}

#[test]
fn a_devin_run_on_windows_is_given_a_profile_of_its_own_and_the_three_variables_that_name_it() {
    let owner = Owner::new();
    let isolation = owner.devin();
    let home = isolation.made_at(NOW).unwrap();
    let vars = isolation.vars_in(&home);
    let names: Vec<&str> = vars.iter().map(|(name, _)| name.as_str()).collect();
    // Devin finds its folders through the system's known folders, which expand `%USERPROFILE%`
    // from the window's environment, and ignores `APPDATA` (T-727, T-1084); the daemon reads
    // Devin's sessions by `APPDATA` first (`cf_harness::devin::paths`). Both move, or the daemon
    // looks in the owner's Roaming for what Devin wrote in the run's.
    assert_eq!(
        names,
        ["USERPROFILE", "APPDATA", "LOCALAPPDATA"],
        "{vars:?}"
    );
    let value = |name: &str| -> PathBuf {
        PathBuf::from(&vars.iter().find(|(held, _)| held == name).unwrap().1)
    };
    assert!(value("USERPROFILE").starts_with(&home));
    assert_eq!(
        value("APPDATA"),
        value("USERPROFILE").join("AppData").join("Roaming")
    );
    assert_eq!(
        value("LOCALAPPDATA"),
        value("USERPROFILE").join("AppData").join("Local")
    );
    // The known-folder call fails with "path not found" for a profile whose AppData folders are
    // missing, so both are made before Devin starts.
    assert!(value("APPDATA").is_dir());
    assert!(value("LOCALAPPDATA").is_dir());
    // None of them names the owner's profile.
    for (name, value) in &vars {
        assert!(
            !value.starts_with(&owner.profile.path().display().to_string()),
            "{name} is {value}"
        );
    }
}

#[test]
fn a_devin_run_on_windows_keeps_the_owners_home_for_what_reads_it_by_home() {
    // Git and Git Bash read `HOME`, which the kit leaves the person's (`Installed::login`): the
    // owner's `.gitconfig` is found, and a commit a Devin window makes has its author.
    let owner = Owner::new();
    let isolation = owner.devin();
    let home = isolation.made_at(NOW).unwrap();
    assert!(
        isolation
            .vars_in(&home)
            .iter()
            .all(|(name, _)| name != "HOME"),
        "the kit's variables for Devin on Windows leave HOME alone"
    );
}

#[test]
fn a_devin_run_on_windows_copies_its_login_and_config_to_the_roaming_folder_and_nothing_else() {
    let owner = Owner::new();
    let isolation = owner.devin();
    let home = isolation.made_at(NOW).unwrap();
    // Its login and its config, where Devin reads them; its sessions, logs, trusted folders,
    // skills and caches are the owner's and are not copied, nor is another program's folder.
    assert_eq!(
        files_below(&home),
        [
            "profile/AppData/Roaming/devin/config.json",
            "profile/AppData/Roaming/devin/credentials.toml"
        ]
    );
    for name in ["config.json", "credentials.toml"] {
        assert_eq!(
            fs::read_to_string(home.join("profile/AppData/Roaming/devin").join(name)).unwrap(),
            SENTINEL
        );
    }
}

#[test]
fn a_devin_login_that_is_not_there_stops_the_run_naming_the_file_before_anything_is_made() {
    let owner = Owner::new();
    fs::remove_file(owner.path("AppData/Roaming/devin/credentials.toml")).unwrap();
    let isolation = owner.devin();
    let said = isolation.made_at(NOW).unwrap_err().to_string();
    assert!(said.contains("devin's login is not in"), "{said}");
    assert!(said.contains("credentials.toml"), "{said}");
    assert_eq!(
        owner.made(),
        0,
        "nothing is made for a login that is not there"
    );
}

#[test]
fn a_devin_config_that_is_not_there_is_no_reason_to_stop() {
    let owner = Owner::new();
    fs::remove_file(owner.path("AppData/Roaming/devin/config.json")).unwrap();
    let isolation = owner.devin();
    let home = isolation.made_at(NOW).unwrap();
    assert_eq!(
        files_below(&home),
        ["profile/AppData/Roaming/devin/credentials.toml"]
    );
}

#[test]
fn a_devin_run_on_windows_that_changes_a_file_of_the_owners_fails_naming_it() {
    let owner = Owner::new();
    let isolation = owner.devin();
    isolation.made_at(NOW).unwrap();
    assert!(
        isolation.finish().is_ok(),
        "nothing happened to the owner's"
    );
    for file in [
        "AppData/Roaming/devin/credentials.toml",
        "AppData/Roaming/devin/config.json",
        "AppData/Roaming/devin/cli/trusted_workspaces.json",
    ] {
        let path = owner.path(file);
        fs::write(&path, format!("{SENTINEL}-written-by-the-run")).unwrap();
        let said = isolation.finish().unwrap_err().to_string();
        assert!(said.contains("DEVIN SETUP"), "{file}: {said}");
        assert!(
            said.contains(&format!("~/{file} is not as it was")),
            "{file}: {said}"
        );
        assert!(
            !said.contains(SENTINEL),
            "nothing a file holds is said: {said}"
        );
        fs::write(&path, SENTINEL).unwrap();
    }
}

#[test]
fn what_devin_writes_in_the_runs_folders_leaves_the_owners_as_they_were() {
    let owner = Owner::new();
    let isolation = owner.devin();
    let home = isolation.made_at(NOW).unwrap();
    // A session, a log, a trusted folder and a cache, in the run's profile.
    for file in [
        "profile/AppData/Roaming/devin/cli/sessions.db",
        "profile/AppData/Roaming/devin/cli/trusted_workspaces.json",
        "profile/AppData/Roaming/devin/cli/logs/devin_run.log",
        "profile/AppData/Local/devin/telemetry_state.json",
    ] {
        write(&home.join(file), "the run's own");
    }
    assert!(
        isolation.finish().is_ok(),
        "the run wrote in its own folders only"
    );
    assert_eq!(
        fs::read_to_string(owner.path("AppData/Roaming/devin/cli/sessions.db")).unwrap(),
        SENTINEL
    );
    // Devin's own copy of the login written again is said, whatever the owner's is.
    write(
        &home.join("profile/AppData/Roaming/devin/credentials.toml"),
        "another login",
    );
    let said = isolation.finish().unwrap_err().to_string();
    assert!(said.contains("WROTE ITS COPY OF THE LOGIN AGAIN"), "{said}");
}

#[test]
fn the_variables_of_devin_on_windows_can_be_written_into_by_a_case_that_configures_it() {
    // A case that configures Devin writes below the variable that names its config folder, as
    // `write("XDG_CONFIG_HOME", "devin/config.json", …)` does on a Mac: `APPDATA` on Windows.
    let owner = Owner::new();
    let isolation = owner.devin();
    isolation
        .write("APPDATA", "devin/config.json", "the case's own")
        .unwrap();
    let home = isolation.made_at(NOW).unwrap();
    assert_eq!(
        fs::read_to_string(home.join("profile/AppData/Roaming/devin/config.json")).unwrap(),
        "the case's own"
    );
    assert_eq!(
        fs::read_to_string(owner.path("AppData/Roaming/devin/config.json")).unwrap(),
        SENTINEL
    );
}

#[test]
fn the_home_of_devin_on_windows_is_removed_with_the_run() {
    let owner = Owner::new();
    let isolation = owner.devin();
    let home = isolation.made_at(NOW).unwrap();
    assert!(home.is_dir());
    drop(isolation);
    assert!(!home.exists());
    assert_eq!(owner.made(), 0);
}

/// The machine's own Roaming folder, as the arm of `of_machine_lasting` takes it.
#[cfg(windows)]
fn machine_roaming(home: &Path) -> PathBuf {
    crate::process::own_var("APPDATA")
        .filter(|value| !value.is_empty())
        .map_or_else(|| home.join("AppData").join("Roaming"), PathBuf::from)
}

#[cfg(windows)]
#[test]
fn on_windows_devin_has_a_home_of_the_runs_own_made_from_the_machines_roaming_folder() {
    let home = Path::new(r"C:\Users\me");
    let made = Isolation::of_machine_lasting(Harness::Devin, home, Duration::from_secs(3600))
        .unwrap()
        .expect("Devin runs on a home of the run's own on Windows");
    assert_eq!(
        PathBuf::from(&made.plan.described),
        machine_roaming(home).join("devin").join("credentials.toml")
    );
    let names: Vec<&str> = made.plan.vars.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ["USERPROFILE", "APPDATA", "LOCALAPPDATA"]);
}
