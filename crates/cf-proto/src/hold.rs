//! The closed form of an account hold (T-2353): what a member's `member.out` event keeps of the
//! refusal that held it, so the board can say which it was, billing or login, and the status
//! and the provider's error type the refusal carried, with no word of the harness's.
//!
//! **Every value of the form is a literal of this file.** The lists below are all the form can
//! hold; [`HoldForm::checked`] gives back the list's own literal for a value that is in a list
//! and nothing for any other, so no piece of a harness's text can pass through it. The harness's
//! readers pick the values by matching the refusal's text against these lists (the provider
//! error types they already match on); what they write is the matched literal.

use serde::Serialize;

/// What was refused: the account's billing or its login.
pub const KINDS: [&str; 2] = ["billing", "login"];

/// The statuses of an account's refusal the form names.
pub const STATUSES: [u16; 3] = [401, 402, 403];

/// The provider error types the form names, each a literal of this list, for a login refused
/// ([`LOGIN_CODES`]) or a billing refused ([`BILLING_CODES`]).
pub const CODES: [&str; 10] = [
    "authentication_error",
    "invalid_grant",
    "invalid_api_key",
    "invalid_x_api_key",
    "refresh_failed",
    "session_expired",
    "billing_error",
    "insufficient_quota",
    "credit_balance_too_low",
    "requires_more_credits",
];

/// The codes of a login refused.
pub const LOGIN_CODES: [&str; 6] = [
    "authentication_error",
    "invalid_grant",
    "invalid_api_key",
    "invalid_x_api_key",
    "refresh_failed",
    "session_expired",
];

/// The codes of a billing refused.
pub const BILLING_CODES: [&str; 4] = [
    "billing_error",
    "insufficient_quota",
    "credit_balance_too_low",
    "requires_more_credits",
];

/// An account hold's kind, status and code: each a literal of this file. **The fields are
/// private and there is no other constructor than [`HoldForm::checked`]**: nothing outside this
/// file can make a form whose value is not a literal of the lists (T-2353).
///
/// ```compile_fail
/// use cf_proto::hold::HoldForm;
/// // A status the lists do not name, a code that is a piece of a harness's text.
/// let _ = HoldForm { kind: "login", status: Some(499), code: Some("owner@example.invalid") };
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct HoldForm {
    kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<&'static str>,
}

impl HoldForm {
    /// Which was refused: one of [`KINDS`].
    pub const fn kind(&self) -> &'static str {
        self.kind
    }

    /// The status the refusal carried, one of [`STATUSES`], where it carried one.
    pub const fn status(&self) -> Option<u16> {
        self.status
    }

    /// The provider's error type, one of [`CODES`], where the refusal named one.
    pub const fn code(&self) -> Option<&'static str> {
        self.code
    }

    /// The form of `kind`, `status` and `code` as the lists name them: the lists' own literals
    /// for what is in them, no status or no code for what is not, and nothing where the kind is
    /// none of [`KINDS`]. A code of the other kind's list is no code.
    pub fn checked(kind: &str, status: Option<u64>, code: Option<&str>) -> Option<Self> {
        let kind = KINDS.iter().find(|known| **known == kind)?;
        let codes: &[&'static str] = if *kind == "login" {
            &LOGIN_CODES
        } else {
            &BILLING_CODES
        };
        Some(Self {
            kind,
            status: status
                .and_then(|status| STATUSES.iter().copied().find(|s| u64::from(*s) == status)),
            code: code.and_then(|code| codes.iter().copied().find(|known| *known == code)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lists_agree_and_every_value_the_form_can_take_is_one_of_their_literals() {
        let mut both: Vec<&str> = LOGIN_CODES.iter().chain(&BILLING_CODES).copied().collect();
        both.sort_unstable();
        let mut all = CODES.to_vec();
        all.sort_unstable();
        assert_eq!(both, all, "the two lists are the one list");
        let mut seen = 0;
        for kind in KINDS {
            for status in std::iter::once(None).chain(STATUSES.iter().map(|s| Some(u64::from(*s))))
            {
                for code in std::iter::once(None).chain(CODES.iter().copied().map(Some)) {
                    let form = HoldForm::checked(kind, status, code).expect("a kind of the list");
                    assert!(KINDS.contains(&form.kind));
                    assert!(form.status.is_none_or(|s| STATUSES.contains(&s)));
                    assert!(form.code.is_none_or(|c| CODES.contains(&c)));
                    seen += 1;
                }
            }
        }
        assert_eq!(
            seen,
            2 * 4 * 11,
            "every kind, status and code, each also absent"
        );
    }

    #[test]
    fn a_value_that_is_in_no_list_writes_nothing() {
        for status in [0, 200, 400, 404, 429, 500, 401_000] {
            let form = HoldForm::checked("login", Some(status), None).unwrap();
            assert_eq!(form.status, None, "{status}");
        }
        for code in [
            "",
            "Authentication_Error",
            " authentication_error",
            "authentication_error: owner@example.invalid",
            "jos\u{e9}@ex\u{e4}mple.invalid",
            "billing_error",
        ] {
            let form = HoldForm::checked("login", None, Some(code)).unwrap();
            assert!(
                form.code.is_none() || (code == "authentication_error"),
                "{code}: {form:?}"
            );
        }
        assert_eq!(
            HoldForm::checked("billing", None, Some("authentication_error"))
                .unwrap()
                .code,
            None
        );
        assert_eq!(
            HoldForm::checked("moved", Some(401), Some("invalid_grant")),
            None
        );
        assert_eq!(HoldForm::checked("", None, None), None);
    }
}
