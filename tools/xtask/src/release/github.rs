//! What a release asks of GitHub: its workflow runs, through `gh`, and the
//! feeds, through `curl` at the address installed apps read them at.
//!
//! A run is watched as release-305.sh watched it: the newest run of the
//! workflow (on the commit, by the event) that is not the one that was newest
//! before it was asked for, then looked at until it has ended. `gh run watch`
//! can return before a run ends, so only the run's own status is read.
//!
//! A hand run of the Release workflow (the trial, [`Release::trial`]) is started
//! with `gh workflow run` and watched the same way, by the event that started it.
//! A watched run that does not end green is refused with what failed in it, the
//! job and the step, and the address to read it at. A run is bound to the commit
//! it is watched for twice: it is looked for by that commit, and once found, its
//! own `headSha` must be it. A `gh` that cannot be asked about a run, once it is
//! asked for, is a refusal too, naming the run and what is left of it.
//!
//! CI is not watched by its newest run: whether it passed a commit is asked as
//! the release's own gate asks it (cf-release's `ci_passed::verdict`, through
//! `gh api`), so that a commit the command tags is one the tag's run takes, and
//! one it refuses is one the tag's run would refuse. It is asked again while a
//! run of CI on the commit goes on, or none has appeared yet.
//!
//! A feed is read as installed apps read it, and read again on a 404 or a
//! network error within the reads the pace allows: a feed just swapped can
//! answer 404 for minutes while GitHub's download links catch up.

mod ci;
mod feed;
mod runs;
mod trial;

pub(super) use runs::{finish_by_hand, rerun_advice, Left, Watched};
