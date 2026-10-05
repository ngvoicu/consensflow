//! A peer on loopback a test scripts, the twin of the Node recorder's
//! `fetch`: each request answered by its route (`GET /session`) with the
//! next answer scripted for it, a head at once, no head, or held until the
//! test releases it; a body at once, cut, or held; every request kept as
//! its caller wrote it.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

use super::polling;
use crate::contract::Work;
use crate::seams::loopback::{BodyFailed, Loopback, Method, Reply, Request, FETCH_FAILED};

/// What a scripted peer answers a request with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Served {
    /// A head, its status, and its body as it comes.
    Head { status: u16, body: Sent },
    /// No head: the connection refused or broken before one.
    NoHead,
    /// When the test releases it.
    Held,
}

/// A body as a scripted peer sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sent {
    /// Whole, at once.
    Now(Vec<u8>),
    /// Broken off.
    Cut,
    /// When the test releases it.
    Held,
}

/// Where a held answer is put when it is released.
type Hold<T> = Rc<RefCell<Option<T>>>;

/// The bodies held, each of a reply to a route.
type HeldBodies = Rc<RefCell<VecDeque<Held<Result<Vec<u8>, BodyFailed>>>>>;

/// What a request or a body waits on, held: its route, the work it
/// belongs to, and where its answer is put.
struct Held<T> {
    route: String,
    work: Option<usize>,
    hold: Hold<T>,
}

/// A peer that answers each request with the next answer scripted for its
/// route. A request with no answer left gets no head, as Node's scripted
/// `fetch` threw `fetch failed`.
#[derive(Default)]
pub struct ScriptedLoopback {
    answers: RefCell<HashMap<String, VecDeque<Served>>>,
    asked: RefCell<Vec<Request>>,
    heads: Rc<RefCell<VecDeque<Held<Served>>>>,
    bodies: HeldBodies,
}

impl ScriptedLoopback {
    /// Scripts the next answers to `route`, after those scripted already.
    pub fn serve(&self, route: &str, answers: impl IntoIterator<Item = Served>) {
        self.answers
            .borrow_mut()
            .entry(route.to_owned())
            .or_default()
            .extend(answers);
    }

    /// The requests asked since the last time they were taken.
    pub fn take_asked(&self) -> Vec<Request> {
        std::mem::take(&mut self.asked.borrow_mut())
    }

    /// The routes with answers scripted and not yet asked for.
    pub fn unused(&self) -> Vec<String> {
        let mut unused: Vec<String> = self
            .answers
            .borrow()
            .iter()
            .filter(|(_, left)| !left.is_empty())
            .map(|(route, _)| route.clone())
            .collect();
        unused.sort();
        unused
    }

    /// Answers the request to `route` held longest: whether one was.
    pub fn release(&self, route: &str, answer: Served) -> bool {
        release(&self.heads, route, answer)
    }

    /// Ends the body held longest of a reply to `route`: whether one was.
    pub fn release_body(&self, route: &str, body: Result<Vec<u8>, BodyFailed>) -> bool {
        release(&self.bodies, route, body)
    }

    /// The routes `work` waits on, a head or a body held, in the order
    /// asked.
    pub fn waits(&self, work: usize) -> Vec<String> {
        let heads = self.heads.borrow();
        let bodies = self.bodies.borrow();
        heads
            .iter()
            .map(|held| (held.work, &held.route))
            .chain(bodies.iter().map(|held| (held.work, &held.route)))
            .filter(|(owner, _)| *owner == Some(work))
            .map(|(_, route)| route.clone())
            .collect()
    }
}

/// Puts `answer` where the one held longest for `route` waits.
fn release<T>(held: &RefCell<VecDeque<Held<T>>>, route: &str, answer: T) -> bool {
    let mut held = held.borrow_mut();
    let Some(at) = held.iter().position(|each| each.route == route) else {
        return false;
    };
    if let Some(each) = held.remove(at) {
        *each.hold.borrow_mut() = Some(answer);
    }
    true
}

/// A request's route: its method and its URL's path.
fn route(request: &Request) -> String {
    let method = match request.method {
        Method::Get => "GET",
        Method::Post => "POST",
    };
    let path = url::Url::parse(&request.url)
        .map(|url| url.path().to_owned())
        .unwrap_or_else(|_| request.url.clone());
    format!("{method} {path}")
}

/// A held answer, once given.
struct Slot<T>(Hold<T>);

impl<T> Future for Slot<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<T> {
        match self.0.borrow_mut().take() {
            Some(answer) => Poll::Ready(answer),
            None => Poll::Pending,
        }
    }
}

/// A held thing for the work polled now.
fn hold<T>(held: &RefCell<VecDeque<Held<T>>>, route: &str) -> Slot<T> {
    let slot = Rc::new(RefCell::new(None));
    held.borrow_mut().push_back(Held {
        route: route.to_owned(),
        work: polling(),
        hold: Rc::clone(&slot),
    });
    Slot(slot)
}

impl Loopback for ScriptedLoopback {
    fn send(&self, request: Request) -> Work<'_, Result<Box<dyn Reply>, String>> {
        let route = route(&request);
        self.asked.borrow_mut().push(request);
        let next = self
            .answers
            .borrow_mut()
            .get_mut(&route)
            .and_then(VecDeque::pop_front);
        let bodies = Rc::clone(&self.bodies);
        let answer = match next {
            Some(Served::Held) => Box::pin(hold(&self.heads, &route)) as Work<'_, Served>,
            Some(served) => Box::pin(async move { served }),
            None => Box::pin(async { Served::NoHead }),
        };
        Box::pin(async move {
            match answer.await {
                Served::Head { status, body } => Ok(Box::new(ScriptedReply {
                    route,
                    status,
                    body: Some(body),
                    bodies,
                }) as Box<dyn Reply>),
                Served::NoHead | Served::Held => Err(FETCH_FAILED.to_owned()),
            }
        })
    }
}

/// A scripted reply whose head came.
struct ScriptedReply {
    route: String,
    status: u16,
    body: Option<Sent>,
    bodies: HeldBodies,
}

impl Reply for ScriptedReply {
    fn status(&self) -> u16 {
        self.status
    }

    fn body(&mut self, limit: usize) -> Work<'_, Result<Vec<u8>, BodyFailed>> {
        let body = self.body.take();
        let held = matches!(body, Some(Sent::Held)).then(|| hold(&self.bodies, &self.route));
        Box::pin(async move {
            let bytes = match (body, held) {
                (_, Some(held)) => held.await?,
                (Some(Sent::Now(bytes)), None) => bytes,
                (Some(Sent::Cut | Sent::Held) | None, None) => return Err(BodyFailed::Cut),
            };
            if bytes.len() > limit {
                return Err(BodyFailed::TooLarge);
            }
            Ok(bytes)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Driver;

    fn get(path: &str) -> Request {
        Request {
            method: Method::Get,
            url: format!("http://127.0.0.1:41000{path}?directory=a+b"),
            headers: Vec::new(),
            body: None,
        }
    }

    #[test]
    fn a_route_is_answered_in_order_and_each_request_kept_as_written() {
        let peer = Rc::new(ScriptedLoopback::default());
        peer.serve(
            "GET /session",
            [
                Served::Head {
                    status: 200,
                    body: Sent::Now(b"{}".to_vec()),
                },
                Served::NoHead,
            ],
        );
        let mut driver = Driver::default();
        let asking = Rc::clone(&peer);
        driver.begin(0, async move {
            let mut first = asking.send(get("/session")).await.unwrap();
            let body = first.body(10).await;
            let second = asking.send(get("/session")).await.err();
            let third = asking.send(get("/other")).await.err();
            (first.status(), body, second, third)
        });
        let mut settled = driver.run();
        let (op, (status, body, second, third)) = settled.pop().unwrap();
        assert_eq!((op, settled.len()), (0, 0));
        assert_eq!((status, body), (200, Ok(b"{}".to_vec())));
        assert_eq!(second.as_deref(), Some("fetch failed"));
        assert_eq!(third.as_deref(), Some("fetch failed"), "no answer left");
        let asked = peer.take_asked();
        assert_eq!(asked[0].url, "http://127.0.0.1:41000/session?directory=a+b");
        assert_eq!(asked.len(), 3);
    }

    #[test]
    fn a_held_head_and_a_held_body_wait_for_the_test_and_are_the_work_s() {
        let peer = Rc::new(ScriptedLoopback::default());
        peer.serve("GET /global/health", [Served::Held]);
        let mut driver = Driver::default();
        let asking = Rc::clone(&peer);
        driver.begin(4, async move {
            let mut reply = asking.send(get("/global/health")).await?;
            reply.body(3).await.map_err(|failed| format!("{failed:?}"))
        });
        assert!(driver.run().is_empty());
        assert_eq!(peer.waits(4), ["GET /global/health"]);
        assert!(peer.release(
            "GET /global/health",
            Served::Head {
                status: 200,
                body: Sent::Held
            }
        ));
        assert!(driver.run().is_empty());
        assert_eq!(peer.waits(4), ["GET /global/health"], "now its body");
        assert!(peer.release_body("GET /global/health", Ok(b"long".to_vec())));
        assert_eq!(driver.run(), [(4, Err("TooLarge".to_owned()))]);
        assert!(peer.waits(4).is_empty());
    }
}
