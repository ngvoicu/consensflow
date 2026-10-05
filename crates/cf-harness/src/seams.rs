//! What an adapter is given besides its launch: the time and its waits,
//! randomness, a free port, and the bundle ConsensFlow ships with. The
//! engine gives the system's (the types here); a test gives fakes it drives
//! by hand (`testing`, behind the `test-support` feature). Each adapter is
//! built with the ones it uses, so nothing in a window reads the system's
//! time or randomness on its own, and a test sees every wait.

use std::net::TcpListener;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use cf_base::env::Env;
use cf_base::time::{Clock, SystemClock};

use crate::contract::{Records, Work};

/// What the engine gives every adapter it builds: the environment its
/// windows run with, the records it reads them through, and the seams.
#[derive(Clone)]
pub struct Services {
    pub env: Env,
    pub records: Rc<dyn Records>,
    pub time: Rc<dyn Time>,
    pub entropy: Rc<dyn Entropy>,
    pub ports: Rc<dyn Ports>,
    pub bundle: Bundle,
}

/// Where an adapter reads the time and waits.
pub trait Time {
    /// The wall clock, in milliseconds since the epoch: what JavaScript's
    /// `Date.now()` read, for a deadline as for a time written down.
    fn wall_ms(&self) -> i64;
    /// A wait of `duration`: a poll's interval, a deadline's timer.
    fn sleep(&self, duration: Duration) -> Work<'_, ()>;
}

/// Where an adapter draws what must not repeat: a session's id, a window's
/// name, a token.
pub trait Entropy {
    /// Fills `bytes`, or says why the system would not.
    fn fill(&self, bytes: &mut [u8]) -> Result<(), String>;
}

/// Where an adapter finds a port on loopback no one listens on.
pub trait Ports {
    fn free_loopback(&self) -> Result<u16, String>;
}

/// What ConsensFlow ships beside the daemon (`src/core/pane-cf.js`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bundle {
    /// Its `bin`, first on every window's PATH.
    pub bin: PathBuf,
    /// Its native `cf`, as a process is started: a Codex window's
    /// supervisor.
    pub cf: PathBuf,
    /// Its `cf` as a window names it (in a role's text, in a hook): with
    /// forward slashes on Windows, which Git Bash keeps where it drops
    /// backslashes, and PowerShell reads alike.
    pub pane_cf: String,
}

/// A uuid drawn from `entropy`, 16 bytes with the version 4 bits set, as
/// `randomUUID` writes one.
pub fn uuid(entropy: &dyn Entropy) -> Result<String, String> {
    let mut bytes = [0; 16];
    entropy.fill(&mut bytes)?;
    Ok(uuid::Builder::from_random_bytes(bytes)
        .into_uuid()
        .to_string())
}

/// The system's time: the time of day as `SystemClock` reads it, waits on
/// the runtime's timer, which the engine's runtime drives.
pub struct SystemTime;

impl Time for SystemTime {
    fn wall_ms(&self) -> i64 {
        SystemClock.now_ms()
    }

    fn sleep(&self, duration: Duration) -> Work<'_, ()> {
        Box::pin(tokio::time::sleep(duration))
    }
}

/// The system's randomness.
pub struct SystemEntropy;

impl Entropy for SystemEntropy {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), String> {
        getrandom::fill(bytes).map_err(|failed| failed.to_string())
    }
}

/// A port the system hands out on loopback, let go at once
/// (`freeLoopbackPort`, `src/channels.js`).
pub struct LoopbackPorts;

impl Ports for LoopbackPorts {
    fn free_loopback(&self) -> Result<u16, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|failed| failed.to_string())?;
        let port = listener
            .local_addr()
            .map_err(|failed| failed.to_string())?
            .port();
        if port == 0 {
            return Err("could not choose a loopback port".to_owned());
        }
        Ok(port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream of the same byte.
    struct Same(u8);

    impl Entropy for Same {
        fn fill(&self, bytes: &mut [u8]) -> Result<(), String> {
            bytes.fill(self.0);
            Ok(())
        }
    }

    #[test]
    fn a_uuid_is_sixteen_bytes_drawn_with_the_version_4_bits_set() {
        assert_eq!(
            uuid(&Same(0)).unwrap(),
            "00000000-0000-4000-8000-000000000000"
        );
        assert_eq!(
            uuid(&Same(0xff)).unwrap(),
            "ffffffff-ffff-4fff-bfff-ffffffffffff"
        );
        let drawn = uuid(&SystemEntropy).unwrap();
        assert_eq!(drawn.len(), 36);
        assert_eq!(&drawn[14..15], "4");
    }

    #[test]
    fn a_free_port_on_loopback_is_one_a_listener_may_take() {
        let port = LoopbackPorts.free_loopback().unwrap();
        assert_ne!(port, 0);
        TcpListener::bind(("127.0.0.1", port)).unwrap();
    }
}
