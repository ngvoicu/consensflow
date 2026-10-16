//! The specs viewer's door: what a project's specs folder holds, whether it
//! holds a file at all (the board's Specs button), one file of it for the page
//! to show the human, and an HTML file of it opened in the browser; read and
//! never written. The folder is the one a chief's launch is given (the
//! project's, else the app's, else `.specs`: [`folder_of`]), and it is shown
//! whether the project has specs on or off, and whoever made it (Specmint run
//! outside ConsensFlow makes one too).
//!
//! Like every page operation, these are the human's alone: the app forwards
//! them from its own page, and no route of the agents' API reaches them. What
//! is read, and what is never read, is [`folder`]'s, through the [`door`]; an
//! HTML file is opened as a copy of what was read ([`copies`]), by the
//! system's opener ([`opener`]). Every look at the folder runs off the daemon's
//! executor, on tokio's blocking threads, so a slow or a network drive holds up
//! the one operation that asked, never the daemon; at most [`MAX_JOBS`] run at
//! once, for a look a dead drive holds is never given back. Whether the folder
//! holds a file is remembered for a moment ([`PRESENT_FOR`]), for the board
//! asks at each of its reads, and asked of the folder once however many ask at
//! once; an answer from memory says when the folder is read again, so the page
//! asks once more then.
//!
//! # Threat model
//!
//! The files of a specs folder are untrusted content: a repository cloned from
//! anywhere, or what an agent wrote. Whatever they hold, or are, is refused or
//! shown as text: a link, a junction, a file of two names, a FIFO, a device, a
//! name Windows reads as another (a drive, a stream, a device, a short name,
//! given by the volume or by hand), a path that climbs out; an HTML file is
//! never run in the app. The project's folder and ConsensFlow's home are the
//! human's, and a link on the way to either is theirs; below the home, what an
//! agent may have left is refused the same way: the copies for the browser are
//! made in a folder reached through no link ([`copies`]), and nothing is
//! removed or written where a link leads. That holds always, for content that
//! sits still.
//!
//! A race between the viewer's check of a name and its use of it is another
//! matter: it needs a process running as the user, changing the folder while
//! the viewer reads it. Such a process already reads and writes everything the
//! user can, the viewer's reads included, so it gains nothing by fooling the
//! viewer. Races are closed where that is cheap: each name is opened in the
//! folder above it, by that folder's descriptor on Unix and on Windows by its
//! path with every folder on the way held so none is moved aside, and read
//! through the handle that was checked; and the browser is given a copy of the
//! bytes checked, not a path that may lead elsewhere by the time it reads it.
//! The rest are accepted, for that reason: a junction set in place on a held
//! folder on Windows ([`door`]), a folder held while it is moved elsewhere on
//! its volume (its files read are those it held a moment before), the copies'
//! folders changed once they are written, for the browser reads a copy by its
//! path.

mod copies;
mod door;
mod folder;
mod names;
mod opener;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cf_base::env::Env;
use cf_base::home::config_root;
use cf_proto::ledger::ProjectView;
use cf_proto::specs::AppSpecs;
use futures_util::future::{LocalBoxFuture, Shared};
use futures_util::FutureExt as _;
use serde_json::{json, Value};

use super::agents::saved;
use super::body::{one, Body, Fields, Said};
use super::Page;

/// How long whether a folder holds a file is remembered.
const PRESENT_FOR: Duration = Duration::from_secs(2);
/// The most looks at specs folders running at once: one more is refused until
/// one ends.
pub(super) const MAX_JOBS: usize = 4;
/// How long an HTML file may take to read and copy before the opener is
/// started: the bridge gives up on an answer at 30 s, and nothing is started
/// for a page that may have stopped waiting.
const OPEN_WITHIN: Duration = Duration::from_secs(20);

/// What opens an HTML file outside ConsensFlow, given the daemon's environment
/// and the absolute path of its copy: the system's own app for it, as the
/// daemon runs ([`Specs::new`]).
pub type Opener = Rc<dyn Fn(&Env, PathBuf) -> LocalBoxFuture<'static, Result<(), String>>>;

/// A look at whether a folder holds a file, which every one asking meanwhile
/// waits for.
type Looking = Shared<LocalBoxFuture<'static, Result<bool, String>>>;

/// What the specs viewer's operations are given besides the page's: the
/// opener, and what was last found of each folder's files.
pub struct Specs {
    open: Opener,
    /// How long a folder's presence is remembered.
    window: Duration,
    /// How long an HTML file's copy is kept.
    kept_for: Duration,
    /// How long an HTML file may take to read before nothing is opened.
    open_within: Duration,
    /// Called on the blocking thread as each look at a folder begins: nothing,
    /// but where a test holds a look to see the daemon go on without it.
    before: Arc<dyn Fn() + Send + Sync>,
    /// How many looks run, each counted until its thread is done with it.
    jobs: Arc<AtomicUsize>,
    /// Whether each folder (by its path) held a file, and when that was read.
    remembered: RefCell<HashMap<PathBuf, (Instant, bool)>>,
    /// The looks at whether a folder holds a file running now, by its path.
    looking: RefCell<HashMap<PathBuf, Looking>>,
}

impl Specs {
    /// As the daemon runs: the system's own opener.
    pub fn new() -> Self {
        Self::opening_with(opener::system())
    }

    /// With `open` in place of the system's opener: what a test gives, so no
    /// test opens a browser.
    pub fn opening_with(open: Opener) -> Self {
        Self {
            open,
            window: PRESENT_FOR,
            kept_for: copies::KEPT_FOR,
            open_within: OPEN_WITHIN,
            before: Arc::new(|| {}),
            jobs: Arc::default(),
            remembered: RefCell::default(),
            looking: RefCell::default(),
        }
    }

    /// Remembering presence for `window` instead.
    #[cfg(test)]
    pub(super) fn remembering_for(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    /// Keeping each copy for `kept_for` instead.
    #[cfg(test)]
    pub(super) fn keeping_copies_for(mut self, kept_for: Duration) -> Self {
        self.kept_for = kept_for;
        self
    }

    /// Opening an HTML file read within `within` only.
    #[cfg(test)]
    pub(super) fn opening_within(mut self, within: Duration) -> Self {
        self.open_within = within;
        self
    }

    /// With `before` called as each look at a folder begins.
    #[cfg(test)]
    pub(super) fn before_each_look(mut self, before: Arc<dyn Fn() + Send + Sync>) -> Self {
        self.before = before;
        self
    }

    /// `look` run on a blocking thread, the hook first: never on the executor,
    /// and refused at once where [`MAX_JOBS`] run. It is counted from now until
    /// its thread is done, whether or not anyone waits for it then.
    fn look<T: Send + 'static>(
        &self,
        look: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> impl std::future::Future<Output = Result<T, String>> + 'static {
        let admitted = Running::admitted(&self.jobs);
        let before = Arc::clone(&self.before);
        async move {
            let running = admitted?;
            tokio::task::spawn_blocking(move || {
                let _running = running;
                before();
                look()
            })
            .await
            .map_err(|failed| format!("the specs folder could not be read: {failed}"))?
        }
    }
}

impl Default for Specs {
    fn default() -> Self {
        Self::new()
    }
}

/// A look counted among those running, until it is dropped.
struct Running(Arc<AtomicUsize>);

impl Running {
    /// A look counted, or why none more may run now.
    fn admitted(jobs: &Arc<AtomicUsize>) -> Result<Self, String> {
        let running = Self(Arc::clone(jobs));
        if jobs.fetch_add(1, Ordering::SeqCst) < MAX_JOBS {
            return Ok(running);
        }
        drop(running);
        Err(format!(
            "the specs folders are slow to answer: {MAX_JOBS} reads of them are still running. \
             Try again once they end"
        ))
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The folder the viewer shows of `project`, where `app` is what the human says
/// of specs for every project: the folder the project names, else the one the
/// app names (Settings → Specs), else `.specs`, as a chief's launch resolves it
/// ([`ProjectView::specs_setting`]), whether specs are on or off.
fn folder_of(project: &ProjectView, app: &AppSpecs) -> String {
    project
        .specs
        .as_ref()
        .and_then(|specs| specs.folder.clone())
        .unwrap_or_else(|| app.folder_or_default().to_owned())
}

/// `specs.tree`: the folder's name, whether it is there, and what it holds,
/// each folder by name, with what was cut in words where a limit was passed. A
/// folder that is not there is no refusal: it is found empty.
pub(super) async fn tree(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let (directory, name) = folder_named(page, body)?;
    let specs = page
        .specs
        .look(move || {
            let found = folder::open(&directory, &name, &|| {})?;
            let mut specs = json!({ "folder": name, "found": found.is_some(), "entries": [] });
            if let Some(found) = found {
                let tree = found.tree();
                specs["entries"] =
                    serde_json::to_value(&tree.entries).map_err(|error| error.to_string())?;
                if let Some(cut) = tree.cut {
                    specs["cut"] = json!(cut);
                }
            }
            Ok(specs)
        })
        .await?;
    one("specs", specs)
}

/// `specs.present`: whether the folder holds a file, which shows the board's
/// Specs button. It stops at the first file, and is remembered for a moment; a
/// folder that is not there, or cannot be read, holds none. An answer from
/// memory says in how many milliseconds the folder is read again
/// (`readAgainIn`), for the page to ask once more then; one read now says
/// nothing of it.
pub(super) async fn present(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let (directory, name) = folder_named(page, body)?;
    let key = directory.join(&name);
    if let Some((present, again)) = page.specs.recall(&key) {
        let mut fields = one("present", present)?;
        fields.insert(
            "readAgainIn".to_owned(),
            json!(u64::try_from(again.as_millis()).unwrap_or(u64::MAX)),
        );
        return Ok(fields);
    }
    let looking = page
        .specs
        .looking
        .borrow_mut()
        .entry(key.clone())
        .or_insert_with(|| {
            page.specs
                .look(move || {
                    Ok(folder::open(&directory, &name, &|| {})
                        .ok()
                        .flatten()
                        .is_some_and(|found| found.present()))
                })
                .boxed_local()
                .shared()
        })
        .clone();
    let answered = looking.clone().await;
    {
        let mut running = page.specs.looking.borrow_mut();
        if running.get(&key).is_some_and(|now| now.ptr_eq(&looking)) {
            running.remove(&key);
        }
    }
    let present = answered.map_err(Said)?;
    page.specs.remember(key, present);
    one("present", present)
}

/// `specs.file`: the file at `path` in the folder (relative, in slashes), its
/// text, its bytes for an image, or its size and why it is not shown.
pub(super) async fn file(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let (directory, name) = folder_named(page, body)?;
    let path = path_of(body)?;
    let shown = page
        .specs
        .look(move || in_folder(&directory, &name)?.read(&path, &|| {}))
        .await?;
    one("file", shown)
}

/// `specs.open`: the HTML file at `path` in the folder opened in the browser,
/// outside ConsensFlow. Only a regular `.html` or `.htm` file of one name, in
/// the folder and reached through no link, is opened, as a copy of what was
/// read, with the assets it names; nothing is opened where reading it took so
/// long that the page may have stopped waiting. The answer is its path.
pub(super) async fn open(page: &Page, body: Body<'_>) -> Result<Fields, Said> {
    let asked = Instant::now();
    let (directory, name) = folder_named(page, body)?;
    let path = path_of(body)?;
    let home = config_root(&page.env)
        .ok_or_else(|| Said("ConsensFlow has no home folder to copy the file into".to_owned()))?;
    let read = path.clone();
    let kept_for = page.specs.kept_for;
    let copy = page
        .specs
        .look(move || {
            let found = in_folder(&directory, &name)?.for_browser(&read, &|| {})?;
            copies::place(&home, &found, kept_for)
        })
        .await?;
    if asked.elapsed() > page.specs.open_within {
        return Err(Said(format!(
            "{path} took too long to read: it was not opened, for the page may have stopped \
             waiting. Open it again"
        )));
    }
    (page.specs.open)(&page.env, copy).await.map_err(Said)?;
    one("opened", path)
}

impl Specs {
    /// Whether the folder at `key` held a file, as read less than a moment ago,
    /// and how long until it is read again.
    fn recall(&self, key: &Path) -> Option<(bool, Duration)> {
        let remembered = self.remembered.borrow();
        let (at, present) = remembered.get(key)?;
        let again = self.window.checked_sub(at.elapsed())?;
        (!again.is_zero()).then_some((*present, again))
    }

    fn remember(&self, key: PathBuf, present: bool) {
        self.remembered
            .borrow_mut()
            .insert(key, (Instant::now(), present));
    }
}

/// The folder of the project the body names: the project's own folder, and
/// the specs folder's name in it. What the app says of specs is read from the
/// agents file as a chief's launch reads it: where it cannot be read, the app
/// names no folder.
fn folder_named(page: &Page, body: Body<'_>) -> Result<(PathBuf, String), Said> {
    let id = body.whole("project")?;
    let project = page
        .ledger
        .borrow()
        .project(id)?
        .ok_or_else(|| Said(format!("no project {id}")))?;
    let app = saved(&page.env)
        .ok()
        .and_then(|agents| agents.roster().app_specs().ok())
        .unwrap_or_default();
    let name = folder_of(&project, &app);
    Ok((PathBuf::from(project.directory), name))
}

/// The path the body names, a file of the folder.
fn path_of(body: Body<'_>) -> Result<String, Said> {
    body.get("path")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            Said(format!(
                "path names a file of the specs folder, not {}",
                body.text("path")
            ))
        })
}

/// The specs folder `name` of the project in `directory`, which is there.
fn in_folder(directory: &Path, name: &str) -> Result<folder::Folder, String> {
    folder::open(directory, name, &|| {})?
        .ok_or_else(|| format!("the project has no specs folder {name}"))
}
