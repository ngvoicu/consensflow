//! `account_refusal` reads every account-refusal text of the repository's fixtures and tests as
//! main's reader did before the held-words work (T-2353): the readings in `account-refusals.jsonl`
//! are main's own (`de74531ff`), taken from the texts that the fixtures, the tests' literals and
//! Pi's published packages hold. Two readings were added, both sure: Anthropic's refresh refused
//! with `invalid_grant`, and Pi's published Meta errors ("session expired (status 401|403)").
//! Those rows say `added`. No rule reads an expired token by its words: it is a login refusal.

use super::*;

fn read(refusal: Option<Refusal>) -> &'static str {
    match refusal {
        Some(Refusal::Billing) => "billing",
        Some(Refusal::Credentials) => "login",
        None => "none",
    }
}

#[test]
fn every_text_of_the_repository_s_fixtures_reads_as_main_read_it_but_for_two_sure_additions() {
    let rows = include_str!("account-refusals.jsonl");
    let (mut seen, mut added) = (0, 0);
    for row in rows.lines() {
        let row: serde_json::Value = serde_json::from_str(row).unwrap();
        let text = row["text"].as_str().unwrap();
        let status = row["status"].as_f64();
        let reading = read(account_refusal(text, status));
        seen += 1;
        if let Some(now) = row["added"].as_str() {
            added += 1;
            assert_eq!(now, "login", "{text}");
            assert_eq!(reading, now, "{text}");
            assert!(
                text.contains("invalid_grant") || text.contains("session expired (status"),
                "an addition is one of the two sure readings: {text}"
            );
        } else {
            assert_eq!(reading, row["main"].as_str().unwrap(), "{text}");
        }
    }
    assert!(seen >= 100, "the corpus: {seen} texts");
    assert_eq!(added, 7, "the rows that main read as none");
}
