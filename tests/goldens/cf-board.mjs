/**
 * The goldens of `cf` inside a window: what Node's board commands
 * (src/core/cli.js) said and asked for each case, written to
 * crates/cf/tests/goldens/board.json for the Rust `cf` to be held to. Each
 * case runs against a scripted API that answers its requests in turn with
 * the case's replies, written as the daemon writes JSON, and keeps what it
 * was asked: method, path, token and body. `npm run goldens:cf` writes the
 * file again.
 */
import { writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { runCoreCli } from '../../src/core/cli.js'

const OUT = new URL('../../crates/cf/tests/goldens/board.json', import.meta.url)

/** A reply of `body` as the daemon writes it, JSON.stringify's own escapes and all. */
const ok = (body, status = 200) => ({ status, text: JSON.stringify(body) })

const task = (fields = {}) => ({
  number: 3,
  state: 'working',
  assignee: 'zeus',
  requester: 'chief',
  title: 'Parser',
  blockedBy: [],
  pool: 'worker',
  tier: 'standard',
  deletedAt: null,
  messages: [],
  ...fields,
})
const message = (fields = {}) => ({
  id: 12,
  kind: 'question',
  state: 'queued',
  sender: 'zeus',
  recipient: 'chief',
  task: 3,
  preview: 'Which one?',
  ...fields,
})
const EVENT = {
  session_id: 'abc',
  hook_event_name: 'PreToolUse',
  tool_name: 'AskUserQuestion',
  tool_input: {
    questions: [
      {
        question: 'Which colour?',
        header: 'Colour',
        options: [{ label: 'red', description: 'Warm' }, { label: 'blue' }],
        multiSelect: false,
      },
      {
        question: 'Which tools?',
        header: 'Tools',
        options: [{ label: 'vite' }, { label: 'esbuild' }],
        multiSelect: true,
      },
    ],
    metadata: { source: 'test' },
  },
}
const ANSWERED = ok({
  question: {},
  answer: { id: 13, from: 'chief', body: 'blue', choices: [['blue'], ['vite', 'esbuild']] },
})
/** An emoji cut in half, as `.slice` cuts a preview or a transcript item. */
const CUT = 'cut 😀'.slice(0, 5)

const CASES = [
  // What cf says it does, and a command written wrong.
  { name: 'usage with no command', args: [] },
  { name: 'usage on help', args: ['help'] },
  { name: 'usage on --help', args: ['--help'] },
  { name: 'usage on -h, as JSON', args: ['-h', '--json'] },
  { name: 'task usage', args: ['task', '--help'] },
  { name: 'task usage on help', args: ['task', 'help'] },
  { name: 'an unknown command', args: ['frobnicate'] },
  { name: 'an unknown task command without a task', args: ['task', 'frobnicate'] },
  { name: 'an unknown task command', args: ['task', 'frobnicate', 'T-3'] },
  { name: 'a task that is no number', args: ['task', 'get', 'T-x'] },
  { name: 'a message that is no number', args: ['inbox', 'read', 'T-3'] },
  { name: 'a note of nothing', args: ['note'] },
  { name: 'asking the human', args: ['ask', '--human', 'may I?'] },
  { name: 'asking nothing', args: ['ask'] },
  { name: 'telling a window nothing', args: ['tell', 'T-3'] },
  { name: 'answering no message', args: ['answer', 'bogus', '-'], stdin: 'never read' },
  { name: 'adding work with no tier', args: ['task', 'add', 'do it'] },
  { name: 'adding own work with nothing to wait for', args: ['task', 'add', '--self', 'x'] },
  { name: 'adding work with no text', args: ['task', 'add', '--tier', 'light'] },
  { name: 'a needs list with a blank', args: ['task', 'add', '--self', '--needs', 'T-3,', 'x'] },
  { name: 'done with no result', args: ['task', 'done', 'T-3'] },
  { name: 'resume with blank words', args: ['task', 'resume', 't-3', ' '] },
  { name: 'reopen with no follow-up', args: ['task', 'reopen', '3'] },
  {
    name: 'a transcript of no items is asked for after the task',
    args: ['task', 'get', 'T-3', '--transcript', '--last', '0'],
    replies: [ok({ task: task() })],
  },

  // The inbox and the messages.
  { name: 'an empty inbox', args: ['inbox'], replies: [ok({ messages: [] })] },
  {
    name: 'an inbox, a preview cut through an emoji among it',
    args: ['inbox'],
    replies: [
      ok({
        messages: [
          message(),
          message({
            id: 13,
            kind: 'notice',
            state: 'read',
            sender: null,
            task: null,
            taskNumber: 4,
          }),
          message({ id: 14, kind: 'note', task: null, preview: CUT }),
        ],
      }),
    ],
  },
  {
    name: 'a message in full',
    args: ['inbox', 'read', 'm-12'],
    replies: [ok({ message: { ...message(), body: 'Which one?\nThe second line.' } })],
  },
  {
    name: 'a note to the human',
    args: ['note', 'the', 'build', 'is', 'green', '--human'],
    replies: [ok({ message: message({ id: 20, recipient: 'human' }) }, 201)],
  },
  {
    name: 'a note from standard input, its last newline dropped',
    args: ['note', '-'],
    stdin: 'line one\n  `$(not run)`\r\n\n',
    replies: [ok({ message: message({ id: 21 }) }, 201)],
  },
  {
    name: 'a question to the chief',
    args: ['ask', 'which', 'one?'],
    replies: [ok({ message: message({ id: 22 }) }, 201)],
  },
  {
    name: 'telling a window something urgent',
    args: ['tell', 'T-3', 'stop', 'that'],
    replies: [ok({ message: message({ id: 23, recipient: 'zeus' }) }, 201)],
  },
  {
    name: 'an answer',
    args: ['answer', 'm-12', 'the', 'second'],
    replies: [ok({ message: message({ id: 24, recipient: 'zeus', state: 'queued' }) }, 201)],
  },
  {
    name: 'an answer the human passes on first',
    args: ['answer', '12', 'yes'],
    replies: [ok({ message: message({ id: 25, recipient: 'zeus', state: 'gated' }) }, 201)],
  },

  // Who is on the board.
  { name: 'no staff', args: ['staff'], replies: [ok({ members: [] })] },
  {
    name: 'the staff',
    args: ['staff'],
    replies: [
      ok({
        members: [
          { handle: 'zeus', roles: ['worker'], tier: 'standard' },
          { handle: 'athena', roles: ['advisor', 'reviewer'], tier: 'critical' },
        ],
      }),
    ],
  },
  {
    name: 'who I am, on no task',
    args: ['whoami'],
    replies: [
      ok({ participant: { handle: 'chief', role: 'chief' }, project: { name: 'app' }, task: null }),
    ],
  },
  {
    name: 'who I am, on a task',
    args: ['whoami'],
    replies: [
      ok({
        participant: { handle: 'zeus', role: 'worker' },
        project: { name: 'app' },
        task: { number: 3, title: 'Parser' },
      }),
    ],
  },
  { name: 'the history', args: ['history'], replies: [ok({ text: 'Human: hello' })] },
  {
    name: 'the history, a page of a search',
    args: ['history', '--page', '2', '--find', 'a b&c=d/é*~!', '--tools'],
    replies: [ok({ text: 'Human: a b&c' })],
  },

  // The board.
  { name: 'an empty board', args: ['task', 'list'], replies: [ok({ open: [], lanes: [] })] },
  {
    name: 'the board, waiting work and lanes',
    args: ['task'],
    replies: [
      ok({
        open: [
          task({ number: 4, state: 'open', assignee: null, blockedBy: [2, 3], pool: 'reviewer' }),
          task({ number: 5, state: 'open', assignee: null, pool: 'designer', tier: null }),
        ],
        lanes: [
          { handle: 'zeus', role: 'worker', tasks: [task()] },
          { handle: 'athena', role: 'advisor', tasks: [] },
        ],
      }),
    ],
  },
  {
    name: 'the board as JSON, keys as JavaScript orders them',
    args: ['task', 'list', '--json'],
    replies: [ok({ open: [], lanes: [], counts: { b: 1, 2: 0, a: 3, 1: 4 } })],
  },
  {
    name: 'work for a tier',
    args: ['task', 'add', '--tier', 'standard', 'Write', 'the', '--force', 'parser'],
    replies: [ok({ task: task({ state: 'open', assignee: null }) }, 201)],
  },
  {
    name: 'work for the nearest tier, gated, waiting and holding',
    args: [
      'task',
      'add',
      '--tier',
      'critical',
      '--purpose',
      'x',
      '--needs',
      'T-1',
      '--before',
      'T-9,T-10',
      'go',
    ],
    replies: [
      ok(
        {
          task: task({ state: 'open', assignee: null, tier: 'complex', blockedBy: [1] }),
          asked: 'critical',
          gated: true,
        },
        201,
      ),
    ],
  },
  {
    name: 'advice, holding one',
    args: ['task', 'add', '--advice', '--tier', 'complex', '--before', 'T-9', 'which?'],
    replies: [
      ok({ task: task({ state: 'open', assignee: null, pool: 'advisor', tier: null }) }, 201),
    ],
  },
  {
    name: 'a review',
    args: ['task', 'add', '--review', '--tier', 'light', 'look'],
    replies: [
      ok({ task: task({ state: 'open', assignee: null, pool: 'reviewer', tier: 'light' }) }, 201),
    ],
  },
  {
    name: 'a design',
    args: ['task', 'add', '--design', 'a', 'logo'],
    replies: [
      ok({ task: task({ state: 'open', assignee: null, pool: 'designer', tier: null }) }, 201),
    ],
  },
  {
    name: 'a follow-up in the same window, gated and waiting',
    args: ['task', 'add', '--after', 'T-3', '--needs', 'T-1,T-2', 'more'],
    replies: [ok({ task: task({ number: 6, blockedBy: [1, 2] }), gated: true }, 201)],
  },
  {
    name: 'own work for later',
    args: ['task', 'add', '--self', '--needs', 'T-3', 'later'],
    replies: [
      ok({ task: task({ number: 7, assignee: 'chief', blockedBy: [3] }), gated: true }, 201),
    ],
  },
  {
    name: 'a task and its thread',
    args: ['task', 'get', 'T-3'],
    replies: [
      ok({
        task: task({
          messages: [
            {
              ...message({ kind: 'task', sender: 'chief', recipient: 'zeus' }),
              body: 'Parser\nAll of it.',
            },
            { ...message({ id: 30, kind: 'result', preview: 'Done' }), body: 'Done.' },
          ],
        }),
      }),
    ],
  },
  {
    name: 'a task the human deleted',
    args: ['task', 'get', 'T-3'],
    replies: [ok({ task: task({ state: 'accepted', deletedAt: '2026-10-04T09:00:00.000Z' }) })],
  },
  {
    name: "a task's transcript, an item cut through an emoji",
    args: ['task', 'get', 'T-3', '--transcript'],
    replies: [
      ok({ task: task() }),
      ok({
        total: 14,
        items: [
          { role: 'user', complete: true, text: 'Write the parser.' },
          { role: 'assistant', complete: false, text: `${'x'.repeat(599)}😀 and more` },
          { role: 'tool', complete: true, text: 'ok' },
          { role: 'custom', complete: true, text: 'a note' },
          { role: 'system', complete: true, text: CUT },
        ],
      }),
    ],
  },
  {
    name: 'a transcript of the last items, the number as JavaScript reads it',
    args: ['task', 'get', 'T-3', '--transcript', '--last', '0x10'],
    replies: [ok({ task: task() }), ok({ total: 0, items: [] })],
  },
  {
    name: 'a transcript as JSON',
    args: ['task', 'get', 'T-3', '--transcript', '--json', '--last', '1'],
    replies: [ok({ task: task() }), ok({ task: 'shadowed', total: 1, items: [] })],
  },
  {
    name: 'a task done',
    args: ['task', 'done', 'T-3', 'it', 'works'],
    replies: [ok({ task: task({ state: 'done' }) })],
  },
  {
    name: 'a task accepted',
    args: ['task', 'accept', 'T-3'],
    replies: [ok({ task: task({ state: 'accepted' }) })],
  },
  {
    name: 'a task paused',
    args: ['task', 'pause', 'T-3'],
    replies: [ok({ task: task({ state: 'paused' }) })],
  },
  {
    name: 'a task resumed',
    args: ['task', 'resume', 'T-3', 'go', 'on'],
    replies: [ok({ task: task() })],
  },
  {
    name: 'a task resumed after its window ended',
    args: ['task', 'resume', 'T-3', 'go'],
    replies: [ok({ task: task({ state: 'open', assignee: null }) })],
  },
  {
    name: 'a refusal, in the API’s words',
    args: ['task', 'accept', 'T-3'],
    replies: [ok({ message: 'T-3 is not yours to accept' }, 403)],
  },
  { name: 'a refusal with no words', args: ['task', 'cancel', 'T-3'], replies: [ok({}, 500)] },

  // The hooks a harness runs.
  {
    name: "Claude's question answered from the board",
    args: ['hook', 'claude'],
    stdin: JSON.stringify(EVENT),
    replies: [ok({ message: { id: 40 } }, 201), ok({ question: {}, answer: null }), ANSWERED],
  },
  {
    name: "Devin's question answered from the board, --json and all",
    args: ['--json', 'hook', 'devin'],
    stdin: JSON.stringify({ ...EVENT, tool_name: 'ask_user_question' }),
    replies: [ok({ message: { id: 41 } }, 201), ANSWERED],
  },
  {
    name: "Claude's question refused by the board",
    args: ['hook', 'claude'],
    stdin: JSON.stringify(EVENT),
    replies: [ok({ message: 'questions: one to 4 questions' }, 400)],
  },
  {
    name: "Devin's question refused by the board",
    args: ['hook', 'devin'],
    stdin: JSON.stringify({ ...EVENT, tool_name: 'ask_user_question' }),
    replies: [ok({ message: 'T-3 was cancelled' }, 409)],
  },
  {
    name: 'questions whose texts are numbers, answered in the order JavaScript keeps',
    args: ['hook', 'claude'],
    stdin: JSON.stringify({
      ...EVENT,
      tool_input: {
        questions: [
          { question: 'b', options: [{ label: 'x' }] },
          { question: '2', options: [{ label: 'y' }] },
          { question: 'b', options: [{ label: 'z' }] },
        ],
      },
    }),
    replies: [ok({ message: { id: 42 } }, 201), ok({ answer: { choices: [['x'], ['y'], ['z']] } })],
  },
  {
    name: 'another tool',
    args: ['hook', 'claude'],
    stdin: JSON.stringify({ ...EVENT, tool_name: 'Bash' }),
  },
  { name: "Claude's tool is not Devin's", args: ['hook', 'devin'], stdin: JSON.stringify(EVENT) },
  { name: 'a harness with no hook', args: ['hook', 'codex'], stdin: JSON.stringify(EVENT) },
  { name: 'an event that is no JSON', args: ['hook', 'claude'], stdin: '{"tool_name":' },
  {
    name: 'a wait of nothing gives up at once',
    args: ['hook', 'claude'],
    env: { CONSENSFLOW_QUESTION_WAIT_MS: '0' },
    stdin: JSON.stringify(EVENT),
    replies: [ok({ message: { id: 43 } }, 201)],
  },
  {
    name: 'a wait that is no number waits not at all',
    args: ['hook', 'claude'],
    env: { CONSENSFLOW_QUESTION_WAIT_MS: 'soon' },
    stdin: JSON.stringify(EVENT),
    replies: [ok({ message: { id: 44 } }, 201)],
  },
]

/** One case against its scripted API: what was asked, said and returned. */
async function run({ name, args, env = {}, stdin = null, replies = [] }) {
  const requests = []
  const queue = [...replies]
  const server = createServer((request, response) => {
    const chunks = []
    request.on('data', (chunk) => chunks.push(chunk))
    request.on('end', () => {
      const body = Buffer.concat(chunks).toString('utf8')
      requests.push({
        method: request.method,
        path: request.url,
        authorization: request.headers.authorization ?? null,
        body: body.length === 0 ? null : body,
      })
      const reply = queue.shift() ?? { status: 599, text: '{"message":"no reply scripted"}' }
      response.writeHead(reply.status, { 'content-type': 'application/json' })
      response.end(reply.text)
    })
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  let stdout = ''
  let stderr = ''
  try {
    const code = await runCoreCli(
      args,
      {
        CONSENSFLOW_URL: `http://127.0.0.1:${server.address().port}`,
        CONSENSFLOW_TOKEN: 'tok',
        ...env,
      },
      {
        out: (line) => {
          stdout += `${line}\n`
        },
        err: (line) => {
          stderr += `${line}\n`
        },
        input: async () => stdin ?? '',
      },
    )
    if (queue.length > 0) throw new Error(`${name}: ${queue.length} replies were never asked for`)
    // What a process writes: a lone surrogate (half an emoji .slice cut)
    // leaves Node's stdout as U+FFFD.
    const written = (text) => Buffer.from(text, 'utf8').toString('utf8')
    return {
      name,
      args,
      env,
      stdin,
      replies,
      requests,
      stdout: written(stdout),
      stderr: written(stderr),
      code,
    }
  } finally {
    server.closeAllConnections()
    await new Promise((resolve) => server.close(resolve))
  }
}

const goldens = []
for (const each of CASES) goldens.push(await run(each))
writeFileSync(OUT, `${JSON.stringify(goldens, null, 2)}\n`)
process.stdout.write(`${goldens.length} cases → ${OUT.pathname}\n`)
