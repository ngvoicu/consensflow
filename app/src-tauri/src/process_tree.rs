//! macOS pane ownership survives setsid and ordinary parent exit. Never match
//! executable names, working directories, or arbitrary command-line substrings.
use std::collections::HashMap;
use std::io;

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
            let mut selected = HashMap::new();
            for p in &snapshot {
                if self.root == Some(p.identity)
                    || self.root.is_some_and(|root| p.parent == root.unique)
                    || owned.get(&p.identity.pid) == Some(&p.identity)
                    || has_marker(p.identity.pid, &self.marker)
                {
                    selected.insert(p.identity.pid, p.identity);
                }
            }
            loop {
                let before = selected.len();
                for p in &snapshot {
                    if selected
                        .values()
                        .chain(owned.values())
                        .any(|parent| parent.unique == p.parent)
                    {
                        selected.insert(p.identity.pid, p.identity);
                    }
                }
                if selected.len() == before {
                    break;
                }
            }
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
    })
}

fn signal(identity: Identity, signal: i32) -> io::Result<()> {
    // Do not act on a PID reused since discovery, including the original root.
    if process(identity.pid).map(|p| p.identity) != Some(identity) {
        return Ok(());
    }
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
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        if count <= 0 {
            return Err(io::Error::last_os_error());
        }
        let mut pids = vec![0i32; count as usize + 256];
        let size = (pids.len() * std::mem::size_of::<i32>()) as i32;
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
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    // macOS bounds argv + environment by ARG_MAX. No arguments or environment
    // values are logged, retained, or returned to the app.
    let mut bytes = vec![0u8; 1024 * 1024];
    let mut size = bytes.len();
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
        return false;
    }
    environment_has_marker(&bytes[..size], marker)
}

fn environment_has_marker(bytes: &[u8], marker: &str) -> bool {
    let Some(count) = bytes
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .map(i32::from_ne_bytes)
    else {
        return false;
    };
    if count <= 0 || count as usize > bytes.len() {
        return false;
    }
    let tail = &bytes[4..];
    let Some(executable_end) = tail.iter().position(|b| *b == 0) else {
        return false;
    };
    let tail = &tail[executable_end..];
    let Some(arguments_start) = tail.iter().position(|b| *b != 0) else {
        return false;
    };
    let mut fields = tail[arguments_start..].split(|b| *b == 0);
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
