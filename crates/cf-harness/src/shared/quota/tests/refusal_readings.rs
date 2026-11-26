//! The readers read every account-refusal text of the repository's fixtures and tests, and the
//! probe inputs of the reviews of T-2350, T-2351 and T-2361, as the base did (`3375b7162`,
//! T-2353): the reading of `account_refusal`, the reason `unavailable_quota` writes for an
//! account's refusal and the reason `busy_quota` writes for a refusal for a moment, the readers'
//! own cut at 300 characters included. The expectations in `account-refusals.jsonl` are the
//! base's own, taken by running the base's `quota` module beside this one. Two readings were
//! added, both sure: Anthropic's refresh refused with `invalid_grant`, and Pi's published Meta
//! errors ("session expired (status 401|403)"). Those rows say `added`. No rule reads an expired
//! token by its words: it is a login refusal.

use super::*;

fn read(refusal: Option<Refusal>) -> &'static str {
    match refusal {
        Some(Refusal::Billing) => "billing",
        Some(Refusal::Credentials) => "login",
        None => "none",
    }
}

#[test]
fn every_text_of_the_repository_s_fixtures_reads_as_the_base_read_it_but_for_two_sure_additions() {
    let rows = include_str!("account-refusals.jsonl");
    let (mut seen, mut added, mut cut) = (0, 0, 0);
    for row in rows.lines() {
        let row: serde_json::Value = serde_json::from_str(row).unwrap();
        let text = row["text"].as_str().unwrap();
        let status = row["status"].as_f64();
        let refusal = account_refusal(text, status);
        let reading = read(refusal);
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
            if let (Some(refusal), Some(base)) = (refusal, row["unavailable"].as_str()) {
                let Ok(Quota::Unavailable { because, .. }) = unavailable_quota(1.0e12, refusal, text)
                else {
                    panic!("unavailable: {text}");
                };
                assert_eq!(because, base, "the reason of an account's refusal: {text}");
            }
        }
        let Ok(Quota::Busy { because, .. }) = busy_quota(1.0e12, text) else {
            panic!("busy: {text}");
        };
        assert_eq!(because, row["busy"].as_str().unwrap(), "the busy reason: {text}");
        if text.trim().chars().count() > 300 {
            cut += 1;
            assert_eq!(because.chars().count(), 300, "{text}");
        }
    }
    assert!(seen >= 130, "the corpus: {seen} texts");
    assert_eq!(added, 8, "the rows that the base read as none");
    assert!(cut >= 3, "texts the readers cut: {cut}");
}
