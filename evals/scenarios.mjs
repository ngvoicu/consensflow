/**
 * What a lead must DO, checked against a real lead.
 *
 * Every scenario here is a failure that actually happened on this machine on
 * 2026-08-24, in a live session. The skill was changed each time; nothing
 * measured whether the change worked, because the unit tests check what the
 * skill SAYS and no test can check what a lead DOES with it. This does.
 *
 * A scenario is a list of things the user says, in one lead session. After
 * each turn the commands that turn ran are checked — the lead's `cf` and
 * `cmux` are recording stubs, so its choices are the observation.
 */

const ran = (log, prefix) => log.some((line) => line.startsWith(prefix))
const sent = (log, text) =>
  log.some((line) => line.startsWith('cmux send') && line.includes(text))
/**
 * The consult as it was actually sent, from `cf run` onwards. Shape is the
 * assertion — a `|` anywhere in a sent line could be the task talking, but a
 * `|` after `cf run` is a pipe around the consult.
 */
const consultLine = (log) => {
  const line = log.find((l) => l.startsWith('cmux send') && l.includes('cf run '))
  return line === undefined ? null : line.slice(line.indexOf('cf run '))
}

export const SCENARIOS = [
  {
    id: 'consult-opens-a-pane',
    why: 'A lead ran `cf run` in its own pane, because it never opened the skill body at all.',
    turns: [
      {
        say: 'ask nyx for a joke',
        expect: [
          // With the agent, not bare: a tab titled `quartz-valley` does not say
          // whose window the pane holds, which is the reason names carry one.
          ['names the conversation first, with the agent', (log) => ran(log, 'cf mint @nyx')],
          ['opens a pane', (log) => ran(log, 'cmux new-pane')],
          ['sends the consult into that pane', (log) => sent(log, 'cf run @nyx')],
          ['does NOT consult in its own pane', (log) => !ran(log, 'cf run')],
        ],
      },
    ],
  },
  {
    id: 'reading-is-not-writing',
    why: 'Asked "can you see what other jokes he said?", a lead SENT another request and invented a new answer instead of reading the existing one.',
    turns: [
      { say: 'ask nyx for a joke', expect: [] },
      {
        say: 'can you see what other jokes he said?',
        expect: [
          ['reads the conversation', (log) => ran(log, 'cf catchup')],
          ['asks for what is new', (log) => log.some((l) => l.startsWith('cf catchup') && l.includes('--unread'))],
          ['sends NOTHING into the pane', (log) => !ran(log, 'cmux send')],
          ['starts no new consult', (log) => !ran(log, 'cf run')],
        ],
      },
    ],
  },
  {
    id: 'look-before-you-send',
    why: 'A follow-up composed against a stale view asks the wrong question — the user may have moved the conversation in its pane.',
    turns: [
      { say: 'ask nyx for a joke', expect: [] },
      {
        say: 'ask him for another one',
        expect: [
          ['looks first', (log) => ran(log, 'cf catchup')],
          ['sends the follow-up into the pane', (log) => ran(log, 'cmux send')],
          ['does not restart the conversation', (log) => !log.some((l) => l.includes('--new'))],
          // Live 2026-08-31: the send landed, and it carried `cd … && cf run
          // @nyx … --session <name>` — a shell line pasted into nyx's own
          // window, where it reads as nyx being told to consult nyx. It counts
          // as a send by every check above, which is why the shape is checked.
          [
            'sends words, not a shell line',
            (log) =>
              log
                .filter((l) => l.startsWith('cmux send'))
                .every((l) => !l.includes('cf run') && !l.includes('cd ')),
          ],
        ],
      },
    ],
  },
  {
    id: 'a-long-answer-is-read-whole',
    why: 'Live 2026-08-27: facing a review tens of thousands of characters long, a lead ran `cf catchup … | tail -60`, reported the tail, and had to be told it had read only the end — the verdict was in the first line.',
    // The stage answers `catchup` with a real review's shape: the verdict at the
    // top, four hundred lines of working under it, and a shrug at the bottom.
    // Only a lead that read from the top can say what was concluded.
    //
    // HONEST LIMIT, measured 2026-08-27: with the rule removed from the skill,
    // a real lead passed this anyway. The live failure came from a lead deep in
    // a long session, defending a context it had already half spent; a two-turn
    // scenario cannot manufacture that pressure, and 44k characters is not
    // enough of it. So this is a regression guard — it catches a lead that
    // stops reading — not evidence that the prose is what stops it. Do not
    // quote it as proof the rule works.
    stage: { longAnswer: true },
    turns: [
      { say: 'ask nyx to review db/0007_add_index.sql before we ship it', expect: [] },
      {
        say: 'what did nyx conclude?',
        expect: [
          ['reads the conversation', (log) => ran(log, 'cf catchup')],
          [
            'reports the verdict, which is at the TOP of a long answer',
            (_log, reply) => /do not ship/i.test(reply ?? ''),
          ],
        ],
      },
    ],
  },
  {
    id: 'answers-from-the-conversation',
    why: 'Asked "did you see her last answer?", a lead answered from its own memory while the user\'s pane turns sat unread.',
    turns: [
      { say: 'ask nyx for a joke', expect: [] },
      { say: 'thanks', expect: [] },
      {
        say: 'did nyx say anything else after that?',
        expect: [
          ['looks instead of remembering', (log) => ran(log, 'cf catchup')],
          ['sends nothing', (log) => !ran(log, 'cmux send')],
        ],
      },
    ],
  },
  {
    id: 'the-consult-line-is-plain',
    why:
      'Live 2026-09-02: a lead followed the pane recipe and added two things to the line — ' +
      'a `--prompt-file` beside a quoted task, which threw the quoted one away unread, and ' +
      '`2>&1 | tee`, which left the consult with no window. It could not read the result, so ' +
      'six minutes later it opened a SECOND conversation with the same agent on the same work. ' +
      'The 2026-08-31 lesson again: the old scenarios asserted a send HAPPENED, never its shape.',
    // The prompt names a file, so the file is really there: without it the lead
    // checks, finds nothing, and asks the user instead of consulting — which
    // scores as "never opened a pane" and blames the skill for a missing prop.
    stage: {
      files: {
        'docs/tranche2-handoff.md': [
          '# Tranche 2 — handoff',
          '',
          'Three loss classes, in this order: single-gap latch (T-431), genesis',
          'stall (T-432), multi-event transitions (T-433). Run the suite bare',
          'after each class and report the numbers before starting the next.',
          'Do not change git state, and never write outside the replay ledgers.',
        ].join('\n'),
      },
    },
    turns: [
      {
        say:
          'ask nyx to implement tranche 2 — the whole handoff with the ticket numbers, design ' +
          'and constraints is in docs/tranche2-handoff.md, he should read it in full first',
        expect: [
          ['opens a pane for it', (log) => ran(log, 'cmux new-pane')],
          ['sends the consult there', (log) => sent(log, 'cf run @nyx')],
          // Shape, checked on the `cf run` tail of the sent line rather than on
          // the whole line: a task string may well contain a `>` or the word
          // tee, and the failure was never the prose. Each of these REQUIRES
          // the consult line to exist — a check that passes because nothing was
          // sent is the 2026-08-31 mistake wearing the other face.
          ['no pipe or redirect on the consult', (log) => consultLine(log) !== null && !/[|>]/.test(consultLine(log))],
          [
            'one task source, not both a file and a quoted task',
            (log) => {
              const line = consultLine(log)
              if (line === null) return false
              return !(line.includes('--prompt-file') && /cf run @nyx\s+["']/.test(line))
            },
          ],
          [
            'confirms the launch landed instead of capturing output',
            (log) => ran(log, 'cf sessions'),
          ],
        ],
      },
    ],
  },
]
