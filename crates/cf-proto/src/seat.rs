//! The seat a participant holds on a board, and the four questions the code asks of it.
//!
//! Every place that used to compare a participant's role with `"chief"` asks one of these
//! instead: is it **the chief** (the project's one seat: the staff, the settings, the
//! releases), does it **coordinate** (the chief, and a lead where there is one), is it **a
//! member** (a window with one task at a time), does it **give tasks**. No other place
//! compares a live role with a word of its own: `tests/seat_comparisons.rs` of this crate
//! fails when one appears.

/// The member roles the ledger stores (`cf_ledger::roles` holds the table, and a test there
/// holds this list equal to its ids). A word of this list is a member's role.
pub const MEMBER_ROLES: [&str; 9] = [
    "worker",
    "frontend",
    "architect",
    "researcher",
    "advisor",
    "reviewer",
    "tester",
    "integrator",
    "designer",
];

/// What a participant is on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    /// The person at the page.
    Human,
    /// The project's one chief.
    Chief,
    /// A second coordinator of the chief's staff.
    Lead,
    /// A member of the staff: a window with one task at a time.
    Member,
}

impl Seat {
    /// The seat of a role word; None for a word that is no role, so that a caller at the
    /// CLI or a text boundary still refuses an unknown role.
    pub fn parse(role: &str) -> Option<Seat> {
        match role {
            "human" => Some(Seat::Human),
            "chief" => Some(Seat::Chief),
            "lead" => Some(Seat::Lead),
            word if MEMBER_ROLES.contains(&word) => Some(Seat::Member),
            _ => None,
        }
    }

    /// The seat of a role the ledger stored, whose CHECK lets no other word in. A word that
    /// is none is read as a member's, which is what `role != "chief"` said of it.
    pub fn of(role: &str) -> Seat {
        Seat::parse(role).unwrap_or(Seat::Member)
    }

    /// The project's one seat.
    pub fn is_chief(self) -> bool {
        self == Seat::Chief
    }

    /// The chief, and a lead where there is one.
    pub fn coordinates(self) -> bool {
        matches!(self, Seat::Chief | Seat::Lead)
    }

    /// A member of the staff.
    pub fn is_member(self) -> bool {
        self == Seat::Member
    }

    /// The person, the chief and a lead give tasks.
    pub fn gives_tasks(self) -> bool {
        matches!(self, Seat::Human | Seat::Chief | Seat::Lead)
    }
}

/// The seats that coordinate, as the list of an SQL `IN`: for a `participant.role` the SQL
/// asks `coordinates()` of. A literal, so that `concat!` can put it in a statement.
#[macro_export]
macro_rules! coordinator_roles_sql {
    () => {
        "('chief', 'lead')"
    };
}

/// The seats that are no member, as the list of an SQL `NOT IN`: for a `participant.role`
/// the SQL asks `is_member()` of.
#[macro_export]
macro_rules! non_member_roles_sql {
    () => {
        "('human', 'chief', 'lead')"
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_that_is_no_role_has_no_seat() {
        for word in ["", "Chief", "chief ", "boss", "scout", "members", "lead-1"] {
            assert_eq!(Seat::parse(word), None, "{word:?}");
        }
    }

    #[test]
    fn every_role_word_has_its_seat() {
        assert_eq!(Seat::parse("human"), Some(Seat::Human));
        assert_eq!(Seat::parse("chief"), Some(Seat::Chief));
        assert_eq!(Seat::parse("lead"), Some(Seat::Lead));
        for word in MEMBER_ROLES {
            assert_eq!(Seat::parse(word), Some(Seat::Member), "{word}");
        }
    }

    #[test]
    fn the_four_questions_are_answered_for_each_seat() {
        let table = [
            (Seat::Human, [false, false, false, true]),
            (Seat::Chief, [true, true, false, true]),
            (Seat::Lead, [false, true, false, true]),
            (Seat::Member, [false, false, true, false]),
        ];
        for (seat, [chief, coordinates, member, gives]) in table {
            assert_eq!(seat.is_chief(), chief, "{seat:?} is_chief");
            assert_eq!(seat.coordinates(), coordinates, "{seat:?} coordinates");
            assert_eq!(seat.is_member(), member, "{seat:?} is_member");
            assert_eq!(seat.gives_tasks(), gives, "{seat:?} gives_tasks");
        }
    }

    #[test]
    fn a_stored_word_that_is_none_reads_as_a_member() {
        assert_eq!(Seat::of("scout"), Seat::Member);
        assert!(Seat::of("chief").is_chief());
    }
}
