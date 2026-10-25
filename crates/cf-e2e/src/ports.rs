//! A port on loopback for a program to listen on, which a case names and the
//! program binds a moment later (the broker of `cf codex-session`).
//!
//! The system names a port by a listener, let go of to give the port to the
//! program. macOS cannot open a socket close-on-exec: std opens it and makes it
//! so in a second call, and a program another thread starts between the two
//! (the cases of a suite run side by side) is left the listener, and holds the
//! port after this has let go of it: the program that is then to listen on it is
//! refused. So the port is looked at once the listener is gone, with a
//! connection to it, which a listener nothing serves takes into its backlog and
//! a port no one listens on refuses, and another is drawn where a program
//! listens on it. `cf-harness`'s `LoopbackPorts` does as much for the ports a
//! window's launch names (T-366); this crate depends on no crate of the product,
//! so it is written again here.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

/// How many ports are drawn at most before one that no program listens on is found.
const DRAWS: usize = 8;

/// How long a look at a port waits for its answer, which loopback gives at once.
const LOOK: Duration = Duration::from_millis(250);

/// A port on loopback that no program listens on.
pub fn free() -> io::Result<u16> {
    free_from(drawn, listened_on)
}

/// The first port `draw` names that `listened` does not say a program listens on.
fn free_from(
    mut draw: impl FnMut() -> io::Result<u16>,
    listened: impl Fn(u16) -> bool,
) -> io::Result<u16> {
    for _ in 0..DRAWS {
        let port = draw()?;
        if !listened(port) {
            return Ok(port);
        }
    }
    Err(io::Error::other(
        "no loopback port that no program listens on",
    ))
}

/// A port the system names on loopback, its listener let go of.
fn drawn() -> io::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port())
}

/// Whether a program listens on `port`: a connection to it is taken, where one to
/// a port no one listens on is refused. Only macOS leaves a program the listener
/// of another thread's (Linux opens a socket close-on-exec as it makes it, and
/// Windows hands nothing over, and is slow to refuse a port).
fn listened_on(port: u16) -> bool {
    cfg!(target_os = "macos")
        && TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)), LOOK).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Whether a program listens on `port`, as a connection to it says, on any system.
    fn listens(port: u16) -> bool {
        TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok()
    }

    #[test]
    fn a_port_a_program_was_left_a_listener_of_is_not_the_one_named() {
        // What the program another thread started was left of the listener that
        // named the port: it stays listened on after this has let go of it.
        let kept = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let taken = kept.local_addr().unwrap().port();
        let free = loop {
            let port = drawn().unwrap();
            if port != taken {
                break port;
            }
        };
        let mut draws = [taken, free].into_iter();
        let named = free_from(|| Ok(draws.next().unwrap()), listens).unwrap();
        assert_eq!(named, free, "the port {taken} is listened on");
        TcpListener::bind((Ipv4Addr::LOCALHOST, named)).unwrap();
    }

    #[test]
    fn no_port_is_named_when_every_one_drawn_is_listened_on() {
        let mut drawn = 0;
        let named = free_from(
            || {
                drawn += 1;
                Ok(40_000)
            },
            |_| true,
        );
        assert!(named.is_err());
        assert_eq!(drawn, DRAWS, "it gives up after the draws it has");
    }

    #[test]
    fn a_draw_the_system_refuses_is_the_failure() {
        let named = free_from(|| Err(io::Error::other("no more")), |_| false);
        assert_eq!(named.unwrap_err().to_string(), "no more");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_port_is_looked_at_by_a_connection_to_it() {
        let kept = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = kept.local_addr().unwrap().port();
        assert!(listened_on(port));
        drop(kept);
        // A program started while the listener was open may hold its copy a moment
        // more: the port is clear once that program has let go.
        let until = std::time::Instant::now() + Duration::from_secs(5);
        while listened_on(port) {
            assert!(std::time::Instant::now() < until, "{port} is listened on");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
