/**
 * Codex's scenarios (`src/adapters/codex.js`, with `src/channels/codex.js`,
 * `src/channels.js` and `src/role-skills.js`): its launches, fresh, resumed
 * and refused, with what a launch asks of Codex's CLI (here); the role it is
 * given through its app-server (`codex-roles.mjs`); its looks and its
 * readiness, by what its broker says the window shows and by Codex's own
 * record (`codex-looks.mjs`); and its deliveries through the broker
 * (`codex-deliveries.mjs`). The cases of `tests/adapter-codex.test.mjs`,
 * `tests/codex-channel.test.mjs` and the Codex cases of
 * `tests/role-skills.test.mjs`, and more.
 *
 * A native queue that does not answer in time is not here: `execFile` ends a
 * run on a timer of its own, which neither clock of the recorder moves, and
 * the probe's 30 s is real time. Rust's tests hold that sentence.
 */

import { deliveryScenarios } from './codex-deliveries.mjs'
import { lookScenarios, readyScenarios, startedScenarios } from './codex-looks.mjs'
import { roleScenarios } from './codex-roles.mjs'
import {
  appServer,
  chief,
  codex,
  ENV,
  lists,
  ofChief,
  prepares,
  SECOND,
  session,
  THREAD,
} from './codex-scenes.mjs'

const WINDOWS = process.platform === 'win32'

/** What a refusal to list the MCP servers says in Rust's words, where V8 gave its own. */
const unreadable = (why) => ({
  why,
  answer: {
    throws:
      "could not list Codex's MCP servers to switch them off: its answer cannot be read as JSON",
  },
})

export function codexScenarios() {
  const prepared = (
    name,
    fields,
    before = [codex()],
    { children = [appServer()], after = [], kept } = {},
  ) => ({
    name: `codex: ${name}`,
    harness: 'codex',
    env: ENV,
    steps: [...before, { ...prepares(fields, ...children), ...(kept ? { kept } : {}) }, ...after],
  })
  const unnamed = session(null, { available: false })
  const plans = [
    prepared(
      'a fresh worker runs Codex under the supervisor, in full permission, the task last, and learns its thread from the broker',
      {},
      [codex()],
      {
        children: [appServer('Existing instructions of the user.')],
        after: [
          {
            started: {},
            served: { 'GET /session': [unnamed, unnamed, session(THREAD)] },
            optional: ['GET /session'],
          },
          { advance: 250, optional: ['GET /session'] },
          { advance: 250 },
        ],
      },
    ),
    prepared(
      'the chief keeps its connectors, asks the human in its own window, and has no model of its own',
      { role: 'chief', participant: chief, agent: null, message: null },
    ),
    prepared(
      'a reviewer is given its task as a window takes text',
      {
        role: 'reviewer',
        message: 'Review\r\nthis \u001b[31mred\u001b[0m line\ttab\u0007bell\u0085',
        agent: { id: 'r', kind: 'codex', model: 'gpt-5.6-luna', effort: '' },
      },
      [codex()],
    ),
    {
      name: 'codex: a member has every MCP server Codex would start switched off, and the chief keeps them',
      harness: 'codex',
      env: ENV,
      steps: [
        codex(
          lists(
            { name: 'cua_repl', enabled: true, transport: { type: 'stdio', command: 'cua' } },
            { name: 'computer-history' },
          ),
        ),
        prepares({}, appServer()),
        prepares({ ...ofChief(), launchId: SECOND }, appServer()),
      ],
    },
    {
      name: 'codex: a member whose list holds no server switches nothing off',
      harness: 'codex',
      env: ENV,
      steps: [
        codex({ 'mcp list --json': { stdout: '{"servers":[]}\n' } }),
        prepares({}, appServer()),
      ],
    },
    {
      name: 'codex: a list that holds names JavaScript reads as text switches them off as that text',
      harness: 'codex',
      env: ENV,
      steps: [
        codex(lists({ name: 5 }, { name: true }, { name: ['x'] }, { name: 1e-7 }, { name: -0 })),
        prepares({}, appServer()),
      ],
    },
    {
      name: 'codex: two launches share what Codex was asked, and draw a broker each',
      harness: 'codex',
      env: ENV,
      steps: [
        codex(),
        prepares({}, appServer()),
        prepares({ launchId: SECOND, resume: THREAD, message: null }, appServer()),
      ],
    },
    prepared(
      'a conversation Codex kept is resumed on its thread, and its window has nothing to learn',
      { resume: THREAD, message: null },
      [codex()],
      { after: [{ started: {} }] },
    ),
    prepared('a resumed conversation is given its follow-up', {
      resume: THREAD,
      message: 'And the tests',
    }),
    prepared(
      'an image agent opens on Codex’s own model, fresh and resumed',
      { agent: { id: 'img', kind: 'codex', model: 'codex-image', effort: 'high', designer: true } },
      [codex()],
      {
        after: [
          prepares(
            {
              launchId: SECOND,
              resume: THREAD,
              message: null,
              agent: { kind: 'codex', model: 'codex-image', effort: 'high', designer: true },
            },
            appServer(),
          ),
        ],
      },
    ),
    {
      name: 'codex: a model alone, an effort alone, empty ones, and none are each a window of their own',
      harness: 'codex',
      env: ENV,
      steps: [
        codex(),
        prepares({ agent: { kind: 'codex', model: 'gpt-5.6-luna' } }, appServer()),
        prepares({ agent: { kind: 'codex', effort: 'max' } }, appServer()),
        prepares({ agent: { kind: 'codex', model: '', effort: '' } }, appServer()),
        prepares(
          { agent: { kind: 'codex', model: 'm', effort: 'e', thinking: 'high' } },
          appServer(),
        ),
        prepares({ agent: null }, appServer()),
      ],
    },
    prepared('a window with no first message', { message: null }),
    prepared('a window with an empty first message', { message: '' }),
    prepared('Codex not installed is refused, and nothing is drawn', {}, [], { children: [] }),
    prepared(
      'an empty directory is refused before Codex is asked anything',
      { directory: '' },
      [codex()],
      {
        children: [],
      },
    ),
    prepared(
      'a window with no role text is refused, after its broker was drawn',
      { instructions: '' },
      [codex()],
      { children: [] },
    ),
    prepared(
      'a conversation of an empty id is refused, after the role and the MCP servers',
      { resume: '', message: null },
      [codex()],
      {
        kept: {
          why: "a sentence of Rust's own where V8 threw its TypeError: no ledger holds an empty id",
          answer: { throws: 'a Codex window resumes a thread by its id' },
        },
      },
    ),
  ]

  const queueless = (name, says, { children = [] } = {}) =>
    prepared(
      name,
      { role: 'chief', participant: chief, agent: null, message: null },
      [codex(says)],
      {
        children,
      },
    )
  const refused = (name, version) =>
    queueless(name, { 'queue --help': { stdout: '', exit: 2 }, '--version': version })
  const noQueue = [
    refused('a Codex without the native queue is refused, naming its version', {
      stdout: 'codex-cli 0.150.0\n',
    }),
    refused('a Codex that says no version is refused as an unnamed one', { stdout: 'codex\n' }),
    refused('a Codex that cannot say its version is refused as an unnamed one', {
      stdout: '',
      exit: 1,
      stderr: 'no\n',
    }),
    refused('a version said with a failing exit is still the version', {
      stdout: 'codex 9.9.9\n',
      exit: 1,
    }),
    refused('the first version number in what it says is its version, and what sticks to it', {
      stdout: 'node 20.1 codex 1.2.3-beta.1+build.7 (extra)\n',
    }),
    refused('a version ends at JavaScript’s spaces: a no-break space and a byte order mark', {
      stdout: 'a 1.2.3 b c 4.5.6﻿d\n',
    }),
    refused('a version does not end at a next-line character, which JavaScript takes for none', {
      stdout: 'v1.2.3\u0085x y\n',
    }),
    refused('a version is written in ASCII digits', {
      stdout: '٠.١.٢ and 1.2\n',
    }),
    queueless('help that holds no flag is no native queue', {
      'queue --help': { stdout: 'Usage: codex queue\n' },
    }),
    queueless('help that holds one of the two flags is no native queue', {
      'queue --help': { stdout: 'Usage: codex queue --thread <id>\n' },
    }),
    queueless('help that holds the other of the two flags is no native queue either', {
      'queue --help': { stdout: 'Usage: codex queue --message <text>\n' },
    }),
    queueless('a flag followed by a letter, a digit or an underscore is not that flag', {
      'queue --help': { stdout: '--threads --message_text --thread1\n' },
    }),
  ]
  const accepted = (name, help) =>
    prepared(name, { role: 'chief', participant: chief, agent: null, message: null }, [
      codex({ 'queue --help': { stdout: help } }),
    ])
  const queued = [
    accepted('a flag followed by a hyphen is that flag', '--thread-id <id> --message-text <text>'),
    accepted(
      'a flag followed by a letter of another alphabet is that flag, as JavaScript’s word is ASCII',
      '--threadé --message٠',
    ),
    accepted('flags at the very end of the help are flags', 'Usage:\n  --message\n  --thread'),
  ]

  const named = (what, servers) =>
    prepared(`a member's MCP server that ${what} stops the launch`, {}, [codex(lists(...servers))])
  const unswitchable = [
    named('has no name', [{}]),
    named('is no object', [7]),
    named('is named null', [{ name: null }]),
    named('is named by an object', [{ name: {} }]),
    named('is named by a list of two', [{ name: ['a', 'b'] }]),
    named('has a name that needs quotes', [{ name: 'a.b' }]),
    named('has a name with a space', [{ name: 'a b' }]),
    named('has an empty name', [{ name: '' }]),
    named('has a name of letters Codex’s keys do not hold', [{ name: 'serveur-é' }]),
    named('comes after one that could be switched off', [{ name: 'fine' }, { name: 'a=b' }]),
    prepared("a member's MCP server listed as null stops the launch", {}, [codex(lists(null))], {
      kept: {
        why: "a sentence of Rust's own where V8 threw its TypeError reading the name of null",
        answer: { throws: 'cannot switch off a Codex MCP server listed as null for a member' },
      },
    }),
    prepared(
      "a member's MCP server named by an object with a toString of its own stops the launch",
      {},
      [codex({ 'mcp list --json': { stdout: '[{"name":{"toString":1}}]\n' } })],
      {
        kept: {
          why: "a sentence of Rust's own where V8 threw its TypeError making the name text",
          answer: { throws: 'an object with a toString of its own cannot be made text' },
        },
      },
    ),
  ]
  const unlisted = [
    prepared(
      'a list that is no JSON stops the launch',
      {},
      [codex({ 'mcp list --json': { stdout: 'not json\n' } })],
      {
        kept: unreadable('V8 gave its JSON.parse message; Rust says the answer cannot be read'),
      },
    ),
    prepared(
      'a list that says nothing is no JSON either',
      {},
      [codex({ 'mcp list --json': { stdout: '' } })],
      {
        kept: unreadable('V8 gave its JSON.parse message; Rust says the answer cannot be read'),
      },
    ),
    prepared(
      'a list that begins with a byte order mark is no JSON',
      {},
      [codex({ 'mcp list --json': { stdout: '﻿[]\n' } })],
      { kept: unreadable('V8 gave its JSON.parse message; Rust says the answer cannot be read') },
    ),
    prepared('a list too long for the buffer stops the launch in execFile’s words', {}, [
      codex({ 'mcp list --json': { overflows: true } }),
    ]),
    ...(WINDOWS
      ? []
      : [
          prepared(
            'a list that fails stops the launch in execFile’s words, the command named',
            {},
            [codex({ 'mcp list --json': { stdout: '', exit: 3, stderr: 'boom\n' } })],
          ),
        ]),
  ]

  return [
    ...plans,
    ...noQueue,
    ...queued,
    ...unswitchable,
    ...unlisted,
    ...roleScenarios(),
    ...lookScenarios(),
    ...readyScenarios(),
    ...startedScenarios(),
    ...deliveryScenarios(),
  ]
}
