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

import { AGENT } from './harness.mjs'

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
        say: `ask ${AGENT} for a joke`,
        expect: [
          // With the agent, not bare: a tab titled `quartz-valley` does not say
          // whose window the pane holds, which is the reason names carry one.
          ['names the conversation first, with the agent', (log) => ran(log, `cf mint @${AGENT}`)],
          ['opens a pane', (log) => ran(log, 'cmux new-pane')],
          ['sends the consult into that pane', (log) => sent(log, `cf run @${AGENT}`)],
          ['does NOT consult in its own pane', (log) => !ran(log, 'cf run')],
        ],
      },
    ],
  },
  {
    id: 'reading-is-not-writing',
    why: 'Asked "can you see what other jokes he said?", a lead SENT another request and invented a new answer instead of reading the existing one.',
    turns: [
      { say: `ask ${AGENT} for a joke`, expect: [] },
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
      { say: `ask ${AGENT} for a joke`, expect: [] },
      {
        say: 'ask him for another one',
        expect: [
          ['looks first', (log) => ran(log, 'cf catchup')],
          ['sends the follow-up into the pane', (log) => ran(log, 'cmux send')],
          ['does not restart the conversation', (log) => !log.some((l) => l.includes('--new'))],
          // Live 2026-08-31: the send landed, and it carried `cd … && cf run
          // @${AGENT} … --session <name>` — a shell line pasted into nyx's own
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
      { say: `ask ${AGENT} to review db/0007_add_index.sql before we ship it`, expect: [] },
      {
        say: `what did ${AGENT} conclude?`,
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
      { say: `ask ${AGENT} for a joke`, expect: [] },
      { say: 'thanks', expect: [] },
      {
        say: `did ${AGENT} say anything else after that?`,
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
          `ask ${AGENT} to implement tranche 2 — the whole handoff with the ticket numbers, design ` +
          'and constraints is in docs/tranche2-handoff.md, he should read it in full first',
        expect: [
          ['opens a pane for it', (log) => ran(log, 'cmux new-pane')],
          ['sends the consult there', (log) => sent(log, `cf run @${AGENT}`)],
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
              return !(line.includes('--prompt-file') && new RegExp(`cf run @${AGENT}\\s+["']`).test(line))
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
  {
    id: 'one-window-per-conversation',
    why:
      'Live 2026-09-03: asked a follow-up while the agent window was still open, a lead opened a '
      + 'SECOND pane and sent `cf run --session` into it — two harness windows on ONE session, '
      + 'two processes writing one store. The skill invited it: its escape hatch for a fresh pane '
      + 'is "the window is gone", which nothing could check until `cmux tree` was named for it.',
    turns: [
      { say: `ask ${AGENT} whether the retry path is safe`, expect: [] },
      {
        say: 'ask him what happens on a timeout too',
        expect: [
          ['looks before it sends', (log) => ran(log, 'cf catchup')],
          ['sends words at the window it already has', (log) => ran(log, 'cmux send')],
          // The three shapes of the failure, each requiring the turn to have
          // actually done something — a check that passes on an empty log is
          // no check at all.
          ['opens no second pane', (log) => !ran(log, 'cmux new-pane')],
          [
            'does not re-spawn the consult',
            (log) => !log.some((line) => line.includes(`cf run @${AGENT}`)),
          ],
          [
            'sends the question, not a shell line',
            (log) => {
              const sends = log.filter((line) => line.startsWith('cmux send'))
              return sends.length > 0 && !sends.some((line) => /cf run|&&|--session/.test(line))
            },
          ],
        ],
      },
    ],
  },
  {
    id: 'an-independent-task-gets-its-own-pane',
    // Not a live failure — the first scenario written before one. Every check
    // above rewards sending into the pane the lead already has, and the skill's
    // only word on starting fresh was one sentence after all the mechanics, so
    // nothing measured the other direction. Added 2026-09-05 as its
    // counterweight, with `a-dependent-task-stays-in-its-pane` below.
    why:
      'Independent work belongs in its own pane: it runs in parallel and inherits nothing. A lead '
      + 'with a live joke conversation must not send an unrelated review into it.',
    stage: {
      files: {
        'scripts/backup.sh': [
          '#!/bin/sh',
          'set -e',
          'tar czf /backups/site-$(date +%F).tgz /var/www/site',
          'rm -rf /var/www/site/tmp/*',
          '',
        ].join('\n'),
      },
    },
    turns: [
      { say: `ask ${AGENT} for a joke`, expect: [] },
      {
        say: `ask ${AGENT} to review scripts/backup.sh — can it lose data if it is interrupted halfway?`,
        expect: [
          ['mints a fresh name', (log) => ran(log, `cf mint @${AGENT}`)],
          ['opens a new pane', (log) => ran(log, 'cmux new-pane')],
          ['sends a fresh consult there', (log) => consultLine(log) !== null && consultLine(log).includes('--new')],
          [
            'sends no bare words into the joke window',
            (log) => log.filter((l) => l.startsWith('cmux send')).every((l) => l.includes('cf run ')),
          ],
        ],
      },
    ],
  },
  {
    id: 'a-dependent-task-stays-in-its-pane',
    // The guard against the rule above over-correcting: a task phrased as new
    // work that still leans on what the agent found. No "too", no "another
    // one" — the dependence is in "the case he flagged", and only the
    // conversation knows which case that is. Added 2026-09-05.
    why:
      'A task that needs what the agent already read and decided goes into the window it has. '
      + 'Opened in a new pane, "the case he flagged" reaches an agent that flagged nothing.',
    // The conversation has to hold the case, or a lead that looks first finds
    // jokes and rightly sends nothing.
    stage: {
      transcript: [
        `amber-tide · @${AGENT} · 2 turns`,
        '',
        '› asked',
        'whether the retry path is safe',
        '',
        `• @${AGENT}`,
        'Mostly, with one exception. A retry after a partial write duplicates the',
        'row: the idempotency key is minted AFTER the insert in src/retry.js, so a',
        'crash between the two leaves no key and the retry inserts again. Everything',
        'else on the path is safe to run twice.',
        '',
      ].join('\n'),
    },
    turns: [
      { say: `ask ${AGENT} whether the retry path is safe`, expect: [] },
      {
        say: `ask ${AGENT} to write a test for the case he flagged`,
        expect: [
          ['looks before it sends', (log) => ran(log, 'cf catchup')],
          ['sends words at the window it already has', (log) => ran(log, 'cmux send')],
          ['opens no second pane', (log) => !ran(log, 'cmux new-pane')],
          ['does not re-spawn the consult', (log) => !log.some((line) => line.includes(`cf run @${AGENT}`))],
          [
            'sends the question, not a shell line',
            (log) => {
              const sends = log.filter((line) => line.startsWith('cmux send'))
              return sends.length > 0 && !sends.some((line) => /cf run|&&|--session/.test(line))
            },
          ],
        ],
      },
    ],
  },
]
