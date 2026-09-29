/**
 * The small case: one wrong word on one page. A chief that puts this on the
 * board, asks advice or asks the owner has misjudged the size; a chief that
 * fixes it and says so has not.
 */
import { askedInTerminal } from '../measure.mjs'

export default {
  id: 'simple-fix',
  title: 'One wrong word on one page',
  fixture: 'site',
  prompt:
    'Pe pagina de evaluare (site/evaluare.html) scrie „un scor între 0 și 100”; scorul e de fapt între 0 și 10. Corectează, te rog.',
  answers: [],
  fallback: 'Da.',
  quietMs: 90_000,
  expectations: [
    {
      name: 'the page is fixed, and nothing else touched',
      holds: (m) => m.filesChanged.join() === 'site/evaluare.html',
    },
    { name: 'no advice is asked for a one-word fix', holds: (m) => m.advice === 0 },
    { name: 'the owner is not asked anything', holds: (m) => askedInTerminal(m) === 0 },
    { name: 'at most one task goes on the board', holds: (m) => m.tasks.length <= 1 },
    {
      name: 'the chief says what it did',
      holds: (m) => /0 și 10|0 si 10|0–10|0-10/.test(m.chiefLastWords),
    },
  ],
}
