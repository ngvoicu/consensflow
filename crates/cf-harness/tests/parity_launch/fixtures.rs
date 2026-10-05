//! What the unit tests of more than one module are made of: the launch's id and
//! the spellings of a root.

use crate::Spellings;

pub(super) const LAUNCH: &str = "11111111-1111-4111-8111-111111111111";

/// The root `/tmp/consensflow launch %#-X/<side>` as Node's `rootForms` spells it.
pub(super) fn spellings_of(side: &str) -> Spellings {
    Spellings {
        file_url: format!("file:///tmp/consensflow%20launch%20%25%23-X/{side}"),
        plain: vec![
            format!("/tmp/consensflow launch %#-X/{side}"),
            format!("%2Ftmp%2Fconsensflow%20launch%20%25%23-X%2F{side}"),
            format!("%2Ftmp%2Fconsensflow+launch+%25%23-X%2F{side}"),
        ],
    }
}

pub(super) fn spellings() -> Spellings {
    spellings_of("rust")
}
