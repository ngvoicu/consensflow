//! A role's text, written for its launch, as Node's role-skills suite held it
//! (the cases of each harness's own loading are its module's).

use std::fs;
use std::path::Path;

use super::*;

/// A home of its own, ConsensFlow's folder in it.
fn fixture() -> (tempfile::TempDir, Env) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().to_string_lossy().into_owned();
    let app = path::join(&[&home, "app"]);
    let env = Env::from_vars([("HOME", home), ("CONSENSFLOW_HOME", app)]);
    (dir, env)
}

fn launch(id: &str) -> LaunchId {
    LaunchId::new(id).unwrap()
}

const ONE: &str = "11111111-1111-4111-8111-111111111111";
const TWO: &str = "22222222-2222-4222-8222-222222222222";

#[test]
fn a_role_text_is_written_in_a_private_directory_and_global_skills_are_left_alone() {
    let (dir, env) = fixture();
    let global = dir.path().join(".claude/skills/consensflow");
    fs::create_dir_all(&global).unwrap();
    fs::write(global.join("SKILL.md"), "global canary").unwrap();
    let written = write_role(
        Harness::Claude,
        "chief",
        &env,
        &launch(ONE),
        "the chief's text",
    )
    .unwrap();
    assert!(written
        .root
        .starts_with(env.text("CONSENSFLOW_HOME").unwrap()));
    let text = fs::read_to_string(
        Path::new(&written.root).join(".claude/skills/consensflow-chief/SKILL.md"),
    )
    .unwrap();
    assert_eq!(text, "the chief's text");
    assert_eq!(
        fs::read_to_string(global.join("SKILL.md")).unwrap(),
        "global canary"
    );
}

#[test]
fn a_window_without_its_role_text_is_refused() {
    let (_dir, env) = fixture();
    assert_eq!(
        write_role(Harness::Pi, "worker", &env, &launch(ONE), ""),
        Err("the worker window needs its role text".to_owned())
    );
    let integrations = path::join(&[env.text("CONSENSFLOW_HOME").unwrap(), "integrations"]);
    assert!(!Path::new(&integrations).exists(), "nothing written");
}

#[test]
fn each_launch_has_a_role_file_of_its_own_one_chief_never_reads_another_projects_staff() {
    let (_dir, env) = fixture();
    for harness in [
        Harness::Claude,
        Harness::Opencode,
        Harness::Pi,
        Harness::Devin,
    ] {
        let folder = harness_folder(harness).unwrap();
        let mut files = Vec::new();
        for (id, staff) in [(ONE, "| saved-worker | worker |"), (TWO, "no staff")] {
            let written = write_role(harness, "chief", &env, &launch(id), staff).unwrap();
            let within = launch_folder(folder, &env, &launch(id)).unwrap();
            assert!(written.file.starts_with(&within), "{}", written.file);
            files.push(written.file);
        }
        assert_eq!(
            fs::read_to_string(&files[0]).unwrap(),
            "| saved-worker | worker |"
        );
        assert_eq!(
            fs::read_to_string(&files[1]).unwrap(),
            "no staff",
            "{harness:?}"
        );
    }
}

#[test]
fn a_role_text_is_private_to_its_launch_and_a_launch_id_names_nothing_outside_it() {
    let (_dir, env) = fixture();
    let written = write_role(Harness::Pi, "worker", &env, &launch(ONE), "worker text").unwrap();
    assert_eq!(fs::read_to_string(&written.file).unwrap(), "worker text");
    // Windows has no POSIX modes; its files answer 0o666 whatever the writer asked.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&written.file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    for id in ["..", "../elsewhere", ""] {
        assert!(LaunchId::new(id).is_none(), "{id}");
    }
}

#[test]
fn codex_keeps_no_role_file_its_window_is_given_the_text_itself() {
    let (_dir, env) = fixture();
    assert_eq!(
        write_role(Harness::Codex, "worker", &env, &launch(ONE), "worker text"),
        Err("No worker role is available for codex".to_owned())
    );
}
