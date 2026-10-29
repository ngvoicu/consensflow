//! No test, smoke, plant or tool reaches the live app. On 2026-10-28 an updater smoke's
//! copy of the app, of the live app's identity, came to the front over the live app, and
//! the Quit a person gave the app in front reached the live app next: the board closed.
//! The app a test starts is never of the live app's identity (`cf_base::bundle`, which
//! the programs xtask and the end-to-end rig start are held to, both ways they start
//! one). What these rules keep is the rest of that: nothing of the tests and the tools
//! starts, quits or signals an app by its name or its identity, which are the live app's
//! as much as any copy's, and nothing starts the program of an app bundle by a way of its
//! own, around the two that are held to it.
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};

/// The checkout's root.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// Whether a line of code reaches an app by its name or its identity: `open -a` and
/// `open -b`, and `open` of an `.app`; AppleScript's `tell application` (and `tell app`,
/// `launch application`, JavaScript's `Application(…)`); AppKit's running applications
/// found by identity; Launch Services' opening; launchd's `launchctl`; and what ends
/// processes by name or every one (`killall`, `pkill`, `pgrep` handing its numbers to
/// `kill`, and the signal to -1).
fn reaches_by_name(code: &str) -> bool {
    let lower = code.to_lowercase();
    let delimited = |at: usize, len: usize| {
        let before = code[..at].chars().next_back();
        let after = code[at + len..].chars().next();
        let edge = |it: Option<char>| it.is_none_or(|it| !it.is_alphanumeric() && it != '_');
        edge(before) && edge(after)
    };
    let words = |word: &str| {
        code.match_indices(word)
            .any(|(at, _)| delimited(at, word.len()))
    };
    let quote = |it: Option<char>| it.is_some_and(|it| it == '"' || it == '\'' || it == ' ');
    let open_by_name = code.match_indices("open").any(|(at, _)| {
        delimited(at, 4) && {
            let near: String = code[at + 4..].chars().take(40).collect();
            ["-a", "-b"].iter().any(|flag| {
                near.match_indices(flag).any(|(found, _)| {
                    quote(near[..found].chars().next_back())
                        && quote(near[found + 2..].chars().next())
                })
            })
        }
    });
    // `open` as a program, with an app among its words: `open X.app`, `"/usr/bin/open", app.app`.
    let opener = code.contains("\"/usr/bin/open\"")
        || code.contains("\"open\"")
        || code.contains("'open'")
        || code.starts_with("open ")
        || ["| open ", "; open ", "&& open ", "$(open ", "`open "]
            .iter()
            .any(|it| code.contains(it));
    let open_an_app = opener && code.contains(".app");
    open_by_name
        || open_an_app
        || lower.contains("tell application")
        || lower.contains("tell app ")
        || lower.contains("tell app \"")
        || lower.contains("launch application")
        || code.contains("Application(\"")
        || code.contains("Application('")
        || code.contains("WithBundleIdentifier")
        || code.contains("launchApplication")
        || code
            .match_indices("LSOpen")
            .any(|(at, _)| code[at + 6..].starts_with(|it: char| it.is_ascii_uppercase()))
        || words("launchctl")
        || words("killall")
        || words("pkill")
        || (words("pgrep") && words("kill"))
        || signals_every_process(code)
}

/// Whether a line signals every process: `kill -1`, `kill -9 -1`, `kill -s KILL -1`,
/// `kill(-1, …)`, `kill(Pid::from_raw(-1), …)`. A signal to a number (`kill -15 "$pid"`) is not.
fn signals_every_process(code: &str) -> bool {
    if ["kill(-1", "kill (-1", "Pid::from_raw(-1)"]
        .iter()
        .any(|it| code.contains(it))
    {
        return true;
    }
    code.match_indices("kill").any(|(at, _)| {
        let before = code[..at].chars().next_back();
        if before.is_some_and(|it| it.is_alphanumeric() || it == '_') {
            return false;
        }
        let words: Vec<&str> = code[at + 4..]
            .split(|it: char| it.is_whitespace() || matches!(it, '"' | '\'' | ',' | '[' | ']'))
            .filter(|word| !word.is_empty())
            .take(3)
            .collect();
        match words.as_slice() {
            ["-1", ..] => true,
            ["-s", _, "-1", ..] => true,
            [signal, "-1", ..] => signal.starts_with('-'),
            _ => false,
        }
    })
}

/// Whether a line spells the live app's path, or the installed Candidate's: an app a test
/// never touches.
fn spells_an_installed_app(code: &str) -> bool {
    ["/Applications/ConsensFlow", "~/Applications/ConsensFlow"]
        .iter()
        .any(|path| {
            code.match_indices(path).any(|(at, _)| {
                code[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|it| !(it.is_alphanumeric() || matches!(it, '_' | '.' | '-' | '/')))
            })
        })
}

/// Whether a file names a built ConsensFlow of Windows, whatever its case: the exe a build
/// leaves (`app.exe` until Tauri names it), a release's portable one, or its installer.
fn names_a_built_windows_app(text: &str) -> bool {
    let text = text.to_lowercase();
    ["consensflow.exe", "app.exe", "-portable.exe", "-setup.exe"]
        .iter()
        .any(|name| text.contains(name))
}

/// The lines of a file that start a program, any way the tests and the tools do, trimmed:
/// Node's and Rust's starts, PowerShell's (its call operator `&` where a command begins, and
/// `Invoke-Expression`, `Invoke-Command`, `Start-Process`) and cmd's `start`.
fn start_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| starts_a_program(line))
        .collect()
}

/// Whether a line starts a program.
fn starts_a_program(line: &str) -> bool {
    let line = line.to_lowercase();
    let called = [
        "spawn(",
        "spawnsync(",
        "exec(",
        "execsync(",
        "execfile(",
        "execfilesync(",
        "fork(",
        "start-process",
        "invoke-expression",
        "invoke-command",
        "command::new(",
        "run::new(",
        "invocation::new(",
    ]
    .iter()
    .any(|start| line.contains(start));
    // PowerShell's call operator: `& ` then a program, where a command begins (not `&&`, nor
    // an `&` between two numbers).
    let operator = line.match_indices("& ").any(|(at, _)| {
        let before = line[..at].trim_end();
        line[at + 2..].starts_with(['$', '"', '\'', '('])
            && !before.ends_with('&')
            && before
                .chars()
                .next_back()
                .is_none_or(|it| "=(;|{".contains(it))
    });
    // cmd's `start "title" program` or `start /wait program`, as a word of its own.
    let cmd = line.match_indices("start ").any(|(at, _)| {
        line[..at]
            .chars()
            .next_back()
            .is_none_or(|it| !(it.is_alphanumeric() || matches!(it, '-' | '_' | '.')))
            && line[at + 6..].starts_with(['"', '/'])
    });
    called || operator || cmd
}

/// What a Windows start of a built app does first: it reads its identity off its file and
/// refuses one that is not of the smoke identity (app/scripts/built-identity.mjs), the
/// refusal kept in a name the file then tests (`const refused = refusalOf(exe)`, then
/// `if (refused)`). A call whose answer is dropped holds nothing.
fn reads_its_identity_first(text: &str) -> bool {
    text.match_indices(" = refusalOf(").any(|(at, _)| {
        let name: String = text[..at]
            .chars()
            .rev()
            .take_while(|it| it.is_alphanumeric() || *it == '_')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        !name.is_empty() && text.contains(&format!("if ({name})"))
    })
}

/// The files that name a built Windows app and start programs, why that starts no app of the
/// release's on a machine someone uses, and the lines that start what they start: held to
/// those, so a start added to such a file is read again.
#[allow(clippy::type_complexity)]
const STARTING_NO_BUILT_APP: [(&str, &str, &[&str]); 7] = [
    (
        ".github/workflows/windows-build.yml",
        "the shipped exe's first start, and its runtime's cf, on GitHub's runner, which holds no one's ConsensFlow",
        &[
            r#"$app = Start-Process -FilePath $exe -PassThru `"#,
            r#"$said = & (Join-Path $runtime 'cli\bin\cf.exe') --version"#,
        ],
    ),
    (
        "app/src-tauri/src/portable/tests.rs",
        "fake exes of a few bytes, which it packs and starts none of",
        &[
            r#"thread::spawn(move || unpacked_runtime(&exe, &local, "9.9.9"))"#,
            r#"thread::spawn(move || {"#,
            r#".spawn()"#,
        ],
    ),
    (
        "tests/install-ps1.test.mjs",
        "a stand-in named ConsensFlow.exe, a copy of ping, and the installer the test builds",
        &[
            r#"return spawnSync(windows ? 'where' : 'which', [program], { encoding: 'utf8' }).status === 0"#,
            r#"const read = spawnSync("#,
            r#"const ran = spawnSync("#,
            r#"const made = spawnSync('reg', ['add', key, '/v', name, '/d', value, '/f'], {"#,
            r#"const child = spawn(program, all, {"#,
            r#"if (windows) spawnSync('reg', ['delete', TEST_KEYS, '/f'])"#,
            r#""$run = Start-Process -FilePath $setup -ArgumentList '/S' -Wait -PassThru","#,
            r#"const running = spawn(program(at, 'ConsensFlow.exe'), ['-n', '60', '127.0.0.1'], {"#,
            r#"`Close-CfOpen $dir; $null = Start-Process -FilePath '${ping}' -ArgumentList '-n', '30', '127.0.0.1' -WindowStyle Hidden; Start-Sleep -Milliseconds 500\n`,"#,
            r#"spawnSync('powershell', ["#,
            r#"const child = spawn(process.execPath, ['-e', ''])"#,
            r#"const child = spawn(program, args, { stdio: 'ignore' })"#,
        ],
    ),
    (
        "tools/cf-publish/src/testing/built.rs",
        "the names of a release's files, which it starts none of: it runs tar",
        &[
            r#"let made = Command::new("tar")"#,
        ],
    ),
    (
        "tests/fixtures/install/make-fixtures.mjs",
        "an installer's name in a fixture's checksums; it runs Tauri's signer",
        &[
            r#"const ran = spawnSync(tauri, words, { cwd: work, env, encoding: 'utf8' })"#,
            r#"const version = spawnSync(tauri, ['--version'], { encoding: 'utf8' }).stdout.trim()"#,
        ],
    ),
    (
        "tests/install-sh.test.mjs",
        "the Windows installer's name in a release's checksums, which the Mac's installer reads; it starts sh and the Mac's tools",
        &[
            r#"if (argc < 2 || posix_spawn(&pid, argv[1], 0, 0, argv + 1, environ) != 0) return 127;"#,
            r#"const ran = spawnSync(program, args, { encoding: 'utf8' })"#,
            r#"const child = spawn(program, words, {"#,
            r#"return spawnSync('/usr/libexec/PlistBuddy', ['-c', 'Print :CFBundleShortVersionString', plist], {"#,
            r#"return spawnSync('ps', ['-axo', 'pid=,comm='], { encoding: 'utf8' })"#,
            r#"return spawnSync('mount', { encoding: 'utf8' })"#,
            r#"return () => spawnSync('hdiutil', ['detach', '-quiet', '-force', mountpoint])"#,
            r#"const ran = spawnSync('/bin/sh', ['-c', `${order}\nnewer_than "$1" "$2"`, 'sh', a, b])"#,
            r#"const ran = spawnSync('sh', [INSTALL_SH], {"#,
            r#"const child = spawn('/bin/sh', ['-s', '--', '--version', '3.0.7'], {"#,
            r#"const child = spawn('/bin/sh', ['-c', 'exit 0'])"#,
            r#"const child = spawn(program, args, {"#,
            r#"spawnSync('hdiutil', ['detach', '-quiet', '-force', mounted])"#,
            r#"const child = spawn('/bin/sh', ['-c', 'exit 0'])"#,
            r#"spawnSync("#,
            r#"const child = spawn('/bin/sh', ['-c', 'exit 0'])"#,
            r#"const child = spawn('/bin/sh', [script], { detached: true, stdio: 'ignore' })"#,
        ],
    ),
    (
        "tests/install-verifiers.test.mjs",
        "the Windows installer's name in the checksums a verifier reads; it starts the verifiers",
        &[
            r#"const ran = spawnSync(process.platform === 'win32' ? 'where' : 'which', [program], {"#,
            r#"return spawnSync('osascript', ['-l', 'JavaScript', script, JSON.stringify(input)], {"#,
            r#"return spawnSync("#,
            r#"const ran = spawnSync("#,
        ],
    ),
];

/// The files that spell an installed app's path, and why that touches nothing: each spells it
/// as data a test reads, never as an app it starts.
const SPELLING_INSTALLED: [(&str, &str); 7] = [
    (
        "tools/xtask/src/updater_smoke/processes/tests.rs",
        "a row of a fake process table: the live app, which the smoke takes for none of its own",
    ),
    (
        "app/src-tauri/src/daemon_command/tests.rs",
        "the cf a daemon command is made for",
    ),
    (
        "crates/cf-e2e/src/shipped.rs",
        "the bundles a fake process table shows running",
    ),
    (
        "crates/cf-e2e/tests/smoke/app.rs",
        "a line of an app's log the smoke reads",
    ),
    (
        "tools/xtask/src/candidate/tests.rs",
        "the rows of a fake process table the Candidate command reads the live app in",
    ),
    (
        "tools/xtask/src/updater_smoke/case/tests/inputs.rs",
        "the paths the updater smoke refuses as the user's installed app",
    ),
    (
        "crates/cf-engine/src/role_change/tests.rs",
        "the cf a role text is written with",
    ),
];

/// The files whose words are rules of the kind, or plants of them: they name what they refuse.
const RULES: [&str; 4] = [
    "tools/xtask/tests/live_app.rs",
    "tests/plants/release/live-app.toml",
    "crates/cf-e2e/src/live/members/descent/tests.rs",
    "crates/cf-evals/tests/long_run/signals.rs",
];

/// The starts held to `cf_base::bundle`, whose tests hand a stand-in the very words they
/// refuse: what they say below their tests' start is the refusal's, and is not read.
const REFUSING: [&str; 3] = [
    "tools/xtask/src/process.rs",
    "tools/cf-release/src/process.rs",
    "crates/cf-e2e/src/process/tests.rs",
];

/// What of `text`, the file at `path`, a rule reads: all of it, but for [`REFUSING`]'s tests.
fn read_of<'a>(path: &str, text: &'a str) -> &'a str {
    if !REFUSING.contains(&path) {
        return text;
    }
    if path.ends_with("/tests.rs") {
        return "";
    }
    text.split("\n#[cfg(test)]").next().unwrap_or_default()
}

/// The files of the tools and of the rig that start a program a caller names (std's
/// `Command::new` of a value, not of a name written there), and what holds them to no app
/// of the live app's identity: the gate each calls (`cf_base::bundle`), or what they start.
const STARTERS: [(&str, &str); 4] = [
    (
        "tools/cf-publish/src/process.rs",
        "curl, gh and tar alone, which its callers name; it is built on serde_json and sha2 alone",
    ),
    (
        "tools/cf-publish/src/testing/mod.rs",
        "the cf-publish the test built",
    ),
    (
        "crates/cf-e2e/src/process/tests.rs",
        "the test's own program, started again",
    ),
    (
        "tools/cf-publish/tests/release_build.rs",
        "cargo, which builds the publisher the test runs",
    ),
];

/// What a file calls to hold what it starts to no app a test may not start
/// (`command_startable_by_a_test`, or the program's half alone).
const GATE: &str = "_startable_by_a_test";

/// Whether `relative` is of the tests and the tools: everything of `tools`, `tests`,
/// `bin` and the app's scripts, the end-to-end and the evals crates, and the tests and
/// test support of every other crate and of the app.
fn of_tests_and_tools(relative: &str) -> bool {
    let tooling = [
        "tools/",
        "tests/",
        "bin/",
        "app/scripts/",
        ".github/",
        "crates/cf-e2e/",
        "crates/cf-evals/",
    ]
    .iter()
    .any(|folder| relative.starts_with(folder));
    let testing = (relative.starts_with("crates/") || relative.starts_with("app/src-tauri/"))
        && [
            "/tests/",
            "/tests.rs",
            "/testing/",
            "/testing.rs",
            "/test_support",
            "_tests.rs",
        ]
        .iter()
        .any(|part| relative.contains(part));
    tooling || testing
}

/// The files of the tests and the tools a rule reads: their programs, scripts and plants.
fn files() -> Vec<(String, String)> {
    fn walk(root: &Path, folder: &Path, found: &mut Vec<(String, String)>) {
        // A folder a checkout has not (one a build makes), or a link to none, holds nothing.
        let Ok(entries) = fs::read_dir(folder) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            if path.is_dir() {
                if !["target", "node_modules", ".git", "golden", "goldens"].contains(&name) {
                    walk(root, &path, found);
                }
                continue;
            }
            let kind = path
                .extension()
                .and_then(|kind| kind.to_str())
                .unwrap_or("");
            if ![
                "rs", "mjs", "js", "cjs", "sh", "toml", "ps1", "py", "yml", "yaml", "json",
            ]
            .contains(&kind)
            {
                continue;
            }
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            if of_tests_and_tools(&relative) {
                if let Ok(text) = fs::read_to_string(&path) {
                    found.push((relative, text));
                }
            }
        }
    }
    let root = root();
    let mut found = Vec::new();
    for folder in ["tools", "tests", "bin", "app", "crates", ".github"] {
        walk(&root, &root.join(folder), &mut found);
    }
    found.sort();
    found
}

/// The lines of `text` that reach an app by its name or its identity, each with its number.
fn reaching_by_name(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, line)| {
            let code = line.trim_start();
            !code.starts_with("//") && !code.starts_with('#') && reaches_by_name(code)
        })
        .map(|(at, line)| (at + 1, line.trim().to_owned()))
        .collect()
}

/// Whether `text` starts a program a caller names: std's `Command::new` of a value, not of
/// a name written there (`Command::new("/bin/sh")`).
fn starts_what_it_is_given(text: &str) -> bool {
    text.lines().any(|line| {
        let code = line.trim_start();
        !code.starts_with("//")
            && code.match_indices("Command::new(").any(|(at, start)| {
                let own = code[..at]
                    .chars()
                    .next_back()
                    .is_none_or(|it| !it.is_alphanumeric() && it != '_');
                let given = code[at + start.len()..].trim_start();
                own && !(given.starts_with('"')
                    || given.starts_with("r\"")
                    || given.starts_with("r#"))
            })
    })
}

#[test]
fn what_reaches_an_app_by_its_name_or_its_identity_is_found() {
    for line in [
        "/usr/bin/open -a ConsensFlow",
        r#"Invocation::new("/usr/bin/open", root).args(["-b", "dev.ngvoicu.consensflow"])"#,
        r#"osascript -e 'tell application id "dev.ngvoicu.consensflow" to quit'"#,
        r#"Run::new("/usr/bin/osascript").args(["-e", "tell application \"ConsensFlow\" to quit"])"#,
        r#"Application("ConsensFlow").quit()"#,
        "$.NSRunningApplication.runningApplicationsWithBundleIdentifier('dev.ngvoicu.consensflow')",
        "NSWorkspace.sharedWorkspace.launchApplication('ConsensFlow')",
        "LSOpenCFURLRef(url, null)",
        "killall ConsensFlow",
        r#"Command::new("pkill").arg("-f").arg("app")"#,
        "kill -9 -1",
        "kill -1",
        "kill -s KILL -1",
        "libc::kill(-1, libc::SIGKILL)",
        "nix::sys::signal::kill(Pid::from_raw(-1), Signal::SIGKILL)",
        r#"osascript -e 'tell app "ConsensFlow" to quit'"#,
        r#"osascript -e 'launch application "ConsensFlow"'"#,
        "pgrep -f ConsensFlow | xargs kill",
        "kill $(pgrep -f app)",
        "launchctl kickstart -k gui/501/dev.ngvoicu.consensflow",
        "open /Applications/Some.app",
        r#"Invocation::new("/usr/bin/open", root).arg("dist/ConsensFlow.app")"#,
        "subprocess.run(['open', '-g', 'build/ConsensFlow.app'])",
    ] {
        assert_eq!(reaching_by_name(line).len(), 1, "{line}");
    }
    for line in [
        "$.NSRunningApplication.runningApplicationWithProcessIdentifier(42).hide",
        "the window is open -- and so is the app",
        "/usr/bin/open https://github.com/ngvoicu/consensflow/releases",
        "// osascript -e 'tell application id \"x\" to quit' is what is refused",
        "kill(-leader, signal)",
        "kill -15 \"$pid\"",
        "kill -TERM \"$child\" 2>/dev/null",
        "Developer ID Application: Name (TEAM)",
        "let (code, found) = run(\"pgrep\", &[\"-f\", word])?;",
        "fs::File::open(&path)",
    ] {
        assert_eq!(reaching_by_name(line), [], "{line}");
    }
    for line in [
        "/Applications/ConsensFlow.app/Contents/MacOS/app",
        "\"/Applications/ConsensFlow.app\"",
        "~/Applications/ConsensFlow Candidate.app",
    ] {
        assert!(spells_an_installed_app(line), "{line}");
    }
    for line in [
        "/box/Applications/ConsensFlow.app",
        "/Users/someone/Applications/ConsensFlow.app",
        "/Applications/Utilities/Some.app",
    ] {
        assert!(!spells_an_installed_app(line), "{line}");
    }
    assert!(starts_what_it_is_given(
        "let mut command = Command::new(program);"
    ));
    assert!(starts_what_it_is_given(
        "std::process::Command::new(&self.program)"
    ));
    assert!(!starts_what_it_is_given("Command::new(\"/bin/echo\")"));
    assert!(!starts_what_it_is_given(
        "Command::new(r\"C:\\Windows\\ping.exe\")"
    ));
    assert!(!starts_what_it_is_given(
        "// Command::new(program) is what is held"
    ));
    assert!(!starts_what_it_is_given("LongCommand::new(mark)"));
}

#[test]
fn no_test_or_tool_reaches_an_app_by_its_name_or_its_identity() {
    let files = files();
    for read in [
        "tools/xtask/src/updater_smoke/app.rs",
        "crates/cf-e2e/tests/smoke/hidden.rs",
        "tests/plants/release/updater-product.toml",
        "app/scripts/windows-smoke.mjs",
        "crates/cf-process/src/started.rs",
    ] {
        let tested = files.iter().any(|(path, _)| path == read);
        // The product's own sources are not the tests' and the tools'.
        assert_eq!(tested, !read.ends_with("started.rs"), "{read}");
    }
    let found: Vec<String> = files
        .iter()
        .filter(|(path, _)| !RULES.contains(&path.as_str()))
        .flat_map(|(path, text)| {
            reaching_by_name(read_of(path, text))
                .into_iter()
                .map(move |(at, line)| format!("{path}:{at}: {line}"))
        })
        .collect();
    assert!(
        found.is_empty(),
        "these reach an app by its name or its identity, which are the live app's too: reach \
         one by the process it is (its pid, as the smoke's hidden case does), and end one as \
         cf_process::Started:\n{}",
        found.join("\n")
    );
}

#[test]
fn every_start_of_the_tools_and_the_rig_of_a_program_it_is_given_refuses_the_live_apps_identity() {
    let files = files();
    let starters: Vec<&str> = files
        .iter()
        .filter(|(path, text)| {
            (path.starts_with("tools/") || path.starts_with("crates/cf-e2e/"))
                && !RULES.contains(&path.as_str())
                && starts_what_it_is_given(text)
        })
        .map(|(path, _)| path.as_str())
        .collect();
    for held in [
        "tools/xtask/src/process.rs",
        "crates/cf-e2e/src/process.rs",
        "tools/cf-release/src/process.rs",
    ] {
        assert!(starters.contains(&held), "{held} is not read as a start");
    }
    let unheld: Vec<&str> = starters
        .iter()
        .copied()
        .filter(|path| {
            let text = &files.iter().find(|(it, _)| it == path).unwrap().1;
            !text.contains(GATE) && !STARTERS.iter().any(|(listed, _)| listed == path)
        })
        .collect();
    assert!(
        unheld.is_empty(),
        "these start a program they are given and are held to nothing: call \
         cf_base::bundle::{GATE} before it starts, as xtask's process and the rig's Run do:\n{}",
        unheld.join("\n")
    );
}

#[test]
fn no_test_or_tool_spells_an_installed_apps_path() {
    let found: Vec<String> = files()
        .iter()
        .filter(|(path, _)| {
            !RULES.contains(&path.as_str())
                && !SPELLING_INSTALLED.iter().any(|(listed, _)| listed == path)
        })
        .flat_map(|(path, text)| {
            text.lines()
                .enumerate()
                .filter(|(_, line)| {
                    let code = line.trim_start();
                    !code.starts_with("//")
                        && !code.starts_with('#')
                        && spells_an_installed_app(code)
                })
                .map(move |(at, line)| format!("{path}:{}: {}", at + 1, line.trim()))
        })
        .collect();
    assert!(
        found.is_empty(),
        "these spell the live app's path, or the installed Candidate's, which no test or tool \
         touches:\n{}",
        found.join("\n")
    );
}

/// The files of `files` that start a built Windows app and do not read its identity first,
/// each with what is wrong: one that is not held to its starts ([`STARTING_NO_BUILT_APP`]),
/// or whose starts are no longer the ones it is held to.
fn unheld_windows_starts(files: &[(String, String)]) -> Vec<String> {
    let mut unheld = Vec::new();
    for (path, text) in files {
        if RULES.contains(&path.as_str())
            || !names_a_built_windows_app(text)
            || start_lines(text).is_empty()
            || reads_its_identity_first(text)
        {
            continue;
        }
        match STARTING_NO_BUILT_APP
            .iter()
            .find(|(listed, _, _)| listed == path)
        {
            None => unheld.push(path.clone()),
            Some((_, _, held)) if start_lines(text) != *held => unheld.push(format!(
                "{path}, whose starts are no longer the ones held to (say why each starts no \
                 app of the release's, and list it):\n  {}",
                start_lines(text).join("\n  ")
            )),
            Some(_) => {}
        }
    }
    unheld
}

/// No test starts a built ConsensFlow on Windows but one of the smoke identity: an app of the
/// release's keeps its runtime in the release's folder of the machine's user, whatever the test
/// sets `LOCALAPPDATA` to, and T-930's start of the CI exe on zeewin2 wrote into Gabriel's
/// (2026-10-29, T-951). Every file of the tests and the tools that names a built Windows app and
/// starts a program reads its identity first, or says why it starts no such app.
#[test]
fn every_windows_start_of_a_built_app_reads_its_identity_first() {
    let files = files();
    for read in [
        "app/scripts/windows-smoke.mjs",
        "app/scripts/windows-smoke-desktop.mjs",
        ".github/workflows/windows-build.yml",
    ] {
        let (_, text) = files
            .iter()
            .find(|(path, _)| path == read)
            .unwrap_or_else(|| panic!("{read} is not read"));
        assert!(
            names_a_built_windows_app(text) && !start_lines(text).is_empty(),
            "{read} is not read as a start of a built Windows app"
        );
    }
    let unheld = unheld_windows_starts(&files);
    assert!(
        unheld.is_empty(),
        "these start a built Windows app and do not read its identity first: keep \
         `const refused = refusalOf(exe)` of app/scripts/built-identity.mjs and test it \
         (`if (refused)`) before it starts, as the Windows smoke does:\n{}",
        unheld.join("\n")
    );
}

/// What the Windows rule reads as a start and as a built app, and what it takes for a read of
/// the identity: every way of starting a program, in any case, and a refusal tested, never
/// one dropped (T-978).
#[test]
fn the_windows_rule_reads_every_start_and_a_refusal_that_is_tested() {
    for start in [
        "const app = spawn(exe, [])",
        "execSync(`${exe} --version`)",
        "exec(exe)",
        "fork(script)",
        "$app = Start-Process -FilePath $exe",
        "$said = & $exe --version",
        "& \"$dir\\ConsensFlow.exe\"",
        "Invoke-Expression $line",
        "invoke-command -ScriptBlock { & $exe }",
        "cmd /c start \"\" \"%EXE%\"",
        "start /wait app.exe",
        "Command::new(&exe)",
    ] {
        assert!(starts_a_program(start), "{start}");
    }
    for no_start in [
        "if (a && (b || c)) {}",
        "const A = point(x & ((1n << 255n) - 1n))",
        "the app's start time",
        "restart /now",
        "skip: windows && 'this is Windows',",
    ] {
        assert!(!starts_a_program(no_start), "{no_start}");
    }
    for name in [
        "ConsensFlow.exe",
        "consensflow.EXE",
        "target/release/app.exe",
        "ConsensFlow_3.0.7_x64-setup.exe",
    ] {
        assert!(names_a_built_windows_app(name), "{name}");
    }
    assert!(reads_its_identity_first(
        "const refused = refusalOf(exe)\nif (refused) {\n  process.exit(2)\n}"
    ));
    for dropped in [
        "refusalOf(exe)\nspawn(exe)",
        "const refused = refusalOf(exe)\nspawn(exe)",
        "import { refusalOf } from './built-identity.mjs'",
    ] {
        assert!(!reads_its_identity_first(dropped), "{dropped}");
    }

    // A file held to its starts is read again when one is added, and one that names a built
    // app and starts a program with its refusal dropped is not taken for one that reads it.
    let files = files();
    let held = |path: &str| {
        files
            .iter()
            .find(|(listed, _)| listed == path)
            .cloned()
            .unwrap_or_else(|| panic!("{path} is not read"))
    };
    let (path, text) = held("tests/install-ps1.test.mjs");
    assert!(unheld_windows_starts(&[(path.clone(), text.clone())]).is_empty());
    let one_more = format!("{text}\nspawn(join(dir, 'ConsensFlow.exe'), [])\n");
    assert_eq!(unheld_windows_starts(&[(path.clone(), one_more)]).len(), 1);
    let (smoke, script) = held("app/scripts/windows-smoke.mjs");
    let dropped = script.replace(
        "const refused = refusalOf(exe)",
        "refusalOf(exe)\nconst refused = null",
    );
    assert_ne!(dropped, script);
    assert_eq!(unheld_windows_starts(&[(smoke.clone(), dropped)]), [smoke]);
    let elsewhere = (
        "tests/new.test.mjs".to_owned(),
        "spawn('ConsensFlow.exe')".to_owned(),
    );
    assert_eq!(unheld_windows_starts(&[elsewhere]), ["tests/new.test.mjs"]);
}
