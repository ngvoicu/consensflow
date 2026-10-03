import assert from 'node:assert/strict'
import path from 'node:path'
import { describe, it } from 'node:test'
import { openLedger, PAGE_BYTES, TRANSCRIPT_ITEM_MAX } from '../src/ledger/index.js'
import { clock, deliver, dropTrace, names, staff, withDir, withLedger } from './ledger-fixtures.mjs'

/** Conversations, their copy, and the lead's across a switch (src/ledger/conversations.js). */

describe('conversations', () => {
  it('gives a participant one current conversation, and a native session to one conversation', async () => {
    await withLedger((ledger) => {
      const { id } = staff(ledger)
      const first = ledger.startConversation(id('zeus'), { harness: 'claude-code' })
      const second = ledger.startConversation(id('zeus'), { harness: 'claude-code' })
      assert.notEqual(first.id, second.id)
      assert.equal(ledger.currentConversation(id('zeus')).id, second.id)

      ledger.bindConversation(second.id, 'native-1')
      assert.equal(ledger.currentConversation(id('zeus')).nativeSession, 'native-1')
      // Unique within a harness: two harnesses may mint the same string.
      const chief = ledger.startConversation(id('chief'), { harness: 'claude-code' })
      assert.throws(() => ledger.bindConversation(chief.id, 'native-1'), {
        code: 'native-session-taken',
      })
      const codex = ledger.startConversation(id('diana'), { harness: 'codex' })
      assert.equal(ledger.bindConversation(codex.id, 'native-1').nativeSession, 'native-1')
      ledger.endConversation(second.id)
      assert.equal(ledger.currentConversation(id('zeus')), null)
    })
  })

  it('refuses to bind or copy a conversation it does not have, and ends one only once', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      assert.throws(() => ledger.bindConversation(99, 'native-1'), {
        code: 'unknown-conversation',
      })
      assert.throws(() => ledger.copyTranscript(99, []), { code: 'unknown-conversation' })
      assert.equal(ledger.endConversation(99), null)
      const conversation = ledger.startConversation(id('zeus'), { harness: 'claude-code' })
      const ended = ledger.endConversation(conversation.id)
      assert.notEqual(ended.endedAt, null)
      assert.deepEqual(ledger.endConversation(conversation.id), ended, 'ended already: as it was')
      assert.equal(
        ledger.events(project.id).filter((event) => event.kind === 'conversation.ended').length,
        1,
      )
    })
  })

  it('follows a window switched to another conversation: a new one bound to it, its own earlier one back, never one another participant holds', async () => {
    await withLedger((ledger) => {
      const { id } = staff(ledger)
      const first = ledger.startConversation(id('zeus'), { harness: 'claude-code' })
      ledger.bindConversation(first.id, 'native-1')
      const follow = (participant, nativeSession) =>
        ledger.followConversation(participant, { harness: 'claude-code', nativeSession })

      // /clear: a native session the ledger has not seen is a new conversation on it.
      const cleared = follow(id('zeus'), 'native-2')
      assert.deepEqual([cleared.nativeSession, cleared.endedAt], ['native-2', null])
      assert.equal(ledger.currentConversation(id('zeus')).id, cleared.id)
      // /resume of its own earlier one: that one goes on, the one in progress ends.
      const resumed = follow(id('zeus'), 'native-1')
      assert.deepEqual([resumed.id, resumed.endedAt], [first.id, null])
      assert.equal(ledger.currentConversation(id('zeus')).id, first.id)
      assert.deepEqual(follow(id('zeus'), 'native-1'), resumed, 'the one it is on: nothing moves')
      // A native session another participant's conversation holds stays with it.
      const other = follow(id('diana'), 'native-1')
      assert.deepEqual([other.participantId, other.nativeSession], [id('diana'), null])
      assert.equal(ledger.currentConversation(id('zeus')).id, first.id)
      assert.throws(
        () => ledger.followConversation(id('zeus'), { harness: 'kimi', nativeSession: 'x' }),
        { code: 'invalid-harness' },
      )
    })
  })
})

describe('switching the lead', () => {
  it('moves the chief to another harness and agent: its conversation ends, its quota clears, the switch is logged', async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const first = ledger.startConversation(id('chief'), { harness: 'claude-code' })
      ledger.bindConversation(first.id, 'claude-session')
      ledger.markOut(id('chief'), { until: '2026-10-02T00:00:00.000Z', reason: 'out' })
      const before = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      assert.equal(ledger.lastSwitch(project.id), null, 'no switch yet')

      ledger.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      const chief = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      assert.deepEqual(
        [chief.harness, chief.agent, chief.outUntil, chief.outSince],
        ['codex', 'astraeus', null, before.outSince],
        "the old harness's quota is not the new one's; when it was marked stays",
      )
      assert.equal(ledger.currentConversation(id('chief')), null, 'every switch starts fresh')
      const switched = ledger.events(project.id).filter((e) => e.kind === 'chief.switched')
      assert.deepEqual(switched.at(-1).data, {
        from: { harness: 'claude-code', agent: null },
        to: { harness: 'codex', agent: 'astraeus' },
        cut: false,
      })
      assert.deepEqual(ledger.lastSwitch(project.id), {
        from: { harness: 'claude-code', agent: null },
        cut: false,
      })

      ledger.switchChief(project.id, { harness: 'pi', agent: 'leto', cut: true })
      const last = { from: { harness: 'codex', agent: 'astraeus' }, cut: true }
      assert.deepEqual(ledger.lastSwitch(project.id), last, 'the old lead was cut mid-turn')
      const back = ledger.project(project.id).participants.find((p) => p.handle === 'chief')
      assert.deepEqual([back.harness, back.agent], ['pi', 'leto'])
      // `image` is no harness: an image agent is Codex's.
      for (const harness of ['kimi', 'image', 'nope']) {
        assert.throws(() => ledger.switchChief(project.id, { harness, agent: 'leto' }), {
          code: 'invalid-harness',
        })
      }
      // A lead is switched to a saved agent, never to a harness's own default.
      for (const agent of ['no such!', undefined, null]) {
        assert.throws(() => ledger.switchChief(project.id, { harness: 'pi', agent }), {
          code: 'invalid-agent',
        })
      }
      assert.deepEqual(ledger.lastSwitch(project.id), last, 'nothing refused was recorded')
    })
  })

  it('knows what the lead was switched from out of the chief, not out of the event log', async () => {
    await withDir(async (dir) => {
      const file = path.join(dir, 'consensflow.db')
      const before = openLedger(file, { now: clock(), names: names() })
      const { project } = staff(before)
      before.switchChief(project.id, { harness: 'codex', agent: 'astraeus', cut: true })
      before.close()
      dropTrace(file)
      const ledger = openLedger(file)
      try {
        assert.deepEqual(ledger.lastSwitch(project.id), {
          from: { harness: 'claude-code', agent: null },
          cut: true,
        })
      } finally {
        ledger.close()
      }
    })
  })

  it("keeps the lead's earlier conversations as its history: oldest first, each with its harness and items", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const claude = ledger.startConversation(id('chief'), { harness: 'claude-code' })
      ledger.copyTranscript(claude.id, [
        { id: 'c1', role: 'user', text: 'the codeword is tern' },
        { id: 'c2', role: 'assistant', text: 'noted' },
      ])
      ledger.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      const codex = ledger.startConversation(id('chief'), { harness: 'codex' })
      ledger.copyTranscript(codex.id, [{ id: 'x1', role: 'user', text: 'and the next step?' }])
      ledger.switchChief(project.id, { harness: 'pi', agent: 'leto' })
      ledger.startConversation(id('chief'), { harness: 'pi' })

      const history = ledger.leadHistory(project.id)
      assert.deepEqual(
        history.map((c) => [c.harness, c.items.map((i) => [i.role, i.text])]),
        [
          [
            'claude-code',
            [
              ['user', 'the codeword is tern'],
              ['assistant', 'noted'],
            ],
          ],
          ['codex', [['user', 'and the next step?']]],
        ],
        'the current conversation is not history',
      )
      assert.ok(history.every((c) => c.endedAt !== null))
    })
  })

  it('lists what waits on the lead: questions to it, results it has not decided, its own unfinished tasks', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const parser = ledger.createTask(project.id, { from: 'chief', to: 'zeus', body: 'Parser' })
      deliver(ledger, parser.message)
      const lexer = ledger.createTask(project.id, { from: 'chief', to: 'diana', body: 'Lexer' })
      deliver(ledger, lexer.message)
      const question = ledger.ask(project.id, {
        from: 'zeus',
        to: 'chief',
        task: 1,
        body: 'Which grammar?',
      })
      ledger.recordResult(project.id, 2, { body: 'Lexer done' })
      const own = ledger.createTask(project.id, { from: 'human', to: 'chief', body: 'Plan' })
      deliver(ledger, own.message)

      const open = ledger.leadOpenWork(project.id)
      assert.deepEqual(
        open.questions.map((m) => [m.id, m.sender, m.taskNumber]),
        [[question.id, 'zeus', 1]],
      )
      assert.deepEqual(
        open.results.map((t) => [t.number, t.assignee]),
        [[2, 'diana']],
      )
      assert.deepEqual(
        open.own.map((t) => [t.number, t.state]),
        [[3, 'working']],
      )

      deliver(ledger, ledger.answer(question.id, { from: question.recipientId, body: 'LL(1)' }))
      ledger.acceptTask(project.id, 2, { by: 'chief' })
      const after = ledger.leadOpenWork(project.id)
      assert.deepEqual([after.questions, after.results], [[], []])
    })
  })

  it('gives back the attempt of a delivery a switch cut off before it could land', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      const note = ledger.note(project.id, { from: 'zeus', to: 'chief', body: 'Ready' })
      ledger.beginDelivery(note.id)
      ledger.retryDelivery(note.id, 'the lead was switched', { refund: true })
      assert.deepEqual(
        [ledger.message(note.id).state, ledger.message(note.id).attempts],
        ['queued', 0],
      )
      ledger.beginDelivery(note.id)
      ledger.retryDelivery(note.id, 'the window closed')
      assert.equal(ledger.message(note.id).attempts, 1, 'an ordinary retry keeps the count')
    })
  })

  it('does not count a chief with a saved agent as staff', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      ledger.switchChief(project.id, { harness: 'codex', agent: 'astraeus' })
      assert.deepEqual(
        ledger.refreshMemberTiers(() => 'critical').map((change) => change.handle),
        ['zeus', 'diana'],
        "the chief's tier is not a member's",
      )
      ledger.setProjectState(project.id, 'suspended')
      assert.equal(ledger.deleteProject(project.id).members, 2)
    })
  })

  it('logs what the lead read of its history: which page, or what it searched for', async () => {
    await withLedger((ledger) => {
      const { project } = staff(ledger)
      ledger.historyRead(project.id, { page: 2 })
      ledger.historyRead(project.id, { page: 1, find: 'parser', tools: true })
      assert.deepEqual(
        ledger
          .events(project.id)
          .filter((event) => event.kind === 'lead.history.read')
          .map((event) => event.data),
        [
          { page: 2, find: null, tools: false },
          { page: 1, find: 'parser', tools: true },
        ],
      )
      assert.throws(() => ledger.historyRead(99, { page: 1 }), { code: 'unknown-project' })
    })
  })
})

describe('the transcript copy', () => {
  const item = (id, role, text, extra = {}) => ({ id, role, text, complete: true, ...extra })
  /** A worker's task assigned to a session with a conversation of its own. */
  function windowed(ledger) {
    const { project, id } = staff(ledger)
    ledger.createTask(project.id, {
      from: 'chief',
      pool: 'worker',
      tier: 'standard',
      body: 'Parser',
    })
    const { message } = ledger.assignTask(project.id, 1, id('zeus'))
    const conversation = ledger.startConversation(message.recipientId, { harness: 'claude-code' })
    return { project, id, session: message.recipientId, conversation }
  }

  it("a task's part of a window's copy starts at its brief: the chief's own, after its other work", async () => {
    await withLedger((ledger) => {
      const { project, id } = staff(ledger)
      const own = ledger.createTask(project.id, { from: 'chief', to: 'chief', body: 'Write it up' })
      const conversation = ledger.startConversation(id('chief'), { harness: 'claude-code' })
      ledger.copyTranscript(conversation.id, [
        item('u0', 'user', 'Earlier, from the human'),
        item('a0', 'assistant', 'Looking into it'),
        item(
          'u1',
          'user',
          `[ConsensFlow m-${own.message.id} · T-1 · task from @chief]\nWrite it up`,
        ),
        item('a1', 'assistant', 'Writing'),
      ])
      const { items, total } = ledger.transcript(project.id, 1)
      assert.equal(total, 2)
      assert.deepEqual(
        items.map((i) => i.id),
        ['u1', 'a1'],
      )
    })
  })

  it("keeps all of a task's part across a pause, a resume and a reopen: it starts at the first brief", async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      const marker = (message) => `[ConsensFlow m-${message.id} · T-1 · task from @chief]`
      const brief = ledger.task(project.id, 1).messages[0]
      deliver(ledger, brief)
      ledger.copyTranscript(conversation.id, [
        item('u1', 'user', `${marker(brief)}\nParser`),
        item('a1', 'assistant', 'Wrote src/parser.js'),
      ])
      ledger.pauseTask(project.id, 1, { by: 'chief' })
      const resumed = ledger.resumeTask(project.id, 1, { by: 'chief', body: 'Use v2' }).message
      deliver(ledger, resumed)
      ledger.copyTranscript(
        conversation.id,
        [
          item('u2', 'user', `${marker(resumed)}\nResumed: Use v2`),
          item('a2', 'assistant', 'On v2'),
        ],
        { from: 2 },
      )
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const again = ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Add tests' }).message
      deliver(ledger, again)
      ledger.copyTranscript(
        conversation.id,
        [item('u3', 'user', `${marker(again)}\nAdd tests`), item('a3', 'assistant', 'Tests added')],
        { from: 4 },
      )
      const { items, total } = ledger.transcript(project.id, 1)
      assert.equal(total, 6)
      assert.deepEqual(
        items.map((i) => i.id),
        ['u1', 'a1', 'u2', 'a2', 'u3', 'a3'],
      )
    })
  })

  it("ends a task's part where a later task in its window began, and takes it up again at its reopen", async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      const marker = (message) => `[ConsensFlow m-${message.id} · task from @chief]`
      const brief = ledger.task(project.id, 1).messages[0]
      deliver(ledger, brief)
      ledger.copyTranscript(conversation.id, [
        item('u1', 'user', `${marker(brief)}\nParser`),
        item('a1', 'assistant', 'Parser done'),
      ])
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      // A follow-up given to the same window: its part is its own.
      const follow = ledger.createTask(project.id, {
        from: 'chief',
        after: 1,
        body: 'Docs',
      }).message
      deliver(ledger, follow)
      ledger.copyTranscript(
        conversation.id,
        [item('u2', 'user', `${marker(follow)}\nDocs`), item('a2', 'assistant', 'Docs done')],
        { from: 2 },
      )
      ledger.recordResult(project.id, 2, { body: 'Docs done' })
      const again = ledger.reopenTask(project.id, 1, { by: 'chief', body: 'Add tests' }).message
      deliver(ledger, again)
      ledger.copyTranscript(
        conversation.id,
        [item('u3', 'user', `${marker(again)}\nAdd tests`), item('a3', 'assistant', 'Tests added')],
        { from: 4 },
      )
      assert.deepEqual(
        ledger.transcript(project.id, 1).items.map((i) => i.id),
        ['u1', 'a1', 'u3', 'a3'],
      )
      assert.deepEqual(
        ledger.transcript(project.id, 2).items.map((i) => i.id),
        ['u2', 'a2'],
      )
    })
  })

  it("shows a reassigned task's first window, then the one that took it", async () => {
    await withLedger((ledger) => {
      const { project, id, conversation } = windowed(ledger)
      const first = ledger.task(project.id, 1).messages[0]
      deliver(ledger, first)
      ledger.copyTranscript(conversation.id, [
        item('u1', 'user', `[ConsensFlow m-${first.id} · T-1 · task from @chief]\nParser`),
        item('a1', 'assistant', 'Half a parser'),
      ])
      ledger.releaseTask(project.id, 1, { because: 'by @human' })
      const second = ledger.assignTask(project.id, 1, id('diana')).message
      deliver(ledger, second)
      const other = ledger.startConversation(second.recipientId, { harness: 'codex' })
      ledger.copyTranscript(other.id, [
        item('u2', 'user', `[ConsensFlow m-${second.id} · T-1 · task from @chief]\nParser`),
        item('a2', 'assistant', 'Parser done'),
      ])
      assert.deepEqual(
        ledger.transcript(project.id, 1).items.map((i) => i.id),
        ['u1', 'a1', 'u2', 'a2'],
      )
    })
  })

  it('copies what is new, brings an item still being written up to date, and reads it by task', async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      assert.deepEqual(ledger.transcript(project.id, 1), { items: [], total: 0 })
      const first = [
        item('u1', 'user', '[ConsensFlow m-1 · T-1 · task from @chief]\nParser'),
        item('a1', 'assistant', 'On it', { complete: false, at: '2026-09-21T08:00:00.000Z' }),
      ]
      assert.equal(ledger.copyTranscript(conversation.id, first), 2)
      assert.equal(
        ledger.copyTranscript(conversation.id, first),
        0,
        'nothing changed: nothing written',
      )
      const then = [
        item('a1', 'assistant', 'On it. Parser done', { at: '2026-09-21T08:00:00.000Z' }),
        item('t1', 'tool', 'ok\n', { at: 7 }),
      ]
      assert.equal(ledger.copyTranscript(conversation.id, then, { from: 1 }), 2)
      const { items, total } = ledger.transcript(project.id, 1)
      assert.equal(total, 3)
      assert.deepEqual(
        items.map((i) => [i.id, i.role, i.text, i.complete, i.at]),
        [
          ['u1', 'user', '[ConsensFlow m-1 · T-1 · task from @chief]\nParser', true, null],
          ['a1', 'assistant', 'On it. Parser done', true, '2026-09-21T08:00:00.000Z'],
          ['t1', 'tool', 'ok\n', true, null],
        ],
      )
      const page = ledger.transcript(project.id, 1, { limit: 2 })
      assert.deepEqual(
        [page.total, page.items.map((i) => i.id)],
        [3, ['a1', 't1']],
        'the last ones',
      )
    })
  })

  it("cuts only a tool's output longer than it keeps, files an unknown role as custom, and skips what has no id", async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      const long = 'x'.repeat(TRANSCRIPT_ITEM_MAX + 5)
      ledger.copyTranscript(conversation.id, [
        item('big', 'tool', long),
        item('said', 'assistant', long),
        item('asked', 'user', long),
        item('odd', 'system', 'hm'),
        { role: 'user', text: 'no id' },
        item('none', 'assistant', undefined),
      ])
      const { items } = ledger.transcript(project.id, 1)
      assert.deepEqual(
        items.map((i) => [i.id, i.role, i.text.length]),
        [
          ['big', 'tool', TRANSCRIPT_ITEM_MAX + `\n… (${long.length} characters; cut here)`.length],
          // Words are kept whole: a lead switched in reads them.
          ['said', 'assistant', long.length],
          ['asked', 'user', long.length],
          ['odd', 'custom', 2],
          ['none', 'assistant', 0],
        ],
      )
      assert.match(items[0].text, /… \(64005 characters; cut here\)$/)
      assert.throws(() => ledger.copyTranscript(999, [item('a', 'user', 'x')]), {
        code: 'unknown-conversation',
      })
      assert.throws(() => ledger.copyTranscript(conversation.id, 'items'), {
        code: 'invalid-items',
      })
    })
  })

  it('reads the last of a long copy in a frame: the newest items that fit, one too long cut, and how many', async () => {
    await withLedger((ledger) => {
      const { project, conversation } = windowed(ledger)
      // More than the page's 1 MiB frame: twenty tool outputs at the most the
      // copy keeps of one, then the agent's words, longer than half of it alone.
      const output = 'PASS src/parser.test.js '.repeat(3_000)
      const words = 'The parser is done, and here is why. '.repeat(16_000)
      ledger.copyTranscript(conversation.id, [
        item('u1', 'user', '[ConsensFlow m-1 · T-1 · task from @chief]\nParser'),
        ...Array.from({ length: 20 }, (_, n) => item(`t${n + 1}`, 'tool', output)),
        item('a1', 'assistant', words),
      ])
      const whole = ledger.transcript(project.id, 1)
      assert.ok(Buffer.byteLength(JSON.stringify(whole)) > 1024 * 1024)

      const read = ledger.latestTranscript(project.id, 1)
      const bytes = Buffer.byteLength(JSON.stringify(read.items))
      assert.ok(bytes <= PAGE_BYTES, `the items take ${bytes} bytes`)
      assert.deepEqual([read.total, read.shown], [22, 8])
      assert.deepEqual(
        read.items.map((i) => i.id),
        ['t14', 't15', 't16', 't17', 't18', 't19', 't20', 'a1'],
      )
      assert.equal(
        read.items.at(-1).text,
        `${words.slice(0, TRANSCRIPT_ITEM_MAX)}\n… (${words.length} characters; cut here)`,
      )
      // A tool's output was cut once, when it was copied.
      assert.equal(read.items[0].text, whole.items[14].text)
      assert.match(read.items[0].text, /^PASS[^…]*… \(72000 characters; cut here\)$/)
      assert.equal(whole.items.at(-1).text, words, 'the copy keeps the words whole')
      assert.deepEqual(ledger.latestTranscript(project.id, 1, { limit: 2 }), {
        items: read.items.slice(-2),
        total: 22,
        shown: 2,
      })
      assert.deepEqual(ledger.latestTranscript(project.id, 1, { limit: 0 }), {
        items: [],
        total: 22,
        shown: 0,
      })
    })
  })

  it("follows a task's window across a continued conversation, and shows nothing for a task on the board", async () => {
    await withLedger((ledger) => {
      const { project, id, session, conversation } = windowed(ledger)
      ledger.copyTranscript(conversation.id, [item('a1', 'assistant', 'Parser done')])
      deliver(ledger, ledger.task(project.id, 1).messages[0])
      ledger.recordResult(project.id, 1, { body: 'Parser done' })
      const again = ledger.createTask(project.id, {
        from: 'chief',
        after: 1,
        body: 'Now the lexer',
      })
      assert.equal(again.task.assignee, ledger.task(project.id, 1).assignee)
      ledger.copyTranscript(conversation.id, [item('a2', 'assistant', 'Lexer done')], { from: 1 })
      assert.deepEqual(
        ledger.transcript(project.id, 2).items.map((i) => i.text),
        ['Parser done', 'Lexer done'],
        'the follow-up shows the same window',
      )
      ledger.createTask(project.id, {
        from: 'chief',
        pool: 'worker',
        tier: 'standard',
        body: 'Tests',
      })
      assert.deepEqual(ledger.transcript(project.id, 3), { items: [], total: 0 })
      assert.throws(() => ledger.transcript(project.id, 9), { code: 'unknown-task' })
      assert.equal(session > 0 && id('chief') > 0, true)
    })
  })

  it('finds the first item the current conversation was given with a text: the proof a delivery arrived', async () => {
    await withLedger((ledger) => {
      const { session, conversation } = windowed(ledger)
      const header = '[ConsensFlow m-7 ·'
      ledger.copyTranscript(conversation.id, [
        item('a1', 'assistant', `quoting ${header} note]`),
        item('u1', 'user', `${header} note] Use JSON`),
        item('u2', 'user', `${header} note] again`),
      ])
      assert.equal(ledger.copiedItemWith(session, header), 'u1', 'given to it, the first')
      assert.equal(ledger.copiedItemWith(session, '[ConsensFlow m-8 ·'), null)
      ledger.startConversation(session, { harness: 'claude-code' })
      assert.equal(
        ledger.copiedItemWith(session, header),
        null,
        'an ended conversation proves nothing',
      )
    })
  })
})
