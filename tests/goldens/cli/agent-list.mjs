/**
 * Scenarios for `cf agent` as a verb and for `cf agent list [--json]`: what
 * the first word after `agent` may be, the options every action accepts and
 * what is wrong with them, and the agents as a table (the columns padded to
 * UTF-16 units, as `padEnd` pads) and as JSON.
 */
import { dir, handwritten, OWN, roster, row, STAMP, withOwn } from './fixtures.mjs'

/** The agents file as it is here, said to whoever reads it, when what is in the file's place is not one. */
const UNREADABLE = [
  ['text that is not JSON', handwritten('{"agents": [],}\n')],
  ['an empty file', handwritten('')],
  ['an array', handwritten('[]\n')],
  ['a number', handwritten('5\n')],
  ['null', handwritten('null\n')],
  ['a text', handwritten('"agents"\n')],
  ['JSON after a byte order mark', handwritten('﻿{"agents":[]}\n')],
  ['a folder', dir('consensflow/agents.json')],
]

export { UNREADABLE }

export function listScenarios() {
  const list = (name, files, args = ['agent', 'list'], more = {}) => ({
    name: `agent list: ${name}`,
    args,
    files,
    ...more,
  })
  const emoji = '😀'
  return [
    list('no file, so the catalog alone', []),
    list('no file, as JSON', [], ['agent', 'list', '--json']),
    list("the human's own agents after the catalog", [withOwn()]),
    list("the human's own agents, as JSON", [withOwn()], ['agent', 'list', '--json']),
    list('words that are no options are ignored', [], ['agent', 'list', 'x', '--model', 'y']),
    list('an input it does not read', [], ['agent', 'list'], { stdin: 'x\n' }),
    list(
      'the file read from the home of the user',
      [roster([OWN[0]], {}, 'home/.consensflow')],
      ['agent', 'list'],
      { env: { CONSENSFLOW_HOME: null } },
    ),
    list(
      'the home as the variable names it, though it says ..',
      [roster([OWN[0]], {}, 'elsewhere')],
      ['agent', 'list'],
      { env: { CONSENSFLOW_HOME: '$ROOT/consensflow/../elsewhere' } },
    ),
    list('a model past 36 units runs into the effort', [
      roster([row('long', 'codex', 'm'.repeat(40), { effort: 'high' })]),
    ]),
    list('the columns are padded to units, not to letters', [
      roster([
        row(`a-${emoji}`, 'codex', `gpt-${emoji}-${emoji}`, { effort: 'high' }),
        row('cjk-日本語', 'codex', 'モデル', { effort: 'low' }),
        row('combining', 'codex', 'éé', { effort: 'low' }),
        row('wide', 'codex', `${emoji.repeat(17)}`, { effort: 'low' }),
        row('exact', 'codex', 'm'.repeat(36), { effort: 'low' }),
        row('one-less', 'codex', 'm'.repeat(35), { effort: 'low' }),
      ]),
    ]),
    list('an agent of a kind it does not run is listed under the kind', [
      roster([row('kim', 'kimi', 'kimi-k3', { effort: 'high' }), OWN[0]]),
    ]),
    list(
      'a Claude model through Pi is in the table though the human keeps to their own harnesses',
      [
        roster([row('relay', 'pi', 'claude-opus-5', { thinking: 'high' }), OWN[0]], {
          preferences: { ownHarnessOnly: true },
        }),
      ],
    ),
    list(
      'a Claude model through Pi is hidden in the JSON when they do',
      [
        roster([row('relay', 'pi', 'claude-opus-5', { thinking: 'high' }), OWN[0]], {
          preferences: { ownHarnessOnly: true },
        }),
      ],
      ['agent', 'list', '--json'],
    ),
    list('an image agent an older build saved on a harness of its own', [
      roster([row('draw', 'image', 'codex-image', { effort: 'high' })]),
    ]),
    list('an agent with no model stops the table where it is', [
      roster([OWN[0], { id: 'blank', name: 'Blank', kind: 'codex', createdAt: STAMP }, OWN[1]]),
    ]),
    list('an agent with no id stops the table where it is', [
      roster([OWN[0], { name: 'Nameless', kind: 'codex', model: 'gpt-6-astra' }, OWN[1]]),
    ]),
    list('an agent with no kind stops the table where it is', [
      roster([OWN[0], { id: 'kindless', model: 'gpt-6-astra' }, OWN[1]]),
    ]),
    list('a work tier that is none of the four fails the table', [
      roster([OWN[0], row('odd', 'codex', 'gpt-6-astra', { workTier: 'bogus' })]),
    ]),
    list(
      'a work tier that is none of the four fails the JSON',
      [roster([OWN[0], row('odd', 'codex', 'gpt-6-astra', { workTier: 'bogus' })])],
      ['agent', 'list', '--json'],
    ),
    ...UNREADABLE.flatMap(([name, entry]) => [
      list(`${name} in the file's place`, [entry]),
      list(`${name} in the file's place, as JSON`, [entry], ['agent', 'list', '--json']),
    ]),
  ]
}

export function agentUsageScenarios() {
  const files = [withOwn()]
  const usage = (name, args) => ({ name: `agent: ${name}`, args, files })
  return [
    usage('no action', ['agent']),
    usage('an action it has not', ['agent', 'frob']),
    usage('an action that is none, with a catalog agent after it', ['agent', 'reset', 'hyperion']),
    usage('an action is named as it is', ['agent', 'ADD', 'x']),
    usage('the JSON flag in the place of the action', ['agent', '--json', 'list']),
    usage('an option in the place of the action', ['agent', '--harness', 'claude', 'list']),
    usage('the action after a word', ['agent', 'x', 'list']),
    usage('the options are read before the action is known', ['agent', 'frob', '--nope']),
    usage('a flag given a value, before the action is known', ['agent', 'frob', '--json=1']),
    // What every action refuses, in Node's words.
    usage('list: an option it has not', ['agent', 'list', '--nope']),
    usage('list: a short option', ['agent', 'list', '-x']),
    usage('list: a flag given a value', ['agent', 'list', '--json=1']),
    usage('list: a text option with no text', ['agent', 'list', '--harness']),
    usage('add: a text option at the end', ['agent', 'add', 'x', '--model']),
    usage('add: a text option followed by a flag', ['agent', 'add', 'x', '--model', '--json']),
    usage('add: a text option followed by what looks like an option', [
      'agent',
      'add',
      'x',
      '--effort',
      '-1',
    ]),
    usage('add: a flag given a value', ['agent', 'add', 'x', '--designer=yes']),
    usage('add: the options of an agent that is no agent', ['agent', 'add', 'trial', '--dry-run']),
    usage('add: an option with an argument that is not one', [
      'agent',
      'add',
      'trial',
      '--from',
      'x',
    ]),
    usage('add: work tier as the last word', ['agent', 'add', 'x', '--work-tier']),
    usage('edit: a flag given a value', ['agent', 'edit', 'mine', '--designer=1']),
    usage('edit: an option it has not', ['agent', 'edit', 'mine', '--presets', 'x']),
    usage('remove: a text option with no text', ['agent', 'remove', 'mine', '--description']),
    usage('remove: an option it has not', ['agent', 'remove', 'mine', '--force']),
  ]
}
