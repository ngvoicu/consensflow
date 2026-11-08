//! Codex's memories folders: `$CODEX_HOME/memories`, and `.codex/memories` of the user's home
//! where `CODEX_HOME` is unset or empty.

use super::*;
use cf_base::path;

fn env_of(vars: &[(&str, &str)]) -> Env {
    Env::from_vars(vars.iter().copied())
}

/// `.codex/memories` of the home `home`, as the system writes it.
fn default_of(home: &str) -> PathBuf {
    PathBuf::from(path::join(&[home, ".codex"])).join("memories")
}

#[test]
fn a_codex_home_that_is_set_is_the_only_one_and_its_memories_folder_is_the_job_s() {
    let env = env_of(&[
        ("CODEX_HOME", "/elsewhere/codex"),
        ("HOME", "/h"),
        ("USERPROFILE", "/u"),
    ]);
    assert_eq!(
        memories_folders(&env),
        [PathBuf::from("/elsewhere/codex").join("memories")]
    );
}

#[test]
fn an_empty_codex_home_is_none_and_the_default_is_of_each_home_the_environment_names() {
    for vars in [
        vec![("CODEX_HOME", ""), ("HOME", "/h")],
        vec![("HOME", "/h")],
    ] {
        assert_eq!(memories_folders(&env_of(&vars)), [default_of("/h")]);
    }
    // Windows' own is the profile, which Codex reads there: it is looked in too, once.
    assert_eq!(
        memories_folders(&env_of(&[("USERPROFILE", "/u")])),
        [default_of("/u")]
    );
    assert_eq!(
        memories_folders(&env_of(&[("HOME", "/h"), ("USERPROFILE", "/u")])),
        [default_of("/h"), default_of("/u")]
    );
    assert_eq!(
        memories_folders(&env_of(&[("HOME", "/h"), ("USERPROFILE", "/h")])),
        [default_of("/h")]
    );
    // An empty home is none, and where there is none there is no memories folder.
    assert_eq!(
        memories_folders(&env_of(&[("HOME", ""), ("USERPROFILE", "/u")])),
        [default_of("/u")]
    );
    assert_eq!(memories_folders(&env_of(&[])), Vec::<PathBuf>::new());
    assert_eq!(
        memories_folders(&env_of(&[("HOME", "")])),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn the_words_say_where_the_command_ran_and_whose_job_it_is() {
    assert_eq!(
        WORDS,
        "This command ran in Codex's memories folder: Codex's memory job isn't on the board."
    );
}
