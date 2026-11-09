//! The install layouts as Node's admin suite held them, with Windows' spellings
//! besides; the recorded goldens (`tests/goldens/admin`) hold each to Node's
//! answer.

use std::fs;

use super::*;
use crate::admin::tests::npm_windows::{
    canonical, executable, lay_npm, lay_npm_cmd, package_of, windows_env,
};
use crate::admin::tests::standalone::{lay_down, lay_down_in, link_dir, release_in, Layout};

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
    let formula = source(Harness::Pi, "/opt/homebrew/Cellar/pi/1/bin/pi", &env);
    assert_eq!(
        said(&formula),
        (
            "https://formulae.brew.sh/api/formula/pi.json",
            "formula",
            Some("Homebrew"),
            Some(
                &[
                    joined(&["/opt/homebrew", "bin", "brew"]),
                    "upgrade".to_owned(),
                    "pi".to_owned()
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
            Harness::Pi,
            "/Users/me/.pi/bin/pi",
            "Pi's installer",
            &["update", "--self"][..],
        ),
        // Pi's managed install, which its installer today writes (`bin` of the agent folder),
        // is updated by `pi update` as the installer says.
        (
            Harness::Pi,
            "/Users/me/.pi/agent/bin/pi",
            "Pi's installer",
            &["update"][..],
        ),
        (
            Harness::Pi,
            r"C:\Users\me\.pi\agent\bin\pi.cmd",
            "Pi's installer",
            &["update"][..],
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

/// The launcher Pi's Unix installer links from a folder of the PATH is told by where the link
/// goes, not by where it is.
#[cfg(unix)]
#[test]
fn a_link_to_pis_managed_launcher_is_updated_as_pis_own_install() {
    let root = tempfile::tempdir().unwrap();
    let launcher = root.path().join(".pi").join("agent").join("bin").join("pi");
    fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    fs::write(&launcher, "").unwrap();
    let linked = root.path().join("bin").join("pi");
    fs::create_dir_all(linked.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&launcher, &linked).unwrap();
    let executable = linked.to_string_lossy().into_owned();
    let found = source(Harness::Pi, &executable, &Env::default());
    // Run as it was found, the link; the install is told by where that goes.
    assert_eq!(found.distribution.as_deref(), Some("Pi's installer"));
    assert_eq!(found.update, Some(vec![executable, "update".to_owned()]));
}

/// A managed install in an agent folder of any name is told by the file Pi's installer writes
/// in it, `install/managed-install.json`, beside `bin`: the installer's own evidence, which holds
/// where the app's environment names no `PI_CODING_AGENT_DIR` (a variable the shell that ran the
/// installer had) and where a link in a folder of the PATH goes into it (T-141, P3-5).
#[test]
fn pis_managed_install_in_an_agent_folder_the_human_named_is_updated_as_its_installer_says() {
    let root = tempfile::tempdir().unwrap();
    let agent = root.path().join("work").join("pi-agent");
    let launcher = agent.join("bin").join("pi");
    let marker = agent.join("install").join("managed-install.json");
    for file in [&launcher, &marker] {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "").unwrap();
    }
    let executable = launcher.to_string_lossy().into_owned();
    let found = source(Harness::Pi, &executable, &Env::default());
    assert_eq!(found.distribution.as_deref(), Some("Pi's installer"));
    assert_eq!(
        found.update,
        Some(vec![executable.clone(), "update".into()])
    );
    // Without the file it is a `pi` in a folder called `bin`, which nothing says how to update.
    fs::remove_file(&marker).unwrap();
    let found = source(Harness::Pi, &executable, &Env::default());
    assert_eq!((found.distribution, found.update), (None, None));
    // The file beside another folder's `bin` is no proof of this one; nor is another harness's.
    fs::write(&marker, "").unwrap();
    let other = root
        .path()
        .join("work")
        .join("other")
        .join("bin")
        .join("pi");
    fs::create_dir_all(other.parent().unwrap()).unwrap();
    fs::write(&other, "").unwrap();
    let other = other.to_string_lossy().into_owned();
    assert_eq!(source(Harness::Pi, &other, &Env::default()).update, None);
    assert_eq!(
        source(Harness::Codex, &executable, &Env::default()).update,
        None
    );
}

/// The environment of a person whose home is `home`: where Codex's home is, `.codex` in it, unless
/// `CODEX_HOME` says another.
fn env_of(home: &std::path::Path, codex_home: Option<&std::path::Path>) -> Env {
    let mut vars = vec![
        ("HOME", home.to_string_lossy().into_owned()),
        ("USERPROFILE", home.to_string_lossy().into_owned()),
    ];
    if let Some(codex_home) = codex_home {
        vars.push(("CODEX_HOME", codex_home.to_string_lossy().into_owned()));
    }
    Env::from_vars(vars)
}

/// What the admin says of the Codex at `executable`: its distribution and its update.
fn codex_at(executable: &std::path::Path, env: &Env) -> (Option<String>, Option<Vec<String>>) {
    let found = source(Harness::Codex, &executable.to_string_lossy(), env);
    (found.distribution, found.update)
}

/// Codex's standalone install (`packages/standalone` under its home): the program is in a
/// release, `current` leads to the release in use, and the folder on the PATH leads to what
/// `current` leads to. `codex update` is Codex's own way to update it (it runs its installer,
/// which moves `current` to the new release), so the update is that, run by the link the person
/// runs.
#[test]
fn codex_installed_by_its_standalone_installer_is_updated_by_its_own_update_through_the_links() {
    for layout in [Layout::Package, Layout::Legacy] {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let visible = root
            .path()
            .join("local")
            .join("Programs")
            .join("OpenAI")
            .join("Codex")
            .join("bin");
        let program = lay_down(&home, &visible, layout);
        let executable = program.to_string_lossy().into_owned();
        let found = source(Harness::Codex, &executable, &env_of(&home, None));
        assert_eq!(
            said(&found),
            (
                "https://registry.npmjs.org/@openai/codex/latest",
                "npm",
                Some("Codex's installer"),
                Some(&[executable.clone(), "update".to_owned()][..]),
            ),
            "{layout:?}: the command runs the link the person runs, not the release it leads to"
        );
    }
}

/// Codex counts a program as standalone only under its own home: `CODEX_HOME` where that is set
/// (it must exist), else `.codex` in the user's home, each read where it leads
/// (`codex-install-context`, 0.167.0). The admin counts the same, so what it offers an Update of
/// is what `codex update` will update, and a Codex whose home the environment names otherwise is
/// not offered one that would answer "Could not detect the Codex installation method".
#[test]
fn codex_is_standalone_only_under_the_home_codex_reads_and_never_by_a_matching_folder_name() {
    let layout = Layout::Package;
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    let visible = |name: &str| base.join("visible").join(name);
    let home = base.join("home");
    // A home chosen by CODEX_HOME, a folder of any name that exists: its install is the one.
    let chosen = base.join("elsewhere").join("codex-home");
    let program = lay_down_in(&chosen, &visible("chosen"), layout);
    assert_eq!(
        codex_at(&program, &env_of(&home, Some(&chosen)))
            .0
            .as_deref(),
        Some("Codex's installer"),
        "CODEX_HOME names the home the install is under"
    );
    // The same install where the environment names no home (`.codex` in the user's is Codex's),
    // or another one that exists, or one that does not: Codex finds no standalone install.
    let other = base.join("other-codex-home");
    std::fs::create_dir_all(&other).unwrap();
    for (what, env) in [
        ("no CODEX_HOME", env_of(&home, None)),
        ("another CODEX_HOME", env_of(&home, Some(&other))),
        (
            "a CODEX_HOME that is not there",
            env_of(&home, Some(&base.join("missing"))),
        ),
        ("no home at all", Env::default()),
    ] {
        assert_eq!(codex_at(&program, &env), (None, None), "{what}");
    }
    // And the install of `.codex` where CODEX_HOME names another home.
    let program = lay_down(&home, &visible("default"), layout);
    assert_eq!(
        codex_at(&program, &env_of(&home, Some(&other))),
        (None, None),
        "CODEX_HOME names a home the install is not under"
    );
    assert_eq!(
        codex_at(&program, &env_of(&home, None)).0.as_deref(),
        Some("Codex's installer")
    );
    // A folder that merely has the names, and a daemon's package under an ancestor that has them:
    // neither is under Codex's home.
    let work = base.join("work").join("project");
    let built = release_in(&standalone_of(&work), "build", Layout::Package);
    assert_eq!(
        codex_at(
            &built.join("bin").join(program_name()),
            &env_of(&home, None)
        ),
        (None, None),
        "a folder of a project that has the names"
    );
    let nested_home = standalone_of(&work).join("releases").join("x").join("home");
    let daemon = nested_home
        .join(".codex")
        .join("packages")
        .join("app-server-daemon");
    let packaged = release_in(&daemon, "0.167.0-x86_64-pc-windows-msvc", Layout::Package);
    assert_eq!(
        codex_at(
            &packaged.join("bin").join(program_name()),
            &env_of(&nested_home, None)
        ),
        (None, None),
        "the daemon's package, under a folder that has the standalone names"
    );
}

/// `packages/standalone` as a project folder has it, for a folder that is none of Codex's.
fn standalone_of(folder: &std::path::Path) -> std::path::PathBuf {
    folder.join("packages").join("standalone")
}

fn program_name() -> &'static str {
    crate::admin::tests::standalone::program_name()
}

/// Where Codex finds the user's home (`dirs::home_dir()`, 0.167.0): on Windows the profile folder,
/// which Git Bash's `HOME` (`/c/Users/me`, or anywhere a shell was told) does not name; elsewhere
/// `HOME`. A daemon started from a Git Bash window has both, and the admin reads the one Codex does,
/// so a `HOME` that is another folder neither hides the Update of the real profile's install nor
/// offers one for a folder Codex would not read as standalone.
#[test]
fn the_home_codex_reads_is_the_windows_profile_on_windows_and_home_elsewhere() {
    let temp = tempfile::tempdir().unwrap();
    let base = root_of(&temp);
    let (profile, shell) = (base.join("profile"), base.join("git-bash-home"));
    std::fs::create_dir_all(&shell).unwrap();
    let in_profile = lay_down(&profile, &base.join("visible-profile"), Layout::Package);
    let in_shell = lay_down(&shell, &base.join("visible-shell"), Layout::Package);
    let windows = |home: &str, profile: &std::path::Path| {
        Env::from_vars([
            ("OS", "Windows_NT".to_owned()),
            ("HOME", home.to_owned()),
            ("USERPROFILE", profile.to_string_lossy().into_owned()),
        ])
    };
    let shell_home = shell.to_string_lossy().into_owned();
    assert_eq!(
        codex_at(&in_profile, &windows(&shell_home, &profile))
            .0
            .as_deref(),
        Some("Codex's installer"),
        "Windows: the install in the profile, with a Git Bash HOME"
    );
    assert_eq!(
        codex_at(&in_shell, &windows(&shell_home, &profile)),
        (None, None),
        "Windows: an install under the HOME of a shell is not under the profile"
    );
    // No profile in a Windows environment: nothing to read the home from, whatever HOME says.
    assert_eq!(
        codex_at(
            &in_shell,
            &Env::from_vars([
                ("OS", "Windows_NT".to_owned()),
                ("HOME", shell_home.clone())
            ])
        ),
        (None, None),
        "Windows: no profile"
    );
}

/// Elsewhere HOME is the home and USERPROFILE, where a variable of that name is set, is not.
#[cfg(unix)]
#[test]
fn off_windows_the_home_is_home_whatever_userprofile_says_and_an_empty_one_is_none() {
    let temp = tempfile::tempdir().unwrap();
    let base = root_of(&temp);
    let (home, other) = (base.join("home"), base.join("other"));
    std::fs::create_dir_all(&other).unwrap();
    let program = lay_down(&home, &base.join("visible"), Layout::Package);
    let env = |home: &std::path::Path, profile: &std::path::Path| {
        Env::from_vars([
            ("HOME", home.to_string_lossy().into_owned()),
            ("USERPROFILE", profile.to_string_lossy().into_owned()),
        ])
    };
    assert_eq!(
        codex_at(&program, &env(&home, &other)).0.as_deref(),
        Some("Codex's installer")
    );
    assert_eq!(codex_at(&program, &env(&other, &home)), (None, None));
    assert_eq!(
        codex_at(
            &program,
            &Env::from_vars([("HOME", ""), ("USERPROFILE", &home.to_string_lossy())])
        ),
        (None, None),
        "an empty HOME"
    );
}

/// A relative `CODEX_HOME` is read against the folder Codex is started in, and the update does not
/// start Codex where the admin looks: Codex reads it in the user's home, and the installer script
/// it runs reads the same variable in a temporary folder `codex update` makes. Two homes for one
/// install, so no Update is offered for it.
#[cfg(unix)]
#[test]
fn a_relative_codex_home_is_offered_no_update() {
    let temp = tempfile::tempdir().unwrap();
    let base = root_of(&temp);
    let chosen = base.join("codex-home");
    let program = lay_down_in(&chosen, &base.join("visible"), Layout::Package);
    // The path to it from this test's folder, which is where a relative one would be read.
    let here = std::fs::canonicalize(std::env::current_dir().unwrap()).unwrap();
    let mut common = 0;
    let (a, b): (Vec<_>, Vec<_>) = (here.components().collect(), chosen.components().collect());
    while common < a.len() && common < b.len() && a[common] == b[common] {
        common += 1;
    }
    let relative: std::path::PathBuf = std::iter::repeat("..")
        .take(a.len() - common)
        .map(std::path::PathBuf::from)
        .chain(
            b[common..]
                .iter()
                .map(|part| std::path::PathBuf::from(part.as_os_str())),
        )
        .collect();
    assert!(relative.is_relative() && relative.is_dir(), "{relative:?}");
    let env = Env::from_vars([
        ("HOME", base.join("nobody").to_string_lossy().into_owned()),
        ("CODEX_HOME", relative.to_string_lossy().into_owned()),
    ]);
    assert_eq!(codex_at(&program, &env), (None, None));
    // The same install by the absolute name is offered.
    let env = env_of(&base.join("nobody"), Some(&chosen));
    assert_eq!(
        codex_at(&program, &env).0.as_deref(),
        Some("Codex's installer")
    );
}

/// Codex reads its home where it leads, and so does the admin: a user's home that is a link, or a
/// `CODEX_HOME` that is, is the home it leads to, and so is the folder the install is read in.
#[test]
fn a_home_that_is_a_link_is_the_home_it_leads_to_as_codex_reads_it() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path();
    // The user's home is a link to the folder that holds `.codex`.
    let real_home = base.join("real-home");
    std::fs::create_dir_all(&real_home).unwrap();
    let program = lay_down(&real_home, &base.join("visible-a"), Layout::Package);
    let linked_home = base.join("linked-home");
    link_dir(&linked_home, &real_home);
    assert_eq!(
        codex_at(&program, &env_of(&linked_home, None)).0.as_deref(),
        Some("Codex's installer"),
        "the user's home is a link"
    );
    // CODEX_HOME is a link to the folder the install is in.
    let real_codex = base.join("real-codex");
    let program = lay_down_in(&real_codex, &base.join("visible-b"), Layout::Package);
    let linked_codex = base.join("linked-codex");
    link_dir(&linked_codex, &real_codex);
    assert_eq!(
        codex_at(&program, &env_of(&base.join("nobody"), Some(&linked_codex)))
            .0
            .as_deref(),
        Some("Codex's installer"),
        "CODEX_HOME is a link"
    );
}

/// What is no standalone CLI is left to the human: the daemon's own packages (a release of
/// `codex` the background server runs, which no `codex update` of the CLI moves), under the home
/// Codex reads, and the standalone folder with no release in it.
#[test]
fn what_codex_keeps_beside_its_standalone_cli_is_no_install_to_update() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let codex_home = home.join(".codex");
    let daemon = release_in(
        &codex_home.join("packages").join("app-server-daemon"),
        "0.167.0-x86_64-pc-windows-msvc",
        Layout::Package,
    );
    let beside = codex_home
        .join("packages")
        .join("standalone")
        .join("current");
    std::fs::create_dir_all(beside.join("bin")).unwrap();
    std::fs::write(beside.join("bin").join(program_name()), "").unwrap();
    for executable in [
        daemon.join("bin").join(program_name()),
        beside.join("bin").join(program_name()),
    ] {
        assert_eq!(
            codex_at(&executable, &env_of(&home, None)),
            (None, None),
            "{}",
            executable.display()
        );
    }
}

/// The folder a test lays its Windows files in, where links are not what `canonicalize` would
/// change the spelling of.
fn root_of(temp: &tempfile::TempDir) -> std::path::PathBuf {
    canonical(temp.path())
}

/// npm's global install on Windows: the prefix holds the shims and `node_modules` beside them, the
/// Node installer's npm is in another folder, and the update is that npm's, told the prefix (so
/// the install is updated where it is, whatever prefix the npm found on the PATH is set to).
#[test]
fn a_global_npm_install_on_windows_is_told_by_its_shim_beside_node_modules_and_updated_into_its_prefix(
) {
    for harness in [Harness::Codex, Harness::Claude, Harness::Pi] {
        let temp = tempfile::tempdir().unwrap();
        let root = root_of(&temp);
        let prefix = root.join("AppData").join("Roaming").join("npm");
        let nodejs = root.join("Program Files").join("nodejs");
        let shim = lay_npm(&prefix, harness);
        let npm = lay_npm_cmd(&nodejs);
        let env = windows_env(&[&nodejs], &root);
        let (package, _) = package_of(harness);
        let found = source(harness, &shim.to_string_lossy(), &env);
        assert_eq!(
            (found.distribution.as_deref(), found.update.as_deref()),
            (
                Some("npm"),
                Some(
                    &[
                        joined(&[&npm.to_string_lossy()]),
                        "install".to_owned(),
                        "-g".to_owned(),
                        "--prefix".to_owned(),
                        joined(&[&prefix.to_string_lossy()]),
                        format!("{package}@latest"),
                    ][..]
                )
            ),
            "{harness:?}"
        );
        // The shell script and the PowerShell script beside the shim are the same install.
        for other in [
            harness.as_str().to_owned(),
            format!("{}.ps1", harness.as_str()),
        ] {
            let found = source(harness, &prefix.join(&other).to_string_lossy(), &env);
            assert_eq!(found.distribution.as_deref(), Some("npm"), "{other}");
        }
    }
}

/// A prefix of its own (`npm config set prefix`, a portable Node folder, nvm-windows's) with its
/// npm in it: that npm is the one run, whatever npm the PATH has first; and where no npm is found
/// the install is named and no command is given.
#[test]
fn the_npm_beside_a_custom_prefix_is_the_one_run_and_none_found_gives_no_command() {
    let temp = tempfile::tempdir().unwrap();
    let root = root_of(&temp);
    let prefix = root.join("tools").join("npm-global");
    let nodejs = root.join("Program Files").join("nodejs");
    let shim = lay_npm(&prefix, Harness::Pi);
    let own = lay_npm_cmd(&prefix);
    lay_npm_cmd(&nodejs);
    let found = source(
        Harness::Pi,
        &shim.to_string_lossy(),
        &windows_env(&[&nodejs], &root),
    );
    assert_eq!(
        found.update.as_deref().map(|argv| argv[0].clone()),
        Some(joined(&[&own.to_string_lossy()])),
        "the npm in the prefix"
    );
    // No npm in the prefix and none on the PATH.
    let bare = root.join("bare");
    let shim = lay_npm(&bare, Harness::Pi);
    let found = source(
        Harness::Pi,
        &shim.to_string_lossy(),
        &windows_env(&[], &root),
    );
    assert_eq!(
        (found.distribution.as_deref(), found.update),
        (Some("npm"), None)
    );
}

/// What is no global install is left to the human: a project's own (`node_modules\.bin` holds its
/// shims, never the folder beside `node_modules`), a shim whose package is not there or is another
/// harness's, a package folder's own file with no shim beside the folder, and a harness npm
/// installs nothing of (Devin, Muse Code: their own installers).
#[test]
fn what_is_no_global_npm_install_on_windows_is_the_humans_to_update() {
    let temp = tempfile::tempdir().unwrap();
    let root = root_of(&temp);
    let nodejs = root.join("Program Files").join("nodejs");
    lay_npm_cmd(&nodejs);
    let env = windows_env(&[&nodejs], &root);
    let say = |harness: Harness, path: &std::path::Path| {
        let found = source(harness, &path.to_string_lossy(), &env);
        (found.distribution, found.update)
    };
    // A project's own install: the shim is in `.bin`.
    let project = root.join("work").join("project");
    lay_npm(&project, Harness::Codex);
    let bin = project.join("node_modules").join(".bin");
    fs::create_dir_all(&bin).unwrap();
    executable(&bin.join("codex.cmd"), "");
    assert_eq!(
        say(Harness::Codex, &bin.join("codex.cmd")),
        (None, None),
        "a project's shim"
    );
    // A shim of Pi's beside a prefix that holds Codex's package only.
    let prefix = root.join("AppData").join("Roaming").join("npm");
    lay_npm(&prefix, Harness::Codex);
    executable(&prefix.join("pi.cmd"), "");
    assert_eq!(
        say(Harness::Pi, &prefix.join("pi.cmd")),
        (None, None),
        "no package of Pi's"
    );
    // A shim of another command beside the harness's package is no shim of the harness.
    executable(&prefix.join("other.cmd"), "");
    assert_eq!(
        say(Harness::Codex, &prefix.join("other.cmd")),
        (None, None),
        "another command"
    );
    // The package's own file, where no shim of the harness is beside `node_modules`.
    let module = prefix.join("node_modules").join("@openai").join("codex");
    fs::remove_file(prefix.join("codex.cmd")).unwrap();
    assert_eq!(
        say(Harness::Codex, &module.join("bin").join("codex.js")),
        (None, None),
        "no shim"
    );
    // Devin and Muse Code have no npm package: a shim of theirs in an npm prefix is nothing.
    for harness in [Harness::Devin, Harness::Muse] {
        let folder = root.join(harness.as_str());
        let shim = folder.join(format!("{}.cmd", harness.as_str()));
        fs::create_dir_all(folder.join("node_modules").join(harness.as_str())).unwrap();
        fs::write(
            folder
                .join("node_modules")
                .join(harness.as_str())
                .join("package.json"),
            "{}",
        )
        .unwrap();
        executable(&shim, "");
        assert_eq!(say(harness, &shim).1, None, "{harness:?}");
    }
}

/// The package's own file is a global install's when its shim is beside `node_modules` (the path
/// of a CLI that is read where its link leads, as a Unix npm's is).
#[test]
fn the_file_of_a_global_package_is_the_install_where_its_shim_is_beside_node_modules() {
    let temp = tempfile::tempdir().unwrap();
    let root = root_of(&temp);
    let prefix = root.join("AppData").join("Roaming").join("npm");
    let nodejs = root.join("Program Files").join("nodejs");
    lay_npm(&prefix, Harness::Codex);
    lay_npm_cmd(&nodejs);
    let env = windows_env(&[&nodejs], &root);
    let file = prefix
        .join("node_modules")
        .join("@openai")
        .join("codex")
        .join("bin")
        .join("codex.js");
    let found = source(Harness::Codex, &file.to_string_lossy(), &env);
    assert_eq!(found.distribution.as_deref(), Some("npm"));
    assert_eq!(
        found.update.unwrap()[4],
        joined(&[&prefix.to_string_lossy()]),
        "the prefix is where `node_modules` is"
    );
}

/// Only on Windows: a Unix npm has its shims in `bin` and its packages in `lib/node_modules`, and
/// a folder that merely looks like Windows' is no npm there.
#[cfg(unix)]
#[test]
fn the_windows_layout_is_read_only_where_the_environment_is_windows() {
    let temp = tempfile::tempdir().unwrap();
    let root = root_of(&temp);
    let prefix = root.join("npm");
    let shim = lay_npm(&prefix, Harness::Codex);
    let found = source(Harness::Codex, &shim.to_string_lossy(), &Env::default());
    assert_eq!((found.distribution, found.update), (None, None));
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
