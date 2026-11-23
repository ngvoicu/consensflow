//! The terminal door (K3, N4 of T-2086): a program run on a pseudo-terminal of the kit's own, as
//! Muse's direct window is. It is a start like any other: it is made from a [`crate::process::Run`]
//! (`Run::terminal`), whose environment holds the seal and whose program is admitted by the file
//! it resolves to, so a window on a pty has the run's homes and the update switches of a
//! harness. The pty itself is made here and nowhere else in the kit's code: the audit finds
//! `native_pty_system`, `CommandBuilder`, `openpty` and `spawn_command` anywhere outside `door/`.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::{Error, Result};

/// How much of what the program drew is kept: the last of it.
const KEPT: usize = 200_000;

/// A program on a pty: its screen's bytes kept, its keys written.
pub struct Terminal {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
    screen: Arc<Mutex<Vec<u8>>>,
}

fn failed(action: &'static str) -> impl Fn(portable_pty::ExtError) -> Error {
    move |why| Error::Daemon(format!("could not {action}: {why}"))
}

/// Starts `program` with `args` in `cwd` on a pty of `rows` and `cols`, on `env` alone.
pub(crate) fn open(
    program: &OsString,
    args: &[OsString],
    env: &[(OsString, OsString)],
    cwd: Option<&Path>,
    (rows, cols): (u16, u16),
) -> Result<Terminal> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(failed("open a pty"))?;
    let mut command = CommandBuilder::new(program);
    command.args(args);
    command.env_clear();
    for (name, value) in env {
        command.env(name, value);
    }
    if let Some(cwd) = cwd {
        command.cwd(cwd);
    }
    let child = pair
        .slave
        .spawn_command(command)
        .map_err(failed("start the program on the pty"))?;
    let mut reader = pair.master.try_clone_reader().map_err(failed("read the pty"))?;
    let writer = pair.master.take_writer().map_err(failed("write to the pty"))?;
    let screen = Arc::new(Mutex::new(Vec::new()));
    let kept = Arc::clone(&screen);
    thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        while let Ok(read) = reader.read(&mut chunk) {
            if read == 0 {
                break;
            }
            let Ok(mut screen) = kept.lock() else {
                break;
            };
            screen.extend_from_slice(&chunk[..read]);
            let len = screen.len();
            if len > KEPT {
                screen.drain(..len - KEPT);
            }
        }
    });
    Ok(Terminal {
        child,
        writer,
        screen,
    })
}

impl Terminal {
    /// Types `bytes`.
    pub fn keys(&mut self, bytes: &[u8]) {
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
    }

    /// The last `bytes` the program drew, as text.
    pub fn tail(&self, bytes: usize) -> String {
        let Ok(screen) = self.screen.lock() else {
            return String::new();
        };
        let from = screen.len().saturating_sub(bytes);
        String::from_utf8_lossy(&screen[from..]).into_owned()
    }

    /// Whether the program has ended.
    pub fn ended(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(Some(_)))
    }

    /// Ends the program: for the door's closure of the kit's own terminal
    /// ([`crate::door::Door::close_own_terminal`]), the only way to.
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }
}
