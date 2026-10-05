//! The MCP servers Codex would start and the flags that switch them off:
//! how Codex is asked, what its answer makes of the list, and how a name is
//! read as JavaScript reads it.

use serde_json::json;

use super::*;
use crate::codex::fakes::Watching;
use crate::seams::processes::Failed;
use crate::testing::finished;

const CODEX: &str = "/usr/local/bin/codex";

/// The servers Codex lists when it answers `stdout`, or why it could not.
fn list(stdout: Result<&str, Failed>) -> Result<Vec<Value>, String> {
    let watching = Watching::default();
    watching
        .scripted
        .run_answer("codex mcp list --json", stdout.map(str::to_owned));
    finished(Box::pin(listed(&watching, &Env::default(), CODEX)))
}

fn refused(cause: &str) -> String {
    format!("could not list Codex's MCP servers to switch them off: {cause}")
}

#[test]
fn the_servers_are_asked_of_codex_in_the_launch_s_environment_with_fifteen_seconds_and_a_megabyte()
{
    let watching = Watching::default();
    watching
        .scripted
        .run_answer("codex mcp list --json", Ok("[]".to_owned()));
    let env = Env::from_vars([("HOME", "/home/me"), ("PATH", "/bin")]);
    assert_eq!(
        finished(Box::pin(listed(&watching, &env, CODEX))),
        Ok(Vec::new())
    );
    let ran = watching.run.borrow();
    let [(program, limits)] = &ran[..] else {
        panic!("{ran:?}");
    };
    assert_eq!(program.executable, PathBuf::from(CODEX));
    assert_eq!(program.args, ["mcp", "list", "--json"]);
    assert_eq!(program.cwd, None);
    assert_eq!(program.env.text("HOME"), Some("/home/me"));
    assert_eq!(program.env.text("PATH"), Some("/bin"));
    assert_eq!(
        *limits,
        Limits {
            timeout: Duration::from_secs(15),
            max_buffer: 1024 * 1024,
        }
    );
}

#[test]
fn a_list_is_the_servers_and_any_other_json_is_none() {
    assert_eq!(
        list(Ok(r#"[{"name":"a"}, 7]"#)).unwrap(),
        [json!({ "name": "a" }), json!(7)]
    );
    assert_eq!(list(Ok(" \n[]\r\n")).unwrap(), Vec::<Value>::new());
    for other in [
        "{}",
        r#"{"servers":[{"name":"a"}]}"#,
        "null",
        "7",
        "\"a\"",
        "true",
    ] {
        assert_eq!(list(Ok(other)).unwrap(), Vec::<Value>::new(), "{other}");
    }
}

#[test]
fn a_run_that_failed_says_why_in_its_own_words_after_ours() {
    for message in [
        "Command failed: /usr/local/bin/codex mcp list --json\nboom\n",
        "spawn /usr/local/bin/codex ENOENT",
        "stdout maxBuffer length exceeded",
    ] {
        for killed in [false, true] {
            let failed = Failed {
                message: message.to_owned(),
                code: None,
                killed,
                stdout: String::new(),
            };
            assert_eq!(list(Err(failed)), Err(refused(message)), "{message}");
        }
    }
}

#[test]
fn an_answer_that_cannot_be_read_as_json_is_refused_in_our_words() {
    let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
    for answer in [
        "",
        "not json",
        "\u{feff}[]",
        "[1,",
        "[]]",
        "{'a':1}",
        "[1e400]",
        &deep,
    ] {
        assert_eq!(
            list(Ok(answer)),
            Err(refused("its answer cannot be read as JSON")),
            "{answer:?}"
        );
    }
}

/// The flags that switch off the servers listed, or why not.
fn switched_off(servers: &[Value]) -> Result<Vec<String>, String> {
    isolation(servers)
}

#[test]
fn every_server_is_given_a_harmless_definition_that_is_disabled() {
    assert_eq!(switched_off(&[]), Ok(Vec::new()));
    assert_eq!(
        switched_off(&[
            json!({ "name": "cua_repl", "enabled": true }),
            json!({ "name": "computer-history" }),
        ]),
        Ok([
            "-c",
            "mcp_servers.cua_repl.command=\"/usr/bin/true\"",
            "-c",
            "mcp_servers.cua_repl.enabled=false",
            "-c",
            "mcp_servers.computer-history.command=\"/usr/bin/true\"",
            "-c",
            "mcp_servers.computer-history.enabled=false",
        ]
        .map(str::to_owned)
        .to_vec())
    );
}

#[test]
fn a_name_is_read_as_javascript_reads_it_whatever_text_a_number_or_a_flag_it_makes() {
    for (name, text) in [
        (json!("a_B-9"), "a_B-9"),
        (json!(5), "5"),
        (json!(true), "true"),
        (json!(false), "false"),
        (json!(["x"]), "x"),
        (json!([["x"]]), "x"),
        (json!(1e-7), "1e-7"),
        (json!(-5), "-5"),
        (json!(-0.0), "0"),
        (
            json!(100_000_000_000_000_000_000.0),
            "100000000000000000000",
        ),
        (json!(1.5e300), "1.5e+300"),
    ] {
        let flags = switched_off(&[json!({ "name": name })]);
        let command = format!("mcp_servers.{text}.command=\"/usr/bin/true\"");
        if text.contains(['.', '+']) {
            assert!(flags.is_err(), "{name}");
        } else {
            assert_eq!(flags.unwrap()[1], command, "{name}");
        }
    }
}

#[test]
fn a_name_the_pattern_refuses_stops_the_launch_naming_it_as_json_writes_it() {
    let refused =
        |name: &str| format!("cannot switch off the Codex MCP server {name} for a member");
    for (server, named) in [
        (json!({}), "undefined"),
        (json!({ "enabled": true }), "undefined"),
        (json!(7), "undefined"),
        (json!("a"), "undefined"),
        (json!([]), "undefined"),
        (json!({ "name": null }), "null"),
        (json!({ "name": "" }), "\"\""),
        (json!({ "name": "a.b" }), "\"a.b\""),
        (json!({ "name": "a b" }), "\"a b\""),
        (json!({ "name": "a=b" }), "\"a=b\""),
        (json!({ "name": "a\nb" }), "\"a\\nb\""),
        (json!({ "name": "serveur-\u{e9}" }), "\"serveur-\u{e9}\""),
        (json!({ "name": "\u{661}" }), "\"\u{661}\""),
        (json!({ "name": {} }), "{}"),
        (json!({ "name": { "a": 1 } }), "{\"a\":1}"),
        (json!({ "name": ["a", "b"] }), "[\"a\",\"b\"]"),
        (json!({ "name": [] }), "[]"),
        (json!({ "name": 1.5 }), "1.5"),
        (json!({ "name": 1e21 }), "1e+21"),
    ] {
        assert_eq!(
            switched_off(std::slice::from_ref(&server)),
            Err(refused(named)),
            "{server}"
        );
        // However many were fine before it.
        let fine = json!({ "name": "fine" });
        assert_eq!(switched_off(&[fine, server]), Err(refused(named)));
    }
}

#[test]
fn a_server_listed_as_null_or_named_by_an_object_with_a_tostring_of_its_own_fails_in_our_words() {
    assert_eq!(
        switched_off(&[json!(null)]),
        Err("cannot switch off a Codex MCP server listed as null for a member".to_owned())
    );
    for name in [json!({ "toString": 1 }), json!([{ "toString": null }])] {
        assert_eq!(
            switched_off(&[json!({ "name": name })]),
            Err("an object with a toString of its own cannot be made text".to_owned())
        );
    }
}
