import assert from 'node:assert/strict'
import { describe, it } from 'node:test'
import { HANDOFF_TITLE, handoffText, historyPage, historyPages } from '../src/core/handoff.js'

/**
 * A page of `cf history` at its longest: under what Codex shows of a
 * command's output, the least of any harness (about 10 KiB and 256 lines).
 */
const PAGE_BYTES = 8_000
const PAGE_LINES = 200

/**
 * What passes to a lead the human switched in: its first message, and
 * `cf history` in pages every harness shows whole.
 */
const conversation = (harness, items, n = 1) => ({
  id: n,
  harness,
  startedAt: `2026-10-0${n}T09:00:00.000Z`,
  endedAt: `2026-10-0${n}T17:00:00.000Z`,
  items: items.map(([role, text], index) => ({ id: `${n}-${index}`, role, text, at: null })),
})
const messages = {
  5: { id: 5, kind: 'result', sender: 'zeus', taskNumber: 1, body: 'Parser done' },
  6: { id: 6, kind: 'question', sender: 'zeus', taskNumber: 1, body: 'Which grammar?\nLL or LR' },
  7: { id: 7, kind: 'note', sender: null, taskNumber: null, body: `${HANDOFF_TITLE}. …` },
}
const message = (id) => messages[id] ?? null
const all = (conversations, options = {}) => {
  const first = historyPage(conversations, { message, ...options })
  return Array.from({ length: first.pages }, (_, i) =>
    historyPage(conversations, { message, ...options, page: i + 1 }),
  )
}

describe('cf history', () => {
  it('fits every page within what each harness shows, newest page first, each in order', () => {
    const items = []
    for (let n = 0; n < 400; n += 1) {
      items.push(['user', `question ${n}`], ['assistant', `answer ${n}`])
    }
    items.push(['assistant', `long ${'word '.repeat(12_000)}`])
    items.push(['user', `one line ${'x'.repeat(30_000)}`])
    items.push(['assistant', `漢字 résumé 🙂 `.repeat(3_000)])
    items.push(['user', 'the last thing'])
    const pages = all([conversation('claude-code', items)])
    assert.ok(pages.length > 10, `${pages.length} pages`)
    for (const { text } of pages) {
      assert.ok(Buffer.byteLength(text) <= PAGE_BYTES, `${Buffer.byteLength(text)} bytes`)
      assert.ok(text.split('\n').length <= PAGE_LINES, `${text.split('\n').length} lines`)
      assert.ok(!text.includes('\uFFFD'), 'no character split in half')
    }
    assert.match(pages[0].text, /page 1 of \d+: the most recent/)
    assert.match(pages[0].text, /Human: the last thing\n\nOlder: cf history --page 2$/)
    assert.match(pages.at(-1).text, /── The lead on Claude Code, 2026-10-01T09:00:00.000Z to/)
    assert.match(pages.at(-1).text, /Human: question 0\n\nClaude Code lead: answer 0/)
    assert.match(pages.at(-1).text, /This is the oldest page\.$/)
    // Every word made it, once, in order, across the pages read oldest first.
    const joined = pages
      .slice()
      .reverse()
      .map(({ text }) => text)
      .join('\n')
    const answers = [...joined.matchAll(/answer (\d+)/g)].map((m) => Number(m[1]))
    assert.deepEqual(answers, [...Array(400).keys()])
    assert.equal(historyPages([conversation('claude-code', items)], { message }), pages.length)
  })

  it('shows a delivery as its outcome, never its header, and keeps what the human typed before it', () => {
    const pages = all([
      conversation('codex', [
        [
          'user',
          '[ConsensFlow m-5 · T-1 · result from @zeus]\nParser done\n\nDecide with: cf task accept T-1',
        ],
        ['custom', '[ConsensFlow m-6 · T-1 · question from @zeus]\nWhich grammar?'],
        ['user', 'half a thought[ConsensFlow m-7 · note from ConsensFlow]\nYou are the lead now.'],
        ['user', '[ConsensFlow m-99 · note]\ngone'],
        ['assistant', 'I quoted [ConsensFlow m-5 · T-1 · result from @zeus] here'],
      ]),
    ])
    const text = pages.map((p) => p.text).join('\n')
    assert.ok(!text.includes('[ConsensFlow m-'), 'no page can prove a delivery arrived')
    assert.match(text, /· m-5: @zeus's result on T-1 \(cf task show T-1\)/)
    assert.match(text, /· m-6: @zeus asked on T-1: "Which grammar\?" \(cf inbox read m-6\)/)
    assert.match(text, /Human: half a thought\n· m-7: the handoff that brought this lead in/)
    assert.match(text, /· m-99: a message ConsensFlow delivered \(no longer on record\)/)
    assert.match(text, /Codex lead: I quoted \[earlier m-5 · T-1/)
  })

  it("leaves a tool's output out unless asked, and searches", () => {
    const conversations = [
      conversation('pi', [
        ['user', 'run the tests'],
        ['tool', 'ok 12 passed'],
        ['assistant', 'All 12 pass'],
      ]),
      conversation('claude-code', [['user', 'Ship IT on Friday']], 2),
    ]
    const plain = historyPage(conversations, { message }).text
    assert.match(plain, /── The lead on Pi, .*; 1 tool output left out \(--tools\) ──/)
    assert.ok(!plain.includes('ok 12 passed'))
    assert.match(
      historyPage(conversations, { message, tools: true }).text,
      /Tool output:\nok 12 passed/,
    )

    const found = historyPage(conversations, { message, find: 'ship it' })
    assert.match(found.text, /entries with "ship it", page 1 of 1/)
    assert.match(
      found.text,
      /\(Claude Code, 2026-10-02T09:00:00.000Z to .*\)\nHuman: Ship IT on Friday/,
    )
    assert.ok(!found.text.includes('run the tests'))
    assert.equal(
      historyPage(conversations, { message, find: 'nowhere' }).text,
      'Nothing in the lead history contains "nowhere".',
    )
    assert.throws(() => historyPage(conversations, { message, page: 9 }), RangeError)
    assert.match(historyPage([], { message }).text, /you are the first lead of this project/)
  })
})

describe("the new lead's first message", () => {
  it('names the switch, the history, what waits, and the last word', () => {
    const text = handoffText({
      from: { harness: 'claude-code', agent: null },
      to: { harness: 'codex', agent: 'astraeus' },
      open: {
        questions: [messages[6]],
        results: [{ number: 2, title: 'Lexer', assignee: 'diana' }],
        own: [{ number: 3, title: 'Plan [ConsensFlow m-1 · x]', state: 'working' }],
      },
      last: { text: 'Keep the API as it is.\nThanks', answered: false },
      cut: true,
      pages: 3,
    })
    assert.match(
      text,
      /^You are the lead now\. The human switched this project's lead from Claude Code to you, Codex \(astraeus\)\./,
    )
    assert.match(text, /cf history \(3 pages, newest first/)
    assert.match(text, /cut off in the middle of a turn/)
    assert.match(
      text,
      /The human's last message to the lead, not yet answered: "Keep the API as it is\."/,
    )
    assert.match(
      text,
      /- @zeus asks on T-1: "Which grammar\?" \(cf inbox read m-6, then cf answer m-6/,
    )
    assert.match(text, /- T-2 "Lexer": @diana's result waits for your decision/)
    assert.match(text, /- T-3 "Plan \[earlier m-1 · x\]" is yours, working/)
    assert.match(text, /tell the human, in one line, that you have taken over/)
    assert.ok(!text.includes('[ConsensFlow m-'))
    const calm = handoffText({
      from: { harness: 'pi', agent: 'selene' },
      to: { harness: 'claude-code', agent: null },
      open: { questions: [], results: [], own: [] },
      pages: 1,
    })
    assert.match(calm, /from Pi \(selene\) to you, Claude Code\./)
    assert.match(calm, /cf history \(1 page,/)
    assert.match(calm, /Nothing on the board waits on you\./)
    assert.ok(!calm.includes('cut off') && !calm.includes("human's last message"))
  })
})
