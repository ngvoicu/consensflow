//! What a plant is, and the data the plants are written in.
//!
//! A plant is a bug put into the product on purpose, to prove that a test
//! notices it. Each family of plants has a folder, `tests/plants/<family>/`:
//!
//! - `kit.toml` says how the family is run: `run_limit_minutes`, the longest one
//!   run may take; `rebuild`, the command that builds again what a run that
//!   `builds` left built from planted sources (none, for a family that builds
//!   nothing); `tables`, the files of plants in the order they are run, which
//!   is also the list of them (a file the folder holds that it does not name,
//!   or the reverse, is an error); `known_unapplied`, the names of the plants
//!   that do not apply yet, which `--check` says known rather than failing (a
//!   name that no plant has is an error); and `[run]`, the runs that several
//!   tables share.
//! - `<table>.toml` are the plants, one table for each area, and `[run]` there is
//!   the runs of that table alone.
//!
//! A run is a command to try a plant with, program first (`cargo xtask …` is
//! this xtask itself: see [`runs`](super::runs)). It is named in a `[run]` as an
//! array of its words, or as `{ command = [...], builds = true }` where it builds
//! the native `cf` from the sources as they are, which has to be built again once
//! the plant is put back. A name is the table's own or the kit's, never both.
//!
//! ```toml
//! [[plant]]
//! name = "parser: a flag given a value is taken"   # what is wrong, once planted; unique
//! meant = "every_list_of_words_is_read_as_node_reads_it"   # the test it was written for
//! runs = ["args", ["cargo", "test", "--offline", "-p", "cf"]]   # a name, or the words
//! builds = true             # optional: its runs build the native `cf`
//! prepare = [["cargo", "xtask", "smoke-updater", "--export-bridge", "--cache", "app/src-tauri/target/updater-smoke"]]   # optional: must succeed first
//! requires = ["app/src-tauri/target/…"]   # optional: files its runs take as given
//! platforms = ["macos"]     # optional: the systems it is run on ("macos", "windows", "linux")
//!
//! [[plant.edit]]            # one or more, each in one file where `find` is found exactly once
//! file = "crates/cf-base/src/args.rs"
//! find = "(Takes::Nothing, Some(_)) => {"
//! replace = "(Takes::Nothing, Some(_)) if false => {"
//! ```
//!
//! The runs are tried in order until one catches the plant. `meant` is the test the
//! plant was written for, named whole (a Node test by its own title, or with the
//! titles of the tests it is in before it, ` › ` between): a run whose failure names
//! it catches the plant, where that run passes without it; one whose failure names
//! only other tests goes on to the next run, and where none names it the plant is
//! MISSED-BY-MEANT. A failure that names no test (a command that says only its
//! status) is a catch by the run itself, which the report says is not by `meant`
//! (the driver tells how each is confirmed). The edits of a plant are made one after the
//! other, each on the text the one before left. Text with a newline in it is best
//! written as TOML's multi-line literal string, `'''`, which holds it as it is
//! (a newline right after the opening quotes is not part of it).
//!
//! A plant of code that one system alone has (a Mac's) says so in `platforms`: on
//! any other it is not run, and is said SKIPPED (platform), neither caught nor
//! missed. A name there that is no system, or none at all, is an error of the table.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::Error;
use crate::context::Context;

/// A family of plants, as its folder says: how it is run, and every plant of it
/// in order.
#[derive(Debug, Clone)]
pub struct Family {
    pub name: String,
    /// The longest one run may take.
    pub limit: Duration,
    /// What builds again what a run that builds leaves built from planted sources.
    pub rebuild: Option<Vec<String>>,
    pub plants: Vec<Plant>,
    /// The plants that do not apply to the sources yet, by name: `check` says them known
    /// rather than failing, until each is mended and taken off this list (a plant on it
    /// that applies again fails the check).
    pub known_unapplied: Vec<String>,
}

/// One bug, and how it is to be noticed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plant {
    /// The table that holds it.
    pub table: String,
    pub name: String,
    pub meant: String,
    pub runs: Vec<Run>,
    /// Whether its runs build the native `cf`, whichever of them is run.
    pub builds: bool,
    /// Commands that must succeed before it is planted.
    pub prepare: Vec<Vec<String>>,
    /// Files, from the checkout's root, that its runs take as given.
    pub requires: Vec<String>,
    /// The systems it is run on, as Rust names them (`std::env::consts::OS`): none
    /// for every one.
    pub platforms: Vec<String>,
    pub edits: Vec<Edit>,
}

/// The systems a plant may be for, as Rust names them.
const PLATFORMS: [&str; 3] = ["macos", "windows", "linux"];

/// A command to try a plant with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// Its words, the program first.
    pub command: Vec<String>,
    /// Whether it builds the native `cf` from the sources as they are.
    pub builds: bool,
}

/// A piece of text replaced in a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// From the checkout's root, with `/` between folders.
    pub file: String,
    pub find: String,
    pub replace: String,
}

/// A file with a plant in it, and what it goes back to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planted {
    /// As the plant names it.
    pub file: String,
    pub path: PathBuf,
    /// The bytes it holds now.
    pub original: Vec<u8>,
    /// The text it is to hold once planted.
    pub text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KitFile {
    run_limit_minutes: u64,
    rebuild: Option<Vec<String>>,
    tables: Vec<String>,
    #[serde(default)]
    known_unapplied: Vec<String>,
    #[serde(default)]
    run: BTreeMap<String, RunSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableFile {
    #[serde(default)]
    run: BTreeMap<String, RunSpec>,
    plant: Vec<PlantSpec>,
}

/// A run as `[run]` writes it.
#[derive(Deserialize)]
#[serde(untagged)]
enum RunSpec {
    Command(Vec<String>),
    Marked {
        command: Vec<String>,
        #[serde(default)]
        builds: bool,
    },
}

/// A run as `runs` writes it: by a name, or in words.
#[derive(Deserialize)]
#[serde(untagged)]
enum RunRef {
    Named(String),
    Command(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlantSpec {
    name: String,
    meant: String,
    runs: Vec<RunRef>,
    #[serde(default)]
    builds: bool,
    #[serde(default)]
    prepare: Vec<Vec<String>>,
    #[serde(default)]
    requires: Vec<String>,
    platforms: Option<Vec<String>>,
    edit: Vec<EditSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditSpec {
    file: String,
    find: String,
    replace: String,
}

impl From<RunSpec> for Run {
    fn from(spec: RunSpec) -> Self {
        match spec {
            RunSpec::Command(command) => Self {
                command,
                builds: false,
            },
            RunSpec::Marked { command, builds } => Self { command, builds },
        }
    }
}

/// The names of the families there is data for under `plants`: the folders that
/// hold a `kit.toml`.
pub fn families(plants: &Path) -> Result<Vec<String>, Error> {
    let mut found = Vec::new();
    for entry in fs::read_dir(plants)
        .map_err(Error::files("list", plants))?
        .flatten()
    {
        if entry.path().join("kit.toml").is_file() {
            found.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    found.sort();
    Ok(found)
}

/// Reads `path` as the data `T`, saying the file when it cannot be.
fn read<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, Error> {
    let text = fs::read_to_string(path).map_err(Error::files("read", path))?;
    toml::from_str(&text).map_err(|cause| Error::data(path, cause.to_string()))
}

impl Family {
    /// The family `name` as `plants/<name>/` holds it.
    pub fn load(plants: &Path, name: &str) -> Result<Self, Error> {
        let folder = plants.join(name);
        let kit: KitFile = read(&folder.join("kit.toml"))?;
        check_tables(&folder, &kit.tables)?;
        let shared = runs_of(kit.run);
        let mut found: Vec<Plant> = Vec::new();
        for table in &kit.tables {
            let path = folder.join(format!("{table}.toml"));
            let file: TableFile = read(&path)?;
            let own = runs_of(file.run);
            if let Some(clash) = own.keys().find(|name| shared.contains_key(*name)) {
                return Err(Error::data(
                    &path,
                    format!("the run `{clash}` is named in kit.toml too"),
                ));
            }
            for spec in file.plant {
                let plant = resolve(table, spec, &own, &shared, &path)?;
                if let Some(first) = found.iter().find(|other| other.name == plant.name) {
                    return Err(Error::data(
                        &path,
                        format!(
                            "the plant `{}` is also in {}.toml: a name says one plant",
                            plant.name, first.table
                        ),
                    ));
                }
                found.push(plant);
            }
        }
        if let Some(name) = kit
            .known_unapplied
            .iter()
            .find(|name| !found.iter().any(|plant| plant.name == name.as_str()))
        {
            return Err(Error::data(
                &folder.join("kit.toml"),
                format!("known_unapplied names `{name}`, which no plant of the family has"),
            ));
        }
        let builds = |plant: &Plant| plant.builds || plant.runs.iter().any(|run| run.builds);
        if kit.rebuild.is_none() && found.iter().any(builds) {
            return Err(Error::data(
                &folder.join("kit.toml"),
                "a plant builds the native `cf`, and no `rebuild` says how to build it again",
            ));
        }
        Ok(Self {
            name: name.to_owned(),
            limit: Duration::from_secs(kit.run_limit_minutes * 60),
            rebuild: kit.rebuild,
            plants: found,
            known_unapplied: kit.known_unapplied,
        })
    }

    /// The plants whose names hold one of `words`, or all of them for none; a
    /// word that holds in no name is no word of this family, and is told.
    pub fn choose(&self, words: &[String]) -> Result<Vec<&Plant>, Error> {
        let holds = |plant: &Plant, word: &String| plant.name.contains(word.as_str());
        if let Some(lost) = words
            .iter()
            .find(|word| !self.plants.iter().any(|plant| holds(plant, word)))
        {
            return Err(Error::NoName {
                word: lost.clone(),
                family: self.name.clone(),
            });
        }
        Ok(self
            .plants
            .iter()
            .filter(|plant| words.is_empty() || words.iter().any(|word| holds(plant, word)))
            .collect())
    }
}

impl Plant {
    /// Whether it is run on the system `os` (`std::env::consts::OS`).
    pub fn runs_on(&self, os: &str) -> bool {
        self.platforms.is_empty() || self.platforms.iter().any(|platform| platform == os)
    }

    /// Every file the plant touches, with the text it holds once the edits are
    /// made: the files in the order the plant names them first, an edit made on
    /// what the one before left. A piece of text that is not found once is the
    /// plant not applying, which is to be mended, not passed.
    pub fn planted(&self, context: &Context) -> Result<Vec<Planted>, Error> {
        let mut files: Vec<Planted> = Vec::new();
        for edit in &self.edits {
            let at = match files.iter().position(|planted| planted.file == edit.file) {
                Some(at) => at,
                None => {
                    files.push(read_file(context, &edit.file)?);
                    files.len() - 1
                }
            };
            let planted = &mut files[at];
            let found = planted.text.matches(edit.find.as_str()).count();
            if found != 1 {
                return Err(Error::NotOnce {
                    plant: self.name.clone(),
                    file: edit.file.clone(),
                    find: serde_json::to_string(&edit.find).unwrap_or_else(|_| edit.find.clone()),
                    found,
                });
            }
            planted.text = planted.text.replacen(&edit.find, &edit.replace, 1);
        }
        Ok(files)
    }
}

/// `file` of the checkout, as it is now.
fn read_file(context: &Context, file: &str) -> Result<Planted, Error> {
    let path = context.path(file);
    let original = fs::read(&path).map_err(Error::files("read", &path))?;
    let text = String::from_utf8(original.clone())
        .map_err(|_| Error::data(&path, "is not text a plant can be made in"))?;
    Ok(Planted {
        file: file.to_owned(),
        path,
        original,
        text,
    })
}

fn runs_of(specs: BTreeMap<String, RunSpec>) -> BTreeMap<String, Run> {
    specs
        .into_iter()
        .map(|(name, spec)| (name, Run::from(spec)))
        .collect()
}

/// The tables the kit lists are the files of the folder, and the folder holds no
/// other: what is listed is run, and what is run is listed.
fn check_tables(folder: &Path, listed: &[String]) -> Result<(), Error> {
    let kit = folder.join("kit.toml");
    for table in listed {
        let path = folder.join(format!("{table}.toml"));
        if !path.is_file() {
            return Err(Error::data(
                &kit,
                format!(
                    "the table `{table}` is listed, and {} is not there",
                    path.display()
                ),
            ));
        }
    }
    for entry in fs::read_dir(folder)
        .map_err(Error::files("list", folder))?
        .flatten()
    {
        let path = entry.path();
        let stem = path.file_stem().map(|stem| stem.to_string_lossy());
        let is_data = path
            .extension()
            .is_some_and(|extension| extension == "toml");
        let known = stem
            .as_deref()
            .is_some_and(|stem| stem == "kit" || listed.iter().any(|table| table == stem));
        if is_data && !known {
            return Err(Error::data(
                &kit,
                format!("{} is a table that is not listed", path.display()),
            ));
        }
    }
    Ok(())
}

/// A plant as the table writes it, its runs named by what they are.
fn resolve(
    table: &str,
    spec: PlantSpec,
    own: &BTreeMap<String, Run>,
    shared: &BTreeMap<String, Run>,
    path: &Path,
) -> Result<Plant, Error> {
    let mut runs = Vec::new();
    for run in spec.runs {
        runs.push(match run {
            RunRef::Command(command) => Run {
                command,
                builds: false,
            },
            RunRef::Named(name) => {
                own.get(&name)
                    .or_else(|| shared.get(&name))
                    .cloned()
                    .ok_or_else(|| {
                        Error::data(
                            path,
                            format!(
                                "the plant `{}` runs `{name}`, which {table}.toml and kit.toml do not name",
                                spec.name
                            ),
                        )
                    })?
            }
        });
    }
    if runs.is_empty() || runs.iter().any(|run| run.command.is_empty()) {
        return Err(Error::data(
            path,
            format!(
                "the plant `{}` has no run, or a run with no words",
                spec.name
            ),
        ));
    }
    let platforms = match spec.platforms {
        None => Vec::new(),
        Some(named) if named.is_empty() => {
            return Err(Error::data(
                path,
                format!(
                    "the plant `{}` is for no system: name one, or leave its platforms out",
                    spec.name
                ),
            ))
        }
        Some(named) => named,
    };
    let unknown = platforms
        .iter()
        .find(|platform| !PLATFORMS.contains(&platform.as_str()));
    if let Some(unknown) = unknown {
        return Err(Error::data(
            path,
            format!(
                "the plant `{}` is for `{unknown}`, which is no system a plant is for: {}",
                spec.name,
                PLATFORMS.join(", ")
            ),
        ));
    }
    Ok(Plant {
        table: table.to_owned(),
        name: spec.name,
        meant: spec.meant,
        runs,
        builds: spec.builds,
        prepare: spec.prepare,
        requires: spec.requires,
        platforms,
        edits: spec
            .edit
            .into_iter()
            .map(|edit| Edit {
                file: edit.file,
                find: edit.find,
                replace: edit.replace,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests;
