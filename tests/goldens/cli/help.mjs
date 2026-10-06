/**
 * Scenarios for what `cf` answers before any verb has a say: the usage and
 * the version in every spelling that asks for them, a command it does not have,
 * and an output nobody reads to the end.
 */

/** The usage and the version: what each word asks for, and the words after it, which are ignored. */
const ASKED = [
  ['no command', []],
  ['help', ['help']],
  ['--help', ['--help']],
  ['help and words after it', ['help', 'agent', '--json']],
  ['--help and a word after it', ['--help', 'x']],
  ['--version', ['--version']],
  ['-v', ['-v']],
  ['version', ['version']],
  ['version and words after it', ['version', '--json', 'x']],
  ['--version before help', ['--version', 'help']],
  ['help before --version', ['help', '--version']],
]

/** What is no command, nor any spelling of the usage or the version. */
const UNKNOWN = [
  ['a command that does not exist', ['frobnicate']],
  ['-h', ['-h']],
  ['-V', ['-V']],
  ['--HELP', ['--HELP']],
  ['Help', ['Help']],
  ['VERSION', ['VERSION']],
  ['an empty word', ['']],
  ['a word with a quote', ['a"b']],
  ['a word with a backslash', ['a\\b']],
  ['a word with a line break', ['one\ntwo']],
  ['a word with a control character', ['bell\u0007']],
  ['a word with letters past U+FFFF', ['é😀']],
  ['a word with spaces', ['two words']],
  ['--', ['--']],
  ['-', ['-']],
  ['a long word', ['x'.repeat(300)]],
  ['hosts', ['hosts', 'claude']],
  ['install', ['install', 'claude']],
  ['uninstall', ['uninstall', 'claude']],
  ['skills', ['skills', 'install', '--force']],
  ['off', ['off']],
  ['off --force', ['off', '--force']],
  ['reset', ['reset']],
  ['reset --yes', ['reset', '--yes']],
  ['task, which only a window has', ['task', 'list']],
]

export function helpScenarios() {
  return [
    ...ASKED.map(([name, args]) => ({ name: `usage: ${name}`, args })),
    { name: 'usage: help and an input it does not read', args: ['help'], stdin: 'ignored\n' },
    {
      name: 'usage: help with an empty window token, which is no window',
      args: ['help'],
      env: { CONSENSFLOW_TOKEN: '' },
    },
    ...UNKNOWN.map(([name, args]) => ({ name: `unknown command: ${name}`, args })),
    {
      name: 'unknown command: leaves a folder with agents in it as it was',
      args: ['frobnicate', '--json'],
      files: [{ path: 'consensflow/agents.json', text: '{"agents":[]}\n' }],
    },
  ]
}

/** Output that nobody reads to the end. */
export function pipeScenarios() {
  return [
    { name: 'pipe: help into a pipe closed before it', args: ['help'], pipe: 'closed' },
    { name: 'pipe: catalog into a pipe closed before it', args: ['catalog'], pipe: 'closed' },
    {
      name: 'pipe: catalog as JSON into a pipe closed before it',
      args: ['catalog', '--json'],
      pipe: 'closed',
    },
    {
      name: 'pipe: agent list into a pipe closed before it',
      args: ['agent', 'list'],
      pipe: 'closed',
    },
    { name: 'pipe: catalog, the first line read', args: ['catalog'], pipe: 'first-line' },
    {
      name: 'pipe: agent list, the first line read',
      args: ['agent', 'list'],
      pipe: 'first-line',
    },
    {
      name: 'pipe: a refusal says nothing on a closed output',
      args: ['agent', 'remove', 'nobody'],
      pipe: 'closed',
    },
    {
      name: 'pipe: an agent is added though nobody reads that it was',
      args: ['agent', 'add', 'mine', '--harness', 'claude', '--model', 'claude-opus-5'],
      pipe: 'closed',
    },
  ]
}
