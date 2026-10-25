//! What Codex's configuration says of its computer use: set up only where the
//! ChatGPT app's plugin is on, its program is there and the service's app is in
//! place; read and never written.

use std::fs;
use std::path::Path;

use super::*;

/// A Codex home as the ChatGPT app leaves it: `config` as `config.toml`, one
/// installed version of the plugin naming `program` for its server, and the
/// service's app.
fn home_with(root: &Path, config: &str, program: &str) -> String {
    let home = root.join("codex");
    let version = home.join("plugins/cache/openai-bundled/unified-computer-use/26.1021.31345");
    fs::create_dir_all(&version).unwrap();
    fs::write(home.join("config.toml"), config).unwrap();
    fs::write(
        version.join(".mcp.json"),
        serde_json::json!({ "mcpServers": { "cua_repl": { "command": program, "args": [] } } })
            .to_string(),
    )
    .unwrap();
    fs::create_dir_all(home.join("computer-use/Codex Computer Use.app/Contents")).unwrap();
    home.to_string_lossy().into_owned()
}

/// A program that is there, as the ChatGPT app's Node is.
fn program(root: &Path) -> String {
    let node = root.join("ChatGPT.app/Contents/Resources/cua_node/bin/node");
    fs::create_dir_all(node.parent().unwrap()).unwrap();
    fs::write(&node, "").unwrap();
    node.to_string_lossy().into_owned()
}

/// What the ChatGPT app writes: the plugin on, and the dead server entry beside it.
const SET_UP: &str = r#"model = "gpt-6-astra"

[plugins."unified-computer-use@openai-bundled"]
enabled = true

[mcp_servers.computer-use]
command = "./Codex Computer Use.app/Contents/SharedSupport/SkyComputerUseClient.app/Contents/MacOS/SkyComputerUseClient"
args = ["mcp"]
cwd = "."
enabled = true
"#;

fn in_home(home: &str) -> Env {
    Env::from_vars([("CODEX_HOME", home), ("HOME", "/no/home")])
}

#[test]
fn set_up_where_the_plugin_is_on_its_program_is_there_and_the_service_app_is_in_place() {
    let root = tempfile::tempdir().unwrap();
    let home = home_with(root.path(), SET_UP, &program(root.path()));
    assert_eq!(computer_use(&in_home(&home)), ComputerUse { set_up: true });
}

#[test]
fn the_home_is_codex_home_else_dot_codex_in_the_users_home() {
    let root = tempfile::tempdir().unwrap();
    let node = program(root.path());
    let user = root.path().join("user");
    fs::create_dir_all(&user).unwrap();
    let home = home_with(root.path(), SET_UP, &node);
    fs::rename(&home, user.join(".codex")).unwrap();
    let env = Env::from_vars([("HOME", user.to_string_lossy().as_ref())]);
    assert!(computer_use(&env).set_up);
    let elsewhere = Env::from_vars([
        ("HOME", user.to_string_lossy().as_ref()),
        ("CODEX_HOME", "/no/codex"),
    ]);
    assert!(!computer_use(&elsewhere).set_up);
}

#[test]
fn not_set_up_where_the_plugin_is_off_or_unnamed_whatever_the_server_entry_says() {
    let root = tempfile::tempdir().unwrap();
    let node = program(root.path());
    for config in [
        SET_UP.replace("enabled = true\n\n[mcp", "enabled = false\n\n[mcp"),
        SET_UP.replace(
            "unified-computer-use@openai-bundled",
            "computer-use@openai-bundled",
        ),
        "[mcp_servers.computer-use]\ncommand = \"./x\"\ncwd = \".\"\nenabled = true\n".to_owned(),
        "[plugins.\"unified-computer-use@openai-bundled\"]\nenabled = \"yes\"\n".to_owned(),
        "not = [toml".to_owned(),
        String::new(),
    ] {
        let case = tempfile::tempdir().unwrap();
        let home = home_with(case.path(), &config, &node);
        assert!(!computer_use(&in_home(&home)).set_up, "{config}");
    }
}

#[test]
fn not_set_up_where_the_plugins_program_or_the_service_app_is_missing() {
    let root = tempfile::tempdir().unwrap();
    let home = home_with(root.path(), SET_UP, "/Applications/Gone.app/node");
    assert!(
        !computer_use(&in_home(&home)).set_up,
        "the ChatGPT app's Node gone"
    );

    let root = tempfile::tempdir().unwrap();
    let home = home_with(root.path(), SET_UP, &program(root.path()));
    fs::remove_dir_all(Path::new(&home).join("computer-use")).unwrap();
    assert!(!computer_use(&in_home(&home)).set_up, "no service app");

    let root = tempfile::tempdir().unwrap();
    let home = home_with(root.path(), SET_UP, &program(root.path()));
    fs::remove_dir_all(Path::new(&home).join("plugins")).unwrap();
    assert!(!computer_use(&in_home(&home)).set_up, "no installed plugin");

    let root = tempfile::tempdir().unwrap();
    let home = home_with(root.path(), SET_UP, &program(root.path()));
    let manifest = Path::new(&home)
        .join("plugins/cache/openai-bundled/unified-computer-use/26.1021.31345/.mcp.json");
    fs::write(&manifest, "{ broken").unwrap();
    assert!(
        !computer_use(&in_home(&home)).set_up,
        "a manifest that is no JSON"
    );
}

#[test]
fn config_toml_is_read_again_only_when_its_size_or_modification_time_changed() {
    let root = tempfile::tempdir().unwrap();
    let home = home_with(root.path(), SET_UP, &program(root.path()));
    let config = Path::new(&home).join("config.toml");
    let modified = fs::metadata(&config).unwrap().modified().unwrap();
    assert!(computer_use(&in_home(&home)).set_up);
    // The same size and the same time: what was read is kept.
    let off = SET_UP.replacen("enabled = true\n\n[mcp", "enabled =false\n\n[mcp", 1);
    assert_eq!(off.len(), SET_UP.len());
    fs::write(&config, &off).unwrap();
    let file = fs::File::options().write(true).open(&config).unwrap();
    file.set_modified(modified).unwrap();
    assert!(
        computer_use(&in_home(&home)).set_up,
        "the unchanged file is not read again"
    );
    // Another time: it is read again.
    file.set_modified(modified + std::time::Duration::from_secs(5))
        .unwrap();
    assert!(
        !computer_use(&in_home(&home)).set_up,
        "the changed file is read again"
    );
}

#[test]
fn a_read_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let home = home_with(root.path(), SET_UP, &program(root.path()));
    let listing = |dir: &str| {
        let mut all: Vec<_> = walk(Path::new(dir))
            .into_iter()
            .map(|file| (file.clone(), fs::read(&file).ok()))
            .collect();
        all.sort();
        all
    };
    let before = listing(&home);
    computer_use(&in_home(&home));
    assert_eq!(listing(&home), before);
}

/// Every file and folder under `dir`.
fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        found.push(path.clone());
        if path.is_dir() {
            found.extend(walk(&path));
        }
    }
    found
}
