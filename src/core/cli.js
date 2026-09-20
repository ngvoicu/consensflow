import { readFile } from 'node:fs/promises'
import { resolve } from 'node:path'
import { askTheBoard } from '../../hosts/lib/question-door.js'

/** `cf` inside a window the new core opened: the agents' commands (`USAGE` lists them). */
export const USAGE = `cf inside a ConsensFlow window: the board's commands.

  cf task add --tier <critical|complex|standard|light> "…"
                                    a task for a member of that tier; ConsensFlow picks
                                    the member (--tags a,b to prefer one, --purpose for
                                    critical work)
  cf task add --self "…"            a task for yourself, on the board
  cf task list                      the board: what waits for a member, then every lane
  cf task get T-3                   one task and its whole thread
  cf task done T-3 "…"              finish a task assigned to you (coordinators)
  cf task review T-3                ask for an independent review of finished work
  cf task accept|cancel T-3         move a task you asked for
  cf task reopen T-3 "…"            send a finished or failed task back with a follow-up
  cf inbox [read m-12]              what is waiting for you, or one message in full
  cf ask "…" [--human]              a question to whoever gave you your task (or the human)
  cf answer m-12 "…"                answer a question put to you
  cf team                           the members: roles, tiers and tags
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

/**
 * A harness's question tool, answered from the board. `claude`: Claude Code's
 * PreToolUse hook for AskUserQuestion. The questions go to whoever gave the
 * task, the hook waits for the answer, and returns it as the tool's input, the
 * way Claude documents it. Anything that goes wrong (no ConsensFlow, a timeout,
 * another tool) ends silently: Claude then shows its own dialog in the window.
 */
async function hook(harness, call, env, input) {
  if (harness !== 'claude') return null
  try {
    const event = JSON.parse(await input())
    const questions = event.tool_input?.questions
    if (event.tool_name !== 'AskUserQuestion' || !Array.isArray(questions)) return null
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
    return {
      hookSpecificOutput: {
        hookEventName: 'PreToolUse',
        permissionDecision: 'allow',
        updatedInput: {
          ...event.tool_input,
          answers: Object.fromEntries(
            questions.map((q, at) => [q.question, (answer.choices?.[at] ?? []).join(', ')]),
          ),
        },
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
      return { data: message, text: `m-${message.id} answered @${message.recipient}.` }
    }
    case 'team': {
      const { members } = await call('GET', '/api/team')
      return {
        data: members,
        text:
          members.length === 0
            ? 'No agents are on this project team yet; the human adds them in the app.'
            : members
                .map(
                  (member) =>
                    `@${member.handle} · ${member.roles.join('+')} · ${member.tier} · ${member.tags.length === 0 ? 'no tags' : member.tags.join(', ')}`,
                )
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
      ['--self'],
      ['--to', '--title', '--file', '--tier', '--tags', '--purpose'],
    )
    const to = handle(flags['--to'] ?? target)
    const tier = flags['--tier']
    if (to === undefined && tier === undefined && flags['--self'] !== true) throw usage(ADD_USAGE)
    const body =
      flags['--file'] === undefined
        ? requireText(text, ADD_USAGE)
        : await readFile(resolve(cwd, flags['--file']), 'utf8')
    const address = flags['--self']
      ? { self: true }
      : to !== undefined
        ? { to }
        : {
            tier,
            ...(flags['--tags'] === undefined ? {} : { tags: tags(flags['--tags']) }),
            ...(flags['--purpose'] === undefined ? {} : { purpose: flags['--purpose'] }),
          }
    const created = await call('POST', '/api/tasks', {
      ...address,
      body,
      ...(flags['--title'] === undefined ? {} : { title: flags['--title'] }),
    })
    const { number, pool } = created.task
    return {
      data: created,
      text: flags['--self']
        ? `T-${number} is yours; finish it with: cf task done T-${number} "what you did".`
        : to !== undefined
          ? `T-${number} queued for @${to}. The result arrives in your inbox when @${to} finishes.`
          : `T-${number} is on the board for a ${tier} ${pool}; the first free one gets it, and its result arrives in your inbox.`,
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
    const { task } = await call('GET', `/api/tasks/${number}`)
    const thread = task.messages.map((m) => `${messageLine(m)}\n${m.body}`).join('\n\n')
    return { data: task, text: `${taskLine(task)}\n\n${thread}` }
  }
  if (action === 'review') {
    const { task } = await call('POST', `/api/tasks/${number}/review`, {})
    return {
      data: task,
      text: `T-${number} goes to an independent reviewer; the verdict arrives in your inbox.`,
    }
  }
  if (['done', 'accept', 'cancel', 'reopen'].includes(action)) {
    const text = rest.slice(1).join(' ')
    if ((action === 'done' || action === 'reopen') && text.trim().length === 0) {
      throw usage(
        `cf task ${action} T-${number} "${action === 'done' ? 'your result' : 'the follow-up'}"`,
      )
    }
    const { task } = await call(
      'POST',
      `/api/tasks/${number}/${action}`,
      text ? { body: text } : {},
    )
    return { data: task, text: taskLine(task) }
  }
  throw usage(
    `unknown task command ${JSON.stringify(action)}: use add, list, get, done, review, accept, reopen or cancel`,
  )
}

const ADD_USAGE = 'cf task add --tier <critical|complex|standard|light> "what to do" (or --self)'

/** `--tags coding,rust` as the list the core takes. */
const tags = (value) =>
  String(value)
    .split(',')
    .map((tag) => tag.trim())
    .filter(Boolean)

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
  `T-${task.number} [${task.state}] ${task.assignee === null ? `for a ${task.tier} ${task.pool}` : `@${task.assignee}`} ← @${task.requester}: ${task.title}`
const messageLine = (message) =>
  `m-${message.id} [${message.state}] ${message.kind}${(message.task ?? message.taskNumber) ? ` T-${message.task ?? message.taskNumber}` : ''} from ${message.sender === null ? 'ConsensFlow' : `@${message.sender}`}${message.preview === undefined ? '' : `: ${message.preview}`}`
