//! What a note says of the last tool call, and what it must never say (T-823, the review of
//! `a26e0848e`): `curl -u alice:swordfish` was quoted whole. A command is quoted only when
//! every word of it can be told safe; anything else, and any tool whose input is a script or
//! a prompt, is the tool's name alone.

use super::*;

/// The note's words of a shell command run by the tool `Bash`.
fn say(command: &str) -> String {
    words(&LastCall::shell("Bash", command))
}

/// What the note says when it is left out whole.
const LEFT_OUT: &str =
    "; its last tool call was Bash; what it was given may name a secret, so it is left out";

#[test]
fn a_build_or_test_command_is_quoted_whole() {
    for command in [
        "cargo fmt --check && cargo test --workspace -- --nocapture",
        "cargo test -p cf-engine --lib shown::",
        "git status --short",
        "ls -la crates/cf-engine/src",
        "cd ~/Projects/ngvoicu/consensflow-trees/failure-why && cargo test 2>&1 | tail -5",
        "RUST_LOG=debug cargo test --workspace",
        "npm run lint && npm test",
        "make build; make test",
        "pytest tests/ -x -k a_long_test_name_that_goes_on_and_on",
    ] {
        assert_eq!(
            say(command),
            format!("; its last tool call was: \"{command}\""),
            "{command}"
        );
    }
}

/// Every credential-bearing shape the review and the chief named, and a few beside them:
/// none of the value reaches the note, and the call is left out whole.
#[test]
fn a_command_that_carries_a_credential_is_left_out_whole() {
    for command in [
        // curl's, wget's and the clients that take a user and a password.
        "curl -u alice:swordfish https://example.test/",
        "curl --user alice:swordfish https://example.test/",
        "curl --proxy-user alice:swordfish https://example.test/",
        "curl -ualice:swordfish https://example.test/",
        "curl https://example.test/ -U proxyuser:swordfish",
        "wget --user alice --password swordfish https://example.test/",
        "mysql -u root -pswordfish",
        "mysql -u root --password=swordfish",
        "sshpass -p swordfish ssh host",
        "redis-cli -a swordfish",
        "docker login -u me -p swordfish",
        // A URL's user, with or without a password, on any scheme and any tool.
        "git clone https://alice:swordfish@github.test/x/y.git",
        "git clone https://ghp_notarealtokenvalue@github.test/x/y.git",
        "cargo install --git ssh://alice:swordfish@host/x",
        "psql postgres://alice:swordfish@db.test/app",
        // Flags that say what they carry.
        "tool --token swordfishvalue",
        "tool --with-token < file",
        "gh auth login --with-token",
        "tool --api-key swordfishvalue",
        "tool --secret=swordfishvalue",
        "npm config set //registry.test/:_authToken=swordfishvalue",
        // Names and headers.
        "export TOKEN=swordfishvalue",
        "TOKEN=swordfishvalue cargo test",
        "curl -H 'Authorization: Bearer swordfishvalue' https://example.test/",
        "curl -H \"X-Api-Key: swordfishvalue\" https://example.test/",
        "echo swordfishvalue | tool",
        // The same on tools the list does say: the words carry it, not the tool.
        "cargo publish --token swordfishvalue",
        "cargo test -- --password=swordfishvalue",
        "npm publish --//registry.test/:_authToken=swordfishvalue",
        "git push https://swordfishvalue@github.test/x/y.git",
        "git -c http.extraHeader=swordfish fetch",
        "ls /home/me/.aws/credentials",
        "pytest --api-key swordfishvalue",
        // Subcommands that take a token on the line.
        "cargo login swordfishvalue",
        "npm login",
        "git credential fill",
    ] {
        let said = say(command);
        assert_eq!(said, LEFT_OUT, "{command}");
        assert!(
            !said.contains("swordfish") && !said.contains("alice"),
            "{command}"
        );
    }
}

/// What cannot be classified is not quoted: a quoted string, a substitution, a heredoc, an
/// interpreter's script, a tool the list does not know, a variable's value at the front.
#[test]
fn a_command_that_cannot_be_told_safe_is_left_out_whole() {
    for command in [
        "git commit -m \"rotate the key to swordfishvalue\"",
        "git commit -m 'rotate to swordfishvalue'",
        "cargo test -- \"a name with spaces\"",
        "cargo test $(cat secret.txt)",
        "cargo test $(cat notes.txt)",
        "cargo test && someothertool --flag",
        "ls | sh",
        "cargo test; curl https://example.test/x",
        "cargo test & curl https://example.test/x",
        "cargo test `cat file`",
        "cat > notes <<EOF\nthe code is swordfishvalue\nEOF",
        "python3 -c 'print(1)'",
        "node -e 1",
        "bash -c 'cargo test'",
        "sh -lc cargo",
        "someothertool --flag value",
        "FOO=swordfishvalue cargo test",
        "cargo test >(cat)",
        "echo hello",
        "printf swordfishvalue",
        "sed -i s/old/swordfishvalue/ file",
        "grep -r swordfishvalue .",
        "xargs rm",
    ] {
        let said = say(command);
        assert_eq!(said, LEFT_OUT, "{command}");
        assert!(!said.contains("swordfish"), "{command}");
    }
}

/// A token-shaped word that the masker would hide leaves the call out, rather than quoting
/// it with a hole; and the decision is on the whole, whatever the note's bound is.
#[test]
fn a_command_with_a_token_shaped_word_or_a_secret_past_the_bound_is_left_out() {
    assert_eq!(
        say("cargo test ghp_abcdefghijklmnopqrstuvwxyz0123456789"),
        LEFT_OUT
    );
    assert_eq!(say(&format!("cargo test {}", "YWJj".repeat(12))), LEFT_OUT);
    // The secret is past the note's 200 characters: it is still the whole call that is judged.
    let far = format!("cargo test {} --token swordfishvalue", "-v ".repeat(80));
    let said = say(&far);
    assert_eq!(said, LEFT_OUT);
    // A long safe command is cut with a mark.
    let long = format!("cargo test {}", "-v ".repeat(120));
    let said = say(long.trim_end());
    assert!(
        said.starts_with("; its last tool call was: \"cargo test -v -v"),
        "{said}"
    );
    assert!(said.ends_with("…\""), "{said}");
    assert_eq!(
        said.trim_start_matches("; its last tool call was: \"")
            .trim_end_matches('"')
            .chars()
            .count(),
        CALL_CHARS
    );
}

/// A tool that was given text of its own (a script, a patch, a prompt, a URL, a query) is
/// the tool's name alone, and a file is quoted with the tool unless it names a secret.
#[test]
fn a_tool_without_a_command_is_its_name_and_a_file_is_quoted_unless_it_names_a_secret() {
    assert_eq!(
        words(&LastCall::tool("exec")),
        "; its last tool call was exec"
    );
    assert_eq!(
        words(&LastCall::on("Read", "crates/cf-engine/src/shown.rs")),
        "; its last tool call was: \"Read crates/cf-engine/src/shown.rs\""
    );
    for target in [
        "/home/me/.config/secrets.toml",
        "/home/me/.aws/credentials",
        "/home/me/api_key.txt",
        "src/ghp_abcdefghijklmnopqrstuvwxyz0123456789.txt",
        "https://alice:swordfish@example.test/x",
        "https://swordfishvalue@example.test/x",
    ] {
        let said = words(&LastCall::on("Read", target));
        assert_eq!(
            said,
            "; its last tool call was Read; what it was given may name a secret, so it is left out",
            "{target}"
        );
    }
    // A tool whose name is a token is masked like any word of a note.
    assert_eq!(
        words(&LastCall::tool(concat!(
            "ghp",
            "_abcdefghijklmnopqrstuvwxyz0123456789"
        ))),
        "; its last tool call was [masked]"
    );
    // A call with no tool named, and a command that cannot be said.
    assert_eq!(words(&LastCall::tool("")), "");
    assert_eq!(
        words(&LastCall::shell("", "curl -u a:b https://x.test")),
        "; its last tool call was a command that may name a secret, so it is left out"
    );
}
