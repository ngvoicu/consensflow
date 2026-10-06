//! The repair at the app's start, as the app makes it: the launchers of the home
//! it runs on are rewritten to run this bundle's `cf`, the launchers of another
//! home and what is not ours are not, none is made, and what fails is a line of
//! the log and no more. `cf_launcher`'s own tests hold the repair itself; these
//! hold what the app does with it.

use std::fs;
use std::path::{Path, PathBuf};

use cf_base::env::Env;
use cf_launcher::{install, Places};

use super::*;

/// A machine of this test's own: the user's home, and a bundle with a `cf` in it.
struct Machine {
    dir: tempfile::TempDir,
}

impl Machine {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("a machine");
        let machine = Self { dir };
        machine.cf_in("This");
        machine
    }

    fn user(&self) -> PathBuf {
        self.dir.path().join("user")
    }

    /// A `cf` that is there, in the bundle of an app called `app`.
    fn cf_in(&self, app: &str) -> PathBuf {
        let cf = self
            .dir
            .path()
            .join(app)
            .join("cli")
            .join("bin")
            .join(if cfg!(windows) { "cf.exe" } else { "cf" });
        fs::create_dir_all(cf.parent().expect("a folder")).expect("the bundle's folders");
        fs::write(&cf, "the program").expect("a cf");
        cf
    }

    /// The live app's environment: no `CONSENSFLOW_HOME`, so its home is `~/.consensflow`.
    fn live(&self) -> Env {
        Env::from_vars([("HOME", self.user())])
    }

    fn live_home(&self) -> PathBuf {
        self.user().join(".consensflow")
    }

    /// The Candidate's: a home of its own, as `isolated_home` sets it.
    fn candidate(&self) -> Env {
        Env::from_vars([
            ("HOME", self.user()),
            ("CONSENSFLOW_HOME", self.candidate_home()),
        ])
    }

    fn candidate_home(&self) -> PathBuf {
        self.user().join(".consensflow-candidate")
    }

    /// The command `cf setup` of an app on `env` wrote, naming `cf`, in `home`'s bin.
    fn setup(&self, env: &Env, cf: &Path, home: &Path) -> PathBuf {
        let bin = home.join("bin");
        install(env, cf, &Places::at(vec![bin.clone()])).expect("a command");
        bin.join(if cfg!(windows) { "cf.cmd" } else { "cf" })
    }
}

#[test]
fn a_marked_command_naming_another_bundle_is_rewritten_when_it_serves_this_apps_home() {
    let machine = Machine::new();
    let this = machine.cf_in("This");
    let other = machine.cf_in("Older");
    for (env, home, pinned) in [
        (machine.live(), machine.live_home(), false),
        (machine.candidate(), machine.candidate_home(), true),
    ] {
        let command = machine.setup(&env, &other, &home);

        let said = repair_with(&env, &this);

        // Both names of the command, one line each.
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(
            said.contains(&format!(
                "the terminal command {} now runs {}",
                command.display(),
                this.display()
            )),
            "{said:?}"
        );
        let text = fs::read_to_string(&command).expect("the command");
        assert!(text.contains(&this.display().to_string()), "{text}");
        assert!(!text.contains(&other.display().to_string()), "{text}");
        // The home it pinned is the home it keeps, and a command that pinned none pins none.
        assert_eq!(text.contains("CONSENSFLOW_HOME"), pinned, "{text}");
    }
}

#[test]
fn a_command_pinned_to_another_home_is_left_byte_for_byte_in_both_directions() {
    let machine = Machine::new();
    let this = machine.cf_in("This");
    let other = machine.cf_in("Older");
    // The Candidate's command found in the live app's bin, and the live app's in
    // the Candidate's: each app looks only in its own home's bin, and finds the
    // other's command there.
    let candidates = machine.setup(&machine.candidate(), &other, &machine.live_home());
    let lives = machine.setup(
        &Env::from_vars([
            ("HOME", machine.user().into_os_string()),
            ("CONSENSFLOW_HOME", machine.live_home().into_os_string()),
        ]),
        &other,
        &machine.candidate_home(),
    );
    let before = (
        fs::read(&candidates).expect("one"),
        fs::read(&lives).expect("the other"),
    );

    // The live app starts, and then the Candidate.
    assert_eq!(repair_with(&machine.live(), &this), Vec::<String>::new());
    assert_eq!(
        repair_with(&machine.candidate(), &this),
        Vec::<String>::new()
    );

    assert_eq!(
        (
            fs::read(&candidates).expect("one"),
            fs::read(&lives).expect("the other")
        ),
        before,
        "each is the other's, and neither app rewrote it"
    );
}

#[test]
fn an_unmarked_command_is_left_byte_for_byte() {
    let machine = Machine::new();
    let this = machine.cf_in("This");
    let theirs = machine
        .live_home()
        .join("bin")
        .join(if cfg!(windows) { "cf.cmd" } else { "cf" });
    fs::create_dir_all(theirs.parent().expect("a folder")).expect("its folder");
    fs::write(&theirs, "someone else's cf\n").expect("their command");

    assert_eq!(repair_with(&machine.live(), &this), Vec::<String>::new());

    assert_eq!(
        fs::read_to_string(&theirs).expect("their command"),
        "someone else's cf\n"
    );
}

#[test]
fn no_command_is_created_where_there_is_none() {
    let machine = Machine::new();
    let this = machine.cf_in("This");

    assert_eq!(repair_with(&machine.live(), &this), Vec::<String>::new());
    assert_eq!(
        repair_with(&machine.candidate(), &this),
        Vec::<String>::new()
    );

    assert!(
        !machine.user().exists(),
        "no home, no bin and no command is made for a machine that never ran `cf setup`"
    );
}

#[test]
fn a_failure_is_one_line_of_the_log_and_the_other_name_is_repaired_all_the_same() {
    let machine = Machine::new();
    let this = machine.cf_in("This");
    let other = machine.cf_in("Older");
    let env = machine.live();
    let command = machine.setup(&env, &other, &machine.live_home());
    let bin = machine.live_home().join("bin");
    // A folder where the app's own name of the command should be.
    let blocked = bin.join(if cfg!(windows) {
        "consensflow.cmd"
    } else {
        "consensflow"
    });
    fs::remove_file(&blocked).expect("the command is made way for");
    fs::create_dir(&blocked).expect("a folder in its place");

    let said = repair_with(&env, &this);

    assert_eq!(said.len(), 2, "{said:?}");
    assert!(
        said.iter().any(|line| line.starts_with(&format!(
            "the terminal command {} could not be repaired: ",
            blocked.display()
        ))),
        "{said:?}"
    );
    assert!(
        said.iter().any(|line| line.starts_with(&format!(
            "the terminal command {} now runs ",
            command.display()
        ))),
        "{said:?}"
    );
    assert!(blocked.is_dir());
}

#[test]
fn a_cf_that_is_not_there_is_no_command_to_name_and_none_is_repaired() {
    let machine = Machine::new();
    let this = machine.cf_in("This");
    let other = machine.cf_in("Older");
    let env = machine.live();
    let command = machine.setup(&env, &other, &machine.live_home());
    let before = fs::read(&command).expect("the command");
    let gone = machine.dir.path().join("Gone").join("cf");

    let said = repair_with(&env, &gone);

    assert_eq!(
        said,
        [format!(
            "the terminal command is not repaired: the bundled cf is missing ({gone:?})"
        )]
    );
    assert_eq!(fs::read(&command).expect("the command"), before);
    // A path that is not absolute is no more one to name.
    assert_eq!(repair_with(&env, Path::new("cf")).len(), 1);
    drop(this);
}

#[cfg(unix)]
#[test]
fn an_app_run_from_a_copy_macos_takes_away_leaves_the_command_and_says_so() {
    let machine = Machine::new();
    let other = machine.cf_in("Older");
    let env = machine.live();
    let command = machine.setup(&env, &other, &machine.live_home());
    let before = fs::read(&command).expect("the command");
    let translocated =
        machine.cf_in("AppTranslocation/4F2A-90C1/d/ConsensFlow.app/Contents/Resources");

    let said = repair_with(&env, &translocated);

    assert_eq!(said.len(), 2, "{said:?}");
    assert!(
        said.iter()
            .all(|line| line.contains("is left as it is: this app runs from a copy")),
        "{said:?}"
    );
    assert_eq!(fs::read(&command).expect("the command"), before);
}

#[cfg(windows)]
#[test]
fn a_cf_in_the_verbatim_spelling_is_named_plainly() {
    let machine = Machine::new();
    let this = machine.cf_in("This");
    let other = machine.cf_in("Older");
    let env = machine.live();
    let command = machine.setup(&env, &other, &machine.live_home());
    // Tauri answers its folders as `\\?\C:\…`, which cmd.exe starts nothing through.
    let verbatim = fs::canonicalize(&this).expect("the verbatim spelling");
    assert!(verbatim.to_string_lossy().starts_with(r"\\?\"));

    repair_with(&env, &verbatim);

    let text = fs::read_to_string(&command).expect("the command");
    assert!(!text.contains(r"\\?\"), "{text}");
    // The plain spelling of the same place: the long one, as a temporary folder
    // given in its short 8.3 form (`RUNNER~1`) resolves to.
    let plain = verbatim.to_string_lossy();
    let plain = plain.strip_prefix(r"\\?\").unwrap_or(&plain);
    assert!(text.contains(plain), "{text}");
}
