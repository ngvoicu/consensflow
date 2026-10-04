use super::*;
use std::fs;

const SESSION: &str = "15fba934-d727-4777-8791-123675a63649";

/// A file of `name` under `folder` of the root, its folders made.
fn touch(root: &Path, folder: &str, name: &str) -> PathBuf {
    let dir = root.join(folder);
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(name);
    fs::write(&file, "").unwrap();
    file
}

fn env_of(vars: &[(&str, &Path)]) -> Env {
    Env::from_vars(vars.iter().map(|(name, value)| (*name, value.as_os_str())))
}

#[test]
fn a_transcript_is_the_first_file_named_exactly_for_its_session() {
    let root = tempfile::tempdir().unwrap();
    let name = format!("{SESSION}.jsonl");
    touch(root.path(), "projects/-a", &format!("x{name}"));
    touch(root.path(), "projects/-a", &format!("{name}.bak"));
    // A folder of that name is no file.
    fs::create_dir_all(root.path().join("projects/-a").join(&name)).unwrap();
    let second = touch(root.path(), "projects/-c/deep/er", &name);
    let env = env_of(&[("CLAUDE_CONFIG_DIR", root.path())]);
    assert_eq!(transcript(SESSION, &env).unwrap(), Some(second));
    // Depth first, folders in name order: `-b` comes before `-c`.
    let first = touch(root.path(), "projects/-b", &name);
    assert_eq!(transcript(SESSION, &env).unwrap(), Some(first));
    assert_eq!(transcript("another", &env).unwrap(), None);
}

#[test]
fn the_config_folder_is_claude_config_dir_even_when_empty_else_claude_in_the_home() {
    let home = tempfile::tempdir().unwrap();
    let file = touch(
        &home.path().join(".claude"),
        "projects/-a",
        &format!("{SESSION}.jsonl"),
    );
    let at_home = env_of(&[("HOME", home.path())]);
    assert_eq!(transcript(SESSION, &at_home).unwrap(), Some(file));
    // `??` keeps an empty value: the sessions at home are not looked for.
    let empty = env_of(&[("HOME", home.path()), ("CLAUDE_CONFIG_DIR", Path::new(""))]);
    assert_eq!(transcript(SESSION, &empty).unwrap(), None);
    // A config folder needs no home.
    let elsewhere = tempfile::tempdir().unwrap();
    let env = env_of(&[("CLAUDE_CONFIG_DIR", elsewhere.path())]);
    assert_eq!(transcript(SESSION, &env).unwrap(), None);
}

#[test]
fn with_neither_a_config_folder_nor_a_home_there_is_nowhere_to_look() {
    assert_eq!(
        transcript(SESSION, &Env::default()).unwrap_err(),
        "missing home in env"
    );
}
