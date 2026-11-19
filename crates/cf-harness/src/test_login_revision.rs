//! Login discovery examines metadata in isolated fixture homes.
use crate::contract::Adapter;
use crate::testing::Fakes;
use cf_base::env::Env;
use std::path::Path;

async fn checks(harness: &str) {
    let home = tempfile::tempdir().unwrap();
    let root = home.path();
    let claude = root.join("claude");
    let codex = root.join("codex");
    let pi = root.join("pi");
    let data = root.join("data");
    let config = root.join("config");
    let env = Env::from_vars([
        ("HOME", root.to_str().unwrap()),
        ("USERPROFILE", root.to_str().unwrap()),
        ("CLAUDE_CONFIG_DIR", claude.to_str().unwrap()),
        ("CODEX_HOME", codex.to_str().unwrap()),
        ("PI_CODING_AGENT_DIR", pi.to_str().unwrap()),
        ("APPDATA", data.to_str().unwrap()),
        ("XDG_DATA_HOME", data.to_str().unwrap()),
        ("XDG_CONFIG_HOME", config.to_str().unwrap()),
    ]);
    let services = Fakes::new(&env).services(&env, Path::new(""));
    let (adapter, path): (Box<dyn Adapter>, _) = match harness {
        "claude" => (
            Box::new(crate::claude::ClaudeAdapter::new(&services)),
            claude.join(".credentials.json"),
        ),
        "codex" => (
            Box::new(crate::codex::CodexAdapter::new(&services)),
            codex.join("auth.json"),
        ),
        "pi" => (
            Box::new(crate::pi::PiAdapter::new(&services)),
            pi.join("auth.json"),
        ),
        "devin" => (
            Box::new(crate::devin::DevinAdapter::new(&services)),
            data.join("devin/credentials.toml"),
        ),
        "muse" => (
            Box::new(crate::muse::MuseAdapter::new(&services)),
            config.join("muse/auth.json"),
        ),
        _ => unreachable!(),
    };
    let missing = adapter
        .login_revision()
        .await
        .expect("a missing login has a revision");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"fixture").unwrap();
    let present = adapter.login_revision().await.unwrap();
    assert_ne!(missing, present, "creation changes the login metadata");
    std::fs::write(&path, b"fixture-longer").unwrap();
    assert_ne!(
        present,
        adapter.login_revision().await.unwrap(),
        "size changes the revision"
    );
}

#[tokio::test]
async fn claude_login_metadata() {
    checks("claude").await;
}
#[tokio::test]
async fn codex_login_metadata() {
    checks("codex").await;
}
#[tokio::test]
async fn pi_login_metadata() {
    checks("pi").await;
}
#[tokio::test]
async fn devin_login_metadata() {
    checks("devin").await;
}
#[tokio::test]
async fn muse_login_metadata() {
    checks("muse").await;
}

#[tokio::test]
async fn codex_revision_tracks_path_time_and_identity_without_reading_content() {
    use std::time::{Duration, UNIX_EPOCH};
    let home = tempfile::tempdir().unwrap();
    let one = home.path().join("one");
    let two = home.path().join("two");
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    let first = one.join("auth.json");
    let second = two.join("auth.json");
    std::fs::write(&first, b"first").unwrap();
    let modified = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let set_time = |p: &Path, at| {
        std::fs::OpenOptions::new()
            .write(true)
            .open(p)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(at))
            .unwrap();
    };
    set_time(&first, modified);
    std::fs::hard_link(&first, &second).unwrap();
    let env1 = Env::from_vars([
        ("CODEX_HOME", one.to_str().unwrap()),
        ("HOME", home.path().to_str().unwrap()),
        ("USERPROFILE", home.path().to_str().unwrap()),
    ]);
    let env2 = Env::from_vars([
        ("CODEX_HOME", two.to_str().unwrap()),
        ("HOME", home.path().to_str().unwrap()),
        ("USERPROFILE", home.path().to_str().unwrap()),
    ]);
    let services1 = Fakes::new(&env1).services(&env1, Path::new(""));
    let services2 = Fakes::new(&env2).services(&env2, Path::new(""));
    let adapter1 = crate::codex::CodexAdapter::new(&services1);
    let adapter2 = crate::codex::CodexAdapter::new(&services2);
    let initial = adapter1
        .login_revision()
        .await
        .expect("login metadata available");
    assert_ne!(
        initial,
        adapter2.login_revision().await.unwrap(),
        "the path is part of the revision"
    );
    std::fs::write(&first, b"other").unwrap();
    set_time(&first, modified);
    assert_eq!(
        initial,
        adapter1.login_revision().await.unwrap(),
        "different bytes with identical metadata are not read"
    );
    set_time(&first, modified + Duration::from_nanos(100));
    assert_ne!(
        initial,
        adapter1.login_revision().await.unwrap(),
        "submillisecond modification time matters"
    );
    set_time(&first, modified);
    std::fs::remove_file(&first).unwrap();
    std::fs::write(&first, b"other").unwrap();
    set_time(&first, modified);
    assert_ne!(
        initial,
        adapter1.login_revision().await.unwrap(),
        "replacement with equal size and time changes file identity"
    );
}
