/**
 * The chief's controls over a running task: `cf tell` stops a worker and
 * asks it something, the worker answers, `cf task resume` sends it on, and
 * `cf task add --after` gives a follow-up to the same window. Plumbing, not
 * judgment: the owner spells out the steps.
 */
export default {
  id: 'control-trip',
  title: 'Tell, answer, resume, a follow-up to the same window',
  fixture: 'site',
  prompt: [
    'Un test de funcționare, te rog, fără să mă întrebi nimic pe mine. Pașii, în ordine:',
    '(1) Pune o sarcină unui worker: să ruleze întâi comanda `sleep 90`, apoi să scrie în fișierul',
    'nou site/notes.md o singură linie, „unu”. (2) Când sarcina e în lucru (verifică cu cf task get',
    'T-1), oprește-o cu cf tell T-1 și întreabă-l în ce fișier va scrie. (3) După ce îți răspunde,',
    'reia sarcina cu cf task resume T-1 „continuă de unde ai rămas”. (4) Când vine rezultatul,',
    'acceptă T-1 și dă o continuare aceleiași ferestre cu cf task add --after T-1: să adauge în',
    'site/notes.md a doua linie, „doi”. (5) Când vine și acest rezultat, acceptă-l. Nu modifica',
    'alte fișiere și nu pune alte sarcini.',
  ].join(' '),
  answers: [],
  fallback: 'Da.',
  quietMs: 120_000,
  expectations: [
    { name: 'the chief stops the task with cf tell', holds: (m) => m.plumbing.tells >= 1 },
    { name: 'the worker answers the tell', holds: (m) => m.plumbing.tellsAnswered >= 1 },
    {
      name: 'the task is paused and resumed',
      holds: (m) => m.plumbing.pauses >= 1 && m.plumbing.resumes >= 1,
    },
    {
      name: 'a follow-up goes to the same window (--after)',
      holds: (m) => m.plumbing.continuations >= 1,
    },
    { name: 'both tasks are accepted', holds: (m) => m.plumbing.accepted >= 2 },
    { name: 'only site/notes.md is new', holds: (m) => m.filesChanged.join() === 'site/notes.md' },
    { name: 'the owner is not asked anything', holds: (m) => m.questionsToHuman.length === 0 },
  ],
}
