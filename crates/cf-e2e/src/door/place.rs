//! The write door's judge (K3, `live-kit-isolation-doors.md` section 3): every writer, allocation
//! and removal of the kit asks [`judge`] first, then acts on the path it resolved. It keeps
//! nothing between calls: the place and every protected root are resolved now, each time.
//!
//! 1. **Resolve.** A path resolves to its deepest part that is there, through every link in the
//!    order the system walks it, with the names that are not there appended. A `..` among the
//!    absent names is refused. A link that leads nowhere counts as its own name and as where it
//!    points. A resolution that fails for any reason but absence refuses.
//! 2. **Inside.** A place that resolves to a protected root or below one is refused, so a root
//!    that is not there yet protects the names it would have: nothing is made in an absent
//!    `~/.codex`. Names are compared as the file system compares them: in any case on a volume
//!    that is not case-sensitive (the default on a Mac and on Windows), the absent ones too.
//! 3. **The tree.** A removal, and both ends of a rename, are also refused where any root
//!    resolves to the tree or inside it.
//! 4. **The ground**, by the act ([`Act`]).

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// What a caller is about to do at a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Make a folder of a random name in the place: nothing more is needed than that the place
    /// is outside every root. Missing folders above it are made one at a time, each new.
    Allocate,
    /// Make the exact file or folder a caller named, new: nothing may be there.
    Name,
    /// Write a file: below a folder this run made, or a file it made and is still the same file.
    Write,
    /// Make a folder: below a folder this run made, or that folder itself, made again.
    Folder,
    /// Take away a file, a link or a folder: a folder this run made, or below one.
    Remove,
    /// One end of a rename: below a folder this run made, or a file it made, and neither end
    /// holding a root.
    Rename,
}

/// A path as the judge resolved it: its deepest part that is there, by its real path, and the
/// names after it that are not there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub there: PathBuf,
    pub absent: Vec<OsString>,
}

impl Resolved {
    /// The whole path, the absent names joined on.
    pub fn whole(&self) -> PathBuf {
        self.absent
            .iter()
            .fold(self.there.clone(), |path, name| path.join(name))
    }
}

/// How deep a chain of links is followed before the judge refuses it, as a system's own limit.
const LINKS: usize = 40;

fn refused(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, why.into())
}

/// The real path of `path`, with Windows' `\\?\` taken off.
pub(crate) fn real(path: &Path) -> io::Result<PathBuf> {
    let found = path.canonicalize()?;
    let said = found.to_string_lossy();
    Ok(match said.strip_prefix(r"\\?\") {
        Some(plain) if !plain.starts_with("UNC\\") => PathBuf::from(plain),
        _ => found,
    })
}

/// Every way `path` resolves: one, or more where a link on the way leads nowhere (its own name,
/// and where it points). An error where a name of the absent part is `..`, where a link chain is
/// too deep, or where a part cannot be read for any reason but its absence.
pub fn resolve(path: &Path) -> io::Result<Vec<Resolved>> {
    resolve_within(path, 0)
}

fn resolve_within(path: &Path, depth: usize) -> io::Result<Vec<Resolved>> {
    if depth > LINKS {
        return Err(refused(format!(
            "{} leads through more links than a system follows",
            path.display()
        )));
    }
    let path = std::path::absolute(path)?;
    let mut absent: Vec<OsString> = Vec::new();
    let mut at = path.clone();
    loop {
        match real(&at) {
            Ok(there) => {
                absent.reverse();
                // A name below a file is no place, whatever the system calls its absence (Windows
                // says a path through a file is not found).
                if !absent.is_empty() && !there.is_dir() {
                    return Err(refused(format!(
                        "{} is a file, and {} is no place below it",
                        there.display(),
                        path.display()
                    )));
                }
                return Ok(vec![Resolved { there, absent }]);
            }
            Err(failed) if failed.kind() == io::ErrorKind::NotFound => {}
            Err(failed) => {
                return Err(refused(format!(
                    "{} cannot be resolved ({failed}), and a place that cannot be is no place to act",
                    at.display()
                )))
            }
        }
        match fs::symlink_metadata(&at) {
            Ok(found) if found.file_type().is_symlink() => {
                // A link that leads nowhere: its own name, and where it points.
                let parent = at.parent().unwrap_or(&at).to_path_buf();
                let name = at.file_name().map(OsString::from).unwrap_or_default();
                let mut rest = absent.clone();
                rest.reverse();
                let mut ways = Vec::new();
                for mut own in resolve_within(&parent, depth + 1)? {
                    own.absent.push(name.clone());
                    own.absent.extend(rest.iter().cloned());
                    ways.push(own);
                }
                let target = fs::read_link(&at)?;
                let target = if target.is_absolute() {
                    target
                } else {
                    parent.join(target)
                };
                let target = rest.iter().fold(target, |path, name| path.join(name));
                ways.extend(resolve_within(&target, depth + 1)?);
                return Ok(ways);
            }
            Ok(_) => {
                return Err(refused(format!(
                    "{} is there and cannot be resolved",
                    at.display()
                )))
            }
            Err(failed) if failed.kind() == io::ErrorKind::NotFound => {}
            Err(failed) => {
                return Err(refused(format!(
                    "{} cannot be read ({failed})",
                    at.display()
                )))
            }
        }
        let name = match at.components().next_back() {
            Some(Component::Normal(name)) => name.to_os_string(),
            Some(Component::CurDir) => OsString::new(),
            Some(Component::ParentDir) => {
                return Err(refused(format!(
                    "{} leads up out of the folder it names",
                    path.display()
                )))
            }
            _ => {
                return Err(refused(format!(
                    "{} has no part that is there",
                    path.display()
                )))
            }
        };
        if !name.is_empty() {
            absent.push(name);
        }
        at = match at.parent() {
            Some(parent) => parent.to_path_buf(),
            None => {
                return Err(refused(format!(
                    "{} has no part that is there",
                    path.display()
                )))
            }
        };
    }
}

/// Whether names are compared in any case below `folder`, a folder that is there: as its volume
/// compares them, read by whether another spelling of a name of it leads to the same file; the
/// platform's default (any case on a Mac and on Windows) where no name of it has a letter.
pub fn any_case_at(folder: &Path) -> bool {
    let mut at = folder.to_path_buf();
    loop {
        let Some(name) = at.file_name().and_then(|name| name.to_str()) else {
            break;
        };
        let other: String = name
            .chars()
            .map(|letter| {
                if letter.is_lowercase() {
                    letter.to_uppercase().next().unwrap_or(letter)
                } else {
                    letter.to_lowercase().next().unwrap_or(letter)
                }
            })
            .collect();
        if other != name {
            let spelled = at.with_file_name(other);
            return match (super::identity::of_path(&at), super::identity::of_path(&spelled)) {
                (Ok(one), Ok(other)) => one == other,
                _ => false,
            };
        }
        match at.parent() {
            Some(parent) => at = parent.to_path_buf(),
            None => break,
        }
    }
    cfg!(any(windows, target_os = "macos"))
}

/// The names of `path`, each folded where `any_case`.
fn names(path: &Path, any_case: bool) -> Vec<String> {
    path.components()
        .map(|part| {
            let name = part.as_os_str().to_string_lossy();
            if any_case {
                name.to_lowercase()
            } else {
                name.into_owned()
            }
        })
        .collect()
}

/// Whether `inner` is `outer` or below it, by whole names, in any case where `any_case`.
pub fn within(inner: &Path, outer: &Path, any_case: bool) -> bool {
    let (inner, outer) = (names(inner, any_case), names(outer, any_case));
    inner.len() >= outer.len() && inner[..outer.len()] == outer[..]
}

/// What the run made, as the judge asks it: a folder, or a file and its identity.
pub trait Registry {
    /// Whether `real` is a folder this run made.
    fn is_made_folder(&self, real: &Path, any_case: bool) -> bool;
    /// Whether `real` is below a folder this run made.
    fn is_below_made(&self, real: &Path, any_case: bool) -> bool;
    /// Whether `real` is a file this run made and is still the same file.
    fn is_made_file(&self, real: &Path, any_case: bool) -> bool;
}

/// Judges `path` for `act`, with the roots `protected` (as spelled) and what the run made
/// (`made`): the place resolved, the one to act on, or why not.
pub fn judge_with(
    path: &Path,
    act: Act,
    protected: &[PathBuf],
    made: &dyn Registry,
) -> io::Result<PathBuf> {
    let last_is_link = fs::symlink_metadata(path).is_ok_and(|found| found.file_type().is_symlink());
    // A removal takes a link itself: it is judged by its own name.
    let ways = if act == Act::Remove && last_is_link {
        let parent = path.parent().unwrap_or(path);
        let name = path.file_name().map(OsString::from).unwrap_or_default();
        let mut ways = resolve(parent)?;
        for way in &mut ways {
            way.absent.push(name.clone());
        }
        ways
    } else {
        resolve(path)?
    };
    if last_is_link && !matches!(act, Act::Remove | Act::Allocate) {
        return Err(refused(format!(
            "{} is a link, and a write goes through a link to what it leads to",
            path.display()
        )));
    }
    let any_case = ways.first().is_some_and(|way| any_case_at(&way.there));
    let mut roots = Vec::new();
    for root in protected {
        // A root that cannot be resolved is held as spelled, and as nothing less.
        match resolve(root) {
            Ok(found) => roots.extend(found.iter().map(Resolved::whole)),
            Err(_) => roots.push(std::path::absolute(root).unwrap_or_else(|_| root.clone())),
        }
        roots.push(std::path::absolute(root).unwrap_or_else(|_| root.clone()));
    }
    for way in &ways {
        let place = way.whole();
        if let Some(root) = roots.iter().find(|root| within(&place, root, any_case)) {
            return Err(refused(format!(
                "{} is inside {}, a store of a harness or of the account, which no run writes",
                place.display(),
                root.display()
            )));
        }
        if matches!(act, Act::Remove | Act::Rename) {
            if let Some(root) = roots.iter().find(|root| within(root, &place, any_case)) {
                return Err(refused(format!(
                    "{} holds {}, a store of a harness or of the account, which no run takes away or moves",
                    place.display(),
                    root.display()
                )));
            }
        }
    }
    let place = ways
        .first()
        .map(Resolved::whole)
        .ok_or_else(|| refused(format!("{} resolves nowhere", path.display())))?;
    let grounded = |place: &Path| match act {
        Act::Allocate => true,
        Act::Name => fs::symlink_metadata(path).is_err(),
        Act::Write | Act::Rename => {
            made.is_below_made(place, any_case) || made.is_made_file(place, any_case)
        }
        Act::Folder => made.is_below_made(place, any_case) || made.is_made_folder(place, any_case),
        Act::Remove => made.is_made_folder(place, any_case) || made.is_below_made(place, any_case),
    };
    if !ways.iter().all(|way| grounded(&way.whole())) {
        return Err(refused(match act {
            Act::Name => format!(
                "{} is there already: a run makes the file or folder a caller names new and never writes over one, so name another",
                path.display()
            ),
            Act::Remove => format!(
                "{} is no folder this run made and is below none",
                place.display()
            ),
            _ => format!(
                "{} is below no folder this run made, so it is another run's, a person's, or nobody's",
                place.display()
            ),
        }));
    }
    Ok(place)
}
