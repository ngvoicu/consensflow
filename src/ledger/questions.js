import { LedgerError, MAX_BODY, MAX_TITLE, requireText } from './model.js'

/**
 * Questions with options, as a harness's own question tool asks them, and
 * their answers by choice: the checks that take them in and the text an
 * inbox or a window shows for them.
 */

const MAX_QUESTIONS = 4

const badQuestions = (why) => new LedgerError('bad-questions', `questions: ${why}`, 400)
const shortText = (value, field) => {
  if (typeof value !== 'string' || value.trim().length === 0 || value.length > MAX_TITLE * 10) {
    throw badQuestions(`${field} is a short text`)
  }
  return value.trim()
}

/**
 * Questions with options as a harness's question tool asks them: one to four,
 * each with its text, a short header, its options (a label, maybe a
 * description) and whether several may be picked.
 */
export function requireQuestions(questions) {
  if (!Array.isArray(questions) || questions.length === 0 || questions.length > MAX_QUESTIONS) {
    throw badQuestions(`one to ${MAX_QUESTIONS} questions`)
  }
  return questions.map((question) => {
    if (question === null || typeof question !== 'object' || !Array.isArray(question.options)) {
      throw badQuestions('each question is an object with an options array')
    }
    // The question itself may run as long as any message; its header and
    // labels are what a picker shows, and stay short.
    if (typeof question.question !== 'string' || question.question.trim().length === 0) {
      throw badQuestions('each question has its text')
    }
    return {
      question: requireText(question.question.trim(), 'question', MAX_BODY),
      header: shortText(question.header, 'header'),
      options: question.options.map((option) => ({
        label: shortText(option?.label, 'an option label'),
        description:
          typeof option.description === 'string' && option.description.trim().length > 0
            ? option.description.trim()
            : null,
      })),
      multiple: question.multiple === true,
    }
  })
}

/** A question with options as text: what an inbox or a window shows. */
export const renderQuestions = (questions) =>
  questions
    .map((q) =>
      [
        `${q.header}: ${q.question}`,
        ...q.options.map(
          (o) => `- ${o.label}${o.description === null ? '' : `: ${o.description}`}`,
        ),
      ].join('\n'),
    )
    .join('\n\n')

const badChoices = (why) => new LedgerError('bad-choices', `answer: ${why}`, 400)

/**
 * The choices for a question with options: one array of picks per question,
 * from explicit `choices` or from text, one line per question, the labels
 * matched regardless of case and free text kept as it is.
 */
export function requireChoices(questions, { choices, body }) {
  const picks =
    choices !== undefined
      ? choices
      : String(body ?? '')
          .split('\n')
          .map((line) => line.trim())
          .filter(Boolean)
          .map((line, at) => (questions[at]?.multiple ? line.split(',') : [line]))
  if (!Array.isArray(picks) || picks.length !== questions.length) {
    throw badChoices(`one answer per question (${questions.length})`)
  }
  return picks.map((pick, at) => {
    const question = questions[at]
    if (!Array.isArray(pick) || pick.length === 0 || (!question.multiple && pick.length > 1)) {
      throw badChoices(
        `${question.header}: ${question.multiple ? 'one or more picks' : 'one pick'}`,
      )
    }
    return pick.map((text) => {
      const wanted = String(text).trim()
      if (wanted.length === 0) throw badChoices('empty pick')
      // A pick in the human's own words ("Something else") is an answer like any other.
      if (wanted.length > MAX_BODY)
        throw badChoices(`pick too long (at most ${MAX_BODY} characters)`)
      const label = question.options.find((o) => o.label.toLowerCase() === wanted.toLowerCase())
      return label === undefined ? wanted : label.label
    })
  })
}

export const renderChoices = (questions, choices) =>
  questions.map((q, at) => `${q.header}: ${choices[at].join(', ')}`).join('\n')
