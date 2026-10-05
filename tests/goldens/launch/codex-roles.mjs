/**
 * The role a Codex window is given (`roleConfiguration` and
 * `codexInstructions`, `src/role-skills.js`): Codex's own instructions, asked
 * of its app-server a line at a time, and the role's text after them. The
 * app-server is a scripted child: the lines it writes, and whether it ends by
 * itself, when asked, or not at all. A child that is not there is not here:
 * Node's recorder lists it as started and written to, which Rust's scripted
 * programs do not, so Rust's tests hold that sentence.
 */
import { appServer, codex, ENV, ofChief, prepares } from './codex-scenes.mjs'

/** An app-server that says `lines`, and goes on until it is asked to end. */
const saying = (...lines) => ({ lines, ends: 'asked' })
/** One that says them and ends. */
const sayingThenEnding = (...lines) => ({ lines, ends: 'itself' })
const initialized = '{"id":1,"result":{}}'
const configured = (config, extra = {}) => JSON.stringify({ id: 2, result: { config }, ...extra })

export function roleScenarios() {
  const dialogue = (name, server, { after = [], fields = {} } = {}) => ({
    name: `codex: ${name}`,
    harness: 'codex',
    env: ENV,
    steps: [codex(), prepares(ofChief(fields), server), ...after],
  })
  const ended = (name, server) => dialogue(`an app-server ${name} refuses the launch`, server)
  const wait = [{ advance: 10_000 }]
  const wholeText = (role) =>
    `# ConsensFlow ${role}\n\nThe ${role}'s whole text: "quotes", \`backticks\`, $HOME\nand newlines.\n| saved-worker |`
  const roles = ['chief', 'advisor', 'worker', 'reviewer', 'designer']
  return [
    {
      name: 'codex: every role enters with its whole text, after the instructions Codex has of its own',
      harness: 'codex',
      env: ENV,
      steps: [
        codex(),
        ...roles.map((role) =>
          prepares(
            { role, instructions: wholeText(role) },
            appServer('User instructions: preserve "quotes", `backticks`, $HOME\nand newlines.'),
          ),
        ),
      ],
    },
    dialogue(
      'texts JSON writes with escapes are carried whole, in the role and in what Codex had',
      appServer('Back\\slash,   line separator, \u007f delete, \u0001 control, 😀 emoji'),
      { fields: { instructions: 'Role   "text" \\ with\ttab\u0000 and é' } },
    ),
    dialogue(
      'a role is given after nothing where Codex has no instructions of its own',
      saying(initialized, configured({ developer_instructions: null })),
    ),
    dialogue(
      'a configuration that is truthy and holds nothing has no instructions of its own',
      saying(initialized, configured(7)),
    ),
    dialogue(
      'an error that is falsy is no error',
      saying('{"id":1,"error":0}', configured({ developer_instructions: 'kept' }, { error: '' })),
    ),
    dialogue(
      'an answer to the configuration comes before it is asked, and is taken',
      saying(configured({ developer_instructions: 'early' })),
    ),
    dialogue(
      'an answer to initialize that comes twice has the configuration asked twice',
      saying(
        '{"id":1.0,"result":{}}',
        '{"id":1e0,"result":{}}',
        configured({ developer_instructions: '' }),
      ),
    ),
    dialogue(
      'lines that answer neither request are let by, and a number is read as the number it is',
      saying(
        '[]',
        '7',
        '"x"',
        '{"id":null}',
        '{"id":"1"}',
        '{"id":1.5}',
        initialized,
        configured({}),
      ),
    ),
    ended('that ends at once', sayingThenEnding()),
    ended('that ends after the lines it said', sayingThenEnding('[]', '{"id":"2"}')),
    ended('that answers initialize with an error', saying('{"id":1,"error":{"message":"no"}}')),
    ended(
      'that answers the configuration with an error',
      saying(initialized, '{"id":2,"error":{}}'),
    ),
    ended('that answers with no configuration', saying(initialized, '{"id":2,"result":{}}')),
    ended('that answers with a null configuration', saying(initialized, '{"id":2,"result":null}')),
    ended(
      'that answers with a falsy configuration',
      saying(initialized, '{"id":2,"result":{"config":0}}'),
    ),
    ended(
      'that answers with instructions that are no text',
      saying(initialized, configured({ developer_instructions: 7 })),
    ),
    ended('that speaks no JSON', saying('not json')),
    ended('that says an empty line', saying('')),
    dialogue('an app-server that says nothing in ten seconds refuses the launch', saying(), {
      after: wait,
    }),
    dialogue(
      'an app-server that answers initialize and then nothing in ten seconds',
      saying(initialized),
      {
        after: wait,
      },
    ),
  ]
}
