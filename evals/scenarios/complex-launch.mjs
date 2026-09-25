/**
 * The large case: a launch with research, several pages, a hard call and
 * work the owner must sign off. Everything the board is for at once: work
 * that runs side by side, advice for the call, a review before the owner
 * sees it, and the owner's decisions put to the owner.
 */
export default {
  id: 'complex-launch',
  title: 'A launch: research, pages, a hard call, a sign-off',
  fixture: 'site',
  prompt: [
    'Lansăm site-ul luna viitoare și vreau trei lucruri deodată. Unu: o pagină nouă „Legislație”',
    'în română și engleză, din content/legislatie-ro.md, terminată și tradusă (secțiunile lipsă sunt',
    'marcate). Doi: o pagină „Pentru manageri” care explică ce văd managerii după evaluare, pornind',
    'de la docs/ghidul-managerului.md; verifică că ce spune ghidul chiar corespunde cu ce arată',
    'site-ul. Trei: un raport scurt (docs/lansare.md) despre ce ne lipsește ca site-ul să fie',
    'publicabil (accesibilitate, ce pagini lipsesc în engleză, ce texte sunt vechi), cu o recomandare',
    'de ordine. Nu publicăm nimic fără să văd eu; unde trebuie o decizie care e a mea, întreabă-mă.',
  ].join(' '),
  answers: [
    { match: /publica|publish/i, text: 'Nu încă; după ce văd paginile.' },
    {
      match: /document(ul)? de referin|reference document/i,
      text: 'Îl păstrăm, cu o notă că e vechi.',
    },
    {
      match: /ordine|order|întâi|first/i,
      text: 'Legislația întâi, apoi managerii, apoi raportul.',
    },
    { match: /recomand|recommend/i, text: 'Da, cum recomanzi.' },
  ],
  fallback: 'Da.',
  quietMs: 150_000,
  expectations: [
    { name: 'at least three tasks go on the board', holds: (m) => m.tasks.length >= 3 },
    { name: 'two tasks run side by side at some point', holds: (m) => m.parallel >= 2 },
    { name: 'advice is asked at least once', holds: (m) => m.advice >= 1 },
    { name: 'finished work goes to a review', holds: (m) => m.reviews >= 1 },
    {
      name: 'the owner is asked on the board, at least two questions',
      holds: (m) => m.questionsToHuman.length >= 2,
    },
    {
      name: 'a finding reaches the owner as a note (the guide and the site disagree)',
      holds: (m) => m.notesToHuman.length >= 1,
    },
    { name: "the chief's own edits stay under ten", holds: (m) => m.chiefEdits < 10 },
  ],
}
