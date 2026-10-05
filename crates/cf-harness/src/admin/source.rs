//! How a harness got onto this machine, read from where its executable
//! really lives (`releaseSource`, `src/harness-admin.js`): the release feed
//! to compare against, the words for the page, and the command that brings
//! it to the latest release the same way. None when the method is not
//! recognized, so the human updates it as they installed it.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use cf_base::env::Env;
use cf_base::json::from_slice_lossy;
use cf_base::path;
use cf_proto::agents::Harness;
use regex::Regex;
use serde_json::Value;

use crate::shared::paths::home;
use crate::shared::pattern::compile;

/// What a release feed's answer is read as, by its name in `src/harness-admin.js`
/// (`source.format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// JSON with the release in `version` (npm's registry, Devin's manifest).
    Npm,
    /// Homebrew's JSON for a cask, the release in `version`.
    Cask,
    /// Homebrew's JSON for a formula, the release in `versions.stable`.
    Formula,
    /// The release as plain text, white space around it taken off.
    Text,
}

impl Format {
    /// Its name, as `src/harness-admin.js` writes it.
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Npm => "npm",
            Format::Cask => "cask",
            Format::Formula => "formula",
            Format::Text => "text",
        }
    }
}

/// Where the latest release of a harness is asked, how a CLI that got onto
/// this machine the way it did is updated, and how that is called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The feed to ask: a GET of this address, with [`crate::admin::feed`]'s
    /// limits.
    pub url: String,
    /// How the feed's answer is read.
    pub format: Format,
    /// How the CLI was installed, for the page; none when not recognized.
    pub distribution: Option<String>,
    /// The command that updates it as it was installed: the program, then its
    /// arguments; none when the install method is not recognized.
    pub update: Option<Vec<String>>,
}

/// The name a harness goes by on the page.
pub(super) fn name(harness: Harness) -> &'static str {
    match harness {
        Harness::Claude => "Claude",
        Harness::Codex => "Codex",
        Harness::Opencode => "OpenCode",
        Harness::Pi => "Pi",
        Harness::Devin => "Devin",
    }
}

/// Where a harness's own feed of releases is (`SOURCES`).
fn feed(harness: Harness) -> &'static str {
    match harness {
        Harness::Claude => "https://registry.npmjs.org/@anthropic-ai/claude-code/latest",
        Harness::Codex => "https://registry.npmjs.org/@openai/codex/latest",
        Harness::Opencode => "https://registry.npmjs.org/opencode-ai/latest",
        Harness::Pi => "https://registry.npmjs.org/@earendil-works/pi-coding-agent/latest",
        Harness::Devin => "https://static.devin.ai/cli/current/manifest.json",
    }
}

/// The npm package a harness is installed as (`NPM_PACKAGES`); Devin has none.
fn npm_package(harness: Harness) -> Option<&'static str> {
    match harness {
        Harness::Claude => Some("@anthropic-ai/claude-code"),
        Harness::Codex => Some("@openai/codex"),
        Harness::Opencode => Some("opencode-ai"),
        Harness::Pi => Some("@earendil-works/pi-coding-agent"),
        Harness::Devin => None,
    }
}

/// A harness's own installer (`OWN_INSTALLER`): the folder it puts the CLI
/// in, as a part of a path written with `/`, and the arguments of the
/// command that updates it. Claude's is told apart another way.
fn own_installer(harness: Harness) -> Option<(&'static str, &'static [&'static str])> {
    match harness {
        Harness::Codex => Some(("/.codex/bin/", &["update"])),
        Harness::Opencode => Some(("/.opencode/bin/", &["upgrade"])),
        Harness::Pi => Some(("/.pi/bin/", &["update", "--self"])),
        Harness::Devin => Some(("/devin/cli/", &["update"])),
        Harness::Claude => None,
    }
}

/// Any character `.` takes in a JavaScript pattern: not a line terminator.
const ANY: &str = r"[^\n\r\x{2028}\x{2029}]";

/// A Homebrew cask or formula: `/(.*)\/(Caskroom|Cellar)\/([^/]+)\//`.
static BREW: LazyLock<Regex> =
    LazyLock::new(|| compile(&format!(r"^({ANY}*)/(Caskroom|Cellar)/([^/]+)/")));

/// Claude's copy on Windows, in the home it was installed for:
/// `/^(.*)\/\.local\/bin\/claude\.exe$/i`, where `i` folds ASCII alone.
static COPIED: LazyLock<Regex> =
    LazyLock::new(|| compile(&format!(r"^({ANY}*)/(?i-u:\.local/bin/claude\.exe)$")));

/// A global npm install: `/^(.*)\/lib\/node_modules\//`.
static NPM: LazyLock<Regex> = LazyLock::new(|| compile(&format!(r"^({ANY}*)/lib/node_modules/")));

/// How `harness`'s CLI at `executable` was installed, and so where its
/// latest release is asked and what updates it (`releaseSource`).
pub fn release_source(harness: Harness, executable: &str, env: &Env) -> Source {
    // The layouts are spelled with `/`; Windows answers with `\`.
    let real = real_path(executable).replace('\\', "/");
    if let Some(brewed) = BREW.captures(&real) {
        let (prefix, kind, package) = (&brewed[1], &brewed[2], &brewed[3]);
        let expected: &[&str] = if harness == Harness::Claude {
            &["claude-code", "claude-code@latest"]
        } else {
            &[harness.as_str()]
        };
        if expected.contains(&package) {
            let cask = kind == "Caskroom";
            let mut update = vec![path::join(&[prefix, "bin", "brew"]), "upgrade".to_owned()];
            if cask {
                update.push("--cask".to_owned());
            }
            update.push(package.to_owned());
            return Source {
                url: format!(
                    "https://formulae.brew.sh/api/{}/{package}.json",
                    if cask { "cask" } else { "formula" }
                ),
                format: if cask { Format::Cask } else { Format::Formula },
                distribution: Some("Homebrew".to_owned()),
                update: Some(update),
            };
        }
    }
    // Claude's installer links ~/.local/bin/claude into ~/.local/share/claude/versions;
    // on Windows it copies the current version there as claude.exe instead.
    if harness == Harness::Claude && is_claude_installer(&real) {
        let channel = channel(env);
        return Source {
            url: format!("https://downloads.claude.ai/claude-code-releases/{channel}"),
            format: Format::Text,
            distribution: Some(format!("Claude's installer, {channel} channel")),
            update: Some(vec![executable.to_owned(), "update".to_owned()]),
        };
    }
    let npm = |distribution: Option<String>, update: Option<Vec<String>>| Source {
        url: feed(harness).to_owned(),
        format: Format::Npm,
        distribution,
        update,
    };
    if let (Some(installed), Some(package)) = (NPM.captures(&real), npm_package(harness)) {
        return npm(
            Some("npm".to_owned()),
            Some(vec![
                path::join(&[&installed[1], "bin", "npm"]),
                "install".to_owned(),
                "-g".to_owned(),
                format!("{package}@latest"),
            ]),
        );
    }
    if let Some((marker, arguments)) = own_installer(harness) {
        if real.contains(marker) {
            let mut update = vec![executable.to_owned()];
            update.extend(arguments.iter().map(|argument| (*argument).to_owned()));
            return npm(Some(format!("{}'s installer", name(harness))), Some(update));
        }
    }
    npm(None, None)
}

/// Whether the CLI at `real` (written with `/`) was put where Claude's own
/// installer puts it: a link into its versions, or on Windows a copy of the
/// current version with the versions beside it.
fn is_claude_installer(real: &str) -> bool {
    real.contains("/claude/versions/")
        || COPIED.captures(real).is_some_and(|copied| {
            let versions = path::join(&[&copied[1], ".local", "share", "claude", "versions"]);
            Path::new(&versions).exists()
        })
}

/// The channel Claude's installer follows: `stable` when its settings say
/// so, else `latest`; also when they cannot be read.
///
/// Kept from Node on purpose: with neither `CLAUDE_CONFIG_DIR` nor a home in
/// the environment Node read the settings in `os.homedir()`, the process's
/// own; a Rust module may not read it, so the channel is `latest`.
fn channel(env: &Env) -> &'static str {
    let folder = match env.os("CLAUDE_CONFIG_DIR") {
        Some(folder) => folder.to_string_lossy().into_owned(),
        None => match home(env) {
            Ok(home) => path::join(&[&home, ".claude"]),
            Err(_) => return "latest",
        },
    };
    let stable = fs::read(path::join(&[&folder, "settings.json"]))
        .ok()
        .and_then(|bytes| from_slice_lossy(&bytes).ok())
        .is_some_and(|settings| {
            settings.get("autoUpdatesChannel").and_then(Value::as_str) == Some("stable")
        });
    if stable {
        "stable"
    } else {
        "latest"
    }
}

/// Where `executable` really is (`realpathSync`), or itself where there is
/// no such place.
///
/// Kept from Node on purpose: the system's own resolution, which names a
/// path on Windows as it is on disk, in the case and the long names it was
/// made with; Node's walk keeps a path as it was typed. The layouts read
/// folder names that either spelling has alike.
fn real_path(executable: &str) -> String {
    fs::canonicalize(executable).map_or_else(
        |_| executable.to_owned(),
        |real| plain(&real.to_string_lossy()),
    )
}

/// A path without the prefix Windows gives a resolved one, which Node never
/// writes: `\\?\C:\x` is `C:\x`, `\\?\UNC\server\share` is `\\server\share`.
fn plain(path: &str) -> String {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{share}");
    }
    path.strip_prefix(r"\\?\").unwrap_or(path).to_owned()
}

#[cfg(test)]
mod tests;
