/**
 * Plants in the release workflow's text (.github/workflows/release.yml): who
 * may publish, what a hand run is told and not failed by, what holds the write
 * token, how the publisher is built and checked before it runs, and the steps
 * that call it. The tests that read the workflow and run its steps as written
 * must catch each.
 */
// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
import { RELEASE_YML, WORKFLOW } from './kit.mjs'

const plant = (name, from, to, meant) => ({
  name: `workflow: ${name}`,
  edits: [[RELEASE_YML, from, to]],
  runs: [WORKFLOW],
  meant,
})

/** The test of the publish job that holds nothing of the tree and runs the publisher that was built. */
const NOTHING_OF_THE_TREE =
  'has_the_publish_job_run_the_publisher_that_was_built_and_nothing_of_the_tree'
/** The test of the publisher job that has no secret in reach. */
const NO_SECRET = 'builds_the_publisher_where_no_secret_is'
/** The test that the token is the two steps' alone. */
const TWO_STEPS = 'gives_the_write_token_to_the_two_steps_that_run_the_publisher_and_to_no_other'
/** The test of the hash check, run as written. */
const CHECKED = 'runs_the_publisher_only_once_it_is_the_binary_the_job_that_built_it_reported'
/** The hash check's own condition. */
const UNLESS_THE_SAME = 'if [ -z "$PUBLISHER_SHA256" ] || [ "$got" != "$PUBLISHER_SHA256" ]; then'
/** The step that publishes: the publisher run with the tag. */
const PUBLISHING =
  '          "$RUNNER_TEMP/publisher/cf-publish" publish --dir . --tag "$GITHUB_REF_NAME" \\'

export const PLANTS = [
  plant(
    'a hand run on a tag publishes: the job asks only for a tag',
    "    if: github.event_name == 'push' && github.ref_type == 'tag'",
    "    if: github.ref_type == 'tag'",
    'is_the_push_of_a_tag_not_a_hand_run_though_it_is_on_a_tag',
  ),
  plant(
    'a hand run on a tag takes the group of the releases',
    "group: ${{ github.event_name == 'push' && github.ref_type == 'tag' && 'release' ||",
    "group: ${{ github.ref_type == 'tag' && 'release' ||",
    'is_the_push_of_a_tag_for_the_group_of_releases_that_go_out_one_at_a_time',
  ),
  plant(
    'a hand run is failed by what a tag would be refused for, before the build',
    'if [ "$GITHUB_EVENT_NAME" != push ]; then dry=(--dry-run); fi\n          app/src-tauri/target/release/cf-publish feeds prerequisites',
    'app/src-tauri/target/release/cf-publish feeds prerequisites',
    'asks_the_old_feeds_before_the_build_a_tag_is_refused_a_hand_run_is_told',
  ),
  plant(
    'a hand run is failed by the plan of the feeds',
    'if [ "$GITHUB_EVENT_NAME" != push ]; then dry=(--dry-run); fi\n          app/src-tauri/target/release/cf-publish feeds plan',
    'app/src-tauri/target/release/cf-publish feeds plan',
    'plans_the_feeds_from_the_archive_a_tag_is_refused_for_a_release_before_the_bridge',
  ),
  plant(
    'the Mac job asks the rule without having built it',
    '      - name: Build cf-publish, the rule of the feeds\n        run: cargo build --release --locked -p cf-publish\n',
    '',
    'builds_the_rule_in_the_mac_job_before_it_asks_it',
  ),
  plant(
    'the Mac job builds the rule with a signing key in reach',
    '      - name: Build cf-publish, the rule of the feeds\n        run: cargo build --release --locked -p cf-publish',
    '      - name: Build cf-publish, the rule of the feeds\n        env:\n          APPLE_CERTIFICATE: ${{ secrets.APPLE_CERTIFICATE }}\n        run: cargo build --release --locked -p cf-publish',
    'builds_the_rule_in_the_mac_job_before_it_asks_it',
  ),
  plant(
    'the tree is checked out in the publish job',
    '    steps:\n      # The files the Mac and Windows jobs built.',
    '    steps:\n      - uses: actions/checkout@v7\n      # The files the Mac and Windows jobs built.',
    NOTHING_OF_THE_TREE,
  ),
  plant(
    'Node is set up in the publish job',
    '    steps:\n      # The files the Mac and Windows jobs built.',
    '    steps:\n      - uses: actions/setup-node@v7\n      # The files the Mac and Windows jobs built.',
    NOTHING_OF_THE_TREE,
  ),
  plant(
    'the publish job does not wait for the publisher',
    'needs: [mac, windows, gate, publisher]',
    'needs: [mac, windows, gate]',
    NOTHING_OF_THE_TREE,
  ),
  plant(
    'the publisher is downloaded into the folder of the release',
    '          name: cf-publish\n          path: ${{ runner.temp }}/publisher',
    '          name: cf-publish\n          path: ${{ runner.temp }}/dist',
    NOTHING_OF_THE_TREE,
  ),
  plant(
    'the publisher is checked against the hash of another job',
    'PUBLISHER_SHA256: ${{ needs.publisher.outputs.sha256 }}',
    'PUBLISHER_SHA256: ${{ needs.mac.outputs.sha256 }}',
    NOTHING_OF_THE_TREE,
  ),
  plant(
    'the publisher is run without its hash being checked',
    UNLESS_THE_SAME,
    'if false; then',
    CHECKED,
  ),
  plant(
    'the publisher that was downloaded is not made to run',
    '          chmod +x "$publisher"\n',
    '',
    CHECKED,
  ),
  plant(
    'the publisher job holds the write permission',
    '    permissions:\n      contents: read\n    outputs:',
    '    permissions:\n      contents: write\n    outputs:',
    NO_SECRET,
  ),
  plant(
    'the publisher is built by a third-party action',
    '      - run: rustup update stable --no-self-update && rustup default stable\n      - name: Build cf-publish, and say what it hashes to',
    '      - uses: dtolnay/rust-toolchain@stable\n      - name: Build cf-publish, and say what it hashes to',
    NO_SECRET,
  ),
  plant(
    'the publisher is built from versions Cargo.lock does not pin',
    '          cargo build --release --locked -p cf-publish\n          sha256=',
    '          cargo build --release -p cf-publish\n          sha256=',
    NO_SECRET,
  ),
  plant(
    'the token is the publish job again',
    '    permissions:\n      contents: write\n    # Nothing of the tree',
    '    permissions:\n      contents: write\n    env:\n      GH_TOKEN: ${{ github.token }}\n    # Nothing of the tree',
    TWO_STEPS,
  ),
  plant(
    'the token reaches the check of the hash',
    '        env:\n          PUBLISHER_SHA256: ${{ needs.publisher.outputs.sha256 }}',
    '        env:\n          GH_TOKEN: ${{ github.token }}\n          PUBLISHER_SHA256: ${{ needs.publisher.outputs.sha256 }}',
    TWO_STEPS,
  ),
  plant(
    'the step that publishes is given no token',
    `        env:
          GH_TOKEN: \${{ github.token }}
          GH_REPO: \${{ github.repository }}
        run: |
          set -euo pipefail
${PUBLISHING}`,
    `        env:
          GH_REPO: \${{ github.repository }}
        run: |
          set -euo pipefail
${PUBLISHING}`,
    TWO_STEPS,
  ),
  plant(
    'the step that publishes is given no repository',
    `        env:
          GH_TOKEN: \${{ github.token }}
          GH_REPO: \${{ github.repository }}
        run: |
          set -euo pipefail
${PUBLISHING}`,
    `        env:
          GH_TOKEN: \${{ github.token }}
        run: |
          set -euo pipefail
${PUBLISHING}`,
    'publishes_the_bridge_and_moves_its_feeds',
  ),
  plant(
    'the step that checks the feeds is gone',
    '- name: The feeds serve this release, and its archive downloads',
    '- name: The feeds are not looked at',
    'checks_the_feeds_after_it_publishes_them',
  ),
  plant(
    'a file is replaced by deleting it first, in a step',
    PUBLISHING,
    `          gh release upload feed-alpha latest.json --clobber\n${PUBLISHING}`,
    'keeps_every_call_of_gh_in_the_publisher_which_is_tested_the_workflow_only_runs_it',
  ),
  plant(
    'the publisher is given the tag without its v',
    '--dir . --tag "$GITHUB_REF_NAME" \\',
    '--dir . --tag "${GITHUB_REF_NAME#v}" \\',
    'publishes_the_bridge_and_moves_its_feeds',
  ),
  plant(
    'a step of the publisher job is not shell: a bracket is left open',
    'needs: [mac, windows, gate, publisher]',
    'needs: [mac, windows, gate, publisher',
    'every_workflow_is_valid_yaml',
  ),
  plant(
    'the check of the hash has no end',
    '            exit 1\n          fi\n',
    '            exit 1\n',
    'all_parse_as_bash_bash_n',
  ),
]
