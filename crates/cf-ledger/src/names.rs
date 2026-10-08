//! The name of a member's session: two plain words after the member's own
//! handle, `diana-amber-pine`, easy to say aloud and to tell apart on the
//! board. Thirty-two of each gives a thousand names per member.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

const ADJECTIVES: [&str; 32] = [
    "amber", "brisk", "calm", "coral", "crisp", "dusky", "eager", "frosty", "gentle", "golden",
    "hazel", "ivory", "jolly", "keen", "lively", "lunar", "mellow", "misty", "noble", "olive",
    "pale", "quiet", "rosy", "rusty", "sandy", "silver", "sunny", "tidy", "velvet", "vivid",
    "windy", "zesty",
];
const NOUNS: [&str; 32] = [
    "anchor", "birch", "brook", "canyon", "cedar", "cliff", "comet", "delta", "dune", "ember",
    "fjord", "glade", "harbor", "island", "juniper", "kestrel", "lagoon", "meadow", "oriole",
    "pebble", "pine", "quarry", "reef", "ridge", "saddle", "summit", "thistle", "tundra", "valley",
    "willow", "window", "yarrow",
];

/// A fresh `adjective-noun`; `random` gives numbers in `[0, 1)`, as
/// JavaScript's `Math.random` did, injected so tests can pick.
pub fn session_name(mut random: impl FnMut() -> f64) -> String {
    let mut pick = |words: &[&'static str]| {
        let at = (random() * words.len() as f64).floor() as usize;
        words[at.min(words.len() - 1)]
    };
    let adjective = pick(&ADJECTIVES);
    format!("{adjective}-{}", pick(&NOUNS))
}

/// A number in `[0, 1)` from the process's random hashing keys: enough to
/// pick a session's name, which only has to differ from its siblings'.
pub fn random_unit() -> f64 {
    let bits = RandomState::new().build_hasher().finish() >> 11;
    bits as f64 / (1u64 << 53) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_an_adjective_and_a_noun_as_the_numbers_say() {
        let mut numbers = [0.0, 0.0].into_iter();
        assert_eq!(session_name(|| numbers.next().unwrap()), "amber-anchor");
        let mut numbers = [0.999_999, 0.999_999].into_iter();
        assert_eq!(session_name(|| numbers.next().unwrap()), "zesty-yarrow");
        let mut numbers = [1.0, 0.5].into_iter();
        assert_eq!(session_name(|| numbers.next().unwrap()), "zesty-lagoon");
    }

    #[test]
    fn draws_numbers_in_the_unit_interval() {
        for _ in 0..1000 {
            assert!((0.0..1.0).contains(&random_unit()));
        }
    }
}
