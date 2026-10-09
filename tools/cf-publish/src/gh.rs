//! GitHub, as the publisher touches it: `gh`, a program the runner has, run as
//! a process. [`Gh`] is what the publisher asks of it, so that the tests stand
//! in a simulator of GitHub for the program; [`Runner`] is the publisher's way
//! of asking: every call is logged, one that must succeed says what it was for
//! when it does not, and a release's state is read the one way.

use std::path::Path;

use serde_json::Value;

use crate::failure::Failure;
use crate::process;

/// What a `gh` call answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

/// One call of `gh`.
pub trait Gh {
    /// Runs `gh` with `args` in the folder `cwd`, where the relative paths among
    /// the arguments are.
    fn run(&self, args: &[String], cwd: &Path) -> Answer;
}

/// The `gh` of the machine, found on its PATH: the token and the repository it
/// works on are the environment's (`GH_TOKEN`, `GH_REPO`).
pub struct ProcessGh;

impl Gh for ProcessGh {
    fn run(&self, args: &[String], cwd: &Path) -> Answer {
        match process::capture("gh", args, Some(cwd)) {
            Ok(output) => Answer {
                status: output.code,
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            },
            Err(cause) => Answer {
                status: 1,
                stdout: String::new(),
                stderr: format!("gh could not be started: {cause}"),
            },
        }
    }
}

/// A file of a release, as `gh release view --json assets` lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub size: Option<u64>,
    pub state: Option<String>,
    /// The address of the asset in GitHub's API, which is what renames it.
    pub api_url: Option<String>,
}

impl Asset {
    fn from_json(asset: &Value) -> Option<Self> {
        Some(Self {
            name: asset.get("name")?.as_str()?.to_string(),
            size: asset.get("size").and_then(Value::as_u64),
            state: asset
                .get("state")
                .and_then(Value::as_str)
                .map(str::to_string),
            api_url: asset
                .get("apiUrl")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    /// Its number in GitHub's API: the end of its address.
    fn id(&self) -> Option<&str> {
        let (_, id) = self.api_url.as_deref()?.rsplit_once("/releases/assets/")?;
        (!id.is_empty() && id.bytes().all(|byte| byte.is_ascii_digit())).then_some(id)
    }
}

/// What a release holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReleaseState {
    pub draft: bool,
    pub assets: Vec<Asset>,
}

impl ReleaseState {
    /// The file named `name`, if the release holds one.
    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|asset| asset.name == name)
    }

    /// Whether the release holds a file named `name`.
    pub fn has(&self, name: &str) -> bool {
        self.asset(name).is_some()
    }
}

/// The publisher's way of asking `gh`, in the folder the release was built in.
pub struct Runner<'a> {
    gh: &'a dyn Gh,
    dir: &'a Path,
    log: &'a dyn Fn(&str),
}

impl<'a> Runner<'a> {
    /// A runner of `gh` calls in `dir`, which says each call to `log`.
    pub fn new(gh: &'a dyn Gh, dir: &'a Path, log: &'a dyn Fn(&str)) -> Self {
        Self { gh, dir, log }
    }

    /// Says `line` to the log.
    pub fn log(&self, line: &str) {
        (self.log)(line);
    }

    /// A call, whatever it answers.
    pub fn ask(&self, args: &[&str]) -> Answer {
        self.log(&format!("gh {}", args.join(" ")));
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
        self.gh.run(&args, self.dir)
    }

    /// The output of a call that must succeed, and what it was for when it did not.
    pub fn must(&self, args: &[&str], what: &str) -> Result<String, Failure> {
        let answer = self.ask(args);
        if answer.status != 0 {
            let said = if answer.stderr.is_empty() {
                answer.stdout.trim()
            } else {
                answer.stderr.trim()
            };
            return Err(Failure::new(if said.is_empty() {
                format!("{what}: gh exited {}", answer.status)
            } else {
                format!("{what}: {said}")
            }));
        }
        Ok(answer.stdout)
    }

    /// What a release holds, or none where there is none; any other failure is
    /// not an absence.
    pub fn state(&self, tag: &str) -> Result<Option<ReleaseState>, Failure> {
        let answer = self.ask(&["release", "view", tag, "--json", "assets,isDraft"]);
        if answer.status == 0 {
            let document: Value = serde_json::from_str(&answer.stdout).map_err(|cause| {
                Failure::new(format!(
                    "could not look at the release {tag}: gh said something that is not JSON ({cause})"
                ))
            })?;
            return Ok(Some(ReleaseState {
                draft: document
                    .get("isDraft")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                assets: document
                    .get("assets")
                    .and_then(Value::as_array)
                    .map(|assets| assets.iter().filter_map(Asset::from_json).collect())
                    .unwrap_or_default(),
            }));
        }
        let said = answer.stderr.to_lowercase();
        if said.contains("not found") || said.contains("http 404") {
            return Ok(None);
        }
        Err(Failure::new(format!(
            "could not look at the release {tag}: {}",
            answer.stderr.trim()
        )))
    }

    /// Renames a release's asset, which GitHub does in one call.
    pub fn rename(&self, repo: &str, asset: Option<&Asset>, to: &str) -> Result<(), Failure> {
        let Some(asset) = asset else {
            return Err(Failure::new(format!("there is no file to rename to {to}")));
        };
        let Some(id) = asset.id() else {
            return Err(Failure::new(format!(
                "gh gave no address for {}, which cannot be renamed",
                asset.name
            )));
        };
        self.must(
            &[
                "api",
                "--method",
                "PATCH",
                &format!("repos/{repo}/releases/assets/{id}"),
                "-f",
                &format!("name={to}"),
            ],
            &format!("renaming {} to {to}", asset.name),
        )?;
        Ok(())
    }
}
