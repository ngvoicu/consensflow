//! The differences from what Node said that step 4 makes on purpose, for
//! `setup` and `doctor`, each stated once with its reason, and what Rust is
//! held to where it has one: Node's answer, written again as the decision has
//! it. The rest of a case is Node's, byte for byte.
//!
//! These are the differences of the launcher. Step 4's decision gives it a new
//! shape that names only the native `cf` (the bundle after the deletion has no
//! Node to name), which changes what `setup` writes, and what it means for
//! `doctor` to say a command is another copy's.

use serde_json::Value;

use super::Expected;

/// What `setup` writes: the launcher runs the native `cf` and nothing else.
pub const SHAPE: &str = "the launcher names only the native cf: step 4's new shape, since \
     the bundle after the deletion has no Node and no cf.mjs to name";

/// What `doctor` says of a command that runs another copy.
pub const COPY: &str = "a native cf has no runtime of its own to compare a command's with, so it \
     tells whose command it is by the file the command runs; the recording's launcher of another \
     runtime runs `$REPO/bin/cf.mjs`, which here is this copy's own";

/// The mark every launcher of ours holds.
const MARK: &str = "Installed by ConsensFlow";

/// The reason of the difference `expected` holds, if it holds one, and
/// `expected` as Rust is held to it.
pub fn pair(case: &Value, expected: &mut Expected) -> Option<&'static str> {
    let verb = case["args"][0].as_str().unwrap_or_default();
    match (verb, case["name"].as_str().unwrap_or_default()) {
        ("setup", _) => launchers_of_ours(&mut expected.after).then_some(SHAPE),
        ("doctor", "doctor: launchers of another runtime that is there") => {
            expected.stdout = expected.stdout.as_deref().map(this_copy);
            Some(COPY)
        }
        _ => None,
    }
}

/// Makes each launcher of ours that `after` lists the one this build writes,
/// and says whether there was one.
fn launchers_of_ours(after: &mut Value) -> bool {
    let mut found = false;
    for entry in after.as_array_mut().unwrap() {
        let Some(text) = entry["text"].as_str().filter(|text| text.contains(MARK)) else {
            continue;
        };
        entry["text"] = Value::String(native(text));
        found = true;
    }
    found
}

/// Node's launcher as this build writes it: the two lines of the comment that
/// say what it runs, and the line that runs it, are ours, and the rest, the
/// mark, the home's pin and every line ending, are Node's.
fn native(node: &str) -> String {
    const COMMENTS: [(&str, &str); 2] = [
        (
            "# Installed by ConsensFlow. Runs the app's own runtime and its own copy of\n# the CLI, so the terminal and the window never drift apart.\n",
            "# Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\n# window never drift apart.\n",
        ),
        (
            "REM Installed by ConsensFlow. Runs the app's own runtime and its own copy of\r\nREM the CLI, so the terminal and the window never drift apart.\r\n",
            "REM Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\n",
        ),
    ];
    let mut text = node.to_owned();
    for (old, new) in COMMENTS {
        text = text.replace(old, new);
    }
    // The line that runs it is the last: a runtime and a `cf.mjs` for Node.
    let body = text.trim_end_matches(['\r', '\n']).len();
    let start = text[..body].rfind('\n').map_or(0, |at| at + 1);
    let runs = &text[start..body];
    let command = if runs.starts_with("exec \"") && runs.ends_with(" \"$@\"") {
        "exec \"$CF\" \"$@\"\n"
    } else if runs.starts_with('"') && runs.ends_with(" %*") {
        "\"$CF\" %*\r\n"
    } else {
        return text;
    };
    format!("{}{command}", &text[..start])
}

/// What `doctor` says with the line about the command that runs another copy
/// said as it is of this one's.
fn this_copy(node: &str) -> String {
    const OTHER: &str =
        " — another ConsensFlow. `cf` runs that one; `cf setup` from this one claims the command.";
    assert!(node.contains(OTHER), "Node does not say another's: {node}");
    node.replace(OTHER, "")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A launcher as the recording holds it, in the form of `sh` and in the
    /// form of cmd.exe (which only a Windows recording has), with a pin and
    /// without one.
    fn node(windows: bool, pinned: bool) -> String {
        if windows {
            let pin = if pinned {
                "setlocal\r\nset \"CONSENSFLOW_HOME=$ROOT\\consensflow\"\r\n"
            } else {
                ""
            };
            format!(
                "@echo off\r\nREM Installed by ConsensFlow. Runs the app's own runtime and its own copy of\r\nREM the CLI, so the terminal and the window never drift apart.\r\n{pin}\"$NODE\" \"$REPO\\bin\\cf.mjs\" %*\r\n"
            )
        } else {
            let pin = if pinned {
                "export CONSENSFLOW_HOME=\"$ROOT/consensflow\"\n"
            } else {
                ""
            };
            format!(
                "#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own runtime and its own copy of\n# the CLI, so the terminal and the window never drift apart.\n{pin}exec \"$NODE\" \"$REPO/bin/cf.mjs\" \"$@\"\n"
            )
        }
    }

    #[test]
    fn a_launcher_is_ours_in_the_lines_that_name_what_it_runs_and_nodes_in_the_rest() {
        assert_eq!(
            native(&node(false, true)),
            "#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\n# window never drift apart.\nexport CONSENSFLOW_HOME=\"$ROOT/consensflow\"\nexec \"$CF\" \"$@\"\n"
        );
        assert_eq!(
            native(&node(false, false)),
            "#!/bin/sh\n# Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\n# window never drift apart.\nexec \"$CF\" \"$@\"\n"
        );
        assert_eq!(
            native(&node(true, true)),
            "@echo off\r\nREM Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\nsetlocal\r\nset \"CONSENSFLOW_HOME=$ROOT\\consensflow\"\r\n\"$CF\" %*\r\n"
        );
        assert_eq!(
            native(&node(true, false)),
            "@echo off\r\nREM Installed by ConsensFlow. Runs the app's own cf, so the terminal and the\r\nREM window never drift apart.\r\n\"$CF\" %*\r\n"
        );
    }

    #[test]
    fn a_text_that_is_no_launcher_of_node_s_is_left_as_it_is() {
        for text in [
            "",
            "echo mine\n",
            "#!/bin/sh\n# Installed by ConsensFlow\necho x\n",
        ] {
            assert_eq!(native(text), text);
        }
    }
}
