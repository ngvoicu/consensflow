/**
 * Plants in the legacy CLI and in the recorder: the checked-in recording is held
 * to what Node answers now (`tests/cli-goldens.test.mjs`), and must notice when
 * the CLI changes, and when the recorder no longer fixes what it fixes.
 */
import { HELD } from './kit.mjs'

const MEANT = 'holds the Rust CLI to what Node answers now'

export const PLANTS = [
  {
    name: 'oracle: the legacy CLI pads a name of the catalog to 13',
    edits: [['bin/cf.mjs', 'entry.name.padEnd(12)', 'entry.name.padEnd(13)']],
    runs: [HELD],
    meant: MEANT,
  },
  {
    name: 'oracle: the legacy CLI says a different usage',
    edits: [
      [
        'bin/cf.mjs',
        'Prepare private launcher and integrations',
        'Prepare the launcher and integrations',
      ],
    ],
    runs: [HELD],
    meant: MEANT,
  },
  {
    name: 'oracle: the clock the recorder gives the CLI is not fixed',
    edits: [['tests/goldens/cli/clock.mjs', 'globalThis.Date = FixedDate', '']],
    runs: [HELD],
    meant: MEANT,
  },
  {
    name: 'oracle: the recorder names no machine place it should',
    edits: [['tests/goldens/cli/world.mjs', "[REPO, '$REPO'],", '']],
    runs: [HELD],
    meant: MEANT,
  },
]
