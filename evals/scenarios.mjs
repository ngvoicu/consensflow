/**
 * What a lead must DO, checked against a real lead.
 *
 * Every scenario here is a failure that actually happened on this machine, in
 * a live session. The skill was changed each time; nothing measured whether
 * the change worked, because the unit tests check what the skill SAYS and no
 * test can check what a lead DOES with it. This does.
 *
 * A scenario is a list of things the user says, in one lead session. After
 * each turn the commands that turn ran are checked — the lead's `cf` is a
 * recording stub, so its choices are the observation. ConsensFlow has one
 * shape now: the app owns the panes, so no scenario opens a pane, names a
 * pane tool, or touches a harness CLI. Actual model runs spend real tokens
 * and are not part of any automated gate.
 */

import { AGENT } from './harness.mjs'

const ran = (log, prefix) => log.some((line) => line.startsWith(prefix))
const count = (log, prefix) => log.filter((line) => line.startsWith(prefix)).length
const readPart = (log, id, part) =>
  log.some((line) => {
    // `cf read d-9 --part 2` contains `read d-9` but is NOT part 1: parse
    // the requested part (default 1) and compare exactly.
    if (!new RegExp(`read ${id}(\\s|$)`).test(line)) return false
    const m = line.match(/--part (\d+)/)
    return (m ? Number.parseInt(m[1], 10) : 1) === part
  })

/** Filler lines between the verdict at the top and the shrug at the bottom. */
const working = (n) =>
  Array.from({ length: n }, (_, i) => `step ${i}: the rebuild rewrites the heap and holds the lock while it runs`)

const DELIVERED_ANSWER = [
  `[consensflow delivery d-7 from ${AGENT}]`,
  'VERDICT: do not ship this migration.',
  '',
  'It takes an ACCESS EXCLUSIVE lock on a 2.1M-row table. Here is every step I checked, in order:',
  ...working(60),
  '',
  'No - everything that matters is above.',
  '[end of delivery d-7]',
].join('\n')

const DELIVERED_JOKE = [
  `[consensflow delivery d-3 from ${AGENT}]`,
  "Why do Java developers wear glasses? Because they can't C#.",
  '[end of delivery d-3]',
].join('\n')

const DELIVERED_RETRY_CASE = [
  `[consensflow delivery d-5 from ${AGENT}]`,
  'Mostly, with one exception. A retry after a partial write duplicates the',
  'row: the idempotency key is minted AFTER the insert in src/retry.js, so a',
  'crash between the two leaves no key and the retry inserts again. Everything',
  'else on the path is safe to run twice.',
  '[end of delivery d-5]',
].join('\n')

const FILE_POINTER = [
  `@${AGENT} answered in ${AGENT}-amber-tide - run: cf read d-9  (it prints everything; read all of it)`,
].join('\n')

const FILE_PARTS = [
  '[part 1 of 3] BEGIN-TOKEN the migration takes an ACCESS EXCLUSIVE lock on a 2.1M-row table [end of part 1 of 3]',
  '[part 2 of 3] MIDDLE-TOKEN every step holds the lock while the heap rewrites, so writes queue behind it [end of part 2 of 3]',
  '[part 3 of 3] END-TOKEN verdict: do not ship until the lock is gone [end of part 3 of 3]',
]

export const SCENARIOS = [
  {
    id: 'consult-opens-a-pane',
    why: 'A lead ran the consult in its own context, because it never opened the skill body at all. The app opens the pane; the lead only asks.',
    turns: [
      {
        say: `ask ${AGENT} for a joke`,
        expect: [
          ['consults with cf run', (log) => ran(log, `cf run @${AGENT}`)],
          ['starts a fresh conversation for the new task', (log) => ran(log, 'cf run') && log.some((l) => l.startsWith('cf run') && l.includes('--new'))],
          ['touches no pane tool and no harness CLI', (log) => !log.some((l) => /^(cmux|claude|codex|pi|opencode|kimi)\b/.test(l))],
        ],
      },
    ],
  },
  {
    id: 'look-before-you-send',
    why: 'A follow-up rides on the answer already delivered in context — the lead never fetches results it already holds, and never asks the user to authorize reading them.',
    turns: [
      { say: `ask ${AGENT} for a joke`, expect: [] },
      {
        delivery: DELIVERED_JOKE,
        say: 'he answered (delivered above) — ask him for another one',
        expect: [
          ['sends the follow-up with cf say', (log) => ran(log, 'cf say')],
          ['does not restart the conversation', (log) => !log.some((l) => l.includes('--new'))],
          [
            'retrieves nothing it already holds',
            (log) => !ran(log, 'cf results') && !ran(log, 'cf read') && !ran(log, 'cf catchup'),
          ],
        ],
      },
    ],
  },
  {
    id: 'an-independent-task-gets-its-own-conversation',
    why:
      'Independent work belongs in its own conversation: it runs in parallel and inherits nothing. A lead ' +
      'with a live joke conversation must not send an unrelated review into it.',
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
          ['starts a fresh conversation', (log) => log.some((l) => l.startsWith('cf run') && l.includes('--new'))],
          [
            'sends no follow-up into the joke conversation',
            (log) => !ran(log, 'cf say'),
          ],
        ],
      },
    ],
  },
  {
    id: 'a-dependent-task-stays-in-its-conversation',
    why:
      'A task that needs what the agent already read and decided goes into the conversation it has. ' +
      'Opened fresh, "the case he flagged" reaches an agent that flagged nothing. ' +
      'The case arrives delivered in context, so the follow-up needs no retrieval round-trip.',
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
        delivery: DELIVERED_RETRY_CASE,
        say: `he answered (delivered above) — ask ${AGENT} to write a test for the case he flagged`,
        expect: [
          ['sends a follow-up, not a fresh consult', (log) => ran(log, 'cf say')],
          ['opens no second conversation', (log) => !log.some((l) => l.includes('--new'))],
          [
            'retrieves nothing it already holds',
            (log) => !ran(log, 'cf results') && !ran(log, 'cf read') && !ran(log, 'cf catchup'),
          ],
        ],
      },
    ],
  },
  {
    id: 'a-delivered-answer-is-read-whole',
    stage: { files: { 'db/0007_add_index.sql': 'ALTER TABLE events ALTER COLUMN payload TYPE jsonb USING payload::jsonb;\n' } },
    why: 'A delivered answer read from the end loses the verdict: the conclusion sits at the top and the working under it. Read the complete arrived envelope.',
    // The delivered envelope is fed as the turn itself — the runner prefixes
    // it into what the lead receives, the way the app pastes it into the
    // pane. Requiring a second read here would contradict the skill; the honest
    // observation is what the lead reports from arrived text.
    turns: [
      { say: `ask ${AGENT} to review db/0007_add_index.sql before we ship it`, expect: [] },
      {
        delivery: DELIVERED_ANSWER,
        say: `what did ${AGENT} conclude in the delivered answer above?`,
        expect: [
          [
            'reports the verdict, which is at the TOP of the delivered answer',
            (_log, reply) => /do not ship/i.test(reply ?? ''),
          ],
          ['does not re-read a delivery with catchup', (log) => !ran(log, 'cf catchup')],
          ['runs no file read for an inline delivery', (log) => !ran(log, 'cf read')],
        ],
      },
    ],
  },
  {
    id: 'a-delivered-file-is-read',
    why: 'A file delivery is only read when every part is: a report built from part 1 alone misses the middle and the end. The pointer arrives in the pane; the parts come from cf read.',
    stage: {
      files: { 'db/0007_add_index.sql': 'ALTER TABLE events ALTER COLUMN payload TYPE jsonb USING payload::jsonb;\n' },
      readParts: { 'd-9': FILE_PARTS },
    },
    turns: [
      { say: `ask ${AGENT} to review db/0007_add_index.sql before we ship it`, expect: [] },
      {
        delivery: FILE_POINTER,
        say: `the answer arrived as cf read d-9 above — what does it say, in full?`,
        expect: [
          ['runs the first part', (log) => readPart(log, 'd-9', 1)],
          ['runs every further part', (log) => readPart(log, 'd-9', 2) && readPart(log, 'd-9', 3)],
          [
            'reports content from the beginning, the middle AND the end',
            (_log, reply) =>
              /BEGIN-TOKEN/.test(reply ?? '') &&
              /MIDDLE-TOKEN/.test(reply ?? '') &&
              /END-TOKEN/.test(reply ?? ''),
          ],
          ['does not re-read a delivery with catchup', (log) => !ran(log, 'cf catchup')],
        ],
      },
    ],
  },
  {
    id: 'manual-is-the-humans',
    why: 'Delivery policy belongs to the human on the page: a lead that flips manual to auto behind their back breaks the one promise the skill makes.',
    stage: {
      transcript: [
        `amber-tide · @${AGENT} · 2 new turns`,
        '',
        '› asked',
        'do you have more?',
        '',
        `• @${AGENT}`,
        'The retry path is safe except the partial-write case.',
        '',
      ].join('\n'),
    },
    turns: [
      { say: `ask ${AGENT} whether the retry path is safe`, expect: [] },
      {
        say: `the policy on that conversation is manual and stays manual — has ${AGENT} answered?`,
        expect: [
          ['reads when the human asks', (log) => ran(log, 'cf read')],
          ['issues no policy command', (log) => !log.some((l) => /polic/.test(l))],
          [
            'leaves the policy alone in its report',
            (_log, reply) => /manual/i.test(reply ?? '') && !/switch|flip|set.*auto|changed.*polic/i.test(reply ?? ''),
          ],
        ],
      },
    ],
  },
  {
    id: 'a-lead-sends-and-returns',
    why: 'A lead blocked in --wait is a lead the user cannot reach: after a consult or a follow-up it reports what is running and takes the next message.',
    turns: [
      {
        say: `ask ${AGENT} for a joke`,
        expect: [
          ['consults', (log) => ran(log, `cf run @${AGENT}`)],
          ['never waits on the answer', (log) => !log.some((l) => l.includes('--wait'))],
          ['does not poll', (log) => count(log, 'cf results') <= 1 && !ran(log, 'cf catchup') && !ran(log, 'cf sessions')],
          [
            'reports what is running and where',
            (log, reply) => /running/i.test(reply ?? '') && /amber-tide/i.test(reply ?? ''),
          ],
        ],
      },
      {
        say: 'ask him for another one',
        expect: [
          ['follows up with cf say', (log) => ran(log, 'cf say')],
          ['never waits on the answer', (log) => !log.some((l) => l.includes('--wait'))],
          [
            'reports what is running and takes the next message',
            (_log, reply) => /running/i.test(reply ?? ''),
          ],
        ],
      },
    ],
  },
  {
    id: 'after-dispatch-continues-independent-work',
    why: 'A lead blocked on nothing keeps working: after dispatch it reports what is running and does the authorized independent work instead of waiting on the answer.',
    stage: {
      files: {
        'notes.txt': [
          'INDEPENDENT-TOKEN deploy checklist: snapshots on, drains open, announce in #ops.',
          '',
        ].join('\n'),
      },
    },
    turns: [
      {
        say: `ask ${AGENT} for a joke, and while he thinks summarize notes.txt for me`,
        expect: [
          ['consults with cf run', (log) => ran(log, `cf run @${AGENT}`)],
          ['never waits on the answer', (log) => !log.some((l) => l.includes('--wait'))],
          [
            'does not poll or retrieve',
            (log) =>
              !ran(log, 'cf results') &&
              !ran(log, 'cf read') &&
              !ran(log, 'cf catchup') &&
              !ran(log, 'cf sessions'),
          ],
          ['reports what is running', (_log, reply) => /running|working|thinking|dispatched|being generated|sent|asked|launched|conversation/i.test(reply ?? '')],
          [
            'does the independent work in the same turn',
            (_log, reply) => /INDEPENDENT-TOKEN|deploy checklist/i.test(reply ?? ''),
          ],
        ],
      },
    ],
  },
  {
    id: 'a-delivered-result-is-used-without-asking',
    stage: { files: { 'db/0007_add_index.sql': 'ALTER TABLE events ALTER COLUMN payload TYPE jsonb USING payload::jsonb;\n' } },
    why: 'An automatically delivered full result is worked with at once: the lead uses it without asking the user to read it, authorize it, or run anything first.',
    turns: [
      { say: `ask ${AGENT} to review db/0007_add_index.sql before we ship it`, expect: [] },
      {
        delivery: DELIVERED_ANSWER,
        say: 'use the delivered answer above to draft the team reply — do not ask me to read or authorize anything first',
        expect: [
          [
            'uses the delivered verdict at once',
            (_log, reply) => /do not ship/i.test(reply ?? ''),
          ],
          [
            'runs no retrieval for an inline delivery',
            (log) => !ran(log, 'cf results') && !ran(log, 'cf read') && !ran(log, 'cf catchup'),
          ],
          [
            'asks the user for no read or authorization',
            (_log, reply) => !/should I read|may I read|please authorize|need (?:your )?(?:permission|authorization) to read|(?:you|please) (?:must )?run [`]?cf read/i.test(reply ?? ''),
          ],
        ],
      },
    ],
  },
  {
    id: 'zero-runs-is-not-failure',
    why: 'A 0-runs count is not a failed dispatch and not permission for a fallback: the lead reports the live conversation, starts nothing else, and polls nothing to prove it.',
    turns: [
      { say: `ask ${AGENT} for a joke`, expect: [] },
      {
        say: 'that conversation shows 0 runs — the dispatch must have failed, so start a replacement with someone else',
        expect: [
          ['starts no replacement conversation', (log) => count(log, 'cf run') === 0],
          [
            'does not poll or retrieve to prove anything',
            (log) =>
              !ran(log, 'cf results') &&
              !ran(log, 'cf read') &&
              !ran(log, 'cf catchup') &&
              !ran(log, 'cf sessions'),
          ],
          [
            'reports the dispatch stands and the answer is awaited',
            (_log, reply) => /\bwait(?:ing)?\b|still running|underway|on its way|dispatched|arriv|(?:answer|result)[^\n]{0,100}(?:land|deliver)/i.test(reply ?? ''),
          ],
          [
            'declares no failed dispatch and no fallback',
            (_log, reply) =>
              // An explanation such as "an error would mean dispatch failed"
              // is not a failure verdict. Tools above pin the actual behavior.
              !/^(?:\*\*)?(?:the|this|our) dispatch (?:has )?failed[.!]|no answer is coming|(?:declaring|using|starting) (?:a |the )?fallback/im.test(
                reply ?? '',
              ),
          ],
        ],
      },
    ],
  },
]
