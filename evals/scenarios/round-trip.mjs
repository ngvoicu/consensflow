/**
 * The plumbing case: the owner asks for one task whose worker must ask the
 * chief something before it can finish, an answer, a result and a review.
 * It measures the board, not the chief's judgment: a brief delivered, a
 * question from a member to the chief, the answer back, the result back,
 * a review, the task accepted. Any harness in any role.
 */
import { askedInTerminal } from '../measure.mjs'

export default {
  id: 'round-trip',
  title: 'A task out, a question back, an answer, a result, a review',
  fixture: 'site',
  prompt: [
    'Un test de funcționare, te rog, fără să mă întrebi pe mine nimic. Pune o singură sarcină',
    'unui worker: înainte de a scrie ceva, să te întrebe pe tine (cu cf ask) ce culoare să aibă',
    'butonul Contact, iar după răspunsul tău să scrie o singură linie cu culoarea în fișierul nou',
    'site/notes.md. Când te întreabă, răspunde-i „albastru”. Când sarcina e gata, cere unui reviewer',
    'să verifice site/notes.md, apoi acceptă sarcina. Nu modifica alte fișiere.',
  ].join(' '),
  answers: [],
  fallback: 'Da.',
  quietMs: 90_000,
  expectations: [
    {
      name: 'one task goes to a worker',
      holds: (m) => m.tasks.some((t) => t.pool !== 'reviewer' && t.pool !== 'advisor'),
    },
    {
      name: 'the worker asks the chief on the board',
      holds: (m) => m.plumbing.memberQuestions >= 1,
    },
    {
      name: 'the chief answers on the board',
      holds: (m) => m.plumbing.memberQuestionsAnswered >= 1,
    },
    { name: 'the finished work goes to a review', holds: (m) => m.reviews >= 1 },
    { name: 'the task is accepted', holds: (m) => m.plumbing.accepted >= 1 },
    { name: 'only site/notes.md is new', holds: (m) => m.filesChanged.join() === 'site/notes.md' },
    { name: 'the owner is not asked anything', holds: (m) => askedInTerminal(m) === 0 },
  ],
}
