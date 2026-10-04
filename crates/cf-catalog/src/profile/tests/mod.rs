//! The profile's tests: the key a model is called by and the tier words
//! (`keys`), and the branches of `agentProfile`, in their order (`branches`).

use super::*;

mod branches;
mod keys;

fn catalog() -> Catalog {
    Catalog::bundled().unwrap()
}

fn preset(name: &str, kind: &str, model: &str, effort: Option<&str>) -> Preset {
    Preset {
        preset: name.to_owned(),
        id: name.to_owned(),
        name: name.to_owned(),
        label: name.to_owned(),
        description: name.to_owned(),
        kind: kind.to_owned(),
        model: model.to_owned(),
        effort: effort.map(str::to_owned),
        thinking: None,
        designer: false,
    }
}
