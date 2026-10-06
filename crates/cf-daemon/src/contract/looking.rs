//! Windows that are looked at once something has come: a timer has elapsed, a
//! worker thread has answered. The engine's looks of a window wait for what
//! an adapter's looks wait for in the daemon (the records' worker, a timer of
//! its polling), and the kit's fake adapter waits for nothing, so each of its
//! windows is wrapped here with a look that waits first, and then sees what
//! the agent shows at that moment, as a look the test held and let go did.

use std::cell::Cell;
use std::rc::Rc;

use cf_engine::seams::Adapters;
use cf_harness::contract::{
    Adapter, Admission, Interrupt, Launch, Observed, Pane, PaneHost, Prepared, Readiness, Window,
    Work,
};

/// What a look waits for, given the handle of the window looked at.
type Wait = dyn Fn(&str) -> Work<'static, ()>;

/// What a look waits for, by the handle of the window looked at, once armed.
pub struct Looks {
    armed: Cell<bool>,
    wait: Box<Wait>,
}

impl Looks {
    /// Looks that wait for what `wait` makes of the window's handle, once
    /// [`Looks::arm`] is called; until then they wait for nothing.
    pub fn new(wait: impl Fn(&str) -> Work<'static, ()> + 'static) -> Rc<Self> {
        Rc::new(Self {
            armed: Cell::new(false),
            wait: Box::new(wait),
        })
    }

    /// From now on a look waits.
    pub fn arm(&self) {
        self.armed.set(true);
    }
}

/// The adapters of `inner`, whose windows look as `looks` says.
pub fn looking(inner: Rc<dyn Adapters>, looks: Rc<Looks>) -> Rc<dyn Adapters> {
    Rc::new(Looking { inner, looks })
}

struct Looking {
    inner: Rc<dyn Adapters>,
    looks: Rc<Looks>,
}

impl Adapters for Looking {
    fn adapter(&self, harness: &str) -> Option<Rc<dyn Adapter>> {
        self.inner.adapter(harness).map(|adapter| {
            Rc::new(LookingAdapter {
                adapter,
                looks: Rc::clone(&self.looks),
            }) as Rc<dyn Adapter>
        })
    }
}

struct LookingAdapter {
    adapter: Rc<dyn Adapter>,
    looks: Rc<Looks>,
}

impl Adapter for LookingAdapter {
    fn prepare<'a>(&'a self, launch: &'a Launch<'a>) -> Work<'a, Result<Prepared, String>> {
        let handle = launch.handle.to_owned();
        let prepared = self.adapter.prepare(launch);
        let looks = Rc::clone(&self.looks);
        Box::pin(async move {
            let Prepared {
                argv,
                env,
                drop_env,
                native_session,
                window,
            } = prepared.await?;
            Ok(Prepared {
                argv,
                env,
                drop_env,
                native_session,
                window: Rc::new(LookingWindow {
                    window,
                    handle,
                    looks,
                }),
            })
        })
    }

    fn interrupt(&self) -> Interrupt {
        self.adapter.interrupt()
    }
}

struct LookingWindow {
    window: Rc<dyn Window>,
    handle: String,
    looks: Rc<Looks>,
}

impl Window for LookingWindow {
    fn opened(&self, pid: Option<u32>) {
        self.window.opened(pid);
    }

    fn follow(&self, session: &str) {
        self.window.follow(session);
    }

    fn started(&self) -> Work<'_, Result<Option<String>, String>> {
        self.window.started()
    }

    fn ready<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
    ) -> Work<'a, Result<Readiness, String>> {
        self.window.ready(host, pane)
    }

    fn deliver<'a>(
        &'a self,
        host: &'a dyn PaneHost,
        pane: &'a Pane,
        text: &'a str,
    ) -> Work<'a, Result<Admission, String>> {
        self.window.deliver(host, pane, text)
    }

    fn observe(&self) -> Work<'_, Result<Observed, String>> {
        let waiting = self
            .looks
            .armed
            .get()
            .then(|| (self.looks.wait)(&self.handle));
        Box::pin(async move {
            if let Some(waiting) = waiting {
                waiting.await;
            }
            self.window.observe().await
        })
    }
}
