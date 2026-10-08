//! The install layouts as Node's admin suite held them, with Windows' spellings
//! besides; the recorded goldens (`tests/goldens/admin`) hold each to Node's
//! answer.

use std::fs;

use super::*;

/// What `path.join` makes of `parts` on this system.
fn joined(parts: &[&str]) -> String {
    path::join(parts)
}

fn source(harness: Harness, executable: &str, env: &Env) -> Source {
    release_source(harness, executable, env)
}

fn said(source: &Source) -> (&str, &str, Option<&str>, Option<&[String]>) {
    (
        &source.url,
        source.format.as_str(),
        source.distribution.as_deref(),
        source.update.as_deref(),
    )
}

#[test]
fn homebrew_is_told_by_its_cask_or_formula_folder_and_named_by_the_package() {
    let env = Env::default();
    let cask = source(
        Harness::Codex,
        "/opt/homebrew/Caskroom/codex/0.1/bin/codex",
        &env,
    );
    assert_eq!(
        said(&cask),
        (
            "https://formulae.brew.sh/api/cask/codex.json",
            "cask",
            Some("Homebrew"),
            Some(
                &[
                    joined(&["/opt/homebrew", "bin", "brew"]),
                    "upgrade".to_owned(),
                    "--cask".to_owned(),
                    "codex".to_owned()
                ][..]
            )
        )
    );
    let formula = source(
        Harness::Opencode,
        "/opt/homebrew/Cellar/opencode/1/bin/opencode",
        &env,
    );
    assert_eq!(
        said(&formula),
        (
            "https://formulae.brew.sh/api/formula/opencode.json",
            "formula",
            Some("Homebrew"),
            Some(
                &[
                    joined(&["/opt/homebrew", "bin", "brew"]),
                    "upgrade".to_owned(),
                    "opencode".to_owned()
                ][..]
            )
        )
    );
}

#[test]
fn claude_is_homebrew_s_under_either_of_its_two_names_and_no_other_harness_is() {
    let env = Env::default();
    for package in ["claude-code", "claude-code@latest"] {
        let found = source(
            Harness::Claude,
            &format!("/opt/homebrew/Caskroom/{package}/2/bin/claude"),
            &env,
        );
        assert_eq!(
            found.url,
            format!("https://formulae.brew.sh/api/cask/{package}.json")
        );
    }
    for (harness, folder) in [
        (Harness::Claude, "Caskroom/claude"),
        (Harness::Codex, "Caskroom/claude-code"),
        (Harness::Codex, "Cellar/codex-cli"),
    ] {
        let found = source(harness, &format!("/opt/homebrew/{folder}/1/bin/x"), &env);
        assert_eq!(found.distribution, None, "{harness:?} {folder}");
    }
}

#[test]
fn the_last_cask_or_formula_folder_names_the_package() {
    let found = source(
        Harness::Codex,
        "/a/Cellar/other/1/Caskroom/codex/2/bin/codex",
        &Env::default(),
    );
    assert_eq!(found.url, "https://formulae.brew.sh/api/cask/codex.json");
    assert_eq!(
        found.update.unwrap()[0],
        joined(&["/a/Cellar/other/1", "bin", "brew"])
    );
}

#[test]
fn claude_s_own_installer_follows_the_channel_its_settings_name() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let config = home.join(".claude");
    fs::create_dir_all(&config).unwrap();
    let env = Env::from_vars([("HOME", home.to_str().unwrap())]);
    let executable = "/Users/me/.local/share/claude/versions/2.1.280";
    let latest = source(Harness::Claude, executable, &env);
    assert_eq!(
        said(&latest),
        (
            "https://downloads.claude.ai/claude-code-releases/latest",
            "text",
            Some("Claude's installer, latest channel"),
            Some(&[executable.to_owned(), "update".to_owned()][..])
        )
    );
    for (settings, channel) in [
        (r#"{"autoUpdatesChannel":"stable"}"#, "stable"),
        (r#"{"autoUpdatesChannel":"beta"}"#, "latest"),
        (r#"{"autoUpdatesChannel":["stable"]}"#, "latest"),
        ("null", "latest"),
        ("[]", "latest"),
        ("not json", "latest"),
        ("\u{FEFF}{\"autoUpdatesChannel\":\"stable\"}", "latest"),
        (
            r#"{"autoUpdatesChannel":"x","autoUpdatesChannel":"stable"}"#,
            "stable",
        ),
    ] {
        fs::write(config.join("settings.json"), settings).unwrap();
        let found = source(Harness::Claude, executable, &env);
        assert_eq!(
            found.distribution,
            Some(format!("Claude's installer, {channel} channel")),
            "{settings}"
        );
        assert!(found.url.ends_with(channel), "{settings}");
    }
}

#[test]
fn claude_settings_are_read_where_the_environment_says_and_otherwise_in_the_home() {
    let root = tempfile::tempdir().unwrap();
    let elsewhere = root.path().join("elsewhere");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::write(
        elsewhere.join("settings.json"),
        r#"{"autoUpdatesChannel":"stable"}"#,
    )
    .unwrap();
    let executable = "/Users/me/.local/share/claude/versions/2.1.280";
    let given = Env::from_vars([
        ("CLAUDE_CONFIG_DIR", elsewhere.to_str().unwrap()),
        ("HOME", "/nowhere"),
    ]);
    assert!(source(Harness::Claude, executable, &given)
        .url
        .ends_with("stable"));
    // No home to read them in, and none named: the latest channel.
    let neither = Env::default();
    assert!(source(Harness::Claude, executable, &neither)
        .url
        .ends_with("latest"));
}

#[test]
fn claude_s_copy_on_windows_counts_only_with_its_versions_beside_it() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let copy = home.join(".local").join("bin").join("claude.exe");
    fs::create_dir_all(copy.parent().unwrap()).unwrap();
    fs::write(&copy, "").unwrap();
    let env = Env::from_vars([("HOME", home.to_str().unwrap())]);
    let executable = copy.to_str().unwrap();
    assert_eq!(
        source(Harness::Claude, executable, &env).distribution,
        None,
        "no versions beside it"
    );
    fs::create_dir_all(home.join(".local/share/claude/versions/2.1.274")).unwrap();
    let found = source(Harness::Claude, executable, &env);
    assert_eq!(
        (found.distribution.as_deref(), found.update),
        (
            Some("Claude's installer, latest channel"),
            Some(vec![executable.to_owned(), "update".to_owned()])
        )
    );
}

#[test]
fn the_name_of_claude_s_copy_is_read_in_any_case_and_a_folder_ends_at_a_line() {
    let copied = |path: &str| COPIED.captures(path).map(|found| found[1].to_owned());
    assert_eq!(
        copied("C:/Users/me/.LOCAL/BIN/CLAUDE.EXE").as_deref(),
        Some("C:/Users/me")
    );
    assert_eq!(copied("C:/Users/me/.local/bin/claude.exe.old"), None);
    // `.` does not take a line terminator, so a folder's name holding one
    // is no prefix.
    assert_eq!(copied("a\nb/.local/bin/claude.exe"), None);
    assert_eq!(copied("ab/.local/bin/claude.exe").as_deref(), Some("ab"));
}

#[test]
fn a_global_npm_install_is_updated_through_the_npm_beside_it() {
    let env = Env::default();
    let pi = source(
        Harness::Pi,
        "/usr/lib/node_modules/@earendil-works/pi-coding-agent/dist/cli.js",
        &env,
    );
    assert_eq!(
        said(&pi),
        (
            "https://registry.npmjs.org/@earendil-works/pi-coding-agent/latest",
            "npm",
            Some("npm"),
            Some(
                &[
                    joined(&["/usr", "bin", "npm"]),
                    "install".to_owned(),
                    "-g".to_owned(),
                    "@earendil-works/pi-coding-agent@latest".to_owned()
                ][..]
            )
        )
    );
    // The last `/lib/node_modules/` is the one the prefix ends at.
    let nested = source(
        Harness::Codex,
        "/a/lib/node_modules/x/lib/node_modules/@openai/codex/bin/codex.js",
        &env,
    );
    assert_eq!(
        nested.update.unwrap()[0],
        joined(&["/a/lib/node_modules/x", "bin", "npm"])
    );
    // Devin has no npm package; Windows' npm folder has no `lib`.
    for (harness, path) in [
        (Harness::Devin, "/usr/lib/node_modules/devin/bin/devin"),
        (
            Harness::Codex,
            r"C:\Users\me\AppData\Roaming\npm\node_modules\@openai\codex\bin\codex.js",
        ),
    ] {
        let found = source(harness, path, &env);
        assert_eq!((found.distribution, found.update), (None, None), "{path}");
    }
}

#[test]
fn each_harness_s_own_installer_is_told_by_its_folder_in_either_spelling() {
    let env = Env::default();
    for (harness, executable, name, arguments) in [
        (
            Harness::Codex,
            "/Users/me/.codex/bin/codex",
            "Codex's installer",
            &["update"][..],
        ),
        (
            Harness::Codex,
            r"C:\Users\me\.codex\bin\codex.exe",
            "Codex's installer",
            &["update"][..],
        ),
        (
            Harness::Opencode,
            "/Users/me/.opencode/bin/opencode",
            "OpenCode's installer",
            &["upgrade"][..],
        ),
        (
            Harness::Pi,
            "/Users/me/.pi/bin/pi",
            "Pi's installer",
            &["update", "--self"][..],
        ),
        (
            Harness::Devin,
            "/Users/me/.local/share/devin/cli/_versions/current/bin/devin",
            "Devin's installer",
            &["update"][..],
        ),
        (
            Harness::Devin,
            r"C:\Users\me\AppData\Local\devin\cli\_versions\current\bin\devin.exe",
            "Devin's installer",
            &["update"][..],
        ),
    ] {
        let found = source(harness, executable, &env);
        let mut update = vec![executable.to_owned()];
        update.extend(arguments.iter().map(|argument| (*argument).to_owned()));
        assert_eq!(
            (found.distribution.as_deref(), found.update, found.format),
            (Some(name), Some(update), Format::Npm),
            "{executable}"
        );
    }
    // Claude has no installer of this kind.
    let claude = source(Harness::Claude, "/Users/me/.claude/bin/claude", &env);
    assert_eq!(claude.distribution, None);
}

#[test]
fn a_cli_found_where_nothing_is_recognized_is_the_humans_to_update() {
    let found = source(Harness::Codex, "/somewhere/else/codex", &Env::default());
    assert_eq!(
        said(&found),
        (
            "https://registry.npmjs.org/@openai/codex/latest",
            "npm",
            None,
            None
        )
    );
    for (harness, url) in [
        (
            Harness::Claude,
            "https://registry.npmjs.org/@anthropic-ai/claude-code/latest",
        ),
        (
            Harness::Opencode,
            "https://registry.npmjs.org/opencode-ai/latest",
        ),
        (
            Harness::Devin,
            "https://static.devin.ai/cli/current/manifest.json",
        ),
    ] {
        assert_eq!(source(harness, "/x/y", &Env::default()).url, url);
    }
}

#[cfg(unix)]
#[test]
fn a_link_is_followed_to_where_it_leads_and_the_command_names_the_link() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let real = fs::canonicalize(root.path()).unwrap();
    let cask = real.join("brew/Caskroom/codex/0.1/bin");
    fs::create_dir_all(&cask).unwrap();
    fs::write(cask.join("codex"), "").unwrap();
    fs::create_dir_all(real.join("brew/bin")).unwrap();
    let link = real.join("brew/bin/codex");
    symlink(cask.join("codex"), &link).unwrap();
    let env = Env::default();
    let brewed = source(Harness::Codex, link.to_str().unwrap(), &env);
    assert_eq!(brewed.distribution.as_deref(), Some("Homebrew"));
    assert_eq!(
        brewed.update.unwrap()[0],
        joined(&[real.join("brew").to_str().unwrap(), "bin", "brew"])
    );
    // A harness's own installer: the command runs the link, not where it leads.
    let own = real.join("home/.codex/bin");
    fs::create_dir_all(&own).unwrap();
    fs::write(own.join("codex"), "").unwrap();
    fs::create_dir_all(real.join("home/.local/bin")).unwrap();
    let linked = real.join("home/.local/bin/codex");
    symlink(own.join("codex"), &linked).unwrap();
    let found = source(Harness::Codex, linked.to_str().unwrap(), &env);
    assert_eq!(found.distribution.as_deref(), Some("Codex's installer"));
    assert_eq!(
        found.update,
        Some(vec![
            linked.to_str().unwrap().to_owned(),
            "update".to_owned()
        ])
    );
}

#[test]
fn a_path_windows_resolved_is_written_without_its_verbatim_prefix() {
    assert_eq!(
        plain(r"\\?\C:\Users\me\.local\bin\claude.exe"),
        r"C:\Users\me\.local\bin\claude.exe"
    );
    assert_eq!(plain(r"\\?\UNC\server\share\x"), r"\\server\share\x");
    assert_eq!(plain("/usr/bin/claude"), "/usr/bin/claude");
    assert_eq!(plain(r"C:\x"), r"C:\x");
}
