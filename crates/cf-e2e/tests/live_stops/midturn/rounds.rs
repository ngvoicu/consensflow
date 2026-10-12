//! The Escapes the daemon pressed, as the rounds they were pressed in, and when a
//! round is looked at. A round of Devin's is two presses 150 ms apart and one more
//! a second later (the one that closes the rewind a stopped turn's two would open);
//! the rounds follow each other three seconds after the one before began, so the
//! gap between the last press of a round and the first of the next is the least
//! that is told by: it is not less than 1.85 s, and no gap inside a round is more
//! than about a second.

/// How long after the last Escape of a round the window is looked at.
pub const LOOK_AFTER_MS: i64 = 1_500;

/// A gap longer than this between two Escapes is between two rounds.
const ROUND_GAP_MS: i64 = 1_500;

/// The Escapes (their times, oldest first) as rounds: a new round begins where the
/// gap from the press before is more than [`ROUND_GAP_MS`].
pub fn rounds_of(presses: &[i64]) -> Vec<&[i64]> {
    let mut rounds = Vec::new();
    let mut from = 0;
    for at in 1..presses.len() {
        if presses[at] - presses[at - 1] > ROUND_GAP_MS {
            rounds.push(&presses[from..at]);
            from = at;
        }
    }
    if !presses.is_empty() {
        rounds.push(&presses[from..]);
    }
    rounds
}

/// The rounds, after the `taken` that were looked at, whose last Escape is
/// [`LOOK_AFTER_MS`] old at `now`: one that is still being pressed is looked at
/// once it is not.
pub fn due(presses: &[i64], taken: usize, now: i64) -> Vec<usize> {
    rounds_of(presses)
        .iter()
        .enumerate()
        .skip(taken)
        .take_while(|(_, round)| round.last().is_some_and(|last| now >= last + LOOK_AFTER_MS))
        .map(|(index, _)| index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three rounds as the daemon presses Devin's: its look comes a moment after the
    /// three seconds are up, so the rounds begin 3.0 to 4.5 s apart.
    const PRESSES: [i64; 9] = [100, 250, 1_250, 3_100, 3_250, 4_250, 6_100, 6_250, 7_250];

    #[test]
    fn the_escapes_are_rounds_where_the_gap_between_two_is_more_than_a_second_and_a_half() {
        assert_eq!(
            rounds_of(&PRESSES),
            [
                &[100, 250, 1_250][..],
                &[3_100, 3_250, 4_250],
                &[6_100, 6_250, 7_250]
            ]
        );
        // The least gap between two rounds (1.85 s) and the most inside one (1.0 s).
        assert_eq!(rounds_of(&[0, 150, 1_150, 3_000]).len(), 2);
        assert_eq!(rounds_of(&[0, 150, 1_150, 2_650]).len(), 1);
        assert_eq!(rounds_of(&[5]), [&[5][..]]);
        assert!(rounds_of(&[]).is_empty());
    }

    #[test]
    fn a_round_is_looked_at_once_a_second_and_a_half_has_passed_since_its_last_escape_and_once() {
        // The first round's last key was at 1_250: it is ripe at 2_750.
        assert!(due(&PRESSES[..5], 0, 2_749).is_empty());
        assert_eq!(due(&PRESSES[..5], 0, 2_750), [0]);
        // The second is still being pressed (its last key so far is 3_250), and a round
        // looked at is not looked at again.
        assert_eq!(due(&PRESSES[..5], 1, 4_000), Vec::<usize>::new());
        assert_eq!(due(&PRESSES, 1, 5_749), Vec::<usize>::new());
        assert_eq!(due(&PRESSES, 1, 5_750), [1]);
        assert_eq!(due(&PRESSES, 0, 5_750), [0, 1]);
        assert!(due(&[], 0, 1_000_000).is_empty());
    }
}
