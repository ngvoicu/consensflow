//! Where the harness feeds are asked: the platform's HTTPS, which `cf-harness`
//! has none of ([`Network`]). It does what Node's `fetch` did of a feed and
//! nothing more: a GET, no redirect followed (one is an answer like another,
//! which [`Feed`] refuses), the head as soon as it is in and then the body a
//! chunk at a time. The rules of the feed (its five seconds, its size, its
//! words) are [`Feed`]'s, and hold whichever network answers.
//!
//! The client is reqwest on rustls, with the platform's own trust in a
//! feed's certificate, so that nothing of the system's TLS library is built or
//! linked (OpenSSL on Windows would have to be). Its crypto is ring's, handed to
//! rustls here: the updater of the app builds rustls on ring too, and a process
//! has one provider to choose from. Nothing is installed process-wide.
//!
//! Where the connection fails the words are Node's `fetch`'s: `fetch failed`
//! where none could be made, `terminated` where it closed while the body came.
//!
//! [`Feed`]: cf_harness::admin::feed::Feed

use std::cell::OnceCell;
use std::sync::Arc;

use cf_harness::admin::feed::{Answer, Network, Request, FETCH_FAILED, TERMINATED};
use cf_harness::contract::Work;
use reqwest::redirect::Policy;
use rustls::ClientConfig;
use rustls_platform_verifier::Verifier;

use crate::start::VERSION;

/// The HTTPS of the harness feeds.
#[derive(Default)]
pub struct HttpsFeed {
    /// Built when the first feed is asked, which most runs never do: reading the
    /// platform's trust is no cost of the daemon's start.
    client: OnceCell<Result<reqwest::Client, String>>,
}

impl HttpsFeed {
    /// A network that has asked no feed yet.
    pub fn new() -> Self {
        Self::default()
    }

    fn client(&self) -> Result<&reqwest::Client, String> {
        self.client
            .get_or_init(client)
            .as_ref()
            .map_err(Clone::clone)
    }
}

/// The client the feeds are asked with: no redirect followed, no proxy
/// (Node's `fetch` read none), HTTP/1.1, and ConsensFlow named as the one
/// asking, as undici named `node`.
fn client() -> Result<reqwest::Client, String> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let verifier = Verifier::new(Arc::clone(&provider)).map_err(|failed| failed.to_string())?;
    let tls = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|failed| failed.to_string())?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .redirect(Policy::none())
        .no_proxy()
        .user_agent(format!("ConsensFlow/{VERSION}"))
        .build()
        .map_err(|failed| failed.to_string())
}

impl Network for HttpsFeed {
    /// The head of the feed's answer. No time is bound here: the request is
    /// dropped when the feed's time runs out, which ends its connection.
    fn get<'a>(&'a self, request: &'a Request<'a>) -> Work<'a, Result<Box<dyn Answer>, String>> {
        debug_assert!(!request.follow_redirects, "this network follows none");
        Box::pin(async move {
            let response = self
                .client()?
                .get(request.url)
                .send()
                .await
                .map_err(|_| FETCH_FAILED.to_owned())?;
            Ok(Box::new(Reply { response }) as Box<dyn Answer>)
        })
    }
}

/// An answer whose head is in.
struct Reply {
    response: reqwest::Response,
}

impl Answer for Reply {
    fn status(&self) -> u16 {
        self.response.status().as_u16()
    }

    fn chunk(&mut self) -> Work<'_, Result<Option<Vec<u8>>, String>> {
        Box::pin(async move {
            match self.response.chunk().await {
                Ok(chunk) => Ok(chunk.map(|bytes| bytes.to_vec())),
                Err(_) => Err(TERMINATED.to_owned()),
            }
        })
    }
}

#[cfg(test)]
mod tests;
