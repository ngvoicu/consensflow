//! What Codex is started with: its arguments divided between the app-server
//! and the TUI, and ConsensFlow's own variables set in Codex's shell policy.

use std::ffi::OsString;

use cf_base::env::Env;

/// The flag that opens Codex in full-permission mode.
pub(crate) const BYPASS: &str = "--dangerously-bypass-approvals-and-sandbox";

/// Codex's arguments as the two programs it runs as are given them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Split {
    /// What the app-server is started with, before `app-server`.
    pub(crate) backend: Vec<OsString>,
    /// What the TUI is started with, after the `--remote` ones.
    pub(crate) tui: Vec<OsString>,
}

/// A flag that wants a value and has none.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum MissingValue {
    #[error("Missing Codex {0} value")]
    Flag(String),
    #[error("Missing Codex model")]
    Model,
}

/// `args` divided between the app-server and the TUI. A remote TUI may not
/// override its permissions, so the native backend owns that configuration:
/// full-permission mode goes to the backend as configuration and not to the
/// TUI at all; configuration and features go to both; a model goes to the
/// TUI as the flag and to the backend as configuration; the rest is the TUI's.
pub(crate) fn split(args: &[OsString]) -> Result<Split, MissingValue> {
    let mut split = Split::default();
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.to_str() {
            Some(BYPASS) => split.backend.extend(
                [
                    "-c",
                    "approval_policy=\"never\"",
                    "-c",
                    "sandbox_mode=\"danger-full-access\"",
                ]
                .map(OsString::from),
            ),
            Some(flag @ ("-c" | "--config" | "--enable" | "--disable")) => {
                let value = args
                    .next()
                    .ok_or_else(|| MissingValue::Flag(flag.to_string()))?;
                split.backend.extend([arg.clone(), value.clone()]);
                split.tui.extend([arg.clone(), value.clone()]);
            }
            Some("--model" | "-m") => {
                let value = args.next().ok_or(MissingValue::Model)?;
                split
                    .backend
                    .extend(["-c".into(), model(&value.to_string_lossy()).into()]);
                split.tui.extend([arg.clone(), value.clone()]);
            }
            _ => split.tui.push(arg.clone()),
        }
    }
    Ok(split)
}

/// `model="<name>"`, the name written as a JSON string, as Codex reads it.
fn model(name: &str) -> String {
    format!("model={}", json_string(name))
}

/// `text` as a JSON string, quotes included.
fn json_string(text: &str) -> String {
    serde_json::Value::from(text).to_string()
}

/// ConsensFlow's own variables, to be set explicitly in Codex's shell policy: a
/// user policy of `inherit = "core"` keeps only a handful of names, and a
/// window whose commands lose `CONSENSFLOW_URL`, `_TOKEN` and `_NODE` has a
/// `cf` that reaches nothing. Everything else stays as the user's policy says.
/// Each `CONSENSFLOW_` variable that has a value, with it, in name order. The
/// broker gives them to each thread in its `config`, never a command line: the
/// token is a secret, and every user of the machine reads a command line.
pub(crate) fn consensflow_variables(env: &Env) -> Vec<(String, String)> {
    let mut named: Vec<(String, String)> = env
        .iter()
        .filter_map(|(name, value)| {
            Some((
                name.to_str()?.to_owned(),
                value.to_string_lossy().into_owned(),
            ))
        })
        .filter(|(name, _)| is_consensflow_variable(name))
        .collect();
    named.sort_by(|(a, _), (b, _)| a.encode_utf16().cmp(b.encode_utf16()));
    named
}

/// `^CONSENSFLOW_[A-Z0-9_]+$`.
fn is_consensflow_variable(name: &str) -> bool {
    name.strip_prefix("CONSENSFLOW_").is_some_and(|rest| {
        !rest.is_empty()
            && rest
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const THREAD: &str = "01a09094-938f-7fd1-a2d3-315cf92b4559";

    fn args(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn names_consensflows_own_variables_with_their_values_in_name_order_and_nothing_else() {
        let env = Env::from_vars([
            ("CONSENSFLOW_URL", "http://127.0.0.1:4100"),
            ("CONSENSFLOW_TOKEN", "tok\"en"),
            ("CF_CODEX_TUI_TOKEN", "internal"),
            ("HOME", "/home/user"),
        ]);
        assert_eq!(
            consensflow_variables(&env),
            pairs(&[
                ("CONSENSFLOW_TOKEN", "tok\"en"),
                ("CONSENSFLOW_URL", "http://127.0.0.1:4100"),
            ])
        );
        assert!(consensflow_variables(&Env::from_vars([("HOME", "/home/user")])).is_empty());
    }

    #[test]
    fn names_only_the_variables_that_are_consensflows_with_something_after_the_prefix() {
        let env = Env::from_vars([
            ("CONSENSFLOW_", "bare"),
            ("XCONSENSFLOW_A", "elsewhere"),
            ("CONSENSFLOW_A1_B", "kept"),
            ("CONSENSFLOW_EMPTY", ""),
        ]);
        assert_eq!(
            consensflow_variables(&env),
            pairs(&[("CONSENSFLOW_A1_B", "kept"), ("CONSENSFLOW_EMPTY", "")])
        );
    }

    /// Windows names variables in any case, so its environment holds them in upper case.
    #[test]
    fn a_name_with_lower_case_letters_is_none_of_consensflows_except_where_names_ignore_case() {
        let env = Env::from_vars([("CONSENSFLOW_lower", "mixed case")]);
        assert_eq!(
            consensflow_variables(&env).len(),
            usize::from(cfg!(windows))
        );
    }

    #[test]
    fn a_value_is_carried_as_it_is() {
        let value = "C:\\Users\\me\n\u{1}\u{e9}\u{1f600}\u{2028}";
        let env = Env::from_vars([("CONSENSFLOW_HOME", value)]);
        assert_eq!(
            consensflow_variables(&env),
            pairs(&[("CONSENSFLOW_HOME", value)])
        );
    }

    #[test]
    fn keeps_native_tui_arguments_while_explicitly_forwarding_backend_model_effort_and_configuration(
    ) {
        let setting = "check_for_update_on_startup=false";
        let given = args(&[
            "-c",
            setting,
            "--model",
            "native-model",
            "-c",
            "model_reasoning_effort=\"high\"",
            BYPASS,
            "real worker task",
        ]);
        let split = split(&given).unwrap();
        assert_eq!(
            split.backend,
            args(&[
                "-c",
                setting,
                "-c",
                "model=\"native-model\"",
                "-c",
                "model_reasoning_effort=\"high\"",
                "-c",
                "approval_policy=\"never\"",
                "-c",
                "sandbox_mode=\"danger-full-access\"",
            ])
        );
        assert_eq!(
            split.tui,
            given
                .iter()
                .filter(|arg| *arg != BYPASS)
                .cloned()
                .collect::<Vec<_>>()
        );
        assert_eq!(
            super::split(&args(&["-c", setting, "resume", THREAD])).unwrap(),
            Split {
                backend: args(&["-c", setting]),
                tui: args(&["-c", setting, "resume", THREAD]),
            }
        );
    }

    #[test]
    fn sends_features_to_both_and_the_short_model_flag_as_the_long_one_goes() {
        let split = split(&args(&[
            "--enable",
            "default_mode_request_user_input",
            "--disable",
            "x",
            "--config",
            "a=1",
            "-m",
            "gpt\"5",
            "--model=inline",
        ]))
        .unwrap();
        assert_eq!(
            split.backend,
            args(&[
                "--enable",
                "default_mode_request_user_input",
                "--disable",
                "x",
                "--config",
                "a=1",
                "-c",
                "model=\"gpt\\\"5\"",
            ])
        );
        assert_eq!(
            split.tui,
            args(&[
                "--enable",
                "default_mode_request_user_input",
                "--disable",
                "x",
                "--config",
                "a=1",
                "-m",
                "gpt\"5",
                "--model=inline",
            ])
        );
    }

    #[test]
    fn a_flag_that_wants_a_value_and_has_none_is_said() {
        for (given, said) in [
            ("-c", "Missing Codex -c value"),
            ("--config", "Missing Codex --config value"),
            ("--enable", "Missing Codex --enable value"),
            ("--disable", "Missing Codex --disable value"),
            ("--model", "Missing Codex model"),
            ("-m", "Missing Codex model"),
        ] {
            let said_by = split(&args(&["resume", given])).unwrap_err().to_string();
            assert_eq!(said_by, said, "{given}");
        }
    }
}
