//! What a run draws by chance: a codeword no model could know from anywhere else,
//! and the id of a conversation. Nothing of it is secret, and all of it is
//! different each time: the standard library seeds each hasher it makes from the
//! system, which is the one source of chance this crate needs.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

/// A number from 0 to `limit` (not including it), as `Math.floor(Math.random() *
/// limit)` draws one; 0 when `limit` is 0.
pub fn below(limit: usize) -> usize {
    match u64::try_from(limit) {
        Ok(limit) if limit > 0 => usize::try_from(word() % limit).unwrap_or(0),
        _ => 0,
    }
}

/// A random 64 bits: a hasher the standard library seeds from the system, asked
/// for nothing.
fn word() -> u64 {
    RandomState::new().build_hasher().finish()
}

/// An id in the shape of a version 4 UUID (`3f2c…-4…-8…-…`), as `randomUUID`
/// makes one: Claude Code and Pi are opened on a conversation id they are given.
pub fn uuid() -> String {
    let mut bytes = [0_u8; 16];
    bytes[..8].copy_from_slice(&word().to_be_bytes());
    bytes[8..].copy_from_slice(&word().to_be_bytes());
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn a_number_below_a_limit_is_below_it_and_none_is_always_the_same() {
        let drawn: HashSet<usize> = (0..200).map(|_| below(6)).collect();
        assert!(drawn.iter().all(|number| *number < 6), "{drawn:?}");
        assert!(drawn.len() > 1, "{drawn:?}");
        assert_eq!(below(0), 0);
        assert_eq!(below(1), 0);
    }

    #[test]
    fn an_id_is_a_version_4_uuid_and_another_each_time() {
        let id = uuid();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(
            parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12],
            "{id}"
        );
        assert!(parts[2].starts_with('4'), "{id}");
        assert!(
            matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'),
            "{id}"
        );
        assert!(id.chars().all(|at| at == '-' || at.is_ascii_hexdigit()));
        let all: HashSet<String> = (0..50).map(|_| uuid()).collect();
        assert_eq!(all.len(), 50);
    }
}
