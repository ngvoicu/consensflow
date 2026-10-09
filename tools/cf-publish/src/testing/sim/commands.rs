//! The `gh` commands the simulator knows: `gh release view|create|upload|
//! delete-asset|edit` and `gh api --method PATCH …` (an asset renamed), as the
//! publisher runs them. A command or a flag it does not know is an error of the
//! simulator's own (status 2), not an answer of GitHub's.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::{Asset, Model, Snapshot, State};
use crate::gh::Answer;

/// A call that went well, and printed `stdout`.
pub(super) fn done(stdout: impl Into<String>) -> Answer {
    Answer {
        status: 0,
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

/// A call GitHub refused, in `stderr`'s words.
pub(super) fn refuse(stderr: &str, status: i32) -> Answer {
    Answer {
        status,
        stdout: String::new(),
        stderr: format!("{stderr}\n"),
    }
}

/// The words a command's arguments say, by what each flag takes.
struct Parsed {
    positional: Vec<String>,
    flags: HashMap<String, String>,
}

fn parse(rest: &[String], values: &[&str], booleans: &[&str]) -> Result<Parsed, String> {
    let mut positional = Vec::new();
    let mut flags = HashMap::new();
    let mut at = 0;
    while at < rest.len() {
        let arg = &rest[at];
        at += 1;
        if !arg.starts_with('-') {
            positional.push(arg.clone());
            continue;
        }
        let bare = arg
            .strip_prefix("--")
            .or_else(|| arg.strip_prefix('-'))
            .unwrap_or(arg);
        match bare.split_once('=') {
            Some((name, joined)) => {
                flags.insert(name.to_string(), joined.to_string());
            }
            None if values.contains(&bare) => {
                flags.insert(bare.to_string(), rest.get(at).cloned().unwrap_or_default());
                at += 1;
            }
            None if booleans.contains(&bare) => {
                flags.insert(bare.to_string(), "true".to_string());
            }
            None => return Err(format!("the simulator does not know the flag {arg}")),
        }
    }
    Ok(Parsed { positional, flags })
}

fn asset_json(repo: &str, tag: &str, name: &str, asset: &Asset) -> Value {
    json!({
        "apiUrl": format!("https://api.github.com/repos/{repo}/releases/assets/{}", asset.id),
        "contentType": "application/octet-stream",
        "id": format!("RA_{}", asset.id),
        "label": "",
        "name": name,
        "size": asset.data.len(),
        "state": "uploaded",
        "url": format!("https://github.com/{repo}/releases/download/{tag}/{name}"),
    })
}

/// `gh release <verb> …`, run in the folder `cwd`.
pub(super) fn release(
    state: &mut State,
    repo: &str,
    verb: &str,
    rest: &[String],
    cwd: &Path,
) -> Result<Answer, String> {
    match verb {
        "view" => {
            let Parsed { positional, flags } = parse(rest, &["json"], &[])?;
            let tag = positional.first().cloned().unwrap_or_default();
            let Some(one) = state.releases.get(&tag) else {
                return Ok(refuse("release not found", 1));
            };
            let document = json!({
                "assets": one.assets.iter().map(|(name, asset)| asset_json(repo, &tag, name, asset)).collect::<Vec<_>>(),
                "isDraft": one.draft,
                "isPrerelease": one.prerelease,
                "name": one.title,
                "body": one.notes,
            });
            let wanted = flags.get("json").cloned().unwrap_or_default();
            let chosen: Map<String, Value> = wanted
                .split(',')
                .filter_map(|key| Some((key.to_string(), document.get(key)?.clone())))
                .collect();
            Ok(done(format!("{}\n", Value::Object(chosen))))
        }
        "create" => {
            let Parsed { positional, flags } = parse(
                rest,
                &["title", "notes", "notes-file"],
                &["draft", "prerelease", "verify-tag"],
            )?;
            if positional.len() != 1 {
                return Err("the simulator makes a release with no files".to_string());
            }
            let tag = positional[0].clone();
            if state.releases.contains_key(&tag) {
                return Ok(refuse("HTTP 422: Validation Failed (already_exists)", 1));
            }
            let notes = match (flags.get("notes"), flags.get("notes-file")) {
                (Some(notes), _) => notes.clone(),
                (None, Some(file)) => {
                    fs::read_to_string(cwd.join(file)).map_err(|cause| cause.to_string())?
                }
                (None, None) => String::new(),
            };
            state.releases.insert(
                tag.clone(),
                Model {
                    draft: flags.get("draft").is_some_and(|draft| draft == "true"),
                    prerelease: flags
                        .get("prerelease")
                        .is_some_and(|prerelease| prerelease == "true"),
                    title: flags.get("title").cloned().unwrap_or_else(|| tag.clone()),
                    notes,
                    assets: Vec::new(),
                },
            );
            Ok(done(format!(
                "https://github.com/{repo}/releases/tag/{tag}\n"
            )))
        }
        "upload" => {
            let Parsed { positional, .. } = parse(rest, &[], &[])?;
            let Some((tag, files)) = positional.split_first() else {
                return Ok(refuse("release not found", 1));
            };
            let State {
                releases, next_id, ..
            } = state;
            let Some(one) = releases.get_mut(tag) else {
                return Ok(refuse("release not found", 1));
            };
            for file in files {
                let name = file.rsplit(['/', '\\']).next().unwrap_or(file).to_string();
                if one.assets.iter().any(|(held, _)| *held == name) {
                    return Ok(refuse(&format!("a file named {name} already exists"), 1));
                }
                let data = fs::read(cwd.join(file)).map_err(|cause| cause.to_string())?;
                one.assets.push((name, Asset { id: *next_id, data }));
                *next_id += 1;
            }
            Ok(done(""))
        }
        "delete-asset" => {
            let Parsed { positional, .. } = parse(rest, &[], &["yes", "y"])?;
            let (tag, name) = (
                positional.first().cloned().unwrap_or_default(),
                positional.get(1).cloned().unwrap_or_default(),
            );
            let Some(one) = state.releases.get_mut(&tag) else {
                return Ok(refuse("release not found", 1));
            };
            let before = one.assets.len();
            one.assets.retain(|(held, _)| *held != name);
            if one.assets.len() == before {
                return Ok(refuse(
                    &format!("asset under the name \"{name}\" not found"),
                    1,
                ));
            }
            Ok(done(""))
        }
        "edit" => {
            let Parsed { positional, flags } = parse(rest, &["title", "notes", "draft"], &[])?;
            let tag = positional.first().cloned().unwrap_or_default();
            let Some(one) = state.releases.get_mut(&tag) else {
                return Ok(refuse("release not found", 1));
            };
            if let Some(title) = flags.get("title") {
                one.title = title.clone();
            }
            if let Some(notes) = flags.get("notes") {
                one.notes = notes.clone();
            }
            if let Some(draft) = flags.get("draft") {
                one.draft = draft != "false";
            }
            Ok(done(""))
        }
        other => Err(format!("the simulator does not know gh release {other}")),
    }
}

/// `gh api --method PATCH repos/<repo>/releases/assets/<id> -f name=<new>`: an asset renamed.
pub(super) fn api(state: &mut State, repo: &str, rest: &[String]) -> Result<Answer, String> {
    let Parsed { positional, flags } = parse(rest, &["method", "f"], &[])?;
    let id = positional
        .first()
        .and_then(|path| path.strip_prefix(&format!("repos/{repo}/releases/assets/")))
        .and_then(|id| id.parse::<u64>().ok());
    let renamed = flags.get("f").and_then(|field| field.strip_prefix("name="));
    let (Some(id), Some(renamed), Some("PATCH")) =
        (id, renamed, flags.get("method").map(String::as_str))
    else {
        return Err(format!(
            "the simulator does not know gh api {}",
            rest.join(" ")
        ));
    };
    for one in state.releases.values_mut() {
        let Some(at) = one.assets.iter().position(|(_, asset)| asset.id == id) else {
            continue;
        };
        if one.assets.iter().any(|(held, _)| held == renamed) {
            return Ok(refuse("HTTP 422: Validation Failed (already_exists)", 1));
        }
        let (_, asset) = one.assets.remove(at);
        one.assets.push((renamed.to_string(), asset));
        return Ok(done("{}\n"));
    }
    Ok(refuse("HTTP 404: Not Found", 1))
}

/// Every release as it stands: whether it is a draft, and its assets' names, sorted.
pub(super) fn snapshot(state: &State) -> BTreeMap<String, Snapshot> {
    state
        .releases
        .iter()
        .map(|(tag, one)| {
            let mut assets: Vec<String> = one.assets.iter().map(|(name, _)| name.clone()).collect();
            assets.sort();
            (
                tag.clone(),
                Snapshot {
                    draft: one.draft,
                    assets,
                },
            )
        })
        .collect()
}
