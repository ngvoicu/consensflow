/**
 * Plants in the release workflow's text (.github/workflows/release.yml): who
 * may publish, what a hand run is told and not failed by, and the steps that
 * call the scripts. The tests that read the workflow and run its steps as
 * written must catch each.
 */
// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
import { RELEASE_YML, WORKFLOW } from './kit.mjs'

const plant = (name, from, to, meant) => ({
  name: `workflow: ${name}`,
  edits: [[RELEASE_YML, from, to]],
  runs: [WORKFLOW],
  meant,
})

export const PLANTS = [
  plant(
    'a hand run on a tag publishes: the job asks only for a tag',
    "    if: github.event_name == 'push' && github.ref_type == 'tag'",
    "    if: github.ref_type == 'tag'",
    'is the push of a tag: not a hand run, though it is on a tag',
  ),
  plant(
    'a hand run on a tag takes the group of the releases',
    "group: ${{ github.event_name == 'push' && github.ref_type == 'tag' && 'release' ||",
    "group: ${{ github.ref_type == 'tag' && 'release' ||",
    'is the push of a tag for the group of releases that go out one at a time',
  ),
  plant(
    'a hand run is failed by what a tag would be refused for, before the build',
    'if [ "$GITHUB_EVENT_NAME" != push ]; then dry=(--dry-run); fi\n          node app/scripts/feeds.mjs prerequisites',
    'node app/scripts/feeds.mjs prerequisites',
    'asks the old feeds before the build: a tag is refused, a hand run is told',
  ),
  plant(
    'a hand run is failed by the plan of the feeds',
    'if [ "$GITHUB_EVENT_NAME" != push ]; then dry=(--dry-run); fi\n          node app/scripts/feeds.mjs plan',
    'node app/scripts/feeds.mjs plan',
    'plans the feeds from the archive: a tag is refused for a release before the bridge',
  ),
  plant(
    'the publisher is not checked out in the publish job',
    '            app/scripts/publish.mjs\n',
    '',
    'has the publish job check out what it runs',
  ),
  plant(
    'the step that checks the feeds is gone',
    '- name: The feeds serve this release, and its archive downloads',
    '- name: The feeds are not looked at',
    'checks the feeds after it publishes them',
  ),
  plant(
    'a file is replaced by deleting it first, in a step',
    'node "$GITHUB_WORKSPACE/app/scripts/publish.mjs" --dir . --tag "$GITHUB_REF_NAME" \\',
    'gh release upload feed-alpha latest.json --clobber\n          node "$GITHUB_WORKSPACE/app/scripts/publish.mjs" --dir . --tag "$GITHUB_REF_NAME" \\',
    'keeps every call of gh in the publisher',
  ),
  plant(
    'the publisher is given the tag without its v',
    '--dir . --tag "$GITHUB_REF_NAME" \\',
    '--dir . --tag "${GITHUB_REF_NAME#v}" \\',
    'publishes the bridge and moves its feeds',
  ),
]
