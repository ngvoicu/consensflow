/**
 * The owner works with the chief the way Gabriel does: a conversation of
 * short messages, each after the chief is done with the last, most of them
 * small changes. On btb (2026-09-25 to 28) a Claude chief took three days of
 * such messages and put nothing on the board, doing every change itself.
 * This measures how much of that work goes out: tasks on the board against
 * the files that changed. (A chief's own edit count misses edits made with
 * shell commands, which is how an Opus chief made all seven here.)
 */
export default {
  id: 'conversation',
  title: 'A working conversation of small asks: how many go to the board',
  fixture: 'site',
  prompt: 'Salut. Ce pagini are site-ul acum și ce face fiecare, pe scurt?',
  followUps: [
    'În site/evaluare.html scorul e de la 0 la 10, dar docs/ghidul-managerului.md zice de la 1 la 5. Corectează ghidul să spună 0 la 10.',
    'Adaugă în subsolul paginii principale un link către pagina de evaluare.',
    'Culorile din site/style.css sunt scrise direct peste tot; mută-le în variabile CSS.',
    'Fă și versiunea în engleză a paginii de evaluare, în site/en/evaluare.html.',
    'Titlul paginii principale să fie „Burnout la locul de muncă — ghid pentru manageri”.',
    'Verifică dacă merg toate linkurile interne din site.',
    'Scrie în README ce s-a schimbat azi, pe scurt.',
  ],
  answers: [],
  fallback: 'Da, cum crezi tu.',
  quietMs: 120_000,
  expectations: [
    { name: 'at least three changes go to the board', holds: (m) => m.tasks.length >= 3 },
    {
      name: 'the English page is a worker’s (a translation runs on its own)',
      holds: (m) => m.tasks.some((t) => /engl|english|en\/evaluare/i.test(t.title ?? '')),
    },
    {
      name: 'the work is done: the English evaluation page exists',
      holds: (m) => m.filesChanged.includes('site/en/evaluare.html'),
    },
  ],
}
