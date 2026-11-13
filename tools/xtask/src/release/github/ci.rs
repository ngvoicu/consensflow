//! Whether CI's gate passed the commit.

use cf_release::ci_passed::{self, GitHub, Refusal};

use super::super::{refused, short, Error, Release, REPOSITORY};
use super::runs::Left;

impl Release<'_> {
    /// Waits until CI's full gate passed `commit` on both platforms, by the
    /// rule the tag's gate holds it to (a run of `ci.yml` by a push or by hand,
    /// green with both summaries), and refuses it once CI ended without: what
    /// was found, then what `left` says to do. A run still going, or none yet,
    /// is waited for. The run that passed it.
    pub(in crate::release) fn ci_passed(&mut self, commit: &str, left: Left) -> Result<u64, Error> {
        let mut looks = 0;
        let mut said_going = false;
        loop {
            match ci_passed::verdict(&mut Api { release: self }, REPOSITORY, commit) {
                Ok(passed) => {
                    self.say(&format!(
                        "ci: run {} passed {} on both platforms",
                        passed.run,
                        short(commit)
                    ));
                    for red in &passed.red {
                        self.say(&format!("ci: {red}, beside it"));
                    }
                    return Ok(passed.run);
                }
                Err(Refusal::Running(said)) => {
                    if !said_going {
                        self.say(&format!(
                            "ci: waiting until CI's run of {} ends ({said})",
                            short(commit)
                        ));
                        said_going = true;
                    }
                    self.system.pause(self.pace.poll);
                }
                Err(Refusal::NeverRan(_)) if looks < self.pace.appear_attempts => {
                    looks += 1;
                    self.system.pause(self.pace.appear);
                }
                Err(Refusal::NeverRan(said)) => {
                    return Err(refused(format!("{said}; {}", left.then(None))));
                }
                Err(Refusal::Red { said, run }) => {
                    return Err(refused(format!("{said}; {}", left.then(Some(run)))));
                }
                Err(Refusal::Unasked(said)) => {
                    return Err(Error::Answer {
                        what: format!("whether CI passed {}", short(commit)),
                        said,
                    });
                }
            }
        }
    }
}

/// GitHub's API as the release's gate asks it, through the machine's `gh`.
struct Api<'r, 'a> {
    release: &'r mut Release<'a>,
}

impl GitHub for Api<'_, '_> {
    fn get(&mut self, path: &str) -> Result<String, String> {
        self.release
            .gh(&["api", path])
            .map_err(|error| error.to_string())
    }
}
