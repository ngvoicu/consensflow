//! What Escape does in a Devin window: nothing of its own, once. Devin's status
//! line says "esc twice to interrupt", and one Escape stops no turn: it takes a
//! second within a short while of the first (the daemon presses two, 150 ms
//! apart, and a third a second later for the rewind two Escapes open at a window
//! that is at rest, which this stand-in has none of). Devin's own window is not
//! documented; the stand-in takes two within half a second, which 150 ms is well
//! inside and the second of a round's own (a second after the first) is not.

use std::time::{Duration, Instant};

/// How close two Escapes are at most to be a double press.
pub const DOUBLE: Duration = Duration::from_millis(500);

/// The Escapes of a window, as far as a double press goes.
#[derive(Default)]
pub struct Escapes {
    last: Option<Instant>,
}

impl Escapes {
    /// An Escape at `at`: whether it is the second of a double press, which
    /// interrupts a turn at work. A first one is remembered for as long as the
    /// double press lasts, and a double press is two: the Escape after it is a
    /// first one again.
    pub fn press(&mut self, at: Instant) -> bool {
        match self.last.take() {
            Some(first) if at.saturating_duration_since(first) <= DOUBLE => true,
            _ => {
                self.last = Some(at);
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(start: Instant, milliseconds: u64) -> Instant {
        start + Duration::from_millis(milliseconds)
    }

    #[test]
    fn two_escapes_a_hundred_and_fifty_milliseconds_apart_are_a_double_press_and_one_is_not() {
        let start = Instant::now();
        let mut escapes = Escapes::default();
        assert!(!escapes.press(start));
        assert!(escapes.press(after(start, 150)));
    }

    #[test]
    fn two_escapes_further_apart_than_half_a_second_are_two_first_presses() {
        let start = Instant::now();
        let mut escapes = Escapes::default();
        assert!(!escapes.press(start));
        // The gap of the round's own second press and the one a second after it.
        assert!(!escapes.press(after(start, 1_000)));
        // The window is half a second, the edge in.
        assert!(escapes.press(after(start, 1_500)));
        let mut escapes = Escapes::default();
        assert!(!escapes.press(start));
        assert!(!escapes.press(after(start, 501)));
    }

    #[test]
    fn a_double_press_is_two_and_the_press_after_it_is_a_first_again() {
        let start = Instant::now();
        let mut escapes = Escapes::default();
        assert!(!escapes.press(start));
        assert!(escapes.press(after(start, 100)));
        // A third at once is no second of a pair that was spent.
        assert!(!escapes.press(after(start, 200)));
        assert!(escapes.press(after(start, 300)));
    }
}
