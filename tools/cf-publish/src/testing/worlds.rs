//! The states of GitHub and of the folders the tests start from: the bridge
//! crossed, a release published, a release about to be, several one after
//! another. Each is dropped (its simulator stopped, its folders deleted) with
//! whatever the test came to.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use super::{
    built_files, folder_of, latest_json, published_assets, quick, rule, v, Files, Github, Spec,
    TempDir, BRIDGE, OLDER, OLD_LAYOUT,
};
use crate::checks::{check_feeds, FEEDS_BEFORE};
use crate::failure::Failure;
use crate::publish::{publish_release, Means, Publication, Published};

/// The feed `feed` as a rolling prerelease that serves `body` as its latest.json.
pub fn feed(github: &Github, feed: &str, body: impl AsRef<[u8]>) {
    github.release(
        feed,
        Spec::new().prerelease().assets([("latest.json", body)]),
    );
}

/// The feed `name` serving the latest.json of the release `version`.
pub fn serving(github: &Github, name: &str, version: &str) {
    feed(github, name, latest_json(github.base(), version, "notes"));
}

/// GitHub as it is once the bridge has crossed: its release published,
/// update-alpha serving it.
pub struct Crossed {
    pub github: Github,
    pub files: Files,
}

/// The bridge `version` crossed.
pub fn crossed_as(version: &str) -> Crossed {
    let github = Github::start();
    let files = built_files(github.base(), version);
    github.release(
        &format!("v{version}"),
        Spec::new().assets(published_assets(&files).iter()),
    );
    feed(
        &github,
        "update-alpha",
        files.get("latest.json").unwrap_or_default(),
    );
    Crossed { github, files }
}

/// The bridge of the rule crossed.
pub fn crossed() -> Crossed {
    crossed_as(BRIDGE)
}

/// A release `version` built and published, its feeds moved as a run that
/// finished leaves them: the bridge crossed, and for a later release its own
/// files and feed-alpha serving it. `check` is `check_feeds` on it.
pub struct After {
    pub github: Github,
    pub files: Files,
    pub dir: TempDir,
    version: String,
}

/// `version` published; `edit` alters what was built before anything is.
pub fn after_publishing_with(version: &str, edit: impl FnOnce(Files) -> Files) -> After {
    let Crossed { github, files } = crossed();
    let mut built = files;
    if version != BRIDGE {
        built = edit(built_files(github.base(), version));
        github.release(
            &format!("v{version}"),
            Spec::new().assets(published_assets(&built).iter()),
        );
    }
    feed(
        &github,
        "feed-alpha",
        built.get("latest.json").unwrap_or_default(),
    );
    let on_disk = built.clone().with(
        "SHA256SUMS",
        published_assets(&built)
            .get("SHA256SUMS")
            .unwrap_or_default(),
    );
    After {
        github,
        files: built,
        dir: folder_of(&on_disk),
        version: version.to_string(),
    }
}

/// `version` published.
pub fn after_publishing(version: &str) -> After {
    after_publishing_with(version, |files| files)
}

impl After {
    /// What the checks find, once the test has altered GitHub.
    pub fn check(&self) -> Vec<String> {
        check_feeds(
            self.dir.path(),
            &v(&self.version),
            self.github.base(),
            &rule(),
            quick(),
        )
        .expect("the check reads the folder")
    }
}

/// GitHub before `version` is published, and the release built in a folder.
/// `publish` runs the publisher on it.
pub struct Before {
    pub github: Github,
    pub files: Files,
    pub dir: TempDir,
    version: String,
    members: Vec<String>,
    log: Arc<Mutex<Vec<String>>>,
}

/// GitHub before `version` is published: `serving` is the release update-alpha
/// serves, which for a later release is the bridge (published, and crossed),
/// and for the bridge the one before it; `members` is what the archive lists.
pub fn before_with(version: &str, serving: Option<&str>, members: &[&str]) -> Before {
    let github = Github::start();
    if version != BRIDGE {
        github.release(
            &format!("v{BRIDGE}"),
            Spec::new().assets(published_assets(&built_files(github.base(), BRIDGE)).iter()),
        );
    }
    let serving = serving.unwrap_or(if version == BRIDGE { OLDER } else { BRIDGE });
    feed(
        &github,
        "update-alpha",
        latest_json(github.base(), serving, "notes"),
    );
    let files = built_files(github.base(), version);
    Before {
        dir: folder_of(&files),
        github,
        files,
        version: version.to_string(),
        members: members.iter().map(|member| (*member).to_string()).collect(),
        log: Arc::default(),
    }
}

/// [`before_with`] the archive the old apps would accept.
pub fn before(version: &str) -> Before {
    before_with(version, None, &OLD_LAYOUT)
}

impl Before {
    /// Publishes the release, as the workflow's step does.
    pub fn publish(&self) -> Result<Published, Failure> {
        let members = self.members.clone();
        let log = Arc::clone(&self.log);
        let said = move |line: &str| log.lock().expect("the log").push(line.to_string());
        let manifest = rule();
        let tag = format!("v{}", self.version);
        let publication = Publication {
            dir: self.dir.path(),
            tag: &tag,
            base: self.github.base(),
            repo: self.github.repo(),
            manifest: &manifest,
            patience: quick(),
        };
        let means = Means {
            gh: &self.github,
            members: &move |_: &Path| Ok(members.clone()),
            log: &said,
        };
        publish_release(&publication, &means)
    }

    /// What the feeds serve afterwards, as the workflow's last step asks.
    pub fn check(&self) -> Vec<String> {
        check_feeds(
            self.dir.path(),
            &v(&self.version),
            self.github.base(),
            &rule(),
            quick(),
        )
        .expect("the check reads the folder")
    }

    /// What the publisher logged.
    pub fn log(&self) -> Vec<String> {
        self.log.lock().expect("the log").clone()
    }
}

/// Several releases published one after another, each built once into a folder
/// of its own, so that running one again has its original artifacts:
/// update-alpha serves the release before the bridge, as it does until the
/// bridge is published.
pub struct Releasing {
    pub github: Github,
    built: RefCell<BTreeMap<String, (Files, TempDir)>>,
    log: Arc<Mutex<Vec<String>>>,
}

/// A world in which releases are published one after another.
pub fn releasing() -> Releasing {
    let github = Github::start();
    feed(
        &github,
        "update-alpha",
        latest_json(github.base(), OLDER, "notes"),
    );
    Releasing {
        github,
        built: RefCell::default(),
        log: Arc::default(),
    }
}

impl Releasing {
    /// The folder release `version` is built into (made on the first ask).
    pub fn folder(&self, version: &str) -> PathBuf {
        let mut built = self.built.borrow_mut();
        let (_, dir) = built.entry(version.to_string()).or_insert_with(|| {
            let files = built_files(self.github.base(), version);
            let dir = folder_of(&files);
            (files, dir)
        });
        dir.path().to_path_buf()
    }

    /// The latest.json release `version` publishes.
    pub fn latest(&self, version: &str) -> String {
        self.folder(version);
        let built = self.built.borrow();
        built[version].0.text("latest.json")
    }

    /// Publishes `version` from its folder; what it logged is [`Releasing::log`].
    pub fn publish(&self, version: &str) -> Result<Published, Failure> {
        self.log.lock().expect("the log").clear();
        let dir = self.folder(version);
        let log = Arc::clone(&self.log);
        let said = move |line: &str| log.lock().expect("the log").push(line.to_string());
        let manifest = rule();
        let tag = format!("v{version}");
        let publication = Publication {
            dir: &dir,
            tag: &tag,
            base: self.github.base(),
            repo: self.github.repo(),
            manifest: &manifest,
            patience: quick(),
        };
        let members = |_: &Path| Ok(OLD_LAYOUT.iter().map(|name| (*name).to_string()).collect());
        let means = Means {
            gh: &self.github,
            members: &members,
            log: &said,
        };
        publish_release(&publication, &means)
    }

    /// What the feeds are asked afterwards.
    pub fn check(&self, version: &str) -> Vec<String> {
        let dir = self.folder(version);
        check_feeds(&dir, &v(version), self.github.base(), &rule(), quick())
            .expect("the check reads the folder")
    }

    /// What the publisher left `version`'s check to hold the feeds to.
    pub fn recorded(&self, version: &str) -> serde_json::Value {
        let dir = self.folder(version);
        let text = fs::read_to_string(dir.join(FEEDS_BEFORE)).expect("the record");
        serde_json::from_str(&text).expect("JSON")
    }

    /// What the last publish logged.
    pub fn log(&self) -> Vec<String> {
        self.log.lock().expect("the log").clone()
    }
}

/// What a run did to each feed, in the order it moved them, as `<feed> <what>`.
pub fn did(published: &Published) -> Vec<String> {
    published
        .feeds
        .iter()
        .map(|(feed, done)| format!("{feed} {done}"))
        .collect()
}

/// The calls that change anything.
pub fn changes(calls: &[Vec<String>]) -> Vec<Vec<String>> {
    calls
        .iter()
        .filter(|args| {
            let group = args.first().map(String::as_str);
            let verb = args.get(1).map(String::as_str);
            group == Some("api")
                || matches!(verb, Some("create" | "upload" | "delete-asset" | "edit"))
        })
        .cloned()
        .collect()
}

/// The calls that name `text` among their arguments.
pub fn mentioning(calls: &[Vec<String>], text: &str) -> Vec<Vec<String>> {
    calls
        .iter()
        .filter(|args| args.iter().any(|arg| arg == text))
        .cloned()
        .collect()
}

/// The most moments in a row, each after a `gh` call, at which a feed lacked
/// its latest.json after having had it: `had` says it had it before the first call.
pub fn gap(github: &Github, feed: &str, had: bool) -> usize {
    let (mut longest, mut run, mut seen) = (0, 0, had);
    for call in github.trace() {
        if call
            .after
            .get(feed)
            .is_some_and(|one| one.assets.iter().any(|name| name == "latest.json"))
        {
            seen = true;
            run = 0;
        } else if seen {
            run += 1;
            longest = longest.max(run);
        }
    }
    longest
}
