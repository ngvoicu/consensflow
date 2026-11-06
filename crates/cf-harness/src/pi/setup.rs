//! What gives a Pi its providers, named: the one thing the human needs said when Pi lists no
//! model (HMD-13). Pi's own answer to `get_available_models` is an empty list for a Pi with
//! nothing set up and for a Pi whose provider could not list (a package's provider whose server
//! is down lists none, and says nothing), so the list cannot tell them apart. This can, by
//! names alone: never a login's token, a key's value or a provider's URL.
//!
//! Pi (1.5.0, `model-runtime.js`, `resource-loader.js`) has its models from, in the order the
//! docs give them:
//! - a login in the agent folder's `auth.json`, an OAuth one or an API key, one entry a provider;
//! - a provider key in the environment (`providers.md`, "Use an API key from the environment");
//! - a custom provider in `models.json`;
//! - a provider an installed Pi package registers, named in `settings.json`'s `packages`;
//! - a provider an extension registers, one in the agent folder's `extensions` folder.
//!
//! A project's own `.pi` folder can name more of each; a read from here knows no project.

use std::path::Path;

use cf_base::env::Env;
use cf_base::file::read_file;
use cf_base::json::from_slice_lossy;
use cf_base::path;
use serde_json::Value;

use super::paths::agent_dir;
use crate::shared::paths::set;

/// The environment variables Pi takes a provider's credentials from (`providers.md`): a key or a
/// token, or the base of a local server's address. The ones that only tune a provider that has a
/// key (`AWS_REGION`, `AZURE_OPENAI_API_VERSION`) are left out.
const CREDENTIALS: [&str; 41] = [
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_OAUTH_TOKEN",
    "ANTHROPIC_AUTH_TOKEN",
    "ANT_LING_API_KEY",
    "OPENAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "NVIDIA_API_KEY",
    "GEMINI_API_KEY",
    "COPILOT_GITHUB_TOKEN",
    "MISTRAL_API_KEY",
    "GROQ_API_KEY",
    "CEREBRAS_API_KEY",
    "XAI_API_KEY",
    "OPENROUTER_API_KEY",
    "AI_GATEWAY_API_KEY",
    "ZAI_API_KEY",
    "ZAI_CODING_CN_API_KEY",
    "OPENCODE_API_KEY",
    "RADIUS_API_KEY",
    "TYPESAFE_API_KEY",
    "HF_TOKEN",
    "FIREWORKS_API_KEY",
    "TOGETHER_API_KEY",
    "BASETEN_API_KEY",
    "KIMI_API_KEY",
    "META_API_KEY",
    "MINIMAX_API_KEY",
    "MINIMAX_CN_API_KEY",
    "MOONSHOT_API_KEY",
    "QWEN_TOKEN_PLAN_API_KEY",
    "QWEN_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_API_KEY",
    "XIAOMI_TOKEN_PLAN_CN_API_KEY",
    "XIAOMI_TOKEN_PLAN_AMS_API_KEY",
    "XIAOMI_TOKEN_PLAN_SGP_API_KEY",
    "AZURE_OPENAI_API_KEY",
    "AWS_BEARER_TOKEN_BEDROCK",
    "AWS_PROFILE",
    "AWS_ACCESS_KEY_ID",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "LLAMA_BASE_URL",
];

/// What the Pi folder and the environment name as the sources of Pi's providers: each list the
/// names of one source, empty where it has none.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Setup {
    /// The providers `auth.json` holds an entry for.
    pub logins: Vec<String>,
    /// The providers `models.json` defines.
    pub custom: Vec<String>,
    /// The sources `settings.json` lists under `packages` (`npm:@scope/name`, a path).
    pub packages: Vec<String>,
    /// What stands in the agent folder's `extensions` folder.
    pub extensions: Vec<String>,
    /// The credential variables set in the environment, by name.
    pub keys: Vec<String>,
}

impl Setup {
    /// The setup of the Pi `env` names (`PI_CODING_AGENT_DIR`, else `.pi/agent` in the home).
    /// A file that is missing or unreadable names nothing; no file is written.
    pub(crate) fn read(env: &Env) -> Self {
        let keys = CREDENTIALS
            .into_iter()
            .filter(|name| set(env, name).is_some())
            .map(str::to_owned)
            .collect();
        let Ok(agent) = agent_dir(env) else {
            return Self {
                keys,
                ..Self::default()
            };
        };
        let file = |name: &str| -> Option<Value> {
            let bytes = read_file(Path::new(&path::join(&[&agent, name]))).ok()?;
            from_slice_lossy(&bytes).ok()
        };
        let names = |value: Option<&Value>| -> Vec<String> {
            let mut names: Vec<String> = value
                .and_then(Value::as_object)
                .map(|table| table.keys().cloned().collect())
                .unwrap_or_default();
            names.sort();
            names
        };
        let providers = file("models.json").and_then(|models| models.get("providers").cloned());
        let packages = file("settings.json")
            .and_then(|settings| settings.get("packages").and_then(Value::as_array).cloned())
            .unwrap_or_default()
            .iter()
            // A package is a source, or an entry that names one (`{ "source": …, "extensions": … }`).
            .filter_map(|package| {
                package
                    .as_str()
                    .or_else(|| package.get("source").and_then(Value::as_str))
                    .map(str::to_owned)
            })
            .collect();
        let mut extensions: Vec<String> = std::fs::read_dir(path::join(&[&agent, "extensions"]))
            .into_iter()
            .flatten()
            .filter_map(|entry| Some(entry.ok()?.file_name().to_string_lossy().into_owned()))
            .filter(|name| !name.starts_with('.'))
            .collect();
        extensions.sort();
        Self {
            logins: names(file("auth.json").as_ref()),
            custom: names(providers.as_ref()),
            packages,
            extensions,
            keys,
        }
    }

    /// Whether nothing at all is set up.
    pub(crate) fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// What Pi's empty list is to be told as: that no provider is set up and how to give it one
    /// where that is true, else what is set up that listed nothing, and why that can be.
    pub(crate) fn unlisted(&self) -> String {
        if self.is_empty() {
            return "Pi lists no model: no provider is set up (no login in auth.json, no provider \
                    in models.json, no installed package or extension, no provider key in the \
                    environment). Run pi and sign in with /login, or set a provider key such as \
                    OPENROUTER_API_KEY, or add a provider to models.json (Pi's providers.md and \
                    models.md say how)"
                .to_owned();
        }
        let parts: Vec<String> = [
            ("a login for", &self.logins),
            ("custom providers", &self.custom),
            ("packages", &self.packages),
            ("extensions", &self.extensions),
            ("provider keys", &self.keys),
        ]
        .into_iter()
        .filter(|(_, names)| !names.is_empty())
        .map(|(what, names)| format!("{what} {}", names.join(", ")))
        .collect();
        format!(
            "Pi lists no model though it has {}: none of them gave it one. A provider that a \
             package or an extension registers can list its models from a server of its own, and \
             lists none while that server is down; a login Pi cannot use lists none",
            parts.join("; ")
        )
    }
}

#[cfg(test)]
mod tests;
