/**
 * The advisor's plumbing: the owner asks for advice through the board, the
 * advice comes back to the chief, the chief accepts it and passes the
 * recommendation to the owner as a note. No file changes.
 */
export default {
  id: 'advice-trip',
  title: 'Advice out, advice back, a note to the owner',
  fixture: 'site',
  prompt: [
    'Un test de funcționare, te rog, fără să mă întrebi pe mine nimic. Cere sfatul unui advisor',
    '(cf task add --advice): pentru un site despre burnout, ce culoare e mai potrivită pentru',
    'butonul Contact, albastru sau verde, și de ce, în cel mult trei propoziții. Când vine',
    'răspunsul, acceptă sarcina și trimite-mi recomandarea lui ca notă (cf note --human).',
    'Nu modifica niciun fișier și nu pune alte sarcini.',
  ].join(' '),
  answers: [],
  fallback: 'Da.',
  quietMs: 90_000,
  expectations: [
    { name: 'one advice task goes on the board', holds: (m) => m.advice === 1 },
    {
      name: 'the advice task is accepted',
      holds: (m) => m.tasks.some((t) => t.pool === 'advisor' && t.state === 'accepted'),
    },
    {
      name: 'the recommendation reaches the owner as a note',
      holds: (m) => m.notesToHuman.length >= 1,
    },
    { name: 'no file changes', holds: (m) => m.filesChanged.length === 0 },
    { name: 'the owner is not asked anything', holds: (m) => m.questionsToHuman.length === 0 },
  ],
}
