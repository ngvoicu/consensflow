//! A secret a launch hands a window: 24 drawn bytes, as
//! `randomBytes(24).toString('base64url')` writes them (Codex's broker
//! token, OpenCode's password and its plugin's token).

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

use crate::seams::Entropy;

/// A new secret, or why the system would not give its randomness.
pub(crate) fn draw(entropy: &dyn Entropy) -> Result<String, String> {
    let mut drawn = [0; 24];
    entropy.fill(&mut drawn)?;
    Ok(URL_SAFE_NO_PAD.encode(drawn))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::ScriptedEntropy;

    #[test]
    fn a_token_is_24_bytes_drawn_written_as_node_writes_them_in_base64url() {
        let entropy = ScriptedEntropy::default();
        // What Node's `toString('base64url')` writes of the same bytes.
        assert_eq!(
            draw(&entropy).as_deref(),
            Ok("AwoRGB8mLTQ7QklQV15lbHN6gYiPlp2k")
        );
        assert_eq!(entropy.take_draws(), [24]);
    }
}
