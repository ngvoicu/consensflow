//! Pi's side of its inbox, as ConsensFlow's extension keeps it: it takes each
//! record in turn, and answers it at its acknowledgement path as the test says.

use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use serde_json::{Map, Value};
use tempfile::TempDir;
use tokio::task::{spawn_local, JoinHandle};

/// How often the extension looks in its inbox.
const LOOK: Duration = Duration::from_millis(5);

/// What the extension does with a record once it has taken it.
#[derive(Clone)]
pub enum Answers {
    /// Takes it and says nothing.
    Nothing,
    /// Acknowledges it: the record's id and these fields, written at its
    /// acknowledgement path.
    Acknowledges(Value),
    /// Makes a folder where the acknowledgement goes, which nothing reads as
    /// a file.
    MakesAFolder,
}

/// An extension watching an inbox and an acknowledgement folder of a launch's
/// own, until it is finished with or dropped.
pub struct Window {
    root: TempDir,
    pub inbox: PathBuf,
    pub ack: PathBuf,
    extension: JoinHandle<()>,
    /// The first failure of the extension's own work.
    failed: Rc<RefCell<Option<String>>>,
    /// The modes the inbox, each record taken and the acknowledgement folder had, as
    /// the extension found them (Unix).
    #[cfg_attr(not(unix), allow(dead_code))]
    pub modes: Rc<RefCell<Vec<[u32; 3]>>>,
}

impl Window {
    /// Opens a window on the local set the caller is in.
    pub fn open(answers: Answers) -> Self {
        let root = tempfile::tempdir().unwrap();
        let inbox = root.path().join("inbox");
        let ack = root.path().join("ack");
        let failed = Rc::new(RefCell::new(None));
        let modes = Rc::new(RefCell::new(Vec::new()));
        let extension = spawn_local({
            let (inbox, ack, failed) = (inbox.clone(), ack.clone(), Rc::clone(&failed));
            let modes = Rc::clone(&modes);
            async move {
                let mut looks = tokio::time::interval(LOOK);
                loop {
                    looks.tick().await;
                    if let Err(cause) = take(&inbox, &ack, &answers, &modes) {
                        failed.borrow_mut().get_or_insert(cause.to_string());
                    }
                }
            }
        });
        Self {
            root,
            inbox,
            ack,
            extension,
            failed,
            modes,
        }
    }

    /// The folder the window's are in.
    pub fn root(&self) -> &Path {
        self.root.path()
    }

    /// The extension stops; a failure of its own work fails the test.
    pub fn finish(self) {
        self.extension.abort();
        assert_eq!(*self.failed.borrow(), None, "the extension failed");
    }
}

impl Drop for Window {
    fn drop(&mut self) {
        self.extension.abort();
    }
}

/// Every record in the inbox, taken and answered, then removed. An inbox that
/// is not a folder holds none.
fn take(
    inbox: &Path,
    ack: &Path,
    answers: &Answers,
    modes: &RefCell<Vec<[u32; 3]>>,
) -> io::Result<()> {
    let Ok(entries) = fs::read_dir(inbox) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().ends_with(".json") {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode =
                |at: &Path| fs::metadata(at).map_or(0, |found| found.permissions().mode() & 0o777);
            modes
                .borrow_mut()
                .push([mode(inbox), mode(&entry.path()), mode(ack)]);
        }
        #[cfg(not(unix))]
        let _ = modes;
        let record: Value = serde_json::from_slice(&fs::read(entry.path())?)?;
        let id = record["id"].as_str().unwrap_or_default();
        fs::create_dir_all(ack)?;
        let file = ack.join(format!("{id}.json"));
        match answers {
            Answers::Nothing => {}
            Answers::Acknowledges(fields) => {
                let mut acknowledgement = Map::new();
                acknowledgement.insert("id".to_owned(), Value::from(id));
                acknowledgement.extend(fields.as_object().cloned().unwrap_or_default());
                fs::write(&file, Value::Object(acknowledgement).to_string())?;
            }
            Answers::MakesAFolder => fs::create_dir_all(&file)?,
        }
        match fs::remove_file(entry.path()) {
            Err(failed) if failed.kind() != io::ErrorKind::NotFound => return Err(failed),
            _ => {}
        }
    }
    Ok(())
}
