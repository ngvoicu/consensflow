//! What a run's Pi is given of the owner's `auth.json`: the entries of the providers its
//! model needs, and only the ones that cannot be refreshed in the run.
//!
//! Pi keeps one credential per provider in `<agent dir>/auth.json`: an API key
//! (`{"type": "api_key", "key": …}`) or an OAuth login (`{"type": "oauth", "access": …,
//! "refresh": …, "expires": <ms since the epoch>}`). Pi uses an OAuth entry as it is
//! until `expires` has passed, then exchanges the refresh token for new ones and writes
//! them back. A refresh token is used once: if the run's copy refreshes, the owner's
//! file holds a token the provider no longer honours, and they are logged out of that
//! provider the next time they use it (T-603, T-696: a live run refreshed an expired
//! `openai` entry of the owner's). So an OAuth entry is copied only if it lasts as long
//! as the run does and a margin; one that does not is a reason to refuse the run, said
//! in words, and never a copy that would refresh.

use std::time::Duration;

use serde_json::{Map, Value};

use super::super::login::span;
use crate::{Error, Result};

/// The provider a Pi model name is on: what comes before the first `/`
/// (`openrouter/meta/muse-spark-1.3` is on `openrouter`). A name with no `/` leaves it to
/// Pi to find the provider, which is none.
pub fn provider_of(model: &str) -> Option<String> {
    let (provider, _) = model.split_once('/')?;
    (!provider.is_empty()).then(|| provider.to_owned())
}

/// Why the owner's entry of a provider is not one a run can use.
enum Unfit {
    /// There is none.
    Missing,
    /// It is neither an API key nor an OAuth login Pi knows.
    Unknown,
    /// An OAuth login that is missing its access token or its expiry.
    Incomplete,
    /// An OAuth login that has ended.
    Expired,
    /// An OAuth login that ends in the run: it lasts this much more.
    Short(Duration),
}

impl Unfit {
    fn said(&self) -> String {
        match self {
            Self::Missing => "has no entry".to_owned(),
            Self::Unknown => "has an entry of a kind Pi does not know".to_owned(),
            Self::Incomplete => "has an OAuth login with no access token or expiry".to_owned(),
            Self::Expired => "has expired".to_owned(),
            Self::Short(left) => format!("expires in {}", span(*left)),
        }
    }
}

/// Whether the entry can be copied for a run that needs it to last `left` from `now`
/// (milliseconds since the epoch).
fn fit(entry: Option<&Value>, now: i64, left: Duration) -> std::result::Result<(), Unfit> {
    let entry = entry.ok_or(Unfit::Missing)?;
    match entry["type"].as_str() {
        // A key does not end, and nothing refreshes it.
        Some("api_key") => Ok(()),
        Some("oauth") => {
            let access = entry["access"]
                .as_str()
                .is_some_and(|text| !text.is_empty());
            let expires = entry["expires"]
                .as_i64()
                // Pi writes a number, which JSON may have given as a float.
                .or_else(|| entry["expires"].as_f64().map(|expires| expires as i64));
            // The refresh token is not asked for: OpenRouter's login is an API key kept as an OAuth
            // entry with an empty one, that ends at the largest number there is. What matters is
            // whether Pi finds the entry ended in the run, and then refreshes it.
            let (true, Some(expires)) = (access, expires) else {
                return Err(Unfit::Incomplete);
            };
            if expires <= now {
                return Err(Unfit::Expired);
            }
            let remaining = Duration::from_millis(u64::try_from(expires - now).unwrap_or(0));
            if remaining < left {
                return Err(Unfit::Short(remaining));
            }
            Ok(())
        }
        _ => Err(Unfit::Unknown),
    }
}

/// The part of `text`, the owner's `auth.json` (said as `from` in what is said), that a run
/// is given: the entries of `providers` (all the providers there are, where the run's model
/// names none), each only if it lasts `left` from `now`. An error says why the run cannot
/// have a login, naming the providers; never what the text held.
pub fn login(
    text: &str,
    providers: Option<&[String]>,
    now: i64,
    left: Duration,
    from: &str,
) -> Result<Value> {
    let not_usable =
        |why: &str| Error::Daemon(format!("Pi's login in {from} cannot be used: {why}"));
    let held: Value = serde_json::from_str(text).map_err(|_| not_usable("it is not JSON"))?;
    let held = held
        .as_object()
        .ok_or_else(|| not_usable("it is not a table of providers"))?;
    let wanted: Vec<&str> = match providers {
        Some(providers) => providers.iter().map(String::as_str).collect(),
        None => held.keys().map(String::as_str).collect(),
    };
    let (mut copied, mut unfit) = (Map::new(), Vec::new());
    for provider in wanted {
        match fit(held.get(provider), now, left) {
            Ok(()) => {
                copied.insert(provider.to_owned(), held[provider].clone());
            }
            Err(why) => unfit.push(format!("`{provider}` {}", why.said())),
        }
    }
    // A run on a model whose provider is not known uses any login that is fit; one on a
    // named provider needs that provider's.
    let refused = if providers.is_some() {
        !unfit.is_empty()
    } else {
        copied.is_empty()
    };
    if !refused {
        return Ok(Value::Object(copied));
    }
    let who = if unfit.is_empty() {
        "it holds no login".to_owned()
    } else {
        unfit.join("; ")
    };
    Err(Error::Daemon(format!(
        "Pi's login in {from} cannot be used for this run: {who}. The run needs a login that lasts at \
         least {} more (its own length and a margin): a run's Pi refreshes an OAuth login that has ended, \
         and the refresh would leave the login this machine keeps unusable (the owner is logged out of \
         that provider the next time they use it). Run again after the owner's own Pi has refreshed it \
         (use Pi once on that provider), run on a model of a provider whose login lasts or is a key \
         (--models pi=…), or make the run shorter",
        span(left)
    )))
}
