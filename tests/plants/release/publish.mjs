/**
 * Plants in the publisher (tools/cf-publish: `publish`, `gh` and `cli`): the
 * versioned release made a draft and published whole, a run cut short finished
 * by running it again, a feed's latest.json never deleted for its replacement,
 * and only the push of a tag publishing. The publisher's tests, against GitHub
 * as its simulator (`cf_publish::testing`) has it, and the workflow's steps run
 * as written, must catch each.
 */
import { CLI_RS, FEED_RS, GH_RS, PUBLISH, PUBLISH_RS, RERUN } from './kit.mjs'

const plant = (name, file, from, to, meant) => ({
  name: `publish: ${name}`,
  edits: [[file, from, to]],
  runs: [PUBLISH],
  meant,
})

/** The guard on the environment of the run: the event and the kind of ref. */
const GUARD =
  'if env.event_name.as_deref() != Some("push") || env.ref_type.as_deref() != Some("tag") {'
/** Where a feed's latest.json is read to tell whether it cannot be, or is not served at all. */
const UNREAD = 'if refusal.status != Some(404) {'
/** Where a feed's latest.json is read, for the release it names. */
const SERVED = 'let served = reads.file(&format!("{}/{feed}/{LATEST}", run.base));'

export const PLANTS = [
  plant(
    'the old latest.json is deleted before the new one is uploaded',
    FEED_RS,
    `fn swap(run: &Run, feed: &str) -> Result<(), Failure> {
    let runner = &run.runner;
`,
    `fn swap(run: &Run, feed: &str) -> Result<(), Failure> {
    let runner = &run.runner;
    runner.must(&["release", "delete-asset", feed, LATEST, "--yes"], "deleting")?;
`,
    'leaves_the_old_latest_json_where_it_was_when_the_upload',
  ),
  plant(
    'the old latest.json is deleted, not set aside, so it cannot be put back',
    FEED_RS,
    'if let Err(cause) = runner.rename(run.repo, held.asset(LATEST), PREVIOUS) {',
    'if let Err(cause) = runner.must(&["release", "delete-asset", feed, LATEST, "--yes"], "deleting").map(drop) {',
    'puts_the_old_latest_json_back_when_the_second_rename_fails',
  ),
  plant(
    'the previous latest.json is not put back when the second rename fails',
    FEED_RS,
    'if let Err(again) = runner.rename(run.repo, aside.as_ref(), LATEST) {',
    'if let Err(again) = Ok::<(), Failure>(drop(aside)) {',
    'puts_the_old_latest_json_back_when_the_second_rename_fails',
  ),
  plant(
    'a rename of a file that is not there is tried all the same',
    GH_RS,
    `        let Some(asset) = asset else {
            return Err(Failure::new(format!("there is no file to rename to {to}")));
        };`,
    `        let missing = Asset {
            name: String::new(),
            size: None,
            state: None,
            api_url: Some("/releases/assets/1".to_string()),
        };
        let asset = asset.unwrap_or(&missing);`,
    'says_what_is_missing_when_the_live_latest_json_is_gone',
  ),
  plant(
    'a feed that already serves this latest.json is swapped all the same',
    FEED_RS,
    'if differs {',
    'if differs || true {',
    'leaves_a_feed_that_already_serves_this_latest_json_untouched',
  ),
  plant(
    'a previous latest.json left by a cut swap is not put back first',
    FEED_RS,
    'if !state.has(LATEST) && state.has(PREVIOUS) {',
    'if false {',
    'puts_the_previous_latest_json_back_first',
  ),
  plant(
    'the versioned release is made public, not as a draft',
    PUBLISH_RS,
    'let mut args = vec!["release", "create", tag, "--draft", "--verify-tag"];',
    'let mut args = vec!["release", "create", tag, "--verify-tag"];',
    'makes_the_release_a_draft_and_publishes_it_only_once_every_file_is_there',
  ),
  plant(
    'the release is not made a prerelease',
    PUBLISH_RS,
    `            if run.version.is_prerelease() {
                args.push("--prerelease");`,
    `            if false {
                args.push("--prerelease");`,
    'makes_the_release_a_draft_and_publishes_it_only_once_every_file_is_there',
  ),
  plant(
    'a draft is published without looking at its files',
    PUBLISH_RS,
    'if !whole {',
    'if false {',
    'does_not_publish_a_draft_whose_file_is_short',
  ),
  plant(
    'a draft left by a run that died is not emptied',
    PUBLISH_RS,
    'runner.must(&args, &format!("emptying the draft {tag}"))?;',
    'let _ = args;',
    'empties_a_draft_that_holds_some_of_the_files',
  ),
  plant(
    'a published release that lacks a file is kept as it is',
    PUBLISH_RS,
    'if lacking.is_empty() {',
    'if true {',
    'adds_what_a_published_release_lacks',
  ),
  plant(
    "a published release's files are not held to the files built",
    PUBLISH_RS,
    'if !wrong.is_empty() {',
    'if false {',
    'refuses_a_published_release_whose_file_is_not_the_one_built',
  ),
  plant(
    'the rule is not asked before anything moves',
    PUBLISH_RS,
    `    if !problems.is_empty() {
        return Err(Failure::new(format!(
            "{version} may not move a feed`,
    `    if false {
        return Err(Failure::new(format!(
            "{version} may not move a feed`,
    'is_refused_while_the_old_feed_still_serves_the_release_before_the_bridge',
  ),
  plant(
    'a failure to look at a release is taken for its absence',
    GH_RS,
    `        Err(Failure::new(format!(
            "could not look at the release {tag}: {}",
            answer.stderr.trim()
        )))`,
    '        Ok(None)',
    'stops_where_gh_cannot_tell_whether_a_release_is_there',
  ),
  plant(
    'the old feed is not marked as pinned',
    FEED_RS,
    'if pinned {',
    'if false {',
    'marks_the_old_feed_as_pinned_to_the_bridge',
  ),
  plant(
    'a hand run on a tag publishes: only the ref type is asked',
    CLI_RS,
    GUARD,
    'if env.ref_type.as_deref() != Some("tag") {',
    'refuses_a_hand_run_though_it_is_on_a_tag',
  ),
  plant(
    'any run publishes',
    CLI_RS,
    GUARD,
    'if false {',
    'refuses_a_push_of_a_branch_a_run_outside_a_workflow',
  ),
  plant(
    'a tag other than the one pushed is published',
    CLI_RS,
    'if env.ref_name.as_deref() != Some(tag) {',
    'if false {',
    'refuses_a_push_of_a_branch_a_run_outside_a_workflow',
  ),
  // No feed is moved backward: a run again after a later release went out leaves its feed at that release.
  plant(
    'a feed is swapped whatever release it names: the feed is not asked',
    FEED_RS,
    'if let Some(later) = later {',
    'if let Some(later) = later.filter(|_| false) {',
    RERUN,
  ),
  plant(
    'a feed that names a later release fails the run instead of being left alone',
    FEED_RS,
    'did = FeedDone::Superseded(later);',
    'return Err(Failure::new("superseded"));',
    RERUN,
  ),
  plant(
    'a feed that was left alone is said to be kept',
    FEED_RS,
    'did = FeedDone::Superseded(later);',
    'did = FeedDone::Kept;',
    RERUN,
  ),
  plant(
    'a feed that is left alone is not said to be, in the log',
    FEED_RS,
    `            runner.log(&format!(
                "{feed} names {later}, which comes after {}: it is left alone, and nothing is moved backward",
                run.version
            ));`,
    '            let _ = runner;',
    RERUN,
  ),
  plant(
    'a feed that cannot be read is swapped all the same',
    FEED_RS,
    UNREAD,
    'if false {',
    'changes_no_feed_it_cannot_read',
  ),
  plant(
    'a feed that is listed and not served is left as it is, not mended',
    FEED_RS,
    UNREAD,
    'if true {',
    'swaps_a_latest_json_that_is_listed_and_not_served_at_all',
  ),
  plant(
    'a feed that is listed and not served is taken for one that names a later release',
    FEED_RS,
    `        let later = served
            .as_ref()
            .ok()
            .and_then(|body| later_release(body, run.version));`,
    `        let later = served.as_ref().map_or_else(
            |_| Some(run.version.clone()),
            |body| later_release(body, run.version),
        );`,
    'swaps_a_latest_json_that_is_listed_and_not_served_at_all',
  ),
  plant(
    'a feed is read once, not again past a blip',
    FEED_RS,
    SERVED,
    'let served = reads.once(&format!("{}/{feed}/{LATEST}", run.base));',
    'reads_a_feed_again_past_a_blip',
  ),
  // What the check that follows is left to hold the new feeds to.
  plant(
    'the check is left no record of what the feeds named',
    PUBLISH_RS,
    `    write(
        dir,
        FEEDS_BEFORE,
        format!("{}\\n", Value::Object(before)).as_bytes(),
    )?;`,
    '    let _ = before;',
    'leaves_the_check_a_record_of_what_each_new_feed_named',
  ),
  plant(
    'the record says what the run publishes, not what the feed named',
    FEED_RS,
    'named = served.as_ref().ok().and_then(|body| named_release(body));',
    'named = Some(run.version.clone());',
    'leaves_the_check_a_record_of_what_each_new_feed_named',
  ),
  plant(
    'the record holds the old feed too',
    PUBLISH_RS,
    'if !pinned {',
    'if true {',
    'leaves_the_check_a_record_of_what_each_new_feed_named',
  ),
]
