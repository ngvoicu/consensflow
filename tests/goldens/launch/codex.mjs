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
  ofChief,
  prepares,
  SECOND,
  session,
  THREAD,
} from './codex-scenes.mjs'

/**
 * What a Codex that has MCP servers would list if it were asked: one reached
 * by command, one by URL, and one (an older Codex) with no transport.
 */
const SERVERS = [
  { name: 'cua_repl', enabled: true, transport: { type: 'stdio', command: 'cua', args: [] } },
  {
    name: 'idea',
    enabled: true,
    transport: { type: 'streamable_http', url: 'http://127.0.0.1:64342/stream' },
  },
  { name: 'computer-history' },
]

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
    prepared('the chief asks the human in its own window, and has no model of its own', {
      role: 'chief',
      participant: chief,
      agent: null,
      message: null,
    }),
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
      name: 'codex: a member whose Codex has MCP servers is launched as the chief is: Codex is never asked to list them, and none is switched off',
      harness: 'codex',
      env: ENV,
      steps: [
        codex({ 'mcp list --json': { stdout: `${JSON.stringify(SERVERS)}\n` } }),
        prepares({}, appServer()),
        prepares({ ...ofChief(), launchId: SECOND }, appServer()),
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
      'a conversation of an empty id is refused, after the role',
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

  return [
    ...plans,
    ...noQueue,
    ...queued,
    ...roleScenarios(),
    ...lookScenarios(),
    ...readyScenarios(),
    ...startedScenarios(),
    ...deliveryScenarios(),
  ]
}
