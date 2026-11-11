//! What the window's program writes to ConsensFlow's log, `daemon.log` in the home, where its
//! terminal says less: a path under the home can name a credential, so a failure that has one says
//! on the terminal that there is a cause, and the log has the cause. A line is in the daemon's own
//! form (`cf-daemon`'s `files::log`, which this program does not depend on): the time, the level
//! and the message, with the cause indented under it. Never a reason for the window to fail: a home
//! that cannot be written loses the line.

use std::path::Path;

use cf_base::env::Env;
use cf_base::file::append_rotating;
use cf_base::home::config_root;
use cf_base::time::{iso, Clock, SystemClock};

/// How big the log may get before it is moved aside, as the daemon keeps it: 5 MB.
const LIMIT: u64 = 5_000_000;

/// Writes a warning to the log of the home `env` names, with the system's `cause` under it.
pub(crate) fn warn(env: &Env, message: &str, cause: &str) {
    if let Some(home) = config_root(env) {
        write(&home, "warn", message, Some(cause));
    }
}

/// Writes a thing worth knowing afterwards, in one line, to the log of `home`.
pub(crate) fn info_in(home: &Path, message: &str) {
    write(home, "info", message, None);
}

fn write(home: &Path, level: &str, message: &str, cause: Option<&str>) {
    let at = iso(SystemClock.now_ms());
    let mut lines = vec![format!("{at} {level} {message}")];
    if let Some(cause) = cause {
        lines.extend(cause.split('\n').map(|line| format!("    {line}")));
    }
    let _ = append_rotating(
        &home.join("daemon.log"),
        &format!("{}\n", lines.join("\n")),
        LIMIT,
    );
}
