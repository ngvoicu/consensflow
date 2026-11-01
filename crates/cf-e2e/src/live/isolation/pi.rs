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

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Map, Value};

use super::super::login::span;
use super::super::renew::{self, Renewal, TestMachine};
use super::watch::Seen;
use crate::{Error, Result};

/// The entry `entry`, the owner's, as a run is given it: all but its refresh token. A copy that
/// holds only the access token cannot renew it, so it cannot use up the refresh token the owner's
/// file keeps (T-1067, measured on zeewin2: Pi 1.4.0 runs on such a copy, and where less than five
/// minutes of the access token are left it tries to renew, is refused (`invalid_grant`) and fails
/// the run loudly, the owner's file untouched).
fn without_renewal(entry: &Value) -> Value {
    let mut copy = entry.clone();
    if let Some(fields) = copy.as_object_mut() {
        fields.remove("refresh");
    }
    copy
}

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

/// When the OAuth `entry` ends: its `expires`, milliseconds since the epoch.
fn expires(entry: &Value) -> Option<i64> {
    entry["expires"]
        .as_i64()
        // Pi writes a number, which JSON may have given as a float.
        .or_else(|| entry["expires"].as_f64().map(|expires| expires as i64))
}

/// When the OAuth entry of `provider` in `text`, the owner's `auth.json`, ends; none where it
/// holds none.
pub fn expiry(text: &str, provider: &str) -> Option<i64> {
    expires(serde_json::from_str::<Value>(text).ok()?.get(provider)?)
}

/// Whether the entry of `provider` in `text` has run short of `left` from `now`, and Pi could
/// renew it: an OAuth login with a refresh token, that has ended or ends within `left`. An entry
/// that is a key, that is missing or unknown, or that has no refresh token is not for a renewal
/// to mend.
pub fn renewable(text: &str, provider: &str, now: i64, left: Duration) -> bool {
    let Some(held) = serde_json::from_str::<Value>(text).ok() else {
        return false;
    };
    let entry = held.get(provider);
    let has_refresh = entry
        .and_then(|entry| entry["refresh"].as_str())
        .is_some_and(|token| !token.is_empty());
    has_refresh && matches!(fit(entry, now, left), Err(Unfit::Expired | Unfit::Short(_)))
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
            let expires = expires(entry);
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
                copied.insert(provider.to_owned(), without_renewal(&held[provider]));
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
         that provider the next time they use it). Pi renews an entry itself once it has ended: run again \
         then (use Pi once on that provider; on a test machine the kit asks it to), run on a model of a provider whose login lasts or is a key \
         (--models pi=…), or make the run shorter",
        span(left)
    )))
}

/// Pi's renewal on `machine`, for the owner whose home is `home` and whose Pi folder is `agent`,
/// started as `program` finds it: Pi run once on the model the run uses ([`renew::pi_words`]),
/// and the owner's files compared before and after, so that nothing but `auth.json` is changed.
pub fn renewal(
    machine: TestMachine,
    home: &Path,
    agent: &Path,
    model: &str,
    program: impl Fn() -> Result<PathBuf> + Send + Sync + 'static,
) -> Renewal {
    let (home, agent, model) = (home.to_path_buf(), agent.to_path_buf(), model.to_owned());
    Renewal::new(machine, "Pi", "only once the entry has ended", move || {
        let program = program()?;
        let files: Vec<_> = ["settings.json", "models.json", "mcp-auth.json"]
            .map(|name| agent.join(name))
            .into();
        let records = [agent.join("sessions")];
        let before = Seen::look(&files, &records);
        let ended = renew::one_shot(&program, &renew::pi_words(&model))?;
        let changed = before.changes_to(&Seen::look(&files, &records), &home);
        if changed.is_empty() {
            return Ok(ended);
        }
        Err(Error::Daemon(format!(
            "it changed the owner's Pi files beside its login: {}",
            changed.join("; ")
        )))
    })
}
