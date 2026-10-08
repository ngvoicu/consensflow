/**
 * Scenarios for `cf catalog [--harness <h>] [--json]`: the ready-made agents,
 * all of them and a harness's, as a table and as JSON, and every way the words
 * after it can be wrong. It reads nothing of the folder and writes nothing.
 */

const KEPT =
  'A name every plain JavaScript object answers for (constructor, __proto__, toString…) was read as the catalog entry of that name, and its table crashed Node (`entries.map is not a function`) after the first line: Rust lists no agents under it, as under any harness it has none for'

/** The record of `catalog --harness nope`, with `nope` as `name`. */
const asUnknown = (recorded, scenario, name) => {
  const base = recorded(scenario)
  return {
    stdout: base.stdout.replaceAll('nope', name),
    stderr: base.stderr,
    code: base.code,
  }
}

export function catalogScenarios() {
  return [
    { name: 'catalog: every harness', args: ['catalog'] },
    { name: 'catalog: every harness as JSON', args: ['catalog', '--json'] },
    ...['claude', 'codex', 'pi', 'opencode', 'devin'].map((harness) => ({
      name: `catalog: ${harness}`,
      args: ['catalog', '--harness', harness],
    })),
    {
      name: 'catalog: a harness as JSON, its name after an equals sign',
      args: ['catalog', '--harness=codex', '--json'],
    },
    {
      name: 'catalog: the JSON flag before the harness',
      args: ['catalog', '--json', '--harness', 'devin'],
    },
    { name: 'catalog: a harness it has none of', args: ['catalog', '--harness', 'nope'] },
    {
      name: 'catalog: a harness it has none of, as JSON',
      args: ['catalog', '--harness=nope', '--json'],
    },
    { name: 'catalog: a harness of no name', args: ['catalog', '--harness='] },
    { name: 'catalog: a harness of no name, as JSON', args: ['catalog', '--harness=', '--json'] },
    {
      name: 'catalog: the kind of a harness is no harness',
      args: ['catalog', '--harness', 'claude-code'],
    },
    { name: 'catalog: a harness is named as it is', args: ['catalog', '--harness', 'Claude'] },
    {
      name: 'catalog: the last of two harnesses',
      args: ['catalog', '--harness', 'nope', '--harness', 'pi'],
    },
    {
      name: 'catalog: a harness that opens with a dash, after an equals sign',
      args: ['catalog', '--harness=-x'],
    },
    { name: 'catalog: a harness that is a dash', args: ['catalog', '--harness', '-'] },
    { name: 'catalog: words that are no options are ignored', args: ['catalog', 'claude', 'x'] },
    {
      name: 'catalog: everything after -- is a word',
      args: ['catalog', '--', '--harness', 'codex'],
    },
    {
      name: 'catalog: an input it does not read',
      args: ['catalog', '--harness', 'pi'],
      stdin: 'x\n',
    },
    {
      name: 'catalog: a folder with an agents file it does not read',
      args: ['catalog', '--harness', 'pi'],
      files: [{ path: 'consensflow/agents.json', text: 'not JSON at all' }],
    },
    {
      name: 'catalog: the folder it does not make',
      args: ['catalog', '--harness', 'pi'],
      env: { CONSENSFLOW_HOME: '$ROOT/nothing/here' },
    },
    {
      name: 'catalog: no folder named',
      args: ['catalog', '--harness', 'pi'],
      env: { CONSENSFLOW_HOME: null, HOME: null },
    },
    {
      name: 'catalog: with an empty window token, which is no window',
      args: ['catalog', '--harness', 'pi'],
      env: { CONSENSFLOW_TOKEN: '' },
    },
    // What it refuses, in Node's words.
    { name: 'catalog: a flag given a value', args: ['catalog', '--json=1'] },
    { name: 'catalog: a harness with no name after it', args: ['catalog', '--harness'] },
    { name: 'catalog: a harness followed by a flag', args: ['catalog', '--harness', '--json'] },
    {
      name: 'catalog: a harness followed by what looks like one',
      args: ['catalog', '--harness', '-x'],
    },
    { name: 'catalog: a harness followed by --', args: ['catalog', '--harness', '--'] },
    { name: 'catalog: an option it has not', args: ['catalog', '--nope'] },
    { name: 'catalog: an option of the other verbs', args: ['catalog', '--model', 'x'] },
    {
      name: 'catalog: a short option',
      args: ['catalog', '-h'],
      kept: {
        why: 'Node refused `-h` after a verb as an option the verb has not; Rust answers it with the usage of the verb, as it answers `--help`: every verb and sub-verb of `cf` answers `-h` and `--help` with its usage and exit code 0',
        rust: (recorded) => ({
          stdout: `${recorded('usage: help')
            .stdout.split('\n')
            .find((line) => line.startsWith('  catalog '))}\n`,
          stderr: '',
          code: 0,
        }),
      },
    },
    { name: 'catalog: several short options', args: ['catalog', '-jq'] },
    { name: 'catalog: a short option past U+FFFF', args: ['catalog', '-😀'] },
    { name: 'catalog: the first of two refusals', args: ['catalog', '--json=1', '--nope'] },
    // What a plain object answers for.
    ...['constructor', '__proto__', 'toString', 'hasOwnProperty'].flatMap((name) => [
      {
        name: `catalog: the harness ${name}`,
        args: ['catalog', '--harness', name],
        kept: {
          why: KEPT,
          rust: (recorded) => asUnknown(recorded, 'catalog: a harness it has none of', name),
        },
      },
      {
        name: `catalog: the harness ${name}, as JSON`,
        args: ['catalog', '--harness', name, '--json'],
        kept: {
          why: KEPT,
          rust: (recorded) =>
            asUnknown(recorded, 'catalog: a harness it has none of, as JSON', name),
        },
      },
    ]),
  ]
}
