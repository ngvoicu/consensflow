/**
 * A lead switched to another harness knows what the old one was told. The
 * owner tells the first lead a codeword (random per run, so no model knows it
 * from anywhere else) and a decision, then pastes notes long enough to push
 * both off the first page of the lead history; then switches the lead to the
 * staff's harness (`--switch-to`) and asks the new one. Only `cf history`
 * holds the answer: the handoff quotes the owner's last words, the notes.
 */
const bird = ['TERN', 'LARK', 'WREN', 'KITE', 'ROOK', 'SWIFT'][Math.floor(Math.random() * 6)]
export const CODEWORD = `${bird}-${1000 + Math.floor(Math.random() * 9000)}`
const NOTES = Array.from(
  { length: 70 },
  (_, n) =>
    `Note ${n + 1}: the style guide asks for sentence case in headings, short paragraphs and alt text on every image; check item ${n + 1} before the next review.`,
).join('\n')

export default {
  id: 'lead-switch',
  title: 'A lead switched to another harness knows what the old one was told',
  fixture: 'site',
  prompt: `Two things to remember for later; nothing to do yet. The codeword for this release is ${CODEWORD}. And we decided the release goes out on Friday, not Thursday. Confirm in one line.`,
  followUps: [
    `My notes from today's review, so you have them; nothing to do yet, confirm in one line.\n${NOTES}`,
    { switch: true },
    'What is the codeword for this release, and which day did we decide it goes out? Answer from what you know; change nothing.',
  ],
  answers: [],
  fallback: 'Nothing to do yet; just answer.',
  quietMs: 90_000,
  expectations: [
    {
      name: 'the lead was switched once, to another harness',
      holds: (m) => m.switches === 1 && m.leads.length === 2 && m.leads[0] !== m.leads[1],
    },
    {
      name: 'the new lead read the lead history with cf history',
      holds: (m) => m.historyReads.length > 0,
    },
    { name: 'the new lead names the codeword', holds: (m) => m.leadWordsNow.includes(CODEWORD) },
    {
      name: 'the new lead knows the release goes out on Friday',
      holds: (m) => /friday/i.test(m.leadWordsNow),
    },
    { name: 'nothing went on the board', holds: (m) => m.tasks.length === 0 },
  ],
}
