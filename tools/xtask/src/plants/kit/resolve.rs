//! Resolve the runs a plant names, in the order it names them.

use std::collections::BTreeMap;
use std::path::Path;

use super::load::RunRef;
use super::{Error, Run};

pub(super) fn runs(
    specs: Vec<RunRef>,
    own: &BTreeMap<String, Run>,
    shared: &BTreeMap<String, Run>,
    table: &str,
    plant: &str,
    path: &Path,
) -> Result<Vec<Run>, Error> {
    let mut runs = Vec::new();
    for run in specs {
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
                                "the plant `{plant}` runs `{name}`, which {table}.toml and kit.toml do not name"
                            ),
                        )
                    })?
            }
        });
    }
    if runs.is_empty() || runs.iter().any(|run| run.command.is_empty()) {
        return Err(Error::data(
            path,
            format!("the plant `{plant}` has no run, or a run with no words"),
        ));
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_and_inline_runs_keep_their_order_and_build_flags() {
        let own = BTreeMap::from([(
            "table".into(),
            Run {
                command: vec!["table-command".into()],
                builds: true,
            },
        )]);
        let shared = BTreeMap::from([(
            "family".into(),
            Run {
                command: vec!["family-command".into()],
                builds: false,
            },
        )]);
        let specs = vec![
            RunRef::Named("family".into()),
            RunRef::Command(vec!["inline-command".into()]),
            RunRef::Named("table".into()),
        ];
        let found = runs(
            specs,
            &own,
            &shared,
            "cases",
            "plant",
            Path::new("cases.toml"),
        )
        .unwrap();
        assert_eq!(
            found,
            vec![
                Run {
                    command: vec!["family-command".into()],
                    builds: false
                },
                Run {
                    command: vec!["inline-command".into()],
                    builds: false
                },
                Run {
                    command: vec!["table-command".into()],
                    builds: true
                },
            ]
        );
    }
}
