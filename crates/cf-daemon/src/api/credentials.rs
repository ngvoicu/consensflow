//! The tokens of the windows that are open now and the UI token the app opens
//! the human's screens with. **Frozen**.
//!
//! Every window gets a token of its own, issued when the engine opens it and
//! revoked when it closes, so a token names exactly one participant of one
//! project. Only the digests are kept. The UI token opens none of the agents'
//! routes, and a window's opens none of the screens.

use std::cell::RefCell;
use std::collections::HashMap;

use cf_engine::seams::QuestionWindowStart;
use cf_harness::seams::{Entropy, SystemEntropy};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Whose window a token is: one participant of one project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Identity {
    pub participant_id: i64,
    pub project_id: i64,
    pub question_start: Option<QuestionWindowStart>,
}

/// The tokens of the windows that are open now.
pub struct Credentials {
    by_digest: RefCell<HashMap<String, Identity>>,
    entropy: Box<dyn Entropy>,
}

impl Default for Credentials {
    fn default() -> Self {
        Self::new()
    }
}

impl Credentials {
    /// No window has a token yet; tokens are drawn from the system's randomness.
    pub fn new() -> Self {
        Self::with_entropy(Box::new(SystemEntropy))
    }

    /// No window has a token yet; tokens are drawn from `entropy`.
    pub fn with_entropy(entropy: Box<dyn Entropy>) -> Self {
        Self {
            by_digest: RefCell::new(HashMap::new()),
            entropy,
        }
    }

    /// A token for `participant`'s window in `project`: 32 random bytes in
    /// hex, or why the system gave none: no token is made without.
    pub fn issue(&self, project: i64, participant: i64) -> Result<String, String> {
        self.issue_with_start(project, participant, None)
    }

    /// A window's token also remembers which task and brief predated it.
    pub fn issue_for_window(
        &self,
        project: i64,
        participant: i64,
        start: QuestionWindowStart,
    ) -> Result<String, String> {
        self.issue_with_start(project, participant, Some(start))
    }

    fn issue_with_start(
        &self,
        project: i64,
        participant: i64,
        question_start: Option<QuestionWindowStart>,
    ) -> Result<String, String> {
        let token = random_hex(self.entropy.as_ref(), 32)?;
        self.by_digest.borrow_mut().insert(
            digest(&token),
            Identity {
                participant_id: participant,
                project_id: project,
                question_start,
            },
        );
        Ok(token)
    }

    /// The token acts no longer. One that is not here is no matter.
    pub fn revoke(&self, token: &str) {
        self.by_digest.borrow_mut().remove(&digest(token));
    }

    /// Whose window `token` is, or none for one that was never issued, was
    /// revoked, or is no token at all (`None`).
    pub fn resolve(&self, token: Option<&str>) -> Option<Identity> {
        let token = token?;
        self.by_digest.borrow().get(&digest(token)).copied()
    }
}

/// The engine's window tokens are these.
impl cf_engine::seams::Credentials for Credentials {
    fn issue(&self, project: i64, participant: i64) -> Result<String, String> {
        Credentials::issue(self, project, participant)
    }

    fn issue_for_window(
        &self,
        project: i64,
        participant: i64,
        start: QuestionWindowStart,
    ) -> Result<String, String> {
        Credentials::issue_for_window(self, project, participant, start)
    }

    fn revoke(&self, token: &str) {
        Credentials::revoke(self, token);
    }
}

/// `bytes` random bytes from `entropy` as lower-case hex
/// (`randomBytes(n).toString('hex')`), or why the system gave none: a secret is
/// never made without.
pub fn random_hex(entropy: &dyn Entropy, bytes: usize) -> Result<String, String> {
    let mut random = vec![0; bytes];
    entropy
        .fill(&mut random)
        .map_err(|failed| format!("the system gave no randomness: {failed}"))?;
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn digest(token: &str) -> String {
    Sha256::digest(token.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Whether `presented` is the UI token `token`, compared as Node compared
/// them (`timingSafeEqual` of the two digests): in constant time, whatever
/// the lengths.
pub fn token_matches(presented: &str, token: &str) -> bool {
    let (presented, token) = (
        Sha256::digest(presented.as_bytes()),
        Sha256::digest(token.as_bytes()),
    );
    bool::from(presented.ct_eq(&token))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_32_random_bytes_in_hex_and_each_is_new() {
        let credentials = Credentials::new();
        let (one, other) = (
            credentials.issue(1, 2).unwrap(),
            credentials.issue(1, 2).unwrap(),
        );
        for token in [&one, &other] {
            assert_eq!(token.len(), 64);
            assert!(token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()));
        }
        assert_ne!(one, other);
    }

    #[test]
    fn a_token_names_one_participant_of_one_project_until_it_is_revoked() {
        let credentials = Credentials::new();
        let zeus = credentials.issue(3, 7).unwrap();
        let hera = credentials.issue(3, 8).unwrap();
        assert_eq!(
            credentials.resolve(Some(&zeus)),
            Some(Identity {
                participant_id: 7,
                project_id: 3,
                question_start: None,
            })
        );
        assert_eq!(
            credentials
                .resolve(Some(&hera))
                .map(|who| who.participant_id),
            Some(8)
        );
        credentials.revoke(&zeus);
        assert_eq!(credentials.resolve(Some(&zeus)), None);
        assert!(
            credentials.resolve(Some(&hera)).is_some(),
            "the others stay"
        );
        credentials.revoke(&zeus);
        credentials.revoke("never issued");
    }

    #[test]
    fn the_engines_window_token_keeps_its_launch_boundary() {
        let credentials = Credentials::new();
        let start = QuestionWindowStart {
            last_brief: Some(10),
            task_in_hand: Some(20),
        };
        let token = <Credentials as cf_engine::seams::Credentials>::issue_for_window(
            &credentials,
            3,
            7,
            start,
        )
        .unwrap();
        assert_eq!(
            credentials.resolve(Some(&token)).unwrap().question_start,
            Some(start)
        );
    }

    #[test]
    fn what_was_never_issued_resolves_to_nobody() {
        let credentials = Credentials::new();
        credentials.issue(1, 1).unwrap();
        assert_eq!(credentials.resolve(None), None);
        assert_eq!(credentials.resolve(Some("")), None);
        assert_eq!(credentials.resolve(Some("not a token")), None);
    }

    #[test]
    fn only_digests_are_kept() {
        let credentials = Credentials::new();
        let token = credentials.issue(1, 1).unwrap();
        let kept = credentials.by_digest.borrow();
        assert!(!kept.contains_key(&token));
        assert_eq!(kept.keys().next().map(String::len), Some(64));
    }

    #[test]
    fn the_ui_token_matches_itself_only() {
        let token = random_hex(&SystemEntropy, 24).unwrap();
        assert_eq!(token.len(), 48);
        assert!(token_matches(&token, &token));
        assert!(!token_matches("", &token));
        assert!(!token_matches(&token[..47], &token));
        assert!(!token_matches(&format!("{token}0"), &token));
        assert!(!token_matches(&token.to_uppercase(), &token));
    }

    #[test]
    fn the_engine_s_issue_and_revoke_are_this_one_s() {
        let credentials = Credentials::new();
        let token = cf_engine::seams::Credentials::issue(&credentials, 4, 9).unwrap();
        assert_eq!(
            credentials.resolve(Some(&token)).map(|who| who.project_id),
            Some(4)
        );
        cf_engine::seams::Credentials::revoke(&credentials, &token);
        assert_eq!(credentials.resolve(Some(&token)), None);
    }

    /// A system that gives no randomness.
    struct NoEntropy;

    impl Entropy for NoEntropy {
        fn fill(&self, _bytes: &mut [u8]) -> Result<(), String> {
            Err("unavailable".to_owned())
        }
    }

    #[test]
    fn a_system_that_gives_no_randomness_makes_no_token_and_says_so() {
        let credentials = Credentials::with_entropy(Box::new(NoEntropy));
        assert_eq!(
            credentials.issue(1, 2),
            Err("the system gave no randomness: unavailable".to_owned())
        );
        assert!(credentials.by_digest.borrow().is_empty());
        assert_eq!(
            random_hex(&NoEntropy, 24),
            Err("the system gave no randomness: unavailable".to_owned())
        );
    }
}
