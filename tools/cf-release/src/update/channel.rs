//! The channels a build can be published to. Installed apps follow one of two
//! feeds: `alpha` takes the alpha prereleases and the stable releases, `stable`
//! only the stable ones.

use crate::version;

/// A feed's channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Alpha,
    Stable,
}

/// A build the channel will not take, or no such channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChannelError {
    #[error("channel must be alpha or stable")]
    Unknown,
    #[error("stable channel requires a stable release")]
    NotStable,
    #[error("alpha channel accepts only alpha prereleases or stable releases")]
    NotAlpha,
}

impl Channel {
    /// The channel `name` names: exactly `alpha` or `stable`.
    pub fn named(name: &str) -> Result<Self, ChannelError> {
        match name {
            "alpha" => Ok(Self::Alpha),
            "stable" => Ok(Self::Stable),
            _ => Err(ChannelError::Unknown),
        }
    }

    /// Whether `version`, a canonical semantic version, may be published here.
    pub fn admits(self, version: &str) -> Result<(), ChannelError> {
        let Some(prerelease) = version::prerelease(version) else {
            return Ok(());
        };
        match self {
            Self::Stable => Err(ChannelError::NotStable),
            // An alpha prerelease is one whose first identifier is `alpha`.
            Self::Alpha if prerelease.split('.').next() == Some("alpha") => Ok(()),
            Self::Alpha => Err(ChannelError::NotAlpha),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_channel_is_alpha_or_stable_and_nothing_else() {
        assert_eq!(Channel::named("alpha"), Ok(Channel::Alpha));
        assert_eq!(Channel::named("stable"), Ok(Channel::Stable));
        for name in [
            "beta", "Alpha", "STABLE", "", " alpha", "alpha ", "alpha\n", "release",
        ] {
            let refused = Channel::named(name).unwrap_err();
            assert_eq!(refused, ChannelError::Unknown, "{name:?}");
            assert_eq!(refused.to_string(), "channel must be alpha or stable");
        }
    }

    #[test]
    fn a_stable_release_goes_to_either_channel() {
        for version in ["3.0.0", "0.0.1", "10.20.30"] {
            assert_eq!(Channel::Alpha.admits(version), Ok(()), "{version}");
            assert_eq!(Channel::Stable.admits(version), Ok(()), "{version}");
        }
    }

    #[test]
    fn only_a_stable_release_goes_to_the_stable_channel() {
        for version in ["3.0.0-alpha.99", "3.0.0-beta.1", "3.0.0-rc.1", "3.0.0-0"] {
            let refused = Channel::Stable.admits(version).unwrap_err();
            assert_eq!(refused, ChannelError::NotStable, "{version}");
            assert_eq!(
                refused.to_string(),
                "stable channel requires a stable release"
            );
        }
    }

    #[test]
    fn the_alpha_channel_takes_prereleases_that_begin_with_alpha() {
        for version in ["3.0.0-alpha.99", "3.0.0-alpha", "3.0.0-alpha.1.x"] {
            assert_eq!(Channel::Alpha.admits(version), Ok(()), "{version}");
        }
        for version in [
            "3.0.0-beta.1",
            "3.0.0-rc.1",
            "3.0.0-alpha1",
            "3.0.0-alpha-1",
            "3.0.0-Alpha.1",
            "3.0.0-x.alpha",
            "3.0.0-0.alpha",
        ] {
            let refused = Channel::Alpha.admits(version).unwrap_err();
            assert_eq!(refused, ChannelError::NotAlpha, "{version}");
            assert_eq!(
                refused.to_string(),
                "alpha channel accepts only alpha prereleases or stable releases"
            );
        }
    }
}
