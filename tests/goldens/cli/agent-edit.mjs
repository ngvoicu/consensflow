/**
 * Scenarios for `cf agent edit <name> [--model <m>] [--effort <e>]
 * [--description <d>] [--work-tier <t>|auto]` and `cf agent remove <name>`:
 * an agent of the human's own changed one field at a time, taken away, and
 * every refusal (a catalog agent is the catalog's, and a refusal writes
 * nothing).
 */
import { UNREADABLE } from './agent-list.mjs'
import { file, handwritten, OWN, roster, row, STAMP, withOwn } from './fixtures.mjs'

const edit = (name, args, more = {}) => ({
  name: `agent edit: ${name}`,
  args: ['agent', 'edit', ...args],
  files: [withOwn()],
  ...more,
})

const remove = (name, args, more = {}) => ({
  name: `agent remove: ${name}`,
  args: ['agent', 'remove', ...args],
  files: [withOwn()],
  ...more,
})

/** A Pi agent that an older build stored with both of its effort keys. */
const BOTH = row('both', 'pi', 'openai-codex/gpt-6.1-sol', { thinking: 'high', effort: 'stale' })

/** A row of a hand-edited file that has no id. */
const NAMELESS = { name: 'Nameless', kind: 'codex', model: 'm', createdAt: STAMP }

/** What Rust says where Node, given no name, found the agent with no id. */
const NO_ONE = {
  why: 'Given no name, Node looked for the row whose id is undefined, and a hand-edited file may have one: it edited or removed that row. Rust asks the roster for no row by an id it was not given, and says no agent has the name',
  rust: { stdout: '', stderr: 'cf: no agent named undefined\n', code: 1, after: 'before' },
}

export function editScenarios() {
  return [
    edit('a model', ['mine', '--model', 'claude-fable-5-1']),
    edit('an effort', ['mine', '--effort', 'low']),
    edit('an effort of no text takes the effort off', ['mine', '--effort=']),
    edit('an effort on an agent that had none', ['opener', '--effort', 'high']),
    edit('a Pi effort, its thinking', ['pilot', '--effort', 'max']),
    edit('a Pi effort, which leaves no stale effort behind', ['both', '--effort', 'low'], {
      files: [roster([BOTH])],
    }),
    edit('a Pi effort taken off, which leaves no stale effort behind', ['both', '--effort='], {
      files: [roster([BOTH])],
    }),
    edit('a description', ['mine', '--description', 'Reviews']),
    edit('a description of no text is kept as no text', ['lunar', '--description=']),
    edit('a work tier', ['mine', '--work-tier', 'critical']),
    edit('a work tier of auto takes the tier off', ['lunar', '--work-tier', 'auto']),
    edit('a work tier of auto on an agent that had none', ['mine', '--work-tier', 'auto']),
    edit('every field', [
      'lunar',
      '--model',
      'gpt-6-astra',
      '--effort',
      'low',
      '--description',
      'Quick',
      '--work-tier',
      'light',
    ]),
    edit('nothing, which only stamps the time', ['mine']),
    edit('the JSON flag is accepted and does nothing', ['mine', '--json', '--model', 'x']),
    edit('the harness is accepted and does nothing', ['mine', '--harness', 'codex']),
    edit('the image flag is accepted and does nothing', ['mine', '--designer']),
    edit('options before the name', ['--model', 'x', 'mine']),
    edit('a model and a name after an equals sign', ['mine', '--model=x', '--effort=low']),
    edit('an effort no harness knows is kept', ['mine', '--effort', 'ultra']),
    edit('a model of an image agent', ['painter', '--model', 'codex-image-2']),
    edit('a description of an image agent', ['painter', '--description', 'Draws']),
    edit('the first of two rows of a name', ['mine', '--model', 'x'], {
      files: [roster([OWN[0], { ...OWN[0], model: 'twin' }])],
    }),
    edit("an agent of the human's that has a catalog agent's name", ['zeus', '--model', 'x'], {
      files: [roster([row('zeus', 'claude-code', 'claude-opus-5')])],
    }),
    edit('drops the display data an older build stored in a row', ['mine', '--model', 'x'], {
      files: [
        roster([
          { ...OWN[0], skills: [], profile: { modelKey: 'old' }, skillsPolicy: 'x', future: 1 },
        ]),
      ],
    }),
    edit('keeps what the file carries beyond the agents', ['mine', '--model', 'x'], {
      files: [
        file(
          'consensflow/agents.json',
          `${JSON.stringify({ schemaVersion: 1, agents: [OWN[0]], note: [1], preferences: { ownHarnessOnly: false } }, null, 2)}\n`,
        ),
      ],
    }),
    edit('the file of the home of the user', ['mine', '--model', 'x'], {
      files: [roster([OWN[0]], {}, 'home/.consensflow')],
      env: { CONSENSFLOW_HOME: null },
    }),
    // What it refuses.
    edit('a catalog agent', ['hyperion', '--effort', 'low']),
    edit('a catalog agent of Claude, by its model', ['zeus', '--model', 'x']),
    edit('the catalog image agent', ['pygmalion', '--description', 'x']),
    edit("a stored copy of a catalog agent is the catalog's still", ['zeus', '--model', 'x'], {
      files: [roster([{ ...row('zeus', 'claude-code', 'claude-opus-5'), preset: 'zeus' }])],
    }),
    edit('an agent nobody has', ['nobody', '--model', 'x']),
    edit('an agent nobody has, with nothing to change', ['nobody']),
    edit('no name', ['--model', 'x']),
    edit('no name and nothing to change', []),
    edit('no name and a work tier that is none', ['--work-tier', 'bogus']),
    edit('no name, in a file it cannot read', ['--model', 'x'], {
      files: [handwritten('not JSON')],
    }),
    edit('no name, in a file with an agent that has no id', ['--model', 'x'], {
      files: [roster([OWN[0], NAMELESS])],
      kept: NO_ONE,
    }),
    edit('an empty name', ['', '--model', 'x']),
    edit('an empty model', ['mine', '--model=']),
    edit('a work tier that is none', ['mine', '--work-tier', 'bogus']),
    edit('a work tier of no text', ['mine', '--work-tier=']),
    edit('a work tier that is none, of an agent nobody has', ['nobody', '--work-tier', 'bogus']),
    edit('an effort on an image agent', ['painter', '--effort', 'low']),
    edit('an effort of no text on an image agent', ['painter', '--effort=']),
    edit('an effort on an agent of a kind it does not run', ['kim', '--effort', 'low'], {
      files: [roster([row('kim', 'kimi', 'kimi-k3', { effort: 'high' })])],
    }),
    edit('a model of an agent of a kind it does not run', ['kim', '--model', 'kimi-k4'], {
      files: [roster([row('kim', 'kimi', 'kimi-k3', { effort: 'high' })])],
    }),
    edit('the model is read before the effort', ['painter', '--model=', '--effort', 'low']),
    edit('the effort is read before the model is kept', [
      'painter',
      '--model',
      'x',
      '--effort',
      'low',
    ]),
    edit('the work tier is read before the file', ['mine', '--work-tier', 'bogus'], {
      files: [handwritten('not JSON')],
    }),
    edit("the name is read before the image agent's effort", ['nobody', '--effort', 'low']),
    ...UNREADABLE.map(([name, entry]) =>
      edit(`leaves ${name} in the file's place as it is`, ['mine', '--model', 'x'], {
        files: [entry],
      }),
    ),
  ]
}

export function removeScenarios() {
  return [
    remove('one of several', ['mine']),
    remove('the last one', ['mine'], { files: [roster([OWN[0]])] }),
    remove('the image agent', ['painter']),
    remove('words after the name are ignored', ['mine', 'lunar']),
    remove('the JSON flag is accepted and does nothing', ['mine', '--json']),
    remove('an option it does not use is accepted', ['mine', '--model', 'x']),
    remove('the first of two rows of a name', ['mine'], {
      files: [roster([OWN[0], { ...OWN[0], model: 'twin' }])],
    }),
    remove("an agent of the human's that has a catalog agent's name", ['zeus'], {
      files: [roster([row('zeus', 'claude-code', 'claude-opus-5')])],
    }),
    remove('an agent of a kind it does not run', ['kim'], {
      files: [roster([row('kim', 'kimi', 'kimi-k3')])],
    }),
    remove('drops the display data of the rows it leaves', ['lunar'], {
      files: [roster([{ ...OWN[0], skills: [], profile: {} }, OWN[1]])],
    }),
    remove('keeps what the file carries beyond the agents', ['mine'], {
      files: [
        file(
          'consensflow/agents.json',
          `${JSON.stringify({ schemaVersion: 1, agents: [OWN[0]], note: [1] }, null, 2)}\n`,
        ),
      ],
    }),
    remove('a catalog agent', ['hyperion']),
    remove('a catalog agent of Claude', ['zeus']),
    remove('the catalog image agent', ['pygmalion']),
    remove("a stored copy of a catalog agent is the catalog's still", ['zeus'], {
      files: [roster([{ ...row('zeus', 'claude-code', 'claude-opus-5'), preset: 'zeus' }])],
    }),
    remove('an agent nobody has', ['nobody']),
    remove('an agent nobody has, in a folder with no file', ['nobody'], { files: [] }),
    remove('no name', []),
    remove('no name, in a file it cannot read', [], { files: [handwritten('not JSON')] }),
    remove('no name, in a file with an agent that has no id', [], {
      files: [roster([OWN[0], NAMELESS])],
      kept: NO_ONE,
    }),
    remove('an empty name', ['']),
    remove('a name that is a catalog agent, as it was typed', ['Hyperion']),
    remove('a row with no id, which no name finds', ['x'], {
      files: [roster([NAMELESS])],
    }),
    ...UNREADABLE.map(([name, entry]) =>
      remove(`leaves ${name} in the file's place as it is`, ['mine'], { files: [entry] }),
    ),
  ]
}
