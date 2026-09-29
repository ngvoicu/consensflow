/**
 * Every question to the owner stays in the chief's terminal. The owner asks
 * for a page and tells the chief to ask two things first, not where: the file's
 * name, which only the owner knows, and which language comes first, a choice
 * between two (a harness's own question dialog, where it has one). The chief
 * must ask in its terminal, in words or in that dialog; nothing reaches the
 * board, it never tries `cf ask`, and the owner's answer reaches the work: the
 * page is made under the name the owner gave.
 */
import { askedInTerminal } from '../measure.mjs'

export default {
  id: 'terminal-questions',
  title: 'The chief asks the owner in its terminal, and the answer reaches the work',
  fixture: 'site',
  prompt: [
    'Vreau pe site o pagină nouă, pentru cariere. Înainte să pornești ceva, întreabă-mă două',
    'lucruri, fără să ghicești: cum se numește fișierul paginii și în ce limbă o facem întâi,',
    'română sau engleză. Apoi pune munca pe board.',
  ].join(' '),
  answers: [
    { match: /nume|fișier|fisier|file|name/i, text: 'Fișierul e site/cariere.html.' },
    { match: /limb|român|roman|englez|english|language/i, text: 'Întâi în română.' },
  ],
  fallback: 'Da, continuă.',
  quietMs: 120_000,
  expectations: [
    { name: 'the owner is asked in the terminal', holds: (m) => askedInTerminal(m) >= 1 },
    { name: 'nothing is asked on the board', holds: (m) => m.questionsOnBoard === 0 },
    { name: 'the chief never tries cf ask', holds: (m) => m.askRefused === 0 },
    { name: 'the work goes on the board', holds: (m) => m.tasks.length >= 1 },
    {
      name: "the owner's answer reaches the work: site/cariere.html exists",
      holds: (m) => m.filesChanged.includes('site/cariere.html'),
    },
  ],
}
