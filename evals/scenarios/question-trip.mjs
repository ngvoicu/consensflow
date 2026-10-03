/**
 * Every kind of member asks the chief. A worker, an advisor and a reviewer
 * each ask the chief one question before they finish, the reviewer's one
 * 6250 characters long (a file it pastes whole, a code on its last line):
 * the question reaches the chief, the chief answers, the answer reaches the
 * member's own window, the result comes back. The chief's note to the owner
 * must hold the code, proof it read the long question to its end.
 * Run with one harness as the whole staff, once per harness; the report
 * says who asked, by role and harness.
 */
import { askedInTerminal } from '../measure.mjs'

/** How the members ask: `cf ask`, or (question-trip-native) their harness's own question tool. */
export function questionTrip({ id, title, how }) {
  return {
    id,
    title,
    fixture: 'questions',
    prompt: [
      'Un test de funcționare pentru întrebări, fără să mă întrebi pe mine nimic. Pune pe board',
      'trei sarcini, toate --tier standard, și scrie în fiecare brief exact ce să te întrebe și',
      `că întrebarea ți-o pune ${how}:`,
      '(1) unui worker: înainte de orice, să te întrebe ce culoare să aibă butonul Contact, apoi să',
      'scrie o singură linie cu culoarea în fișierul nou site/notes.md;',
      '(2) unui advisor (--advice): înainte de răspuns, să te întrebe pentru ce public e site-ul, apoi',
      'să recomande un font;',
      '(3) unui reviewer (--review): să verifice site/index.html, iar înainte de concluzii să te',
      'întrebe punând în întrebare, întreg și exact, textul din interne/intrebare-reviewer.md (e lung,',
      'cu un cod pe ultimul rând).',
      'Răspunde-le: workerului „albastru”, advisorului „manageri”, reviewerului „doar versiunea',
      'română”. Când ai toate trei rezultatele, acceptă-le și scrie-mi o singură notă cu ce a întrebat',
      'fiecare și codul de pe ultimul rând al întrebării reviewerului. Nu modifica alte fișiere.',
    ].join(' '),
    answers: [],
    fallback: 'Da.',
    quietMs: 120_000,
    expectations: [
      {
        name: 'a worker, an advisor and a reviewer each get a task',
        holds: (m) =>
          ['advisor', 'reviewer'].every((pool) => m.tasks.some((t) => t.pool === pool)) &&
          m.tasks.some((t) => t.pool !== 'advisor' && t.pool !== 'reviewer'),
      },
      ...['worker', 'advisor', 'reviewer'].map((role) => ({
        name: `the ${role} asks the chief, and the answer reaches its window`,
        holds: (m) =>
          (m.memberQuestionsBy ?? []).some(
            (row) => row.role === role && row.asked >= 1 && row.delivered >= 1,
          ),
      })),
      {
        name: 'the long question reaches the chief whole (over 4000 characters)',
        holds: (m) => Math.max(0, ...(m.memberQuestionsBy ?? []).map((row) => row.longest)) > 4000,
      },
      {
        name: "the chief's note holds the long question's code",
        holds: (m) => m.notesText.includes('PLOP-6142'),
      },
      { name: 'all three tasks are accepted', holds: (m) => m.plumbing.accepted >= 3 },
      { name: 'the owner is not asked anything', holds: (m) => askedInTerminal(m) === 0 },
    ],
  }
}

export default questionTrip({
  id: 'question-trip',
  title: 'A worker, an advisor and a reviewer each ask the chief (cf ask)',
  how: 'cu cf ask',
})
