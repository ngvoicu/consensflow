//! What a side found, raw and normalized: the root written `$ROOT` in every
//! spelling, and what a plan draws named by its order of appearance.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::{Change, Outcome, Plan, Spellings};

/// What a side planned in and found, raw.
pub(super) struct Raw<'a> {
    pub(super) spellings: &'a Spellings,
    pub(super) env: &'a [(String, String)],
    pub(super) directory: &'a str,
    pub(super) outcome: &'a Outcome,
    pub(super) changes: &'a [Change],
}

/// What a side found, normalized.
pub(super) struct Normal {
    /// What it planned in, its environment in order and its folder: the
    /// roots are held to one shape, so that what differs is the adapters'.
    pub(super) setting: Vec<String>,
    pub(super) outcome: Outcome,
    pub(super) changes: Vec<Change>,
    /// The hashes Pi's bundle was published under, as they are.
    pub(super) pi_hashes: Vec<String>,
}

/// Names what a plan draws by its order of appearance, each kind apart: the
/// same value is the same name each time it comes, and what a plan is given
/// has none.
struct Normalizer<'a> {
    spellings: &'a Spellings,
    given: &'a [&'a str],
    seen: BTreeMap<&'static str, Vec<String>>,
    pi_hashes: Vec<String>,
}

static BUNDLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(extensions[\\/])(opencode|pi)([\\/])([0-9a-f]{64})").unwrap());
static UUID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}\b").unwrap()
});
static SESSION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\bses_[A-Za-z0-9]+").unwrap());
/// Pi's name for a conversation of its own: `cf-<project>-<handle>-<4 bytes in hex>`.
static PI_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(cf-\d+-[A-Za-z0-9_-]+-)([0-9a-f]{8})\b").unwrap());
static PORT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(127\.0\.0\.1:|"port":)([0-9]+)"#).unwrap());
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Za-z0-9_-]+").unwrap());

impl<'a> Normalizer<'a> {
    fn new(spellings: &'a Spellings, given: &'a [&'a str]) -> Self {
        Self {
            spellings,
            given,
            seen: BTreeMap::new(),
            pi_hashes: Vec::new(),
        }
    }

    /// `value`'s name among the `kind`s seen so far: `$UUID1`, `$UUID2`…
    fn name(&mut self, kind: &'static str, value: &str) -> String {
        let seen = self.seen.entry(kind).or_default();
        let at = seen
            .iter()
            .position(|held| held == value)
            .unwrap_or_else(|| {
                seen.push(value.to_owned());
                seen.len() - 1
            });
        format!("${kind}{}", at + 1)
    }

    /// `text` with the root written `$ROOT` and what is drawn named.
    fn text(&mut self, text: &str) -> String {
        let mut text = text.replace(&self.spellings.file_url, "file://$ROOT");
        for spelling in &self.spellings.plain {
            text = text.replace(spelling.as_str(), "$ROOT");
        }
        let text = BUNDLE
            .replace_all(&text, |found: &Captures| {
                let hash = found[4].to_owned();
                if &found[2] == "pi" && !self.pi_hashes.contains(&hash) {
                    self.pi_hashes.push(hash.clone());
                }
                format!(
                    "{}{}{}{}",
                    &found[1],
                    &found[2],
                    &found[3],
                    self.name("HASH", &hash)
                )
            })
            .into_owned();
        let text = UUID
            .replace_all(&text, |found: &Captures| self.drawn("UUID", &found[0]))
            .into_owned();
        let text = SESSION
            .replace_all(&text, |found: &Captures| self.drawn("SESSION", &found[0]))
            .into_owned();
        let text = PI_NAME
            .replace_all(&text, |found: &Captures| {
                let name = found[0].to_owned();
                if self.given.contains(&name.as_str()) {
                    name
                } else {
                    format!("{}{}", &found[1], self.name("NAME", &found[2]))
                }
            })
            .into_owned();
        let text = PORT
            .replace_all(&text, |found: &Captures| {
                format!("{}{}", &found[1], self.name("PORT", &found[2]))
            })
            .into_owned();
        // A token is 24 bytes drawn and written in base64url: a word of 32.
        WORD.replace_all(&text, |found: &Captures| {
            if found[0].len() == 32 {
                self.name("TOKEN", &found[0])
            } else {
                found[0].to_owned()
            }
        })
        .into_owned()
    }

    /// `value` as it stands if a launch was given it, else its name.
    fn drawn(&mut self, kind: &'static str, value: &str) -> String {
        if self.given.contains(&value) {
            value.to_owned()
        } else {
            self.name(kind, value)
        }
    }

    /// An argument list, the port that follows `--port` named as any port.
    fn argv(&mut self, argv: &[String]) -> Vec<String> {
        let mut named = Vec::new();
        for (at, argument) in argv.iter().enumerate() {
            let is_port = at > 0
                && argv[at - 1] == "--port"
                && !argument.is_empty()
                && argument.bytes().all(|byte| byte.is_ascii_digit());
            named.push(if is_port {
                self.name("PORT", argument)
            } else {
                self.text(argument)
            });
        }
        named
    }
}

/// What a side found, as one normalizer reads it: what it planned in, its
/// plan or refusal in the order its parts come in, then each change by path.
pub(super) fn normalize(raw: &Raw, given: &[&str]) -> Normal {
    let mut names = Normalizer::new(raw.spellings, given);
    let mut setting: Vec<String> = raw
        .env
        .iter()
        .map(|(name, value)| format!("{name}={}", names.text(value)))
        .collect();
    setting.push(format!("directory={}", names.text(raw.directory)));
    let outcome = match raw.outcome {
        Outcome::Plan(plan) => Outcome::Plan(Plan {
            argv: names.argv(&plan.argv),
            env: plan
                .env
                .iter()
                .map(|(name, value)| (name.clone(), names.text(value)))
                .collect(),
            drop_env: plan.drop_env.clone(),
            native_session: plan.native_session.as_deref().map(|text| names.text(text)),
        }),
        Outcome::Refused(sentence) => Outcome::Refused(names.text(sentence)),
    };
    let mut sorted = raw.changes.to_vec();
    sorted.sort_by(|left, right| left.path.cmp(&right.path));
    let changes = sorted
        .into_iter()
        .map(|change| Change {
            path: names.text(&change.path),
            text: change.text.as_deref().map(|text| names.text(text)),
            target: change.target.as_deref().map(|target| names.text(target)),
            ..change
        })
        .collect();
    Normal {
        setting,
        outcome,
        changes,
        pi_hashes: names.pi_hashes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixtures::{spellings, LAUNCH};

    #[test]
    fn a_root_is_written_in_every_spelling_it_is_read_in() {
        let spellings = spellings();
        let mut names = Normalizer::new(&spellings, &[]);
        for (text, written) in [
            (
                "/tmp/consensflow launch %#-X/rust/consensflow/a",
                "$ROOT/consensflow/a",
            ),
            (
                "file:///tmp/consensflow%20launch%20%25%23-X/rust/consensflow/a",
                "file://$ROOT/consensflow/a",
            ),
            (
                "?directory=%2Ftmp%2Fconsensflow%20launch%20%25%23-X%2Frust%2Fwork",
                "?directory=$ROOT%2Fwork",
            ),
            (
                "?directory=%2Ftmp%2Fconsensflow+launch+%25%23-X%2Frust",
                "?directory=$ROOT",
            ),
        ] {
            assert_eq!(names.text(text), written);
        }
        // Another root is no part of this one's.
        let other = "/tmp/consensflow launch %#-X/node/consensflow/a";
        assert_eq!(names.text(other), other);
    }

    #[test]
    fn what_a_plan_draws_is_named_by_its_order_of_appearance_and_what_it_is_given_is_not() {
        let given = [LAUNCH, "ses_given0001"];
        let spellings = spellings();
        let side = |first: &str, second: &str, session: &str| {
            let mut names = Normalizer::new(&spellings, &given);
            names.text(&format!(
                "{first} {second} {first} {LAUNCH} ses_given0001 {session} {session}"
            ))
        };
        let named = "$UUID1 $UUID2 $UUID1 11111111-1111-4111-8111-111111111111 ses_given0001 $SESSION1 $SESSION1";
        // Two sides that drew other values, in the same places, are the same.
        assert_eq!(
            side(
                "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                "ses_ef4449f98ffeVbNNlDLO94GcTN"
            ),
            named
        );
        assert_eq!(
            side(
                "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
                "ses_0123456789abAbCdEfGhIjKlMn"
            ),
            named
        );
        // A draw that is one value on a side and two on the other is not.
        let same = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
        let other = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
        assert_ne!(
            Normalizer::new(&spellings, &given).text(&format!("{same} {same}")),
            Normalizer::new(&spellings, &given).text(&format!("{same} {other}"))
        );
    }

    #[test]
    fn a_port_a_token_a_hash_and_a_name_of_pi_s_are_each_named_in_their_own_kind() {
        let spellings = spellings();
        let given = ["cf-7-rhea-0a1b2c3d"];
        let mut names = Normalizer::new(&spellings, &given);
        let hash = "257abc9f5e18ad4b748fcadfcbf6899c11427087aa5eeaf76c72bed9462ced0f";
        let text = format!(
            concat!(
                r#"{{"launchId":"x","port":41234,"token":"abcdefghijklmnopqrstuvwxyzABCDEF"}} "#,
                "http://127.0.0.1:41235/s cf-7-rhea-9f8e7d6c cf-7-rhea-0a1b2c3d /x/extensions/pi/{}/hosts ",
                "/x/extensions/opencode/{}/hosts"
            ),
            hash,
            "0123456789abcdef".repeat(4)
        );
        assert_eq!(
            names.text(&text),
            concat!(
                r#"{"launchId":"x","port":$PORT1,"token":"$TOKEN1"} "#,
                "http://127.0.0.1:$PORT2/s cf-7-rhea-$NAME1 cf-7-rhea-0a1b2c3d /x/extensions/pi/$HASH1/hosts ",
                "/x/extensions/opencode/$HASH2/hosts"
            )
        );
        assert_eq!(names.pi_hashes, [hash], "Pi's hashes are kept as they are");
        names.text(&format!("/y/extensions/pi/{hash}/hosts"));
        assert_eq!(names.pi_hashes, [hash], "and each is kept once");
    }

    #[test]
    fn a_token_is_a_word_of_thirty_two_and_no_part_of_a_longer_or_shorter_one() {
        let spellings = spellings();
        let mut names = Normalizer::new(&spellings, &[]);
        for kept in [
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6x",
            "a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d",
            "consensflow-delivery.mjs",
        ] {
            assert_eq!(names.text(kept), kept);
        }
        assert_eq!(
            names.text("x a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6 y-_z"),
            "x $TOKEN1 y-_z"
        );
        assert_eq!(
            names.text("Tn6how5MeNkKJwojV781E5ANtOo6bavs"),
            "$TOKEN2",
            "base64url's own characters are in a token"
        );
    }

    #[test]
    fn the_port_that_follows_port_is_named_as_any_port_and_no_other_number() {
        let spellings = spellings();
        let mut names = Normalizer::new(&spellings, &[]);
        let argv: Vec<String> = ["--port", "41000", "--hostname", "127.0.0.1", "--n", "41000"]
            .map(str::to_owned)
            .to_vec();
        assert_eq!(
            names.argv(&argv),
            [
                "--port",
                "$PORT1",
                "--hostname",
                "127.0.0.1",
                "--n",
                "41000"
            ]
        );
    }
}
