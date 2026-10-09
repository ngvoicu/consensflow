/**
 * Plants in the rule of the feeds and its checks (tools/cf-publish: `rule`,
 * `checks`, `version`, `assets` and `read`): which release is the bridge, what a
 * later release finds true before it moves a feed, what the feeds serve once it
 * has, and that no feed is moved backward (the comparison of what a feed names
 * with the release run, and what the check accepts of a feed left alone). The
 * publisher's tests of the rule, against GitHub as its simulator
 * (`cf_publish::testing`) has it, must catch each.
 */
import {
  ASSETS_RS,
  CHECKS_RS,
  FEEDS,
  PUBLISH,
  READ_RS,
  RERUN,
  RULE_RS,
  VERSION_RS,
} from './kit.mjs'

const plant = (name, file, from, to, meant, runs = [FEEDS]) => ({
  name: `feeds: ${name}`,
  edits: [[file, from, to]],
  runs,
  meant,
})

/** Where a feed's latest.json is asked whether it names a release after the run's own. */
const COMPARISON = 'named_release(body).filter(|named| named > version)'
/** The tests of the rule that say which release a feed names, and what the publisher does about it. */
const ASKED = 'says_which_release_a_feed_names_where_that_one_is_after_a_given_release'
/** The test of a feed held to what it named before the publisher changed it. */
const BEFORE = 'an_earlier_release_than_it_did_when_the_publisher_read_it'
/** What the check accepts of a new feed: this release's latest.json, or a later release's. */
const ACCEPTS = 'Ok(body) if *body == latest || later_release(body, version).is_some() => body,'
/** The tests of what a check does not approve of a new feed. */
const NOT_APPROVED =
  'does_not_approve_a_feed_that_names_an_earlier_release_this_one_with_other_bytes_or_none'
/** Where a feed's record of what it named before is told to be gone backward. */
const WENT_BACKWARD = 'if now < *was {'

/**
 * The comparison planted wrong, met twice: by the rule's own test of it, and by
 * the publisher run end to end against GitHub as the tests have it.
 */
const compared = (name, to, rerun = RERUN) => [
  plant(`${name} (asked of the rule)`, VERSION_RS, COMPARISON, to, ASKED),
  plant(`${name} (a run again, end to end)`, VERSION_RS, COMPARISON, to, rerun, [PUBLISH]),
]

export const PLANTS = [
  plant(
    'the bridge is taken for a later release',
    RULE_RS,
    `    Ok(if order == Ordering::Equal {
        Role::Bridge
    } else {
        Role::Later
    })`,
    '    Ok(Role::Later)',
    'moves_the_bridge_to_the_new_feed_and_the_old_feed',
  ),
  plant(
    'a release before the bridge is let through',
    RULE_RS,
    'if order == Ordering::Less {',
    'if order == Ordering::Less && false {',
    'refuses_a_release_before_the_bridge',
  ),
  plant(
    'a later release moves the old feeds too',
    RULE_RS,
    'if role_of(version, manifest)? == Role::Later {',
    'if role_of(version, manifest)? == Role::Later && false {',
    'moves_a_later_release_to_the_new_feeds_only',
  ),
  plant(
    'an old feed is held to the bridge by what it names, not byte for byte',
    CHECKS_RS,
    'if found == published {',
    'if found == published || true {',
    'refuses_where_the_old_feed_still_serves_the_release_before_the_bridge',
  ),
  plant(
    'an old feed that cannot be read is taken for absent',
    CHECKS_RS,
    `                problems.push(format!(
                    "{feed} cannot be read ({}): the apps before the bridge read it",
                    refusal.why
                ));`,
    '                let _ = refusal;',
    'refuses_a_feed_that_is_not_there_a_required_feed_is_not_absent',
  ),
  plant(
    "the bridge's archive is asked to be served, not to be the one published",
    CHECKS_RS,
    'Ok(got) if got != *wanted => problems.push(format!(',
    'Ok(got) if got != *wanted && false => problems.push(format!(',
    'refuses_a_bridge_archive_that_serves_junk',
  ),
  plant(
    "the bridge's Windows files are not asked for",
    ASSETS_RS,
    'vec![&self.archive, &self.installer, &self.portable]',
    'vec![&self.archive]',
    'refuses_the_bridges_windows_installer_or_portable_that_is_gone',
  ),
  plant(
    'the bridge is not looked for after the release is published',
    CHECKS_RS,
    'problems.extend(bridge_problems(base, manifest, &mut reads));',
    'let _ = bridge_problems;',
    'finds_the_bridge_not_carried_to_the_old_feed',
  ),
  plant(
    'a later release finds nothing to check before it moves',
    CHECKS_RS,
    'Ok(Role::Later) => bridge_problems(base, manifest, &mut Reader::new(patience)),',
    'Ok(Role::Later) => Vec::new(),',
    'refuses_where_the_old_feed_still_serves_the_release_before_the_bridge',
  ),
  plant(
    'only the archive of the release is held to the file built',
    CHECKS_RS,
    'for path in assets.all().into_iter().chain(["SHA256SUMS"]) {',
    'for path in [assets.archive.as_str()] {',
    'finds_a_file_of_the_release_that_does_not_download_as_the_one_built',
  ),
  plant(
    'a feed served stale is read once',
    READ_RS,
    `        until(
            self.patience,
            || fetch_body(url),
            |found| match found {`,
    `        until(
            Patience::new(1, self.patience.wait),
            || fetch_body(url),
            |found| match found {`,
    'waits_out_an_old_feed_served_stale_for_a_moment',
  ),
  plant(
    'the old feed of a channel not in use may not be absent',
    CHECKS_RS,
    'Err(refusal) if refusal.status == Some(404) => continue,',
    'Err(refusal) if false && refusal.status == Some(404) => continue,',
    'lets_the_old_feed_of_a_channel_not_in_use_be_absent',
  ),
  plant(
    'versions are compared as text',
    VERSION_RS,
    'a.len().cmp(&b.len()).then_with(|| a.cmp(b))',
    'a.cmp(b)',
    'orders_versions_as_semantic_versioning_does',
  ),
  plant(
    "the bridge's latest.json is not asked to name the bridge",
    CHECKS_RS,
    `    if document.get("version").and_then(Value::as_str) != Some(version.as_str()) {
        problems.push(format!(
            "the bridge's {tag}/latest.json names {}, not {version}",`,
    `    if false {
        problems.push(format!(
            "the bridge's {tag}/latest.json names {}, not {version}",`,
    'refuses_a_bridge_whose_latest_json_names_another_release',
  ),
  plant(
    "a release's latest.json is not asked to name its own archive",
    CHECKS_RS,
    'if archive_named(&document).and_then(Value::as_str) != Some(wanted.as_str()) {',
    'if false {',
    'finds_a_latest_json_that_names_no_archive_of_this_release',
  ),
  plant(
    'a missing SHA256SUMS is let pass',
    CHECKS_RS,
    `            problems.push(format!(
                "the bridge's {tag}/SHA256SUMS cannot be read ({})",
                refusal.why
            ));
            return problems;`,
    `            let _ = refusal;
            return problems;`,
    'refuses_where_sha256sums_is_gone',
  ),
  // No feed is moved backward: the comparison of what a feed names with the release run, planted wrong.
  ...compared(
    'no feed is ever taken for one that names a later release',
    'named_release(body).filter(|named| named > version && false)',
  ),
  ...compared(
    'every feed that names a release is taken for one that names a later release',
    'named_release(body).filter(|named| named > version || true)',
  ),
  ...compared(
    'the comparison is reversed: an earlier release is taken for a later one',
    'named_release(body).filter(|named| named < version)',
  ),
  ...compared(
    'the very same release is taken for a later one',
    'named_release(body).filter(|named| named >= version)',
    'for_equal_is_not_later',
  ),
  plant(
    'a version that is not a semantic one is compared all the same',
    VERSION_RS,
    'Version::parse(document.get("version")?.as_str()?).ok()',
    'Version::parse(document.get("version")?.as_str()?).ok().or_else(|| Version::parse("99.0.0").ok())',
    'moves_a_feed_whose_latest_json_names_a_version_that_is_not_a_semantic_one',
    [PUBLISH],
  ),
  plant(
    'what is not a latest.json is taken for one that names a later release',
    VERSION_RS,
    'let document: Value = serde_json::from_slice(body).ok()?;',
    'let document: Value = serde_json::from_slice(body).unwrap_or_else(|_| serde_json::json!({ "version": "99.0.0" }));',
    'moves_a_feed_whose_latest_json_is_not_a_latest_json',
    [PUBLISH],
  ),
  // What the check accepts of a feed that a run left alone.
  plant(
    'the check does not accept a feed left at a later release',
    CHECKS_RS,
    ACCEPTS,
    'Ok(body) if *body == latest => body,',
    'accepts_a_new_feed_that_names_a_later_release',
  ),
  plant(
    'the check accepts what is not a later release instead of what is',
    CHECKS_RS,
    ACCEPTS,
    'Ok(body) if *body == latest || later_release(body, version).is_none() => body,',
    NOT_APPROVED,
  ),
  plant(
    'the check approves any feed it can read',
    CHECKS_RS,
    ACCEPTS,
    'Ok(body) => body,',
    NOT_APPROVED,
  ),
  // What the check holds a feed to that the publisher left a record of: that it did not go back.
  ...[
    ['the check does not look at what a feed named before', 'if false {'],
    ['a feed that is as it was is taken for one that went backward', 'if now <= *was {'],
    ['a feed that moved forward is taken for one that went backward', 'if now > *was {'],
  ].flatMap(([name, to]) => [
    plant(`${name} (asked of the check)`, CHECKS_RS, WENT_BACKWARD, to, BEFORE),
    plant(`${name} (a run again, end to end)`, CHECKS_RS, WENT_BACKWARD, to, RERUN, [PUBLISH]),
  ]),
  plant(
    'a record is trusted whatever it holds',
    CHECKS_RS,
    '.filter_map(|(feed, named)| Some((feed, Version::parse(named.as_str()?).ok()?)))',
    '.filter_map(|(feed, named)| Some((feed, Version::parse(named.as_str()?).ok().or_else(|| Version::parse("99.0.0").ok())?)))',
    BEFORE,
  ),
  plant(
    'a feed that names a later release is waited for as if it were served stale',
    READ_RS,
    `                Ok(body) => {
                    body == expected
                        || own
                            .as_ref()
                            .is_some_and(|own| later_release(body, own).is_some())
                }`,
    '                Ok(body) => body == expected,',
    'and_does_not_wait_for_it',
  ),
]
