import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { askTheBoard } from '../../hosts/lib/question-door.js'

/** `cf` inside a window the new core opened: the agents' commands (`USAGE` lists them). */
export const USAGE = `cf inside a ConsensFlow window: the board's commands.

  cf task add --tier <critical|complex|standard|light> "…"
                                    work for a worker of that tier; ConsensFlow picks
                                    the member (--purpose for critical work)
  cf task add --advice --tier <tier> "…"
                                    a question for an advisor of that tier: findings and
                                    recommendations back, no file changed
  cf task add --review --tier <tier> "…"
                                    a review for a reviewer of that tier: say what to
                                    review; findings back, no file changed
  cf task add --design "…"          an image from the image designer: what to draw, what
                                    to use as reference, where to save it
  cf task add --after T-3 "…"       a follow-up for the window that did T-3, which
                                    keeps its context; only when that context matters
  cf task add --self --needs T-3 "…" your own later step: its brief comes back to you when T-3 is accepted
  … --needs T-3,T-4                 the task waits on the board until T-3 and T-4 are accepted
  … --before T-9,T-10               T-9 and T-10 (still on the board) wait for this task
  cf task list                      the board: what waits for a member, then every lane
  cf task get T-3                   one task and its whole thread
  cf task get T-3 --transcript      what its window did so far (the last 10 items; --last 30 for more)
  cf task done T-3 "…"              finish a task assigned to you (the lead)
  cf task accept|cancel T-3         move a task you asked for
  cf task reopen T-3 "…"            send a finished or failed task back with a follow-up
  cf task pause T-3                 stop a worker's task: the agent stops, its window and work wait
  cf task resume T-3 "…"            go on with it: the same window, with your words
  cf inbox [read m-12]              what is waiting for you, or one message in full
  cf ask "…" [--human]              a question to whoever gave you your task (or the human)
  cf note "…" [--human]             something they should know; nothing waits on it
  cf answer m-12 "…"                answer a question put to you
  cf team                           the members: roles and tiers
  cf whoami                         your project, role and current task

Add --json for machine output.`

const HELP = new Set(['help', '--help', '-h'])

export async function runCoreCli(
  args,
  env,
  { out, err, cwd = process.cwd(), input = readStandardInput },
) {
  const json = args.includes('--json')
  const words = args.filter((arg) => arg !== '--json')
  const call = client(env)
  try {
    const [verb, ...rest] = words
    // A harness hook is not a command the model runs: it prints only what the
    // harness must read, and nothing at all when it has nothing to say.
    if (verb === 'hook') {
      const output = await hook(rest[0], call, env, input)
      if (output !== null) out(JSON.stringify(output))
      return 0
    }
    const result = await command(verb, rest, call, cwd)
    out(json ? JSON.stringify(result.data, null, 2) : result.text)
    return 0
  } catch (cause) {
    err(`cf: ${cause.message}`)
    return cause.usage ? 2 : 1
  }
}

async function readStandardInput() {
  const chunks = []
  for await (const chunk of process.stdin) chunks.push(chunk)
  return Buffer.concat(chunks).toString('utf8')
}

/** Each harness's question tool, as its PreToolUse hook names it. */
const QUESTION_TOOLS = { claude: 'AskUserQuestion', devin: 'ask_user_question' }

/**
 * A harness's question tool, answered from the board: a PreToolUse hook on
 * Claude Code's AskUserQuestion or Devin's ask_user_question. The questions go
 * to whoever gave the task and the hook waits for the answer. Claude takes it
 * back as the tool's input, the way it documents; Devin draws its dialog even
 * over a pre-filled input (probed 2026-09-20), so its hook refuses the tool
 * and hands the answer over as the refusal's reason, which Devin reads and
 * continues with. Anything that goes wrong (no ConsensFlow, a timeout, another
 * tool) ends silently: the harness then shows its own dialog in the window.
 */
async function hook(harness, call, env, input) {
  const tool = QUESTION_TOOLS[harness]
  if (tool === undefined) return null
  try {
    const event = JSON.parse(await input())
    const questions = event.tool_input?.questions
    if (event.tool_name !== tool || !Array.isArray(questions)) return null
    const { answer } = await askTheBoard(
      call,
      questions.map((q) => ({
        question: q.question,
        header: q.header,
        options: (q.options ?? []).map((o) => ({ label: o.label, description: o.description })),
        multiple: q.multiSelect === true,
      })),
      env.CONSENSFLOW_QUESTION_WAIT_MS ? { waitMs: Number(env.CONSENSFLOW_QUESTION_WAIT_MS) } : {},
    )
    if (answer === null) return null
    const answers = questions.map((q, at) => [q.question, (answer.choices?.[at] ?? []).join(', ')])
    if (harness === 'devin') {
      return {
        decision: 'block',
        reason: `ConsensFlow answered from the board: ${answers
          .map(([question, choice]) => `${question} ${choice}`)
          .join(' · ')}. Continue with that answer; do not ask again.`,
      }
    }
    return {
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'allow',
        updatedInput: { ...event.tool_input, answers: Object.fromEntries(answers) },
      },
    }
  } catch {
    return null
  }
}

async function command(verb, rest, call, cwd) {
  if (verb === undefined || HELP.has(verb)) return { data: { usage: USAGE }, text: USAGE }
  switch (verb) {
    case 'task':
      return taskCommand(rest, call, cwd)
    case 'inbox': {
      if (rest[0] === 'read') {
        const id = messageId(rest[1])
        const { message } = await call('GET', `/api/inbox/${id}`)
        return { data: message, text: `${messageLine(message)}\n\n${message.body}` }
      }
      const { messages } = await call('GET', '/api/inbox')
      return {
        data: messages,
        text: messages.length === 0 ? 'Your inbox is empty.' : messages.map(messageLine).join('\n'),
      }
    }
    case 'note': {
      const { flags, text } = split(rest, ['--human'], ['--to', '--task'])
      const to = flags['--human'] ? 'human' : handle(flags['--to'])
      const { message } = await call('POST', '/api/notes', {
        body: requireText(text, 'cf note "what to know"'),
        ...(to === undefined ? {} : { to }),
        ...(flags['--task'] === undefined ? {} : { task: taskNumber(flags['--task']) }),
      })
      return {
        data: message,
        text: `m-${message.id} noted to @${message.recipient}; nothing waits on it.`,
      }
    }
    case 'ask': {
      const { flags, text } = split(rest, ['--human'], ['--to', '--task'])
      const to = flags['--human'] ? 'human' : handle(flags['--to'])
      const { message } = await call('POST', '/api/questions', {
        body: requireText(text, 'cf ask "your question"'),
        ...(to === undefined ? {} : { to }),
        ...(flags['--task'] === undefined ? {} : { task: taskNumber(flags['--task']) }),
      })
      return {
        data: message,
        text: `m-${message.id} asked @${message.recipient}. The answer arrives as a message; end your turn now.`,
      }
    }
    case 'answer': {
      const [id, ...words] = rest
      const { message } = await call('POST', '/api/answers', {
        question: messageId(id),
        body: requireText(words.join(' '), 'cf answer m-<id> "your answer"'),
      })
      return {
        data: message,
        text:
          message.state === 'gated'
            ? `m-${message.id} answered @${message.recipient}; the human passes it on first.`
            : `m-${message.id} answered @${message.recipient}.`,
      }
    }
    case 'team': {
      const { members } = await call('GET', '/api/team')
      return {
        data: members,
        text:
          members.length === 0
            ? 'No agents are on this project team yet; the human adds them in the app.'
            : members
                .map((member) => `@${member.handle} · ${member.roles.join('+')} · ${member.tier}`)
                .join('\n'),
      }
    }
    case 'whoami': {
      const me = await call('GET', '/api/whoami')
      return {
        data: me,
        text:
          `@${me.participant.handle} (${me.participant.role}) in project ${me.project.name}` +
          (me.task === null ? '' : `, on T-${me.task.number}: ${me.task.title}`),
      }
    }
    default:
      throw usage(
        `unknown command ${JSON.stringify(verb ?? '')}: use task, inbox, ask, answer, team or whoami`,
      )
  }
}

async function taskCommand([action, ...rest], call, cwd) {
  if (HELP.has(action)) {
    const lines = USAGE.split('\n').filter((line) => /^\s+cf task |^ {36}/.test(line))
    return { data: { usage: lines.join('\n') }, text: lines.join('\n') }
  }
  if (action === 'add') {
    const { flags, text, target } = split(
      rest,
      ['--self', '--advice', '--review', '--design'],
      ['--to', '--title', '--file', '--tier', '--purpose', '--after', '--needs', '--before'],
    )
    const to = handle(flags['--to'] ?? target)
    const tier = flags['--tier']
    const after = flags['--after'] === undefined ? undefined : taskNumber(flags['--after'])
    const needs = taskNumbers(flags['--needs'])
    const before = taskNumbers(flags['--before'])
    // Your own work needs no board while you are at it; on the board it is a
    // wake-up: the brief comes back to this window when what it waits for is accepted.
    if (flags['--self'] === true && needs === undefined && before === undefined) {
      throw usage(
        'you are already at it: do it now, or give it --needs T-3 to be woken when T-3 is accepted',
      )
    }
    if (
      to === undefined &&
      tier === undefined &&
      after === undefined &&
      flags['--self'] !== true &&
      flags['--design'] !== true
    ) {
      throw usage(ADD_USAGE)
    }
    const body =
      flags['--file'] === undefined
        ? requireText(text, ADD_USAGE)
        : await readFile(resolve(cwd, flags['--file']), 'utf8')
    const address = flags['--self']
      ? { self: true }
      : after !== undefined
        ? { after }
        : to !== undefined
          ? { to }
          : flags['--design']
            ? { design: true }
            : {
                tier,
                ...(flags['--advice'] ? { advice: true } : {}),
                ...(flags['--review'] ? { review: true } : {}),
                ...(flags['--purpose'] === undefined ? {} : { purpose: flags['--purpose'] }),
              }
    const created = await call('POST', '/api/tasks', {
      ...address,
      ...(needs === undefined ? {} : { needs }),
      ...(before === undefined ? {} : { before }),
      body,
      ...(flags['--title'] === undefined ? {} : { title: flags['--title'] }),
    })
    const { number, pool, assignee } = created.task
    // With human approval required, nothing moves until the human passes it on.
    const gated =
      created.gated && !flags['--self'] ? ' The human approves each message before it moves.' : ''
    const waits =
      created.task.blockedBy.length === 0
        ? ''
        : ` It waits until ${tasks(created.task.blockedBy)} ${created.task.blockedBy.length === 1 ? 'is' : 'are'} accepted.`
    const holds =
      before === undefined ? '' : ` ${tasks(before)} wait${before.length === 1 ? 's' : ''} for it.`
    return {
      data: created,
      text: flags['--self']
        ? `T-${number} is yours; finish it with: cf task done T-${number} "what you did".${waits}`
        : after !== undefined
          ? `T-${number} continues in @${assignee}, the window that did T-${after}; its result arrives in your inbox.${gated}${waits}`
          : to !== undefined
            ? `T-${number} queued for @${to}. The result arrives in your inbox when @${to} finishes.${gated}${waits}`
            : `T-${number} is on the board for ${aPool(pool, tier)}; the first free one gets it, and its result arrives in your inbox.${waits}${holds}${gated}`,
    }
  }
  if (action === 'list' || action === undefined) {
    const board = await call('GET', '/api/tasks')
    const lines = [
      ...(board.open.length === 0 ? [] : ['Waiting for a member', ...board.open.map(taskLine)]),
      ...board.lanes.flatMap((lane) =>
        lane.tasks.length === 0
          ? []
          : [`@${lane.handle} (${lane.role})`, ...lane.tasks.map(taskLine)],
      ),
    ]
    return { data: board, text: lines.length === 0 ? 'No tasks yet.' : lines.join('\n') }
  }
  const number = taskNumber(rest[0])
  if (action === 'get') {
    const { flags } = split(rest.slice(1), ['--transcript'], ['--last'])
    const { task } = await call('GET', `/api/tasks/${number}`)
    if (flags['--transcript'] === true) {
      const last = flags['--last'] === undefined ? 10 : Number(flags['--last'])
      if (!Number.isInteger(last) || last < 1) throw usage('--last takes a number of items')
      const copy = await call('GET', `/api/tasks/${number}/transcript?last=${last}`)
      const items = copy.items.map(
        (item) =>
          `[${TRANSCRIPT_ROLE[item.role] ?? item.role}${item.complete ? '' : ' · still writing'}]\n${clip(item.text, 600)}`,
      )
      return {
        data: { task, ...copy },
        text: `${taskLine(task)}\n\n${
          copy.total === 0
            ? 'Its window has written nothing yet.'
            : `What its window did, the last ${copy.items.length} of ${copy.total} items:\n\n${items.join('\n\n')}`
        }`,
      }
    }
    const thread = task.messages.map((m) => `${messageLine(m)}\n${m.body}`).join('\n\n')
    return { data: task, text: `${taskLine(task)}\n\n${thread}` }
  }
  if (['done', 'accept', 'cancel', 'reopen', 'pause', 'resume'].includes(action)) {
    const text = rest.slice(1).join(' ')
    if (['done', 'reopen', 'resume'].includes(action) && text.trim().length === 0) {
      throw usage(
        `cf task ${action} T-${number} "${action === 'done' ? 'your result' : action === 'resume' ? 'what to do now' : 'the follow-up'}"`,
      )
    }
    const { task } = await call(
      'POST',
      `/api/tasks/${number}/${action}`,
      text ? { body: text } : {},
    )
    const said =
      action === 'pause'
        ? `T-${number} is paused: its window stops and its work waits. Resume it with: cf task resume T-${number} "what to do now"`
        : action === 'resume'
          ? task.state === 'open'
            ? `T-${number} is back on the board for ${aPool(task.pool, task.tier)}: the window that had it has ended.`
            : `T-${number} resumes in @${task.assignee} with your words.`
          : taskLine(task)
    return { data: task, text: said }
  }
  throw usage(
    `unknown task command ${JSON.stringify(action)}: use add, list, get, done, accept, reopen, cancel, pause or resume`,
  )
}

const ADD_USAGE =
  'cf task add --tier <critical|complex|standard|light> "what to do" (with --advice for an advisor or --review for a reviewer; or --design, --after T-3, or --self; --needs T-3,T-4 and --before T-9,T-10 order the board)'

/** "a standard worker", "an image designer": who an open task waits for. */
const aPool = (pool, tier) => (pool === 'designer' ? 'an image designer' : `a ${tier} ${pool}`)
/** "T-3, T-4": task numbers in a sentence. */
const tasks = (numbers) => numbers.map((number) => `T-${number}`).join(', ')

function client(env) {
  const url = env.CONSENSFLOW_URL
  const token = env.CONSENSFLOW_TOKEN
  return async (method, path, body, { signal } = {}) => {
    if (typeof url !== 'string' || url.length === 0) {
      throw new Error('CONSENSFLOW_URL is not set: run cf from a window ConsensFlow opened')
    }
    const response = await fetch(`${url}${path}`, {
      method,
      headers: { authorization: `Bearer ${token}`, 'content-type': 'application/json' },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      ...(signal === undefined ? {} : { signal }),
    }).catch((cause) => {
      throw new Error(`ConsensFlow is not answering at ${url} (${cause.message})`)
    })
    const value = await response.json().catch(() => ({}))
    if (!response.ok) throw new Error(value.message ?? `ConsensFlow answered ${response.status}`)
    return value
  }
}

/** Positional words, one leading `@target`, and the named flags. */
/** How a transcript item's role reads in a window. */
const TRANSCRIPT_ROLE = {
  user: 'Sent to the window',
  assistant: 'The agent',
  tool: 'Tool output',
  custom: 'Note',
}
const clip = (text, at) => (text.length > at ? `${text.slice(0, at)}…` : text)

function split(words, booleans, valued) {
  const flags = {}
  const text = []
  let target
  for (let at = 0; at < words.length; at += 1) {
    const word = words[at]
    if (booleans.includes(word)) flags[word] = true
    else if (valued.includes(word)) {
      flags[word] = words[at + 1]
      at += 1
    } else if (target === undefined && text.length === 0 && word.startsWith('@')) target = word
    else text.push(word)
  }
  return { flags, text: text.join(' '), target }
}

function handle(value) {
  if (value === undefined) return undefined
  return value.startsWith('@') ? value.slice(1) : value
}

function taskNumber(value) {
  const match = /^(?:T-)?(\d+)$/i.exec(value ?? '')
  if (match === null) throw usage(`not a task: ${JSON.stringify(value ?? '')} (write T-3)`)
  return Number(match[1])
}

/** "T-3,T-4" as numbers; undefined when the flag was not given. */
function taskNumbers(value) {
  if (value === undefined) return undefined
  return value.split(',').map((part) => taskNumber(part.trim()))
}

function messageId(value) {
  const match = /^(?:m-)?(\d+)$/i.exec(value ?? '')
  if (match === null) throw usage(`not a message: ${JSON.stringify(value ?? '')} (write m-12)`)
  return Number(match[1])
}

function requireText(text, example) {
  if (typeof text !== 'string' || text.trim().length === 0) throw usage(`say what: ${example}`)
  return text
}

function usage(message) {
  const error = new Error(message)
  error.usage = true
  return error
}

const taskLine = (task) =>
  `T-${task.number} [${task.state}] ${task.assignee === null ? `${task.blockedBy.length === 0 ? '' : `blocked by ${tasks(task.blockedBy)} · `}for ${aPool(task.pool, task.tier)}` : `@${task.assignee}`} ← @${task.requester}: ${task.title}`
const messageLine = (message) =>
  `m-${message.id} [${message.state}] ${message.kind}${(message.task ?? message.taskNumber) ? ` T-${message.task ?? message.taskNumber}` : ''} from ${message.sender === null ? 'ConsensFlow' : `@${message.sender}`}${message.preview === undefined ? '' : `: ${message.preview}`}`
