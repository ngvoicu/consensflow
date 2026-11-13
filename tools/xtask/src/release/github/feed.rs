//! Reading the feeds as installed apps read them.

use cf_publish::version::{named_release, Version};

use super::super::{refused, Error, Release, DOWNLOADS};
use crate::process::Invocation;

/// One read of a feed.
enum Read {
    /// It serves this release.
    Serves(Version),
    /// It answered, with no release in it, or with an HTTP error (404 among them).
    Nothing,
    /// It could not be reached: what curl said.
    Unreachable(String),
}

impl Release<'_> {
    /// The address installed apps read the feed `name` at.
    pub(in crate::release) fn feed_address(name: &str) -> String {
        format!("{DOWNLOADS}/{name}/latest.json")
    }

    /// One read of the feed `name`.
    fn read_feed(&mut self, name: &str) -> Result<Read, Error> {
        let invocation = Invocation::new("curl", &self.root)
            .args(["-sfL", "--max-time", "60"])
            .arg(Self::feed_address(name));
        let answered = self.system.capture(&invocation)?;
        Ok(match answered.code {
            0 => named_release(answered.stdout.as_bytes()).map_or(Read::Nothing, Read::Serves),
            // curl's status for an HTTP error, 404 among them, with -f.
            22 => Read::Nothing,
            code => Read::Unreachable(format!("curl ended {code}: {}", answered.stderr.trim())),
        })
    }

    /// Reads the feed `name` until `wanted` says what it serves is the answer,
    /// within the reads the pace allows: the last thing it served or answered
    /// when none was.
    fn read_until(
        &mut self,
        name: &str,
        wanted: impl Fn(&Version) -> bool,
    ) -> Result<Result<Version, String>, Error> {
        let mut last = String::from("no answer");
        for attempt in 0..self.pace.feed_attempts {
            if attempt > 0 {
                self.system.pause(self.pace.feed);
            }
            match self.read_feed(name)? {
                Read::Serves(version) if wanted(&version) => return Ok(Ok(version)),
                Read::Serves(version) => last = format!("it serves {version}"),
                Read::Nothing => last = "it serves no release".to_owned(),
                Read::Unreachable(said) => last = said,
            }
        }
        Ok(Err(last))
    }

    /// The release the feed `name` serves, read again on a 404 or a network
    /// error; a feed that never answers one is an outage, and refused.
    pub(in crate::release) fn served_by(&mut self, name: &str) -> Result<Version, Error> {
        self.served(name, || "nothing was done, run again later".to_owned())
    }

    /// The same, after the Release run `run` ended green, when `name` served
    /// `was` before it: an outage then says the release is out, and to read
    /// the feed by hand.
    pub(in crate::release) fn served_after(
        &mut self,
        name: &str,
        run: u64,
        was: &Version,
    ) -> Result<Version, Error> {
        self.served(name, || {
            format!(
                "release run {run} ended green, so the release is published; read {name} by hand, \
                 `curl -sL {}`, which served {was} before it, and run nothing again",
                Self::feed_address(name)
            )
        })
    }

    fn served(&mut self, name: &str, then: impl Fn() -> String) -> Result<Version, Error> {
        let reads = self.pace.feed_attempts;
        self.read_until(name, |_| true)?.map_err(|last| {
            refused(format!(
                "{name} answered no release in {reads} reads ({last}): an outage of the feed or of GitHub, \
                 not a version to change; {}",
                then()
            ))
        })
    }

    /// Reads the feed `name` until it serves `version`, after the Release run
    /// `run` ended green, and says what to do when it still does not.
    pub(in crate::release) fn wait_for_feed(
        &mut self,
        name: &str,
        version: &str,
        run: u64,
    ) -> Result<(), Error> {
        let reads = self.pace.feed_attempts;
        match self.read_until(name, |served| served.as_str() == version)? {
            Ok(_) => {
                self.say(&format!("{name} serves {version}"));
                Ok(())
            }
            Err(last) => Err(refused(format!(
                "release run {run} ended green, its own check of the feeds with it, yet {name} \
                 does not serve {version} after {reads} reads ({last}): the release is published, and \
                 GitHub's download links can lag; read it again in a few minutes, `curl -sL {}`, \
                 and run nothing again",
                Self::feed_address(name)
            ))),
        }
    }
}
