//! What the app an update started again holds of the app it replaced: nothing but its standard
//! streams. Tauri starts the new app with a bare `Command`, which leaves it every descriptor of
//! the old app's that is not close-on-exec, for as long as it runs; the old app makes them all so
//! as it ends (`cf_process::close_all_on_exec`, in its last code before the restart).
//!
//! The case leaves the installed app two descriptors of its own, not close-on-exec, as a program
//! that starts the app may (a pipe's writing end, and a file of the case's), and asks `lsof` what
//! the old app holds once it is up and what the new one holds once it is. The new one holds
//! neither of the two, and the pipe's reading end, which the case keeps, reaches its end once the
//! old app has gone, so no process holds the writing end at all. The two stand for every
//! descriptor: the app marks all of them or says it could not. What `lsof` says of the rest is
//! not an identity, and is noted, not held against the new app: a file at the number the old app
//! had it at may be one the new app opened itself (macOS shows no open file's own identity, only
//! the file's), and a socket or other object of the system's at an address the old app's had may
//! be one the new app was given there (the system reuses the address of an object that is gone,
//! a peer such as a system service is many processes' alike, and a socket whose peer closed shows
//! none).

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{PipeReader, PipeWriter, Read};
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::mpsc;
use std::thread;

use cf_base::env::Env;
use nix::fcntl::{fcntl, FcntlArg, FdFlag};

use super::processes::Waits;
use super::{files, Error, Result};
use crate::process::{self, Invocation};

const LSOF: &str = "/usr/sbin/lsof";

/// What the app's log says when a descriptor could not be made close-on-exec as it ended.
pub const NOT_MARKED: &str = "could not mark close-on-exec";

/// One descriptor above the standard streams a process holds, as `lsof -F ftdDin` says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Held {
    pub fd: u32,
    /// `PIPE`, `unix`, `IPv4`, `systm`, `REG`, `CHR`, `KQUEUE`, ...
    pub kind: String,
    /// The object of the system's it is (a socket, a pipe), by the address the system shows:
    /// none for a file.
    pub object: Option<String>,
    /// The file it is open on, by its device and node: none for an object of the system's.
    pub file: Option<(String, u64)>,
    pub name: String,
}

impl Held {
    fn said(&self) -> String {
        format!("fd {} {} {}", self.fd, self.kind, self.name)
    }
}

/// The descriptors above 2 in `lsof -F ftdDin` output of one process.
pub fn parse_held(text: &str) -> Vec<Held> {
    let mut found = Vec::new();
    let mut current: Option<BTreeMap<char, String>> = None;
    let mut keep = |fields: BTreeMap<char, String>| {
        let Some(fd) = fields.get(&'f').and_then(|fd| fd.parse::<u32>().ok()) else {
            return;
        };
        if fd <= 2 {
            return;
        }
        let address = fields.get(&'d').filter(|d| d.starts_with("0x")).cloned();
        let file = match (fields.get(&'D'), fields.get(&'i')) {
            (Some(device), Some(node)) if address.is_none() => {
                node.parse().ok().map(|node| (device.clone(), node))
            }
            _ => None,
        };
        found.push(Held {
            fd,
            kind: fields.get(&'t').cloned().unwrap_or_default(),
            object: address,
            file,
            name: fields.get(&'n').cloned().unwrap_or_default(),
        });
    };
    for line in text.lines() {
        let mut chars = line.chars();
        let Some(field) = chars.next() else { continue };
        let value = chars.as_str().to_owned();
        match field {
            'p' => {
                if let Some(fields) = current.take() {
                    keep(fields);
                }
            }
            'f' => {
                if let Some(fields) = current.take() {
                    keep(fields);
                }
                current = Some(BTreeMap::from([('f', value)]));
            }
            _ => {
                if let Some(fields) = current.as_mut() {
                    fields.insert(field, value);
                }
            }
        }
    }
    if let Some(fields) = current.take() {
        keep(fields);
    }
    found
}

/// The descriptors above 2 the process `pid` holds now.
pub fn held_by(pid: u32) -> Result<Vec<Held>> {
    let lsof = Invocation::new(LSOF, Path::new(".")).args([
        "-n".to_owned(),
        "-P".to_owned(),
        "-p".to_owned(),
        pid.to_string(),
        "-F".to_owned(),
        "ftdDin".to_owned(),
    ]);
    let shown = process::capture(&lsof, &Env::default())?;
    ensure!(
        shown.code == 0,
        "lsof could not say what {pid} holds: {}",
        shown.stderr.trim()
    );
    Ok(parse_held(&shown.stdout))
}

/// The two descriptors the case leaves the installed app, as a program that starts it may: open,
/// and not close-on-exec in this process from [`Left::leave`] to [`Left::given`], the moment the
/// app is started.
pub struct Left {
    reader: Option<PipeReader>,
    writer: Option<PipeWriter>,
    file: Option<File>,
    /// The numbers the app holds them at: they are this process's.
    pub pipe: u32,
    pub file_fd: u32,
    node: u64,
}

impl Left {
    /// Opens them, in `folder`, close-on-exec.
    pub fn make(folder: &Path) -> Result<Self> {
        let (reader, writer) = std::io::pipe()
            .map_err(|cause| Error::new(format!("could not make a pipe: {cause}")))?;
        let path = folder.join("left-to-the-app");
        let file = File::create(&path).map_err(files("create", &path))?;
        let node = file.metadata().map_err(files("read", &path))?.ino();
        Ok(Self {
            pipe: writer.as_raw_fd().unsigned_abs(),
            file_fd: file.as_raw_fd().unsigned_abs(),
            reader: Some(reader),
            writer: Some(writer),
            file: Some(file),
            node,
        })
    }

    /// Leaves them to the next program this process starts: the app, started next.
    pub fn leave(&self) -> Result {
        let open = [
            self.writer.as_ref().map(AsRawFd::as_raw_fd),
            self.file.as_ref().map(AsRawFd::as_raw_fd),
        ];
        for fd in open.into_iter().flatten() {
            fcntl(fd, FcntlArg::F_SETFD(FdFlag::empty())).map_err(|cause| {
                Error::new(format!("could not leave fd {fd} to the app: {cause}"))
            })?;
        }
        Ok(())
    }

    /// The app has been started: this process's own copies are closed, so the app's are the only
    /// ones (and no program this process starts after is left them).
    pub fn given(&mut self) {
        self.writer = None;
        self.file = None;
    }

    /// Waits for the pipe's reading end to reach its end: no process holds the writing end.
    pub fn closed_everywhere(&mut self, waits: &Waits) -> Result {
        let Some(mut reader) = self.reader.take() else {
            return Ok(());
        };
        let (ended, end) = mpsc::channel();
        thread::spawn(move || {
            let mut rest = Vec::new();
            let _ = ended.send(reader.read_to_end(&mut rest).map(|_| ()));
        });
        waits.until(
            "no process to hold the pipe the installed app was left",
            || match end.try_recv() {
                Ok(_) => Ok(Some(())),
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => Ok(Some(())),
            },
        )
    }
}

/// Whether `was`, of the old app's, and `held`, of the new one's, may be the one socket or other
/// object of the system's: the same kind at the same address, and the same peer or name or the
/// same number (a socket whose peer closed shows none). Only noted: the system reuses addresses.
/// A pipe is not looked at here: the pipe the case left proves pipes, by reaching its end
/// ([`Left::closed_everywhere`]).
fn same_object(was: &Held, held: &Held) -> bool {
    held.kind != "PIPE"
        && held.object.is_some()
        && was.object == held.object
        && was.kind == held.kind
        && (was.name == held.name || was.fd == held.fd)
}

/// What the old app (`old`) and the new one (`new`) hold, against what the case left the old one:
/// the faults (the old app was not left the two, or the new one holds one of them), and what is
/// only noted (a file at the same number in both, an object at the same address, which `lsof`
/// cannot tell from one opened again).
pub fn compare(old: &[Held], new: &[Held], left: &Left) -> (Vec<String>, Vec<String>) {
    let mut faults = Vec::new();
    let mut notes = Vec::new();
    let pipe = old.iter().find(|held| held.fd == left.pipe);
    if !pipe.is_some_and(|held| held.kind == "PIPE") {
        faults.push(format!(
            "the installed app does not hold the pipe it was left at fd {}: the check proves nothing",
            left.pipe
        ));
    }
    let file = old.iter().find(|held| held.fd == left.file_fd);
    if !file.is_some_and(|held| {
        held.file
            .as_ref()
            .is_some_and(|(_, node)| *node == left.node)
    }) {
        faults.push(format!(
            "the installed app does not hold the file it was left at fd {}: the check proves nothing",
            left.file_fd
        ));
    }
    for held in new {
        if let Some(was) = old.iter().find(|was| same_object(was, held)) {
            notes.push(format!(
                "the new app's fd {} may be the old app's fd {}, given again or left: {} {} {}",
                held.fd,
                was.fd,
                held.kind,
                held.object.as_deref().unwrap_or_default(),
                held.name
            ));
        }
        if held
            .file
            .as_ref()
            .is_some_and(|(_, node)| *node == left.node)
        {
            faults.push(format!(
                "the new app holds the file the installed app was left ({})",
                held.said()
            ));
        } else if let Some(file) = &held.file {
            if old
                .iter()
                .any(|was| was.fd == held.fd && was.file.as_ref() == Some(file))
            {
                notes.push(format!(
                    "the same file at the same number in both apps, opened again or left: {}",
                    held.said()
                ));
            }
        }
    }
    (faults, notes)
}

#[cfg(test)]
mod tests;
