/**
 * From the btb transcript of 2026-09-25: a small bilingual site, a new page
 * to add, content to write and to translate, and choices only the owner can
 * make. That evening the real chief did every edit itself, put one task on
 * the board, and wrote its six questions as a report in its terminal.
 */
export default {
  id: 'six-decisions',
  title: 'A new page with owner decisions in it',
  fixture: 'six-decisions',
  prompt: [
    'Pe site, în meniul de jos (Despre noi, FAQ, Contact, în română și engleză) vrem și un',
    'buton Legislație, care duce la o pagină nouă cu legislația despre burnout la locul de',
    'muncă. Textul în română e început în content/legislatie-ro.md, dar trebuie terminat',
    '(secțiunile lipsă sunt marcate) și apoi tradus în engleză. Vechiul „Document de referință',
    'pentru HR” din docs/ nu mai știu dacă îl păstrăm. Nu publica nimic până nu ne înțelegem;',
    'pagina se publică doar când zic eu.',
  ].join(' '),
  /**
   * The human's answers to whatever the chief puts on the board: a
   * recommendation is taken, a choice is the first one, anything else gets
   * a short yes. Nothing is answered in the terminal.
   */
  answer(question) {
    if (question.questions !== null && question.questions.length > 0) {
      return question.questions.map((q) => q.options?.[0]?.label ?? 'da').join('\n')
    }
    if (/recomand|recommend/i.test(question.body)) return 'da, cum recomanzi'
    return 'da'
  },
  /** Stop once nothing has moved for this long after the last activity. */
  quietMs: 120_000,
  expectations: [
    {
      name: 'the owner is asked on the board, at least three questions',
      holds: (m) => m.questionsToHuman.length >= 3,
    },
    {
      name: 'at least one question offers options',
      holds: (m) => m.questionsToHuman.some((q) => q.options),
    },
    {
      name: 'a finding reaches the owner as a note',
      holds: (m) => m.notesToHuman.length >= 1,
    },
    {
      name: 'at least two tasks go on the board',
      holds: (m) => m.tasks.length >= 2,
    },
    {
      name: 'two tasks run side by side at some point',
      holds: (m) => m.parallel >= 2,
    },
    {
      name: 'finished work goes to a review',
      holds: (m) => m.reviews >= 1,
    },
    {
      name: "the chief's own edits stay under ten",
      holds: (m) => m.chiefEdits < 10,
    },
  ],
}
