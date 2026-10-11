//! macOS pane ownership survives setsid and ordinary parent exit. Never match
//! executable names, working directories, or arbitrary command-line substrings.
//! A tree is looked at ([`ProcessTree::members`]), asked to end
//! ([`ProcessTree::ask_to_exit`]) and, last, ended ([`ProcessTree::terminate`]).
use std::collections::HashMap;
use std::io;

use crate::background::{Member, ProcessKey};

pub(crate) const OWNER_ENV: &str = "CF_PANE_PROCESS_OWNER";

#[derive(Clone, Copy, PartialEq, Eq)]
struct Identity {
    pid: i32,
    born: (u64, u64),
    unique: u64,
}

struct Process {
    identity: Identity,
    parent: u64,
    /// The process group it is in.
    group: i32,
    /// What the system calls it, for a process whose command line cannot be read.
    name: String,
}

/// The processes of a tree that a close asked to end, and is waiting for.
pub(crate) struct Asked(Vec<Identity>);

impl Asked {
    /// Whether any of them is still there: the process itself, not one that
    /// took its number.
    pub fn any_alive(&self) -> bool {
        self.0
            .iter()
            .any(|asked| process(asked.pid).map(|p| p.identity) == Some(*asked))
    }
}

// XNU proc_info_private.h declares this 56-byte structure as an API. The
// original parent's unique ID survives reparenting; a reused PID cannot match.
#[repr(C)]
#[derive(Default)]
struct NativeIdentity {
    uuid: [u8; 16],
    unique: u64,
    parent: u64,
    version: i32,
    parent_version: i32,
    reserved: [u64; 2],
}

#[derive(Clone)]
pub(crate) struct ProcessTree {
    pub marker: String,
    root: Option<Identity>,
}

impl ProcessTree {
    pub fn new() -> Self {
        let mut random = [0u8; 16];
        // SAFETY: the buffer is writable for its full declared length.
        unsafe { libc::arc4random_buf(random.as_mut_ptr().cast(), random.len()) };
        Self {
            marker: random.iter().map(|byte| format!("{byte:02x}")).collect(),
            root: None,
        }
    }

    pub fn attach(&mut self, pid: i32) {
        self.root = process(pid).map(|p| p.identity);
    }

    /// The processes of `snapshot` that are the pane's: the root, its children,
    /// whatever carries the pane's marker in its environment (a process that
    /// outlived its parent, or left its session), `owned` ones found before, and
    /// every descendant of those, by the parent each was started by. A system
    /// binary (`sleep`, `bash`, `tail`) shows no environment to anyone, so it
    /// carries no marker to be found by, and one whose parent has gone is found
    /// by the parent it was started by only where the system still names it: on
    /// macOS 26 it named launchd once launchd had taken the job (seen with a
    /// `sleep` that `sh` left behind), and nothing then finds it.
    fn select(
        &self,
        snapshot: &[Process],
        owned: &HashMap<i32, Identity>,
    ) -> HashMap<i32, Identity> {
        let mut selected = HashMap::new();
        for p in snapshot {
            if self.root == Some(p.identity)
                || self.root.is_some_and(|root| p.parent == root.unique)
                || owned.get(&p.identity.pid) == Some(&p.identity)
                || self.carries_marker(p)
            {
                selected.insert(p.identity.pid, p.identity);
            }
        }
        loop {
            let before = selected.len();
            for p in snapshot {
                if selected
                    .values()
                    .chain(owned.values())
                    .any(|parent| parent.unique == p.parent)
                {
                    selected.insert(p.identity.pid, p.identity);
                }
            }
            if selected.len() == before {
                return selected;
            }
        }
    }

    /// Whether `p` carries the pane's marker. One that began before the root
    /// cannot have got it: the marker was made with the pane.
    fn carries_marker(&self, p: &Process) -> bool {
        self.root.is_none_or(|root| p.identity.born >= root.born)
            && has_marker(p.identity.pid, &self.marker)
    }

    /// The processes of the tree now, each with the command line it was
    /// started with: a look, nothing is signalled or stopped.
    pub fn members(&self) -> io::Result<Vec<Member>> {
        let snapshot = processes()?;
        let selected = self.select(&snapshot, &HashMap::new());
        Ok(snapshot
            .iter()
            .filter(|p| selected.get(&p.identity.pid) == Some(&p.identity))
            .map(|p| Member {
                key: ProcessKey {
                    pid: p.identity.pid.unsigned_abs(),
                    began: p.identity.born.0 * 1_000_000 + p.identity.born.1,
                },
                command: procargs(p.identity.pid)
                    .and_then(|bytes| command_line(&bytes))
                    .unwrap_or_else(|| p.name.clone()),
            })
            .collect())
    }

    /// Asks the tree to end as a closing terminal does: TERM, then HUP, to
    /// every process of it outside `group`, the pane's own process group,
    /// which the caller signals whole. Nothing is stopped first: a stopped
    /// process cannot answer a signal. The processes asked.
    pub fn ask_to_exit(&self, group: Option<i32>) -> io::Result<Asked> {
        let snapshot = processes()?;
        let selected = self.select(&snapshot, &HashMap::new());
        let outside: Vec<Identity> = snapshot
            .iter()
            .filter(|p| selected.get(&p.identity.pid) == Some(&p.identity))
            .filter(|p| Some(p.group) != group)
            .map(|p| p.identity)
            .collect();
        let mut failure = None;
        for asking in [libc::SIGTERM, libc::SIGHUP] {
            for identity in &outside {
                if let Err(error) = signal(*identity, asking) {
                    failure = Some(error);
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(Asked(outside)),
        }
    }

    pub fn terminate(&self) -> io::Result<()> {
        let mut owned = HashMap::new();
        let mut failure = None;
        // Freeze proven parents before rescanning, so they cannot keep forking
        // through shutdown. Always kill already frozen processes on an error.
        for pass in 0..16 {
            let snapshot = match processes() {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    failure = Some(error);
                    break;
                }
            };
            let selected = self.select(&snapshot, &owned);
            let fresh: Vec<_> = selected
                .values()
                .filter(|p| owned.get(&p.pid) != Some(p))
                .copied()
                .collect();
            if fresh.is_empty() {
                break;
            }
            for identity in fresh {
                if let Err(error) = signal(identity, libc::SIGSTOP) {
                    failure = Some(error);
                }
                owned.insert(identity.pid, identity);
            }
            if pass == 15 {
                failure = Some(io::Error::other("pane process tree did not stabilize"));
            }
        }
        for identity in owned.values() {
            if let Err(error) = signal(*identity, libc::SIGKILL) {
                failure = Some(error);
            }
        }
        // A failed process enumeration must still stop the exact root we own.
        if let Some(root) = self.root {
            if let Err(error) = signal(root, libc::SIGKILL) {
                failure = Some(error);
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn process(pid: i32) -> Option<Process> {
    if pid <= 1 || pid == std::process::id() as i32 {
        return None;
    }
    // SAFETY: proc_pidinfo writes a fixed-size native struct into this buffer.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of_val(&info);
    // SAFETY: the buffer is `info`, writable for the `size` bytes passed with
    // it, and proc_pidinfo writes no more than that.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size as i32,
        )
    };
    if read != size as i32
        || info.pbi_pid != pid as u32
        // SAFETY: getuid takes no arguments, reads no memory of ours and
        // cannot fail.
        || info.pbi_uid != unsafe { libc::getuid() }
        || info.pbi_status == libc::SZOMB
    {
        return None;
    }
    let mut native = NativeIdentity::default();
    let size = std::mem::size_of_val(&native);
    // SAFETY: flavor 17 returns the exact native 56-byte identity structure.
    let read = unsafe {
        libc::proc_pidinfo(
            pid,
            17,
            0,
            (&mut native as *mut NativeIdentity).cast(),
            size as i32,
        )
    };
    if read != size as i32 || native.unique == 0 {
        return None;
    }
    Some(Process {
        identity: Identity {
            pid,
            born: (info.pbi_start_tvsec, info.pbi_start_tvusec),
            unique: native.unique,
        },
        parent: native.parent,
        group: info.pbi_pgid as i32,
        name: name_of(&info),
    })
}

/// What the system calls a process: its long name, or the short one.
fn name_of(info: &libc::proc_bsdinfo) -> String {
    let long = info.pbi_name.map(|byte| byte as u8);
    let short = info.pbi_comm.map(|byte| byte as u8);
    let chosen = if long[0] == 0 { &short[..] } else { &long[..] };
    let end = chosen
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(chosen.len());
    String::from_utf8_lossy(&chosen[..end]).into_owned()
}

fn signal(identity: Identity, signal: i32) -> io::Result<()> {
    // Do not act on a PID reused since discovery, including the original root.
    if process(identity.pid).map(|p| p.identity) != Some(identity) {
        return Ok(());
    }
    // SAFETY: kill takes two integers and touches no memory of ours; the pid
    // was checked above to still be the process this tree discovered.
    if unsafe { libc::kill(identity.pid, signal) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

fn processes() -> io::Result<Vec<Process>> {
    for _ in 0..3 {
        // SAFETY: a null buffer of size 0 only asks how many processes there
        // are; nothing is written.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        if count <= 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pids = vec![0i32; count as usize + 256];
        let size = (pids.len() * std::mem::size_of::<i32>()) as i32;
        // SAFETY: the buffer is `pids`, writable for the `size` bytes passed
        // with it, and proc_listallpids writes no more than that.
        let read = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), size) };
        if read <= 0 {
            return Err(io::Error::last_os_error());
        }
        if read as usize >= pids.len() {
            continue;
        }
        return Ok(pids[..read as usize]
            .iter()
            .filter_map(|pid| process(*pid))
            .collect());
    }
    Err(io::Error::other("could not snapshot pane processes"))
}

fn has_marker(pid: i32, marker: &str) -> bool {
    // No arguments or environment values are logged, retained, or returned to
    // the app: the environment is only read for the marker.
    procargs(pid).is_some_and(|bytes| environment_has_marker(&bytes, marker))
}

/// What the system keeps of a process's arguments and environment
/// (`KERN_PROCARGS2`): the argument count, the program's path, and the
/// arguments and the environment, each ended by a NUL. None for a process that
/// cannot be read (it ended, or it is not ours to read).
fn procargs(pid: i32) -> Option<Vec<u8>> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // macOS bounds argv + environment by ARG_MAX.
    let mut bytes = vec![0u8; 1024 * 1024];
    let mut size = bytes.len();
    // SAFETY: `mib` holds the `mib.len()` name entries passed with it, and
    // `bytes` is writable for the `size` bytes passed with it, which sysctl
    // lowers to what it wrote. Nothing is set: the new value is null, of length 0.
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            bytes.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return None;
    }
    bytes.truncate(size);
    Some(bytes)
}

/// The fields after the program's path and its padding: the arguments, as many
/// as the count says, and then the environment, as far as the first empty field.
fn fields_of(bytes: &[u8]) -> Option<(usize, impl Iterator<Item = &[u8]> + '_)> {
    let count = bytes
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .map(i32::from_ne_bytes)?;
    if count <= 0 || count as usize > bytes.len() {
        return None;
    }
    let tail = &bytes[4..];
    let executable_end = tail.iter().position(|b| *b == 0)?;
    let tail = &tail[executable_end..];
    let arguments_start = tail.iter().position(|b| *b != 0)?;
    Some((
        count as usize,
        tail[arguments_start..].split(|b: &u8| *b == 0),
    ))
}

fn environment_has_marker(bytes: &[u8], marker: &str) -> bool {
    let Some((count, mut fields)) = fields_of(bytes) else {
        return false;
    };
    for _ in 0..count {
        if fields.next().is_none() {
            return false;
        }
    }
    let expected = format!("{OWNER_ENV}={marker}");
    fields
        .take_while(|field| !field.is_empty())
        .any(|field| field == expected.as_bytes())
}

/// The command line a process was started with: its arguments, one blank
/// apart. None where the arguments cannot be read or there are none.
fn command_line(bytes: &[u8]) -> Option<String> {
    let (count, fields) = fields_of(bytes)?;
    let arguments: Vec<String> = fields
        .take(count)
        .map(|argument| String::from_utf8_lossy(argument).into_owned())
        .collect();
    (!arguments.is_empty()).then(|| arguments.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What `KERN_PROCARGS2` answers for `sleep 31` started with `environment`:
    /// the count, the program's path and its padding, the arguments, the
    /// environment, and the NULs that end each.
    fn procargs_of(environment: &[&str]) -> Vec<u8> {
        let mut bytes = 2i32.to_ne_bytes().to_vec();
        bytes.extend_from_slice(b"/bin/sleep\0\0\0\0\0");
        bytes.extend_from_slice(b"sleep\0");
        bytes.extend_from_slice(b"31\0");
        for variable in environment {
            bytes.extend_from_slice(variable.as_bytes());
            bytes.push(0);
        }
        bytes.push(0);
        bytes
    }

    #[test]
    fn a_command_line_is_the_arguments_the_process_was_started_with_and_not_its_environment() {
        let bytes = procargs_of(&["PATH=/usr/bin", "HOME=/Users/zeus"]);
        assert_eq!(command_line(&bytes).as_deref(), Some("sleep 31"));
        assert_eq!(command_line(&procargs_of(&[])).as_deref(), Some("sleep 31"));
    }

    #[test]
    fn a_process_the_system_shows_nothing_of_has_no_command_line() {
        assert_eq!(command_line(&[]), None);
        assert_eq!(command_line(&0i32.to_ne_bytes()), None);
        assert_eq!(command_line(b"\x02\0\0\0/bin/sleep"), None);
    }

    #[test]
    fn the_marker_is_found_among_the_environment_and_not_among_the_arguments() {
        let marker = format!("{OWNER_ENV}=abc123");
        let owned = procargs_of(&["PATH=/usr/bin", &marker]);
        assert!(environment_has_marker(&owned, "abc123"));
        assert!(!environment_has_marker(&owned, "abc124"));
        assert!(!environment_has_marker(&procargs_of(&[]), "abc123"));
        // A marker typed as an argument is no marker.
        let mut typed = 2i32.to_ne_bytes().to_vec();
        typed.extend_from_slice(b"/bin/echo\0\0\0\0echo\0");
        typed.extend_from_slice(marker.as_bytes());
        typed.extend_from_slice(b"\0\0");
        assert!(!environment_has_marker(&typed, "abc123"));
    }
}
