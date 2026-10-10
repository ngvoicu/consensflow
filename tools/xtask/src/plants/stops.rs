//! What ends a run of plants before its end: Ctrl-C, and where the system has
//! them a terminal that is closed and a `kill`. A plant is in the sources while
//! its runs go, so a stop is not the end of this program: it is heard, the run in
//! hand is ended, and the driver puts every file back before it ends.
//!
//! tokio hears them: a handler is installed for each and nothing is blocked, so
//! the programs a run starts have no signal blocked or ignored on account of
//! this one (as they would if this program had masked them and waited). A stop is
//! kept until it is asked for; but tokio tells it to who listens only when the
//! runtime it was made in gets its turn, which is what [`Execute`](super::runs::Execute)
//! gives it.

use std::io;
use std::task::{Context, Poll};

#[cfg(unix)]
use tokio::signal::unix::{signal, Signal, SignalKind};
#[cfg(windows)]
use tokio::signal::windows::{ctrl_break, ctrl_c, ctrl_close, CtrlBreak, CtrlC, CtrlClose};

/// The stops this program is listening for.
#[derive(Debug)]
pub struct Stops {
    #[cfg(unix)]
    signals: Vec<Signal>,
    #[cfg(windows)]
    consoles: Option<Consoles>,
}

/// What a console says of a program that is to stop.
#[cfg(windows)]
#[derive(Debug)]
struct Consoles {
    ctrl_c: CtrlC,
    ctrl_break: CtrlBreak,
    ctrl_close: CtrlClose,
}

impl Stops {
    /// Starts listening for what stops this program: SIGINT, SIGTERM and SIGHUP on
    /// Unix; Ctrl-C, Ctrl-Break and a closed console on Windows. Called in the
    /// context of the runtime that will wait for them.
    pub fn listen() -> io::Result<Self> {
        #[cfg(unix)]
        {
            Self::on(&[
                SignalKind::interrupt(),
                SignalKind::terminate(),
                SignalKind::hangup(),
            ])
        }
        #[cfg(windows)]
        {
            Ok(Self {
                consoles: Some(Consoles {
                    ctrl_c: ctrl_c()?,
                    ctrl_break: ctrl_break()?,
                    ctrl_close: ctrl_close()?,
                }),
            })
        }
    }

    /// The stops that come as these signals.
    #[cfg(unix)]
    pub fn on(kinds: &[SignalKind]) -> io::Result<Self> {
        let signals = kinds
            .iter()
            .map(|kind| signal(*kind))
            .collect::<io::Result<_>>()?;
        Ok(Self { signals })
    }

    /// No stop ever: for the tests, which would otherwise take the Ctrl-C of whoever runs them.
    #[cfg(test)]
    pub fn never() -> Self {
        #[cfg(unix)]
        {
            Self {
                signals: Vec::new(),
            }
        }
        #[cfg(windows)]
        {
            Self { consoles: None }
        }
    }

    /// Whether a stop has come, as a poll for the task `cx` belongs to: it is
    /// woken when one does.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        #[cfg(unix)]
        let heard = self
            .signals
            .iter_mut()
            .any(|signal| signal.poll_recv(cx).is_ready());
        #[cfg(windows)]
        let heard = self.consoles.as_mut().is_some_and(|consoles| {
            let one = consoles.ctrl_c.poll_recv(cx).is_ready();
            let two = consoles.ctrl_break.poll_recv(cx).is_ready();
            let three = consoles.ctrl_close.poll_recv(cx).is_ready();
            one || two || three
        });
        if heard {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests;
