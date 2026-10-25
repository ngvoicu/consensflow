//! `release alpha`: the version the sources agree on, `X.Y.Z-alpha.N`, from
//! `main` as upstream has it, to `feed-alpha` alone. The steps are the module's
//! above; this is their order.

use cf_publish::version::Version;
use cf_release::version::source_version;

use super::github::Watched;
use super::{notes_at, plain_of, refused, short, Error, Options, Release, ALPHA_FEED, STABLE_FEED};

pub(super) fn release(release: &mut Release, options: &Options) -> Result<String, Error> {
    let version = source_version(&release.root).map_err(|cause| refused(cause.to_string()))?;
    plain_of(&version)?;
    let notes = notes_at(&options.notes)?;
    let commit = release.main_as_upstream_has_it()?;
    let tag = format!("v{version}");
    release.tag_is_new(&tag)?;

    // The publisher never moves a feed backward: a version feed-alpha does not
    // come before would be released and never served.
    let served = release.served_by(ALPHA_FEED)?;
    let ours = Version::parse(&version).map_err(|cause| refused(cause.to_string()))?;
    if served >= ours {
        return Err(refused(format!(
            "{ALPHA_FEED} serves {served}, which {version} does not come after: set a later version"
        )));
    }
    let stable_before = release.served_by(STABLE_FEED)?;
    release.say(&format!(
        "{version} on {}: {ALPHA_FEED} serves {served}",
        short(&commit)
    ));

    release.watch(&Watched {
        label: "ci",
        workflow: "ci.yml",
        commit: &commit,
        event: None,
        before: None,
        both_platforms: true,
        tag: None,
    })?;
    let before = release.newest_run("release.yml", Some(&commit), Some("workflow_dispatch"))?;
    release.dispatch("release.yml", "main")?;
    release.watch(&Watched {
        label: "dry run",
        workflow: "release.yml",
        commit: &commit,
        event: Some("workflow_dispatch"),
        before,
        both_platforms: false,
        tag: None,
    })?;
    if options.dry_run {
        return Ok(format!(
            "dry run: every check passed; {tag} would be tagged on {} with {} and pushed to origin and upstream\n",
            short(&commit),
            notes.display()
        ));
    }

    let before = release.newest_run("release.yml", Some(&commit), Some("push"))?;
    release.tag_and_push(&tag, &notes, &commit)?;
    let run = release.watch(&Watched {
        label: "release",
        workflow: "release.yml",
        commit: &commit,
        event: Some("push"),
        before,
        both_platforms: false,
        tag: Some(&tag),
    })?;
    release.wait_for_feed(ALPHA_FEED, &version, run)?;
    // By the release it names, read as the one before was: a 404 is no change.
    let stable_after = release.served_after(STABLE_FEED, run, &stable_before)?;
    if stable_after != stable_before {
        return Err(refused(format!(
            "{STABLE_FEED} serves {stable_after}, and served {stable_before} before {version} was released: \
             an alpha never moves it; read the publish job of run {run}"
        )));
    }
    Ok(format!(
        "released {version} on {}: {ALPHA_FEED} serves it, {STABLE_FEED} is as it was\n",
        short(&commit)
    ))
}
