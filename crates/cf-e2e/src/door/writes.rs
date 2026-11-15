//! The door's writers: every file or folder the live kit makes, writes, copies onto, renames
//! onto or opens for writing goes through one of these (T-1592, round 4), and each asks
//! [`super::writable`] first: a path below a folder this run made through the door
//! ([`super::made_in`]), or a file this run made new ([`made_file`], [`create_new`]), and no
//! other. What lies elsewhere (another run's folders, a login copy in use, what a person keeps in
//! the live folder, the temp folder or a config folder a caller named) is never written, however
//! the case reaches it.
//!
//! They are `std::fs`'s calls with the same shapes, so that a case that wrote with one writes
//! with the other: a refusal is an error that names the path and says why
//! ([`std::io::ErrorKind::PermissionDenied`]). The rule in `rule.rs` fails the tests where the
//! live kit's code writes another way (`std::fs::write`, `File::create`, `OpenOptions`, the plain
//! helpers of [`crate::files`], …); [`crate::files`]'s writers are for the tests and the tools
//! that are none of a live run.
//!
//! What a writer does not do: it does not take anything away (that is [`super::Door`]'s, with a
//! ground for each), and it does not stop a program the run starts from writing where it likes
//! (the stand-ins' writes are guarded the same way, by the folders of this run that were handed
//! down to them, [`super::handed_down`]; a real harness's are not the kit's).

use std::fs;
use std::io;
use std::path::Path;

use crate::{Error, Result};

/// The error a refused write is: the path, and why.
fn refused(action: &'static str, path: &Path, why: String) -> Error {
    Error::file(action, path)(io::Error::new(io::ErrorKind::PermissionDenied, why))
}

/// Whether the kit may make something at `path`: [`super::writable`], as an error that names the
/// path where it may not.
fn allowed(action: &'static str, path: &Path, folder: bool) -> Result {
    super::writable(path, folder).map_err(|why| refused(action, path, why))
}

/// Makes the folder `path` is in where it is not there: a file this run made in a folder of a
/// caller's has its folder there already, and is none of the run's to make.
fn folder_of(path: &Path) -> Result {
    match path.parent() {
        Some(folder) if !folder.is_dir() => make_dir(folder),
        _ => Ok(()),
    }
}

/// Makes the folder `path`, and the ones above it that are not there, below a folder this run
/// made (or that folder itself, made again).
pub fn make_dir(path: impl AsRef<Path>) -> Result {
    let path = path.as_ref();
    allowed("make", path, true)?;
    fs::create_dir_all(path).map_err(Error::file("make", path))
}

/// Writes `contents` to the file `path`, in a folder made for it if it is not there, over what
/// the file held: only where `path` is below a folder this run made, or is a file it made.
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result {
    let path = path.as_ref();
    allowed("write", path, false)?;
    folder_of(path)?;
    fs::write(path, contents).map_err(Error::file("write", path))
}

/// Writes `contents` to the file `path` as [`write`] does, and makes it a program a system will
/// start: executable on Unix, where a script is no program without that.
pub fn write_executable(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result {
    let path = path.as_ref();
    allowed("write", path, false)?;
    folder_of(path)?;
    executable(path, contents.as_ref()).map_err(Error::file("write", path))
}

#[cfg(unix)]
fn executable(path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    use std::os::unix::fs::OpenOptionsExt as _;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o755)
        .open(path)?;
    file.write_all(contents)
}

#[cfg(not(unix))]
fn executable(path: &Path, contents: &[u8]) -> io::Result<()> {
    fs::write(path, contents)
}

/// Copies the file `from` to `to`, in a folder made for it if it is not there, over what `to`
/// held: `from` is read, as it is, and `to` is written new, at the default mode, below a folder
/// this run made.
pub fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result {
    let (from, to) = (from.as_ref(), to.as_ref());
    allowed("copy to", to, false)?;
    let bytes = fs::read(from).map_err(Error::file("read", from))?;
    folder_of(to)?;
    fs::write(to, bytes).map_err(Error::file("write", to))
}

/// [`copy`] that gives `to` the permissions `from` has (a login file is 0600, and a copy that
/// is written new is 0644: T-841).
pub fn copy_keeping_mode(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result {
    let (from, to) = (from.as_ref(), to.as_ref());
    allowed("copy to", to, false)?;
    folder_of(to)?;
    fs::copy(from, to)
        .map(|_| ())
        .map_err(Error::file("copy", from))
}

/// [`copy_keeping_mode`] that makes a clone where the system makes one (`cp -c` on a Mac's APFS:
/// a Claude Code's program is three hundred megabytes), and a copy where it does not.
// The system's own `cp` makes the clone; the target was asked of the door first.
#[allow(clippy::disallowed_methods)]
pub fn copy_cloning(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result {
    let (from, to) = (from.as_ref(), to.as_ref());
    allowed("copy to", to, false)?;
    folder_of(to)?;
    #[cfg(target_os = "macos")]
    {
        let cloned = std::process::Command::new("/bin/cp")
            .arg("-c")
            .arg(from)
            .arg(to)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if cloned {
            return Ok(());
        }
    }
    fs::copy(from, to)
        .map(|_| ())
        .map_err(Error::file("copy", from))
}

/// Renames `from` to `to`, over what `to` held: both are below a folder this run made, for a
/// rename takes `from` away.
pub fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) -> Result {
    let (from, to) = (from.as_ref(), to.as_ref());
    allowed("rename", from, false)?;
    allowed("rename to", to, false)?;
    fs::rename(from, to).map_err(Error::file("rename", from))
}

/// Makes `path` empty, or new, for writing: the file is made, or cut to nothing where it is there.
pub fn create(path: impl AsRef<Path>) -> Result<fs::File> {
    let path = path.as_ref();
    allowed("create", path, false)?;
    fs::File::create(path).map_err(Error::file("create", path))
}

/// Opens `path` to add to, making it where it is not there.
pub fn append(path: impl AsRef<Path>) -> Result<fs::File> {
    let path = path.as_ref();
    allowed("open", path, false)?;
    fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
        .map_err(Error::file("open", path))
}

/// Makes the file `path` new, below a folder this run made: refused where anything is there.
pub fn create_new(path: impl AsRef<Path>) -> Result<fs::File> {
    let path = path.as_ref();
    allowed("create", path, false)?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(Error::file("create", path))
}

/// [`create_new`] for a file that only its owner may read (mode 600 where the system has a mode):
/// a copy of a login.
pub fn create_new_private(path: impl AsRef<Path>) -> Result<fs::File> {
    let path = path.as_ref();
    allowed("create", path, false)?;
    private(path).map_err(Error::file("create", path))
}

#[cfg(unix)]
fn private(path: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt as _;

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// Makes the file `path`, which a caller named for the run to write (`--fixture FILE`, a folder
/// it asked an export in), as a new one of this run's own: refused, before anything runs, where
/// anything is there, so that a name a caller reuses is never written over. The file is then a
/// file this run made, and [`write`] and [`append`] may go on in it. The folder it is in must be
/// there.
pub fn made_file(path: impl AsRef<Path>) -> Result<fs::File> {
    // A say-only run whose audit file cannot be used stops before it makes anything.
    super::Door::init();
    let path = path.as_ref();
    let file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|failed| {
            if failed.kind() == io::ErrorKind::AlreadyExists {
                Error::file("make", path)(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "it is there already: a run makes the file a caller names new and never writes over one, so name another",
                ))
            } else {
                Error::file("make", path)(failed)
            }
        })?;
    super::register(path);
    Ok(file)
}

/// Makes the folder `path`, which a caller named for the run to keep things in (an export, a
/// reports folder), as a new one of this run's own: refused where anything is there, so that a
/// name a caller reuses is never written into. The place it is in is made empty where it is not
/// there ([`super::make_place`]). The folder is then a folder this run made.
pub fn made_dir(path: impl AsRef<Path>) -> Result {
    // A say-only run whose audit file cannot be used stops before it makes anything.
    super::Door::init();
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        super::make_place(parent).map_err(Error::file("make", parent))?;
    }
    fs::create_dir(path).map_err(|failed| {
        if failed.kind() == io::ErrorKind::AlreadyExists {
            Error::file("make", path)(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "it is there already: a run makes the folder a caller names new and never writes into one, so name another",
            ))
        } else {
            Error::file("make", path)(failed)
        }
    })?;
    super::register(path);
    Ok(())
}

/// Gives the permissions `permissions` to `path`, a file or a folder below a folder this run
/// made, or the folder itself.
pub fn set_permissions(path: impl AsRef<Path>, permissions: fs::Permissions) -> Result {
    let path = path.as_ref();
    allowed("set the permissions of", path, path.is_dir())?;
    fs::set_permissions(path, permissions).map_err(Error::file("set the permissions of", path))
}

/// Stamps `path`, a file or a folder below a folder this run made, or the folder itself, as last
/// changed at `time`.
pub fn set_modified(path: impl AsRef<Path>, time: std::time::SystemTime) -> Result {
    let path = path.as_ref();
    allowed("stamp", path, path.is_dir())?;
    let mut open = fs::OpenOptions::new();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        // A folder is opened only with the flag that asks for it (`FILE_FLAG_BACKUP_SEMANTICS`).
        open.write(true).custom_flags(0x0200_0000);
    }
    #[cfg(not(windows))]
    open.read(true);
    open.open(path)
        .and_then(|file| file.set_modified(time))
        .map_err(Error::file("stamp", path))
}

/// Makes the folder `path`, and the ones above it that are not there, readable by their owner
/// alone (mode 700 where the system has modes): a copy of a login is in no folder another user
/// of the machine enters.
pub fn make_dir_private(path: impl AsRef<Path>) -> Result {
    let path = path.as_ref();
    allowed("make", path, true)?;
    let mut made = fs::DirBuilder::new();
    made.recursive(true);
    #[cfg(unix)]
    std::os::unix::fs::DirBuilderExt::mode(&mut made, 0o700);
    made.create(path).map_err(Error::file("make", path))
}

/// The folder `from` copied under `to` below a folder this run made: its folders and files, each
/// file with the mode it has ([`copy_keeping_mode`]: an executable a package bundles still runs),
/// a link to a file as the file it leads to, and no entry `skip` takes. `from` is only read.
pub fn copy_tree(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
    skip: &dyn Fn(&Path) -> bool,
) -> Result {
    let (from, to) = (from.as_ref(), to.as_ref());
    make_dir(to)?;
    for entry in fs::read_dir(from)
        .map_err(Error::file("read", from))?
        .flatten()
    {
        let (source, target) = (entry.path(), to.join(entry.file_name()));
        if skip(&source) {
            continue;
        }
        if entry
            .file_type()
            .map_err(Error::file("read", &source))?
            .is_dir()
        {
            copy_tree(&source, &target, skip)?;
        } else {
            copy_keeping_mode(&source, &target)?;
        }
    }
    Ok(())
}

/// Makes `link` a link to `original`, below a folder this run made.
#[cfg(unix)]
pub fn symlink(original: impl AsRef<Path>, link: impl AsRef<Path>) -> Result {
    let (original, link) = (original.as_ref(), link.as_ref());
    allowed("link", link, false)?;
    std::os::unix::fs::symlink(original, link).map_err(Error::file("link", link))
}

/// Writes `text` to the file `file` whole or not at all, for a program to say something to a case
/// that waits for the file and reads it at once: under another name, then renamed into place.
/// `fs::write` makes the file before it writes into it, and what a case reads in that moment is
/// nothing, which is no pid.
pub fn write_whole(file: impl AsRef<Path>, text: &str) -> Result {
    let file = file.as_ref();
    let mut writing = file.as_os_str().to_owned();
    writing.push(".writing");
    let writing = std::path::PathBuf::from(writing);
    write(&writing, text)?;
    rename(&writing, file)
}

/// Writes the process id of this program to `<file>.pid`, whole ([`write_whole`]): the file in
/// which a stand-in tells a case which process it is.
pub fn say_its_process(file: impl AsRef<Path>) -> Result {
    let file = file.as_ref();
    let mut named = file.as_os_str().to_owned();
    named.push(".pid");
    write_whole(Path::new(&named), &std::process::id().to_string())
}
