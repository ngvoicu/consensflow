//! Loading a family and its ordered tables from their TOML files.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::Duration;

use serde::Deserialize;

use super::{resolve, Error, Family, Plant, Run};

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
pub(super) enum RunRef {
    Named(String),
    Command(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PlantSpec {
    pub(super) name: String,
    pub(super) meant: String,
    pub(super) runs: Vec<RunRef>,
    #[serde(default)]
    pub(super) builds: bool,
    #[serde(default)]
    pub(super) prepare: Vec<Vec<String>>,
    #[serde(default)]
    pub(super) requires: Vec<String>,
    pub(super) platforms: Option<Vec<String>>,
    #[serde(default)]
    pub(super) harnesses: Vec<String>,
    pub(super) edit: Vec<EditSpec>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EditSpec {
    pub(super) file: String,
    pub(super) find: String,
    pub(super) replace: String,
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
