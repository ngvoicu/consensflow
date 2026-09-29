/**
 * The results file: one row per report, newest last, the same numbers the
 * summary prints. Pure, so it is tested without a report on disk.
 */

/** The columns of one report, as strings. */
export function row(report) {
  const m = report.metrics
  const held = (list) =>
    list === undefined ? '?' : `${list.filter((c) => c.ok).length}/${list.length}`
  return [
    report.name?.slice(0, 16) ?? '',
    report.scenario,
    `${report.chief ?? 'claude'}${report.arm && report.arm !== 'card' ? ` [${report.arm}]` : ''} (${report.model}${'effort' in report ? `, ${report.effort ?? 'default'}` : ''})`,
    `${report.arm === 'bare' ? 'none' : (report.staff ?? ['claude']).join('+')}${report.staffEffort ? ` (${report.staffEffort})` : ''}`,
    held(report.checks),
    held(report.mechanics),
    String(m.tasks.length),
    String(m.parallel),
    String(m.advice),
    String(m.reviews),
    String(m.questionsToHuman.length),
    String(m.notesToHuman.length),
    String(m.chiefEdits ?? '?'),
    String((m.filesChanged ?? []).length),
    String(report.terminalAnswers ?? '?'),
    m.ownerQuestions === undefined
      ? '?'
      : `${m.ownerQuestions.board.decisions}/${m.ownerQuestions.terminal.questions}`,
    String(Math.round(report.seconds / 60)),
  ]
}

export const COLUMNS = [
  'when',
  'scenario',
  'chief (model, effort)',
  'staff',
  'judgment',
  'plumbing',
  'tasks',
  'parallel',
  'advice',
  'reviews',
  'questions',
  'notes',
  'chief edits',
  'files',
  'terminal',
  'asked',
  'min',
]

/** The results as a Markdown table, with what each column means above it. */
export function resultsTable(reports) {
  const lines = [
    '# Chief eval results',
    '',
    'One row per report in `evals/reports/`, oldest first; `npm run eval:summary` rewrites this file.',
    "Judgment: how many of the scenario's expectations the chief met. Plumbing: how many of the",
    "board's own checks held (briefs delivered, results back to the chief, members' questions",
    'answered and the answers delivered, every task on the board). Terminal: how often the owner',
    "had to answer in the chief's terminal because nothing was on the board. `?` is a report from",
    'before that column existed. Asked: the decisions put to the owner on the board / the questions',
    'the chief ended its turns with in its terminal. A chief',
    'marked [nocard] had a one-line card naming no board; [bare] ran without ConsensFlow.',
    '',
    `| ${COLUMNS.join(' | ')} |`,
    `| ${COLUMNS.map(() => '---').join(' | ')} |`,
    ...reports.map((report) => `| ${row(report).join(' | ')} |`),
  ]
  return `${lines.join('\n')}\n`
}
