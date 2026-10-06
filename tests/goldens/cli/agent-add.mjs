/**
 * Scenarios for `cf agent add <name> --harness <h> --model <m> [--effort <e>]
 * [--description <d>] [--work-tier <t>] [--designer]`: an agent of the
 * human's own, in every harness, every refusal of it (and that a refusal
 * writes nothing), and what it keeps of a file that has more than agents.
 */

import { UNREADABLE } from './agent-list.mjs'
import { file, handwritten, OWN, roster, row, STAMP, withOwn } from './fixtures.mjs'

const add = (name, args, more = {}) => ({
  name: `agent add: ${name}`,
  args: ['agent', 'add', ...args],
  ...more,
})
const MODEL = ['--harness', 'claude', '--model', 'claude-opus-5']

export function addScenarios() {
  const names = (label, words) =>
    words.map((word) => add(`${label} ${JSON.stringify(word)}`, [word, ...MODEL]))
  return [
    // An agent in each harness.
    add('claude with an effort', ['mine', ...MODEL, '--effort', 'max']),
    add('codex with an effort', [
      'lunar',
      '--harness',
      'codex',
      '--model',
      'gpt-5.6-luna',
      '--effort',
      'xhigh',
    ]),
    add('pi with an effort, kept as its thinking', [
      'pilot',
      '--harness',
      'pi',
      '--model',
      'openai-codex/gpt-6.1-sol',
      '--effort',
      'high',
    ]),
    add('opencode with no effort', [
      'opener',
      '--harness',
      'opencode',
      '--model',
      'openrouter/z-ai/glm-5.3',
    ]),
    add('devin with an effort', [
      'devon',
      '--harness',
      'devin',
      '--model',
      'claude-fable-5-1',
      '--effort',
      'max',
    ]),
    add('an image agent, which only Codex can be', [
      'my-draw',
      '--harness',
      'codex',
      '--model',
      'codex-image',
      '--designer',
    ]),
    add('an image agent with a description', [
      'my-draw',
      '--harness',
      'codex',
      '--model',
      'codex-image',
      '--description',
      'Draws',
      '--designer',
    ]),
    // The other fields.
    add('a description', ['mine', ...MODEL, '--description', 'Fast, cheap & "quoted"']),
    ...['critical', 'complex', 'standard', 'light'].map((tier) =>
      add(`a work tier of ${tier}`, ['mine', ...MODEL, '--work-tier', tier]),
    ),
    add('a work tier of auto is none', ['mine', ...MODEL, '--work-tier', 'auto']),
    add('an effort of no text is none', ['mine', ...MODEL, '--effort=']),
    add('a description of no text is none', ['mine', ...MODEL, '--description=']),
    add('an effort no harness knows is kept', ['mine', ...MODEL, '--effort', 'ultra']),
    add('every field', [
      'mine',
      ...MODEL,
      '--effort',
      'high',
      '--description',
      'All',
      '--work-tier',
      'complex',
    ]),
    add('the JSON flag is accepted and does nothing', ['mine', ...MODEL, '--json']),
    add('the options before the name', [...MODEL, 'mine']),
    add('the name between the options', [
      '--harness',
      'claude',
      'mine',
      '--model',
      'claude-opus-5',
    ]),
    add('words after the name are ignored', ['mine', 'more', 'words', ...MODEL]),
    add('every option given twice, the last kept', [
      'mine',
      ...MODEL,
      '--model',
      'claude-fable-5-1',
      '--effort',
      'low',
      '--effort',
      'high',
    ]),
    add('options after an equals sign', [
      'mine',
      '--harness=claude',
      '--model=claude-opus-5',
      '--effort=max',
    ]),
    add('a model that opens with a dash, after an equals sign', [
      'mine',
      '--harness=claude',
      '--model=-x',
    ]),
    add('an input it does not read', ['mine', ...MODEL], { stdin: 'x\n' }),
    add('with an empty window token, which is no window', ['mine', ...MODEL], {
      env: { CONSENSFLOW_TOKEN: '' },
    }),
    ...names('a name that is valid', ['x', 'a-', 'a--b', 'a1', 'trailing-dash-']),
    // The home the file is in.
    add('the home of the user, with no variable for it', ['mine', ...MODEL], {
      env: { CONSENSFLOW_HOME: null },
    }),
    add('the home as the variable names it, though it says ..', ['mine', ...MODEL], {
      env: { CONSENSFLOW_HOME: '$ROOT/consensflow/../elsewhere' },
    }),
    add('the home when the folder is there and empty', ['mine', ...MODEL], {
      files: [{ dir: 'consensflow' }],
    }),
    add('after the agents there already', ['again', ...MODEL], { files: [withOwn()] }),
    // A file that has more than the agents.
    add(
      'keeps what the file carries beyond the agents, and each row as it is',
      ['mine', ...MODEL],
      {
        files: [
          file(
            'consensflow/agents.json',
            `${JSON.stringify(
              {
                schemaVersion: 2,
                note: { kept: [1, 2, { deep: true }] },
                agents: [
                  { ...OWN[1], future: 'a field it does not know' },
                  row('plain', 'pi', 'openai-codex/gpt-6.1-sol', { thinking: 'low' }),
                ],
                preferences: { ownHarnessOnly: true, other: 1 },
              },
              null,
              2,
            )}\n`,
          ),
        ],
      },
    ),
    add('drops the display data an older build stored in a row', ['fresh', ...MODEL], {
      files: [
        roster([
          {
            ...OWN[0],
            skillsPolicy: 'x',
            skillPaths: ['a'],
            skills: [],
            skillPath: 'b',
            profile: { modelKey: 'old' },
          },
        ]),
      ],
    }),
    add(
      'turns an image agent of an older build into a Codex agent that designs',
      ['mine', ...MODEL],
      {
        files: [roster([row('draw', 'image', 'codex-image')])],
      },
    ),
    add('gives a file with neither of the two keys its version and its list', ['mine', ...MODEL], {
      files: [handwritten('{"note": "kept"}\n')],
    }),
    add('gives a file whose version is null its version', ['mine', ...MODEL], {
      files: [handwritten('{"schemaVersion": null, "agents": []}\n')],
    }),
    add('reads a list that is none as an empty one', ['mine', ...MODEL], {
      files: [handwritten('{"schemaVersion": 1, "agents": "none"}\n')],
    }),
    add('rewrites a file written compactly as it writes its own', ['fresh', ...MODEL], {
      files: [handwritten(JSON.stringify({ schemaVersion: 1, agents: [OWN[0]] }))],
    }),
    add('is not troubled by the agents of a harness it does not run', ['mine', ...MODEL], {
      files: [roster([row('kim', 'kimi', 'kimi-k3')])],
    }),
    // What it refuses.
    add('a name that is a catalog agent', ['hyperion', ...MODEL]),
    add('a name that is a catalog agent of Claude', ['zeus', ...MODEL]),
    add('a name that is the catalog image agent', [
      'pygmalion',
      '--harness',
      'codex',
      '--model',
      'codex-image',
      '--designer',
    ]),
    add('a catalog agent needs no harness to be refused', ['hyperion']),
    add('no name, no harness, no model', []),
    add('no name', MODEL),
    add('no name, nothing else but options', [
      '--harness',
      'claude',
      '--model',
      'm',
      '--effort',
      'low',
    ]),
    add('no harness', ['nemo', '--model', 'x']),
    add('no model', ['nemo', '--harness', 'claude']),
    add('neither', ['nemo']),
    add('an empty model', ['nemo', '--harness', 'claude', '--model=']),
    add('an empty harness', ['nemo', '--harness=', '--model', 'x']),
    add('a harness that is none', ['nemo', '--harness', 'image', '--model', 'x']),
    add('a harness named as its kind', ['nemo', '--harness', 'claude-code', '--model', 'x']),
    add('a harness in capitals', ['nemo', '--harness', 'Claude', '--model', 'x']),
    add('an image agent that is not Codex', [
      'pi-draw',
      '--harness',
      'pi',
      '--model',
      'x',
      '--designer',
    ]),
    add('a work tier that is none', ['nemo', ...MODEL, '--work-tier', 'bogus']),
    add('a work tier of no text', ['nemo', ...MODEL, '--work-tier=']),
    add('a work tier in capitals', ['nemo', ...MODEL, '--work-tier', 'Critical']),
    add('a name that is taken', ['mine', ...MODEL], { files: [withOwn()] }),
    add('a name that is taken, in a file with two rows of it', ['mine', ...MODEL], {
      files: [roster([OWN[0], { ...OWN[0], model: 'other' }])],
    }),
    add('a name that is taken by an agent with no kind', ['bare', ...MODEL], {
      files: [roster([{ id: 'bare' }])],
    }),
    add(
      'a work tier that is none is refused before a name that is taken',
      ['mine', ...MODEL, '--work-tier', 'bogus'],
      {
        files: [withOwn()],
      },
    ),
    add('the first refusal is the name, and then the harness', [
      'Bad',
      '--harness',
      'x',
      '--model',
      'x',
    ]),
    add('the harness is read before the image flag', [
      'nemo',
      '--harness',
      'x',
      '--model',
      'x',
      '--designer',
    ]),
    add('the image flag is read before the model', [
      'nemo',
      '--harness',
      'claude',
      '--model=',
      '--designer',
    ]),
    add('the model is read before the work tier', [
      'nemo',
      '--harness',
      'claude',
      '--model=',
      '--work-tier',
      'bogus',
    ]),
    add('the work tier is read before the file', ['nemo', ...MODEL, '--work-tier', 'bogus'], {
      files: [handwritten('not JSON')],
    }),
    ...names('a name that is no name', [
      'Mine',
      '1x',
      'my_agent',
      'a b',
      'é',
      '',
      'a😀',
      'a.b',
      'a/b',
      'MINE',
      'a\nb',
      'a"b',
    ]),
    add('a name that opens with a dash is an option', ['-x', ...MODEL]),
    add('a name after --', ['--', 'mine', ...MODEL]),
    add('a name that is an option, after --', ['--', '-x', '--harness', 'claude', '--model', 'm']),
    // A file it cannot use is left as it is, and nothing is made.
    ...UNREADABLE.map(([name, entry]) =>
      add(`leaves ${name} in the file's place as it is`, ['mine', ...MODEL], { files: [entry] }),
    ),
    add(
      'a row that is no agent is kept by Node, and refuses the file in Rust',
      ['mine', ...MODEL],
      {
        files: [
          handwritten(`{"schemaVersion":1,"agents":[5, {"id":"a","createdAt":"${STAMP}"}]}\n`),
        ],
        kept: {
          why: 'A row of the wrong shape refuses the file in Rust, as one that is no agents file (the roster holds it so: stricter than Node, which wrote it back as it was and went on)',
          rust: {
            stdout: '',
            stderr:
              'cf: Your agents file $ROOT/consensflow/agents.json is not an agents file: fix it or move it away. ConsensFlow left it as it is.\n',
            code: 1,
            after: 'before',
          },
        },
      },
    ),
  ]
}
