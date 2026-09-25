/**
 * From the btb transcript of 2026-09-25: a small bilingual site, a new page
 * to add, content to write and to translate, and choices only the owner can
 * make. That evening the real chief did every edit itself, put one task on
 * the board, and wrote its six questions as a report in its terminal.
 */
export default {
  id: 'six-decisions',
  title: 'A new page with owner decisions in it',
  fixture: 'site',
  prompt: [
    'Pe site, în meniul de jos (Despre noi, FAQ, Contact, în română și engleză) vrem și un',
    'buton Legislație, care duce la o pagină nouă cu legislația despre burnout la locul de',
    'muncă. Textul în română e început în content/legislatie-ro.md, dar trebuie terminat',
    '(secțiunile lipsă sunt marcate) și apoi tradus în engleză. Vechiul „Document de referință',
    'pentru HR” din docs/ nu mai știu dacă îl păstrăm. Nu publica nimic până nu ne înțelegem;',
    'pagina se publică doar când zic eu.',
  ].join(' '),
  /**
   * The owner's answers, free text, by what the question is about (the
   * specific subjects first: most questions mention publishing in passing);
   * then options; then the fallback.
   */
  answers: [
    {
      match: /document(ul)? de referin|reference document|docs\//i,
      text: 'Îl păstrăm, dar pune sus o notă că pagina de legislație e sursa actuală.',
    },
    { match: /recomand|recommend/i, text: 'Da, cum recomanzi.' },
    {
      match: /publica|publish/i,
      text: 'Nu publicăm încă. Vreau să văd pagina întâi; îți spun eu când.',
    },
  ],
  fallback: 'Da.',
  /** What the owner types into the chief's terminal when the chief stops there instead of asking on the board. */
  nudge: 'Da, cum recomanzi. Continuă.',
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
      name: "the chief's own edits stay under ten (counted for a Claude chief only)",
      holds: (m) => m.chiefEdits === null || m.chiefEdits < 10,
    },
  ],
}
