/**
 * What Node's agents' API answers in the corners no suite and no recorded trace
 * looks at, one line a request: the number a follow-up names, what a transcript
 * is asked for, a post to a task's `transcript`, the words a tell or a note may
 * not be, a chief's history with something in it and with nothing.
 *
 *   node tests/goldens/daemon/probes/api-corners.mjs
 *
 * The Rust tests of the routes (`crates/cf-daemon/src/api/routes/<route>/tests.rs`)
 * say "what Node answered" for these: this is where to see it again, on the Node
 * of the day. It prints the same lines on every run but for the times, which
 * are `«now»`.
 */
import { deliver, withApi } from '../../../core-api-fixture.mjs'

const NOW = /\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d\.\d{3}Z/g

const show = async (label, promise) => {
  const { status, body } = await promise
  console.log(`${label}\n    -> ${status} ${JSON.stringify(body).replace(NOW, '«now»')}`)
}

await withApi(async ({ ledger, project, token, call }) => {
  const chief = token('chief')
  const zeus = token('zeus')

  console.log('--- a follow-up (`after`), as the chief')
  for (const [label, after] of [
    ['"abc"', 'abc'],
    ['2.5', 2.5],
    ['null', null],
    ['true', true],
    ['{}', {}],
    ['[3]', [3]],
    ['"0x10"', '0x10'],
    ['1e21', 1e21],
    ['-3', -3],
    ['""', ''],
    ['{"toString":1}', { toString: 1 }],
    ['[{"toString":1}]', [{ toString: 1 }]],
  ]) {
    await show(`after ${label}`, call(chief, 'POST', '/api/tasks', { after, body: 'x' }))
  }
  await show('after "abc", no body', call(chief, 'POST', '/api/tasks', { after: 'abc' }))
  await show(
    'after "abc", needs wrong',
    call(chief, 'POST', '/api/tasks', { after: 'abc', body: 'x', needs: 'T-1' }),
  )
  await show(
    'after "abc", before wrong',
    call(chief, 'POST', '/api/tasks', { after: 'abc', body: 'x', before: [0] }),
  )
  await show(
    'after 2.5, a tier that is none',
    call(chief, 'POST', '/api/tasks', { after: 2.5, body: 'x', tier: 'huge' }),
  )

  console.log('--- a task for a pool and a tier, as the chief')
  await show('no tier', call(chief, 'POST', '/api/tasks', { body: 'x' }))
  await show('tier 5', call(chief, 'POST', '/api/tasks', { body: 'x', tier: 5 }))
  await show('tier null', call(chief, 'POST', '/api/tasks', { body: 'x', tier: null }))
  await show(
    'critical, no purpose',
    call(chief, 'POST', '/api/tasks', { body: 'x', tier: 'critical' }),
  )
  await show(
    'design, a tier that is none',
    call(chief, 'POST', '/api/tasks', { body: 'x', design: true, tier: 'junk' }),
  )
  await show(
    'design "true" is no flag',
    call(chief, 'POST', '/api/tasks', { body: 'x', design: 'true', tier: 'standard' }),
  )
  await show(
    'design and advice: design',
    call(chief, 'POST', '/api/tasks', { body: 'x', design: true, advice: true }),
  )
  await show('self', call(chief, 'POST', '/api/tasks', { body: 'Plan', self: true }))
  await show(
    'self and after',
    call(chief, 'POST', '/api/tasks', { body: 'Plan', self: true, after: 1 }),
  )
  await show('body 7', call(chief, 'POST', '/api/tasks', { body: 7, tier: 'standard' }))
  await show(
    'needs "T-1"',
    call(chief, 'POST', '/api/tasks', { body: 'x', tier: 'standard', needs: 'T-1' }),
  )
  await show(
    'before [0]',
    call(chief, 'POST', '/api/tasks', { body: 'x', tier: 'standard', before: [0] }),
  )
  await show(
    'needs [1.0, 1]',
    call(chief, 'POST', '/api/tasks', { body: 'x', tier: 'standard', needs: [1.0, 1] }),
  )
  await show(
    'a task to work on',
    call(chief, 'POST', '/api/tasks', { body: 'Parser', tier: 'standard' }),
  )

  console.log('--- the task route')
  await show('GET T-1', call(chief, 'GET', '/api/tasks/1'))
  await show('POST T-1/transcript, no body', call(chief, 'POST', '/api/tasks/1/transcript'))
  await show(
    'POST T-1/transcript, words',
    call(chief, 'POST', '/api/tasks/1/transcript', { body: 'again' }),
  )
  await show(
    'POST T-1/transcript, zeus',
    call(zeus, 'POST', '/api/tasks/1/transcript', { body: 'again' }),
  )
  await show('GET T-1/tell', call(chief, 'GET', '/api/tasks/1/tell'))
  await show('POST T-1', call(chief, 'POST', '/api/tasks/1'))
  await show(
    'POST T-1/done, zeus, nobody has it',
    call(zeus, 'POST', '/api/tasks/1/done', { body: 'x' }),
  )
  await show('POST T-1/accept, zeus', call(zeus, 'POST', '/api/tasks/1/accept', {}))
  await show('POST T-1/accept, open', call(chief, 'POST', '/api/tasks/1/accept', {}))
  await show('POST T-1/resume, no words', call(chief, 'POST', '/api/tasks/1/resume', {}))
  await show('POST T-1/resume, words 5', call(chief, 'POST', '/api/tasks/1/resume', { body: 5 }))
  await show('POST T-1/resume, open', call(chief, 'POST', '/api/tasks/1/resume', { body: 'go' }))
  await show('POST T-1/pause', call(chief, 'POST', '/api/tasks/1/pause', {}))
  await show(
    'POST T-1/tell, paused, nobody has it',
    call(chief, 'POST', '/api/tasks/1/tell', { body: 5 }),
  )
  await show('POST T-1/cancel', call(chief, 'POST', '/api/tasks/1/cancel', {}))
  await show('GET T-007', call(chief, 'GET', '/api/tasks/007'))

  console.log('--- questions and notes, as a member with no task')
  await show('question', call(zeus, 'POST', '/api/questions', { body: 'Which?' }))
  await show('question, no body', call(zeus, 'POST', '/api/questions', {}))
  await show('question, body 5', call(zeus, 'POST', '/api/questions', { body: 5 }))
  await show('questions null', call(zeus, 'POST', '/api/questions', { body: 'x', questions: null }))
  await show('questions []', call(zeus, 'POST', '/api/questions', { body: 'x', questions: [] }))
  await show('questions "x"', call(zeus, 'POST', '/api/questions', { body: 'x', questions: 'x' }))
  await show(
    'questions with options, a body that is not read',
    call(zeus, 'POST', '/api/questions', {
      body: 5,
      questions: [
        {
          question: 'Which parser?',
          header: 'Parser',
          options: [{ label: 'A' }, { label: 'B', description: 'bee' }],
        },
      ],
    }),
  )
  await show('note', call(zeus, 'POST', '/api/notes', { body: 'hello' }))
  await show('note to the human', call(zeus, 'POST', '/api/notes', { body: 'hello', to: 'human' }))
  await show('note, body 5', call(zeus, 'POST', '/api/notes', { body: 5 }))
  await show('note, no body', call(zeus, 'POST', '/api/notes', {}))
  await show('chief note', call(chief, 'POST', '/api/notes', { body: 'to the human' }))
  await show(
    'chief note to "chief"',
    call(chief, 'POST', '/api/notes', { body: 'to the human', to: 'chief' }),
  )

  console.log('--- a chief history with nothing in it')
  for (const query of ['?page=abc', '?page=2.5', '?page=0', '?find=x', '?find=']) {
    await show(`history ${query}`, call(chief, 'GET', `/api/history${query}`))
  }
  await show('history, zeus', call(zeus, 'GET', '/api/history'))

  console.log('--- a chief history with something in it')
  const chiefRow = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
  const old = ledger.startConversation(chiefRow.id, { harness: 'claude-code' })
  ledger.copyTranscript(old.id, [
    { id: 'u1', role: 'user', text: 'Build the parser', complete: true },
    { id: 'a1', role: 'assistant', text: 'On it. The codeword is plum.', complete: true },
    { id: 't1', role: 'tool', text: 'ls -la output', complete: true },
  ])
  ledger.switchChief(project.id, { harness: 'claude-code', agent: 'other' })
  for (const query of [
    '',
    '?tools=1',
    '?tools=true',
    '?find=PLUM',
    '?find=zzz',
    '?page=1',
    '?page=2',
    '?page=abc',
    '?page=1.5',
    '?page=',
    '?page=1e0',
    '?page=%201%20',
  ]) {
    await show(`history ${query}`, call(chief, 'GET', `/api/history${query}`))
  }
  const read = ledger
    .events(project.id, 0, 500)
    .filter((event) => event.kind === 'chief.history.read')
    .map((event) => event.data)
  console.log(
    `the chief's reads of its history, as the ledger wrote them\n    -> ${JSON.stringify(read)}`,
  )
})

// A transcript of five items, and how many Node gives for each `last`.
await withApi(async ({ ledger, project, token, call }) => {
  const chief = token('chief')
  const zeus = ledger.project(project.id).participants.find((p) => p.handle === 'zeus')
  const { task, message } = ledger.createTask(project.id, {
    from: 'chief',
    to: 'zeus',
    body: 'Parser',
  })
  deliver(ledger, message)
  const conversation = ledger.startConversation(zeus.id, { harness: 'claude-code' })
  const items = Array.from({ length: 5 }, (_, at) => ({
    id: `i${at + 1}`,
    role: 'assistant',
    text: `item ${at + 1}`,
    complete: true,
  }))
  ledger.copyTranscript(conversation.id, items)
  console.log('--- the last items of a transcript of five, by `last`')
  for (const last of [
    undefined,
    '',
    '0',
    '1',
    '1.2',
    '2.5',
    '0.5',
    '4.5',
    '5',
    '5.5',
    '7',
    '1e1',
    '0x2',
    ' 3 ',
    '+3',
    'Infinity',
    '-Infinity',
    'NaN',
    '1_0',
    '-2.5',
    '2,5',
    '3abc',
    '.5',
    '2.',
  ]) {
    const query = last === undefined ? '' : `?last=${encodeURIComponent(last)}`
    const { status, body } = await call(
      chief,
      'GET',
      `/api/tasks/${task.number}/transcript${query}`,
    )
    console.log(
      `last ${JSON.stringify(last)} -> ${status} ${body.items?.map((item) => item.id).join(',')}`,
    )
  }
})
