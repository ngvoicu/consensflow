//! What the tests of Specmint's detection share: a home of the test's with
//! skills installed in it, and the one rule every harness's own detection
//! (`global_specmint` of its `paths`) is held to.

use std::fs;
use std::path::Path;

use cf_base::env::Env;
use cf_base::path;
use cf_proto::specs::Variant;
use tempfile::TempDir;

/// A user's home in a folder of the test's own.
pub(crate) struct Fake {
    pub(crate) home: TempDir,
}

impl Fake {
    pub(crate) fn new() -> Self {
        Self {
            home: tempfile::tempdir().expect("a home"),
        }
    }

    /// `relative` under the home, as a path.
    pub(crate) fn at(&self, relative: &str) -> String {
        path::join(&[&self.home.path().to_string_lossy(), relative])
    }

    /// The environment of the user whose home it is, with the variables of
    /// `vars` besides, each naming a place under the home.
    pub(crate) fn env(&self, vars: &[(&str, &str)]) -> Env {
        let mut all = vec![("HOME".to_owned(), self.at(""))];
        all.extend(
            vars.iter()
                .map(|(name, relative)| ((*name).to_owned(), self.at(relative))),
        );
        Env::from_vars(all)
    }

    /// A skill named `name` in `folder` of the skills folder `root` (under the
    /// home): the file's path.
    pub(crate) fn install(&self, root: &str, folder: &str, name: &str) -> String {
        let folder = path::join(&[&self.at(root), folder]);
        fs::create_dir_all(&folder).expect("a skill's folder");
        let file = path::join(&[&folder, "SKILL.md"]);
        fs::write(
            &file,
            format!("---\nname: {name}\ndescription: >\n  A skill.\n---\n\n# {name}\n"),
        )
        .expect("a skill");
        file
    }

    /// `link` in `root` (under the home), a link to the folder `target`
    /// (under the home) holds a skill in.
    #[cfg(unix)]
    pub(crate) fn link(&self, root: &str, link: &str, target: &str) {
        fs::create_dir_all(self.at(root)).expect("a skills folder");
        std::os::unix::fs::symlink(self.at(target), path::join(&[&self.at(root), link]))
            .expect("a link");
    }
}

/// Whether `file` is there.
pub(crate) fn exists(file: &str) -> bool {
    Path::new(file).exists()
}

/// What a harness's detection of a Specmint variant is held to, whatever the
/// harness: each variant installed in a folder the harness `reads` is found
/// there, and no other variant is, by that install; the first of the folders
/// it reads that has the variant wins; the folder's name does not matter, only
/// the skill's; a link to an install is followed; and an install in a folder
/// it `ignores` is nowhere found. `vars` name the places the harness's own
/// variables point at, under the home.
pub(crate) fn is_held_to(
    detect: &dyn Fn(&Env, Variant) -> Option<String>,
    vars: &[(&str, &str)],
    reads: &[&str],
    ignores: &[&str],
) {
    for variant in Variant::ALL {
        let name = variant.skill_name();
        for root in reads {
            let fake = Fake::new();
            let env = fake.env(vars);
            let file = fake.install(root, name, name);
            assert_eq!(detect(&env, variant), Some(file), "{variant:?} in {root}");
            for other in Variant::ALL.into_iter().filter(|other| *other != variant) {
                assert_eq!(
                    detect(&env, other),
                    None,
                    "{other:?} is no {variant:?}: {root}"
                );
            }
        }
        for root in ignores {
            let fake = Fake::new();
            let env = fake.env(vars);
            fake.install(root, name, name);
            assert_eq!(
                detect(&env, variant),
                None,
                "{variant:?} in {root}, which is not read"
            );
        }
        // The folder is its own business, and the skill's name is the skill's.
        let fake = Fake::new();
        let env = fake.env(vars);
        let renamed = fake.install(reads[0], "folder-by-another-name", name);
        assert_eq!(detect(&env, variant), Some(renamed));
        let elsewhere = Fake::new();
        elsewhere.install(reads[0], name, "specmint-something-else");
        assert_eq!(
            detect(&elsewhere.env(vars), variant),
            None,
            "a folder is no name"
        );
    }
    // The first folder read that has it wins, and it is found once.
    if let [first, .., last] = reads {
        let fake = Fake::new();
        let env = fake.env(vars);
        let name = Variant::Core.skill_name();
        fake.install(last, name, name);
        let kept = fake.install(first, name, name);
        assert_eq!(detect(&env, Variant::Core), Some(kept));
    }
    // A link to an install is followed, from the folder as from the root's skill.
    #[cfg(unix)]
    {
        let fake = Fake::new();
        let env = fake.env(vars);
        let name = Variant::Tdd.skill_name();
        let target = fake.install("elsewhere", name, name);
        assert!(exists(&target));
        fake.link(reads[0], name, &format!("elsewhere/{name}"));
        assert_eq!(
            detect(&env, Variant::Tdd),
            Some(path::join(&[&fake.at(reads[0]), name, "SKILL.md"])),
            "found once, where the harness finds it"
        );
        // And the whole skills folder is a link.
        let fake = Fake::new();
        let env = fake.env(vars);
        fake.install("real-skills", name, name);
        if let Some((parent, leaf)) = reads[0].rsplit_once('/') {
            fs::create_dir_all(fake.at(parent)).expect("a parent");
            std::os::unix::fs::symlink(
                fake.at("real-skills"),
                fake.at(&format!("{parent}/{leaf}")),
            )
            .expect("a link");
            assert_eq!(
                detect(&env, Variant::Tdd),
                Some(path::join(&[&fake.at(reads[0]), name, "SKILL.md"]))
            );
        }
    }
}
