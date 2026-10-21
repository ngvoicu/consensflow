//! The breaks a release may carry: each a known bug, deferred to an issue, listed in
//! `evals/known-breaks.json` and read in when cf-evals is built. A break is known where
//! an entry names its invariant and its shape, both: the shape is what a check says of
//! the break's kind ([`Break::shape`]), so that an A3 whose late notice is the
//! background "ended" note is known and an A3 of another notice is not. A known break
//! is kept with its evidence and its issue; it fails no run.

use std::sync::LazyLock;

use serde_json::Value;

use super::invariants::Break;

/// The list, as the repository holds it.
const LIST: &str = include_str!("../../../../evals/known-breaks.json");

/// A known break: its invariant and its shape, and the issue it is deferred to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Known {
    pub invariant: u8,
    pub shape: String,
    pub issue: String,
}

/// The known breaks, or why the list could not be read: a list that cannot be read
/// knows nothing, and the report says why (a run then fails on every break).
pub static KNOWN: LazyLock<Result<Vec<Known>, String>> = LazyLock::new(|| read(LIST));

/// The entries of the list `text`.
fn read(text: &str) -> Result<Vec<Known>, String> {
    let list: Value =
        serde_json::from_str(text).map_err(|cause| format!("it is no JSON: {cause}"))?;
    list["known"]
        .as_array()
        .ok_or("it has no list of known breaks")?
        .iter()
        .map(|entry| {
            let field = |name: &str| {
                entry[name]
                    .as_str()
                    .filter(|said| !said.is_empty())
                    .map(str::to_owned)
                    .ok_or(format!("an entry has no {name}: {entry}"))
            };
            Ok(Known {
                invariant: entry["invariant"]
                    .as_u64()
                    .and_then(|number| u8::try_from(number).ok())
                    .ok_or(format!("an entry has no invariant: {entry}"))?,
                shape: field("shape")?,
                issue: field("issue")?,
            })
        })
        .collect()
}

/// The entry of `known` that `found` is, if it is one: its invariant and its shape.
pub fn of<'k>(found: &Break, known: &'k [Known]) -> Option<&'k Known> {
    let shape = found.shape?;
    known
        .iter()
        .find(|entry| entry.invariant == found.invariant && entry.shape == shape)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_repositorys_list_is_read_and_holds_a3s_ended_note() {
        let known = KNOWN.as_ref().unwrap();
        let a3 = known.iter().find(|entry| entry.invariant == 3).unwrap();
        assert_eq!(a3.shape, "ended-note");
        assert!(a3.issue.contains("3.0.6"), "{}", a3.issue);
        // Said whole wherever the run says it: nothing in it has a token's shape.
        let names = crate::long_run::mask::Names::default();
        for entry in known {
            assert_eq!(
                crate::long_run::mask::mask(&entry.issue, &names),
                entry.issue
            );
        }
    }

    #[test]
    fn a_break_is_known_by_its_invariant_and_its_shape_both() {
        let known = [Known {
            invariant: 3,
            shape: "ended-note".to_owned(),
            issue: "3.0.6".to_owned(),
        }];
        let a3 = |shape| {
            Break::new(
                3,
                "3:m-20",
                "T-2's ended notice m-20 came after its result m-18",
            )
            .with_shape(shape)
        };
        assert_eq!(of(&a3("ended-note"), &known), Some(&known[0]));
        // An A3 of another notice, a break of another invariant in the same shape, and a
        // break of no shape are none of it.
        assert_eq!(of(&a3("let-go-note"), &known), None);
        assert_eq!(
            of(
                &Break::new(4, "4:m-20", "not its answer").with_shape("ended-note"),
                &known
            ),
            None
        );
        assert_eq!(of(&Break::new(3, "3:m-20", "late"), &known), None);
    }

    #[test]
    fn a_list_that_cannot_be_read_says_why() {
        assert!(read("{").unwrap_err().starts_with("it is no JSON"));
        assert_eq!(read("{}").unwrap_err(), "it has no list of known breaks");
        assert!(read(r#"{"known":[{"invariant":3,"shape":"ended-note"}]}"#)
            .unwrap_err()
            .starts_with("an entry has no issue"));
        assert_eq!(read(r#"{"known":[]}"#), Ok(Vec::new()));
    }
}
