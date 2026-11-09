//! What a note says of the last tool call, and what it must never say (T-823, the reviews of
//! `a26e0848e` and `860010398`): never the arguments of a command. The tool, and for a shell
//! call the first word where it is a program the list names, and nothing after it.

use super::*;

/// The note's words of a shell command run by the tool `Bash`.
fn say(command: &str) -> String {
    words(&LastCall::shell("Bash", command))
}

/// What the note says when the command's first word is no program the list names: the tool.
const TOOL_ONLY: &str = "; its last tool call was Bash";

/// The note says the program and that the command went on: `cargo …`.
fn program_then_more(program: &str) -> String {
    format!("; its last tool call was Bash: \"{program} \u{2026}\"")
}

#[test]
fn a_listed_program_is_named_and_what_follows_it_is_not() {
    for (command, program) in [
        (
            "cargo fmt --check && cargo test --workspace -- --nocapture",
            "cargo",
        ),
        ("cargo test -p cf-engine --lib shown::", "cargo"),
        ("git status --short", "git"),
        ("npm run lint && npm test", "npm"),
        ("make build; make test", "make"),
        (
            "pytest tests/ -x -k a_long_test_name_that_goes_on_and_on",
            "pytest",
        ),
        ("ls -la crates/cf-engine/src", "ls"),
        // A path to the program is its last part, and the path is not said.
        ("/usr/bin/git status", "git"),
        ("./cargo test", "cargo"),
        ("/home/me/.secret-place/cargo test", "cargo"),
        // The command goes on over a line.
        ("cargo\n  test", "cargo"),
    ] {
        assert_eq!(say(command), program_then_more(program), "{command}");
    }
    // A command of one word goes on over nothing.
    assert_eq!(say("ls"), "; its last tool call was Bash: \"ls\"");
    // The tool's name is said as it is, and a call with none is a command.
    assert_eq!(
        words(&LastCall::shell("shell", "cargo test")),
        "; its last tool call was shell: \"cargo \u{2026}\""
    );
    assert_eq!(
        words(&LastCall::shell("", "cargo test")),
        "; its last tool call was a command: \"cargo \u{2026}\""
    );
}

/// The three commands the re-check persisted, and every shape the first review named, each on a
/// program the list names: the note says the program and not one word more.
#[test]
fn nothing_of_a_command_after_its_program_is_ever_said() {
    for (command, program, secret) in [
        // T-1222: a wrapper, an option with an argument before the subcommand, a script.
        (
            "npm exec -- docker login -p redgreenblue",
            "npm",
            "redgreenblue",
        ),
        (
            "git -C /w config http.extraHeader X-Session:redgreenblue",
            "git",
            "redgreenblue",
        ),
        ("node login.js alice redgreenblue", "node", "redgreenblue"),
        // T-1216 and the first review: a URL's user, a token in a config, a login.
        (
            "git clone https://alice:swordfish@github.test/x/y.git",
            "git",
            "swordfish",
        ),
        (
            "git clone https://swordfishvalue@localhost/x/y.git",
            "git",
            "swordfishvalue",
        ),
        (
            "npm config set //registry.test/:_authToken=swordfishvalue",
            "npm",
            "swordfishvalue",
        ),
        ("cargo login swordfishvalue", "cargo", "swordfishvalue"),
        (
            "cargo publish --token swordfishvalue",
            "cargo",
            "swordfishvalue",
        ),
        (
            "cargo test -- --password=swordfishvalue",
            "cargo",
            "swordfishvalue",
        ),
        (
            "git commit -m \"rotate the key to swordfishvalue\"",
            "git",
            "swordfishvalue",
        ),
        (
            "git -c http.extraHeader=swordfish fetch",
            "git",
            "swordfish",
        ),
        (
            "python3 -c 'print(1)' swordfishvalue",
            "python3",
            "swordfishvalue",
        ),
        ("node -e 1 swordfishvalue", "node", "swordfishvalue"),
        (
            "cat > notes <<EOF\nls swordfishvalue\nEOF",
            "cat",
            "swordfishvalue",
        ),
        ("cat /home/me/.aws/credentials", "cat", "credentials"),
        ("ls /home/me/.config/secrets.toml", "ls", "secrets"),
        (
            "pytest --api-key swordfishvalue",
            "pytest",
            "swordfishvalue",
        ),
        (
            "cargo test ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "cargo",
            "abcdefghij",
        ),
        (
            "cargo test && curl -u alice:swordfish https://example.test/",
            "cargo",
            "swordfish",
        ),
        (
            "tar xf a.tgz --passphrase swordfishvalue",
            "tar",
            "swordfishvalue",
        ),
    ] {
        let said = say(command);
        assert_eq!(said, program_then_more(program), "{command}");
        assert!(!said.contains(secret), "{command}");
    }
}

/// A command whose first word is no program on the list is the tool's name alone: a variable on
/// its line, a wrapper, a client that takes a credential, a quote, a substitution.
#[test]
fn a_command_whose_first_word_is_not_a_listed_program_is_the_tool_alone() {
    for command in [
        "curl -u alice:swordfish https://example.test/",
        "curl --user alice:swordfish https://example.test/",
        "wget --user alice --password swordfish https://example.test/",
        "mysql -u root -pswordfish",
        "sshpass -p swordfish ssh host",
        "redis-cli -a swordfish",
        "docker login -u me -p swordfish",
        "gh auth login --with-token",
        "TOKEN=swordfishvalue cargo test",
        "export TOKEN=swordfishvalue",
        "RUST_LOG=debug cargo test",
        "sudo cargo test --token swordfish",
        "env TOKEN=swordfish cargo test",
        "bash -c 'cargo test swordfish'",
        "sh -lc 'cargo test swordfish'",
        "echo swordfishvalue | tool",
        "printf swordfishvalue",
        "sed -i s/old/swordfishvalue/ file",
        "grep -r swordfishvalue .",
        "xargs rm swordfish",
        "\"cargo\" test swordfish",
        "'cargo' test swordfish",
        "$(which cargo) test swordfish",
        "`which cargo` test swordfish",
        "cargo;rm swordfish",
        "cargo-evil swordfish",
        // A first word that is written with more than a program's characters.
        "$HOME/cargo test swordfish",
        "$(evil)/cargo test swordfish",
        "xcargo test swordfish",
        "cd /w && cargo test --token swordfish",
        "someothertool --flag swordfish",
        "",
        "   ",
    ] {
        let said = say(command);
        assert_eq!(said, TOOL_ONLY, "{command:?}");
        assert!(
            !said.contains("swordfish") && !said.contains("alice"),
            "{command:?}"
        );
    }
}

/// A tool that was given text of its own is the tool's name alone, and so is a tool that works
/// on a file: no path is ever said (the re-check T-1224 read a token split by blanks, a key's
/// body of twelve characters and a percent-encoded URL out of the paths a note quoted).
#[test]
fn a_tool_without_a_command_is_its_name_and_a_file_is_never_said() {
    assert_eq!(
        words(&LastCall::tool("exec")),
        "; its last tool call was exec"
    );
    for target in [
        "crates/cf-engine/src/shown.rs",
        "/tmp/sk-abcd efgh ijkl mnop qrst uvwx yz12 3456.txt",
        "/tmp/hkiG9w0BAQEFAASC.txt",
        "/tmp/https%3A%2F%2Falice%3Aswordfish%40example.test.json",
        "/home/me/.config/secrets.toml",
        "/home/me/.aws/credentials",
        "src/ghp_abcdefghijklmnopqrstuvwxyz0123456789.txt",
        "https://alice:swordfish@example.test/x",
        "https://swordfishvalue@localhost/x",
    ] {
        for tool in ["Read", "Edit", "Write", "NotebookEdit"] {
            assert_eq!(
                words(&LastCall::on(tool, target)),
                format!("; its last tool call was {tool}"),
                "{tool} {target}"
            );
        }
    }
    // A tool's name is cut to the bound a name has, with a mark.
    let said = words(&LastCall::tool(&format!(
        "mcp__server__{}",
        "tool_".repeat(30)
    )));
    let name = said
        .strip_prefix("; its last tool call was ")
        .unwrap_or_default();
    assert_eq!(name.chars().count(), TOOL_CHARS, "{said}");
    assert!(
        name.starts_with("mcp__server__tool_") && name.ends_with('\u{2026}'),
        "{said}"
    );
    // A tool whose name is a token is masked like any word of a note.
    assert_eq!(
        words(&LastCall::tool(concat!(
            "ghp",
            "_abcdefghijklmnopqrstuvwxyz0123456789"
        ))),
        "; its last tool call was [masked]"
    );
    // A call with no tool named and no command says nothing.
    assert_eq!(words(&LastCall::tool("")), "");
    assert_eq!(
        words(&LastCall::shell("", "curl -u a:b https://x.test")),
        "; its last tool call was a command"
    );
}

/// T-1328, found by the Windows jobs of the CI: a process the system lists on Windows has its
/// program as a quoted path, with blanks in it. The note names the program the list names and
/// not one word of the path or of what follows.
#[test]
fn a_windows_process_is_named_by_its_program_and_not_by_its_path() {
    for (command, want) in [
        (
            r#""C:\Program Files\Git\usr\bin\sleep.exe" 600"#,
            Some(("sleep", true)),
        ),
        (
            r#""C:\Program Files\Git\usr\bin\SLEEP.EXE""#,
            Some(("sleep", false)),
        ),
        (
            r#"  "C:\tools\pi hunter22.cmd\cargo.exe" test -- --token abc"#,
            Some(("cargo", true)),
        ),
        (r#""/usr/bin/git" status"#, Some(("git", true))),
        (
            r#""C:\Program Files\nodejs\node.exe""#,
            Some(("node", false)),
        ),
        // Not a listed program, or no name at all.
        (
            r#""C:\Program Files\Git\bin\bash.exe" -c "sleep 600; true""#,
            None,
        ),
        (r#""C:\tools\sk-abc defg hijk lmno.exe""#, None),
        (r#""C:\tools\sleep.exe.evil" 1"#, None),
        (r#""C:\tools\sleep;rm.exe" 1"#, None),
        (r#""C:\tools\.exe""#, None),
        (r#""unclosed\sleep.exe 600"#, None),
        (r#""""#, None),
    ] {
        assert_eq!(command_line_program(command), want, "{command}");
    }
    // An unquoted command line is read as any command is.
    assert_eq!(command_line_program("sleep 600"), Some(("sleep", true)));
    assert_eq!(
        command_line_program(r"C:\Program Files\sleep.exe 600"),
        None
    );
}
