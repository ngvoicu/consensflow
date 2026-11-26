import { button, element, icon, iconButton, redraw } from '../dom.js'
import { notePanes } from './computer-use.js'
import { preview, render } from './markdown.js'
import { aPool, roleIds, roleLabel, takesNoTier } from './roles.js'

/**
 * The board: a kanban of the project's tasks. One row per participant, one
 * column per state, every task a card that stays where it ended, with its
 * result on it. A review or a test is a task like any other, on its reviewer's
 * or its tester's row. A task no member has (never given to one, or of one who
 * left the staff) is on the row of whoever asked for it, so none goes unseen.
 * Above the grid, what waits for the human: messages to approve when the
 * project asks for approval, notes to read, and, while the chief waits for
 * the human in its terminal, the way there. The human is asked nothing here:
 * the chief asks in its terminal, and what is for the chief stays its own.
 *
 * The rows are grouped: the chief's alone first, then one group a role, each
 * folded until the human opens it, and folded, it says what of it needs them.
 * A member and its backup are one row, a team, with one lamp and one status line;
 * unfolded, the row says what each of the two is doing, under it.
 *
 * Everything here is drawn from the daemon's state with `textContent`, never
 * markup, so an agent-written title cannot become HTML. Actions go out through
 * the callbacks; the controller calls the daemon and redraws.
 */

const ACTIVE = ['working', 'waiting', 'queued', 'paused', 'open']
/** What the chief (or the human) may stop: a task on the board or in a window. */
const PAUSABLE = ['open', 'queued', 'working', 'waiting']
/** What the human may give back to the board for another member of its tier. */
const REASSIGNABLE = ['queued', 'working', 'waiting', 'paused']
/** The states in which the human may take what a worker said last as the task's result. */
const FINISHABLE = ['working', 'waiting', 'paused']
/** What is over, accepted or not: the human may delete it from the board. */
const FINISHED = ['accepted', 'failed', 'cancelled']
/** The columns, in reading order; whatever is over, accepted or not, shares the last one. */
const COLUMNS = [
  ['open', 'Backlog'],
  ['queued', 'Queued'],
  ['working', 'Working'],
  ['waiting', 'Waiting'],
  ['done', 'Done'],
  ['finished', 'Finished'],
]
/**
 * A session's tools, on its row and its card in the dock, and a crowded
 * cell's stack, each drawn as an icon: path data on a 24-unit grid.
 */
export const ICONS = {
  show: ['M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12Z', 'M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0Z'],
  hide: [
    'M3 3l18 18',
    'M10.6 5.1A10.4 10.4 0 0 1 12 5c6.4 0 10 7 10 7a17.6 17.6 0 0 1-2.2 3.2',
    'M6.6 6.6C3.8 8.5 2 12 2 12s3.6 7 10 7a9.7 9.7 0 0 0 5.4-1.6',
    'M9.9 9.9a3 3 0 0 0 4.2 4.2',
  ],
  remove: ['M4 7h16', 'M9 7V4h6v3', 'M6 7l1 13h10l1-13', 'M10 11v6', 'M14 11v6'],
  stack: [
    'M4 11h16a1 1 0 0 1 1 1v8a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1v-8a1 1 0 0 1 1-1Z',
    'M5 7.5h14',
    'M7 4h10',
  ],
  fold: ['M9 6l6 6-6 6'],
}
/** How many cards a cell shows: more are one tile, a stack and how many, whose cards open in a dialog. */
const CELL_HOLDS = 3
/**
 * The column a card sits in: its state's, but for a paused task. One in a
 * window waits in the queue for its resumption; one no member has (it was
 * paused in the backlog, or its member left the staff) stays in the backlog.
 */
export const columnOf = (task) =>
  task.questionPending
    ? 'working'
    : FINISHED.includes(task.state)
      ? 'finished'
      : task.state === 'paused'
        ? task.laneless
          ? 'open'
          : 'queued'
        : task.state
const STATE_LABEL = {
  open: 'Open',
  queued: 'Queued',
  working: 'Working',
  waiting: 'Waiting',
  paused: 'Paused',
  done: 'Done',
  accepted: 'Accepted',
  failed: 'Failed',
  cancelled: 'Cancelled',
}
/**
 * What a task that goes on in the background waits on, in a few words: the jobs its
 * worker started, which its harness wakes it for as each ends, and the agents it
 * started. A job that never ends keeps the task here until the chief or the human
 * finishes it.
 */
export const heldWords = ({ jobs, agents }) => {
  if (agents === 0) return jobs === 1 ? 'a background job' : `${jobs} background jobs`
  if (jobs === 0) return agents === 1 ? 'a background agent' : `${agents} background agents`
  return 'background jobs and agents'
}
/**
 * How a card says its state: a task that waits for an answer while its window
 * goes on working (it asked, and kept at it) is at work, with a question
 * pending, and sits in Working; one whose worker ended its turn with a job or an
 * agent running is at work, waiting on it, and sits in Working too.
 */
export const stateLabel = (task) =>
  task.questionPending
    ? 'Working, question pending'
    : task.held
      ? `Working, waiting on ${heldWords(task.held)}`
      : STATE_LABEL[task.state]
const ACTIVITY_LABEL = {
  working: 'Working',
  idle: 'Idle',
  waiting: 'Waiting for you',
  starting: 'Starting',
  closed: 'No window',
  unknown: 'Unknown',
  out: 'Out of quota',
}
/**
 * What a task waiting to be tried again says (D3's N35): its turn failed, and ConsensFlow's own
 * Resume tries it again in its window. Held, it names the time and the try ("Trying again at
 * 14:02:31 (1 of 2)"; the drawer says "its turn failed: tries again at ..."); from that time
 * until its window has the words, that it waits for its window. `drawer` is the drawer's line.
 */
const retryWords = (retry, drawer = false, now = Date.now(), why = null) => {
  const count = ` (${retry.which} of ${retry.of})`
  const due = retry.dueAt ? new Date(retry.dueAt).getTime() : null
  if (due !== null && due > now) {
    const at = new Date(due).toLocaleTimeString([], {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      hour12: false,
    })
    return drawer ? `its turn failed: tries again at ${at}${count}` : `Trying again at ${at}${count}`
  }
  // From its time on, what it waits for is the real state: the window, or the project's
  // resume, or its member's return from quota.
  const waits = why ?? 'its window'
  return drawer
    ? `its turn failed: trying again, waiting for ${waits}${count}`
    : `Trying again, waiting for ${waits}${count}`
}
/** What a retry past its time waits for, from the board's own state: null where it is its window. */
const retryWaitsFor = (closed, participant, now = Date.now()) => {
  if (closed) return "the project's resume"
  if (participant && outOfQuota(participant, now)) return 'its member to be back'
  return null
}
/** How a message's kind reads. */
const KIND_LABEL = {
  task: 'Task',
  result: 'Result',
  question: 'Question',
  answer: 'Answer',
  note: 'Note',
}

/** "just now", or "4m ago": how long ago something last changed. */
const ago = (iso, now) => {
  const since = age(iso, now)
  return since === 'just now' ? since : `${since} ago`
}

/** "just now", "4m", "2h", "3d": how long something has been in its state. */
function age(iso, now = Date.now()) {
  const seconds = Math.max(0, Math.round((now - Date.parse(iso)) / 1000))
  if (!Number.isFinite(seconds) || seconds < 45) return 'just now'
  const minutes = Math.round(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 36) return `${hours}h`
  return `${Math.round(hours / 24)}d`
}

/** How long a task has been in its state, without rounding up to the next unit. */
function stateAge(iso, now = Date.now()) {
  const seconds = Math.max(0, Math.floor((now - Date.parse(iso)) / 1000))
  if (!Number.isFinite(seconds)) return '—'
  if (seconds < 60) return `${seconds}s`
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ${String(minutes % 60).padStart(2, '0')}m`
  return `${Math.floor(hours / 24)}d ${hours % 24}h`
}

/**
 * A member of the staff, as the ledger counts one: a participant on a harness,
 * neither one of a member's sessions, nor the chief, nor the human.
 */
export const isMember = (participant) =>
  participant.harness !== null &&
  participant.member === null &&
  participant.role !== 'chief' &&
  participant.role !== 'human'

/** The role a member's task is for: its pool's. */
const roleOf = (task, roles) => task.pool ?? roles[0]

/**
 * A row's tasks, the newest first: its own, and those it asked for that no
 * lane has (`board.open`: waiting for a member, paused, called off or failed
 * before one had it, or of a member who left the staff), each `laneless` and
 * in the column of its state. A task of its own that waits while its window
 * is at work has a question pending, and says so: the board keeps it
 * `waiting`, and the window's activity is what tells the two apart. A working
 * task that its lane says is held (its worker ended its turn with a job or an
 * agent running) carries what it waits on, and says so, though its window is idle.
 */
export const rowTasks = (lane, board) =>
  [
    ...lane.tasks.map((task) => {
      let shown = task
      if (task.state === 'waiting' && lane.activity?.state === 'working') {
        shown = { ...shown, questionPending: true }
      } else if (task.state === 'working' && lane.held?.task === task.number) {
        shown = { ...shown, held: lane.held }
      }
      if (lane.resumeWait?.task === task.number) {
        shown = { ...shown, resumeWait: lane.resumeWait }
      }
      return shown
    }),
    ...board.open
      .filter((task) => task.requester === lane.participant.handle)
      .map((task) => ({ ...task, laneless: true })),
  ].sort((a, b) => b.number - a.number)

/**
 * The cards a row holds, the newest first: a team's are its member's and its backup's, and a
 * folded member's row holds its sessions' too, so folding takes rows off the board and no work
 * with them.
 */
function rowCards(row, board, folded) {
  return [row, ...(row.backup ? [row.backup] : []), ...(folded ? (row.sessions ?? []) : [])]
    .flatMap((lane) => rowTasks(lane, board))
    .sort((a, b) => b.number - a.number)
}

/** What keeps a member's row unfolded on a board: its project, member and role. */
const foldKey = (board, { handle, role }) => `${board.project.id}:${handle}:${role}`

/**
 * The board's rows: a member with several roles heads one row per role, each
 * holding that role's cards and, in `sessions`, that role's sessions, so a
 * worker and a reviewer read as two things; everything else in lane order. The
 * human has no row: nothing assigns them a task, and what is for them is in For
 * you. The chief has one only while a task is on it, its own or one it asked
 * for: what it is doing is on its card in the dock.
 *
 * A member and its backup are one member on the board, a team (the owner, 2026-11-18: "they
 * should behave like a team; so same status same way just one member"): the backup has no row
 * of its own. It is the row's `backup`, its lane for the row's role, whose roles and tier follow
 * its member's; its tasks are the row's, and its sessions are with the member's under the row,
 * each marked `ofBackup`.
 */
function boardRows(board) {
  const ordered = laneOrder(board.lanes).filter(
    (lane) =>
      lane.participant.role !== 'human' &&
      (lane.participant.role !== 'chief' || rowTasks(lane, board).length > 0),
  )
  const members = new Set(
    ordered.filter((lane) => isMember(lane.participant)).map((lane) => lane.participant.handle),
  )
  // Each member's backup, by the member's id, where both are on the board.
  const backups = new Map(
    ordered
      .filter(
        ({ participant }) =>
          isMember(participant) &&
          ordered.some(
            (owner) => owner.participant.id === participant.backupOf && isMember(owner.participant),
          ),
      )
      .map((lane) => [lane.participant.backupOf, lane]),
  )
  const teamed = new Set([...backups.values()].map((lane) => lane.participant.handle))
  const rows = []
  for (const lane of ordered) {
    const { participant } = lane
    // A session is placed under its member's row for its role.
    if (participant.member !== null && members.has(participant.member)) continue
    if (teamed.has(participant.handle)) continue
    if (!members.has(participant.handle)) {
      rows.push(lane)
      continue
    }
    const backup = backups.get(participant.id) ?? null
    const sessions = ordered
      .filter(
        (other) =>
          other.participant.member === participant.handle ||
          (backup !== null && other.participant.member === backup.participant.handle),
      )
      .map((session) =>
        session.participant.member === participant.handle
          ? session
          : { ...session, participant: { ...session.participant, ofBackup: true } },
      )
    const roles = participant.roles.length > 0 ? participant.roles : [participant.role]
    for (const role of roles) {
      const row = {
        ...lane,
        participant: { ...participant, role, roles: [role], also: role !== roles[0] },
        tasks: lane.tasks.filter((task) => roleOf(task, roles) === role),
        sessions: sessions.filter((session) => session.participant.role === role),
      }
      if (backup !== null) {
        row.backup = {
          ...backup,
          participant: { ...backup.participant, role, roles: [role] },
          tasks: backup.tasks.filter((task) => roleOf(task, roles) === role),
        }
      }
      rows.push(row)
    }
    // A session of a role the member no longer holds still shows, last, on its own.
    rows.push(...sessions.filter((session) => !roles.includes(session.participant.role)))
  }
  return rows
}

/**
 * A team's parts, the member first; a member with no backup is a team of one, and any other
 * row is its own.
 */
const partsOf = (row) => [row, ...(row.backup ? [row.backup] : [])]

/** Whether a part of a team can take work now: not out, not waiting for an edit, not resting. */
const canWork = ({ participant }, now) =>
  !outOfQuota(participant, now) && !participant.needsEdit && !participant.vacationSince

/**
 * When a part that cannot run work is back by itself, to order them: its reset, and never for
 * one that waits for an edit or for the human's word (its account, its moved model).
 */
const backBy = ({ participant }, now) =>
  participant.needsEdit || unavailable(participant) || !outOfQuota(participant, now)
    ? Number.POSITIVE_INFINITY
    : Date.parse(participant.outUntil)

/**
 * The part of a team that speaks for it, on its lamp and its status line: the member while it
 * can take work, else its backup while that can; a member on Vacation, for the team rests with
 * it (the engine's rule); with neither, the one back first, for then the team is back. So a team
 * is at work when either is, free when either can take a task, and out only when neither can
 * run work. A member with no backup speaks for itself.
 */
function teamLead(row, now) {
  const parts = partsOf(row)
  const working = parts.find((part) => canWork(part, now))
  if (working !== undefined) return working
  if (row.participant.vacationSince) return row
  const held = parts.filter(
    ({ participant }) => outOfQuota(participant, now) || participant.needsEdit,
  )
  if (held.length === 0) return row
  return held.reduce((first, part) => (backBy(part, now) < backBy(first, now) ? part : first))
}

/**
 * What a row says it is doing, and the state that colours it: a team's, said by the part that
 * speaks for it, with every terminal of the team counted; any other row's own.
 */
function rowStatus(row, board, now) {
  if (!row.backup) return laneStatus(row, board, now)
  const lead = teamLead(row, now)
  // The team's own line says nothing of who takes whose tasks: each part says what it does.
  return laneStatus({ ...lead, backupReady: undefined, team: partsOf(row) }, board, now)
}

/**
 * A member's lane's status line, `said`, less what the words under its name (`words`, from
 * `partState`) say already (T-1855, after T-1850: "Waiting for you" over "Waiting: permission
 * to run a command"): its window at work, waiting or between tasks, and the edit it needs. The
 * words say the same as the line where their state is the same; a member on Vacation whose
 * window still waits says On Vacation in its words, and the line says what the window waits for.
 */
const IN_ITS_WORDS = ['working', 'waiting', 'idle', 'missing']
const besideWords = (said, [state]) =>
  said !== null && IN_ITS_WORDS.includes(said[0]) && said[0] === state ? null : said

/**
 * A team's one status line (T-1847): the team's, as `rowStatus` says it, but where neither part
 * can run work, only when the first is back, for each part says under the lane why it is out;
 * and nothing the part that speaks for it says in its own words.
 */
function teamStatus(row, board, now) {
  const said = besideWords(rowStatus(row, board, now), partState(teamLead(row, now), row, now))
  if (said?.[0] !== 'out') return said
  const lead = teamLead(row, now)
  const back = backBy(lead, now)
  const words = Number.isFinite(back)
    ? `Neither can take a task until ${backAt(lead.participant.outUntil, now)}`
    : 'Neither can take a task now'
  return ['out', lead.holding ? `${words}. ${HOLDING}` : words]
}

/**
 * A member's lane with no backup, its one status line (T-1850): its own, as `laneStatus` says
 * it, but not why it cannot run work, which its own words under its name say, with its Try
 * again now beside them, as a team's member's do; nor anything else they say.
 */
function soloStatus(lane, board, now) {
  const said = besideWords(laneStatus(lane, board, now), partState(lane, lane, now))
  if (said?.[0] !== 'out') return said
  return lane.holding ? ['holding', HOLDING] : null
}

/**
 * What one part of a team does, in a few words, and the state that colours its lamp and its
 * words (T-1847, the owner on alpha.11: "you have to expand to see the backup"): out, and until
 * when; held, refused or waiting for an edit; on Vacation; else what its windows of the row's
 * role do: at work, or waiting for the human; else the member is ready for a task, and its
 * backup stands by, or takes its tasks while the member cannot. So a folded lane says which of
 * the two works and which is out.
 */
function partState(part, row, now) {
  const { participant } = part
  if (participant.vacationSince) return ['closed', 'On Vacation']
  if (participant.needsEdit) return ['missing', NEEDS_EDIT]
  if (outOfQuota(participant, now)) return ['out', outShort(part, now)]
  // A member on Vacation rests its team: its backup takes none of its tasks (the engine's rule).
  if (part !== row && row.participant.vacationSince) return ['closed', 'Rests with its member']
  const windows = [
    part,
    ...(row.sessions ?? []).filter((session) => session.participant.member === participant.handle),
  ].map((lane) => lane.activity ?? {})
  if (windows.some(({ state }) => state === 'working')) return ['working', 'Working']
  // What its window waits for, where the window says (T-1855).
  const waiting = windows.find(({ state }) => state === 'waiting')
  if (waiting) return ['waiting', waiting.reason ? `Waiting: ${waiting.reason}` : 'Waiting for you']
  if (part === row) return ['idle', 'Ready for a task']
  // A new task goes to the member, never straight to its backup: the backup stands by while
  // its member can work, and takes the member's tasks while it cannot.
  return canWork(row, now) ? ['closed', 'Standing by'] : ['idle', "Takes its member's tasks"]
}

/** The most characters of a held member's words the chief's card says; the lane says them all. */
const CARD_WORDS = 140

/**
 * `words` as the chief's card says them: the first line, cut at a word where it is long, and
 * saying so. The words in full stay in the member's lane (`outWords` with no cut).
 */
function cardWords(words) {
  const line = words.split('\n', 1)[0].trim()
  if (line.length <= CARD_WORDS) return line
  const room = line.slice(0, CARD_WORDS)
  const space = room.lastIndexOf(' ')
  return `${(space > CARD_WORDS / 2 ? room.slice(0, space) : room).trimEnd()}…`
}

/**
 * What a held member's harness said, as the hold keeps it (`outBecause`, which says which of
 * billing or login was refused, and the harness's own words), or else as its window's activity
 * says it under `prefix`: a hold an older build made keeps no words, and a window can be gone.
 */
function heldSaid(participant, activity, cut = false) {
  return (prefix) => {
    const words =
      participant.outBecause ||
      (activity?.reason?.startsWith(prefix) ? activity.reason.slice(prefix.length) : null)
    return words !== null && cut ? cardWords(words) : words
  }
}

/**
 * Why a part of a team cannot run work, short: its words in full are `outWords`, which a lane
 * of its own says; in a team the way back is the button beside them.
 */
function outShort({ participant, activity, busy }, now) {
  if (typeof busy === 'string') return `Model busy: tried again at ${backAt(busy, now)}`
  const said = heldSaid(participant, activity)
  if (modelMoved(participant)) {
    return `Held: ${said('moved: ') ?? 'its model moved on its own'}`
  }
  if (unavailable(participant)) {
    return `Unavailable: ${said('unavailable: ') ?? 'its account was refused for billing or login'}`
  }
  return `Out of quota until ${backAt(participant.outUntil, now)}`
}

/**
 * The board's rows in their groups: the chief's alone first (it has one only
 * while a task is on it), then one group a role, in the roles' order, each with
 * its rows in lane order: a member with two roles is in both groups, its row of
 * each role there with that role's sessions under it. A role the table does not
 * name comes last, by its id.
 */
function boardGroups(board) {
  const rows = boardRows(board)
  const order = roleIds()
  const rank = (role) => (order.includes(role) ? order.indexOf(role) : order.length)
  const groups = new Map()
  for (const row of rows) {
    const { role } = row.participant
    if (role === 'chief') continue
    if (!groups.has(role)) groups.set(role, [])
    groups.get(role).push(row)
  }
  return {
    chief: rows.filter((row) => row.participant.role === 'chief'),
    groups: [...groups]
      .sort(([a], [b]) => rank(a) - rank(b) || a.localeCompare(b))
      .map(([role, members]) => ({ role, rows: members })),
  }
}

/** A group's name: "Workers", "Frontend designers". */
const groupName = (role) => `${roleLabel(role) ?? role}s`

/** What this browser keeps a project's open groups under. */
const groupsKey = (projectId) => `cf.board.groups.${projectId}`

/**
 * The groups open on project `projectId` in this browser, by role: none kept, or no
 * storage, is none, every group folded. A per-viewer convenience, so every touch is guarded.
 */
function keptGroups(projectId) {
  try {
    const kept = JSON.parse(localStorage.getItem(groupsKey(projectId)) ?? '[]')
    return new Set(Array.isArray(kept) ? kept.filter((role) => typeof role === 'string') : [])
  } catch {
    return new Set()
  }
}

/** Keeps the groups open on project `projectId`; with no storage they hold for this page alone. */
function keepGroups(projectId, open) {
  try {
    localStorage.setItem(groupsKey(projectId), JSON.stringify([...open]))
  } catch {
    // No storage: the groups stay as they are until the page is reloaded.
  }
}

/** What a group's head says folded, in the order it says it, and the words for each. */
const MARKS = [
  ['working', (count) => `${count} working`],
  ['held', (count) => `${count} waiting on background work`],
  ['waiting', (count) => `${count} waiting for you`],
  ['question', (count) => `${plural(count, 'question')} waiting`],
  ['paused', (count) => `${count} paused, to resume`],
  ['done', (count) => `${count} done, to decide`],
  ['failed', (count) => `${count} failed`],
  ['out', (count) => `${count} out of quota`],
  ['attention', (count) => `${count} ${count === 1 ? 'needs' : 'need'} you`],
]

/**
 * What a folded group holds that the human would look for, so nothing that needs
 * them hides in it: its windows at work, those whose task goes on in the background
 * while they rest (a server that never ends waits for a Finish), and those waiting for
 * them; its tasks waiting on a question, paused whatever held them (a window that
 * closed, a pause, a window a Resume could not bring back, a quota, a model the human
 * must let go on: a session of a role its member no longer has shows no "out of
 * quota", so its paused mark is all its group says), done and waiting for a decision,
 * or failed; its members out of quota, and its rows whose status asks for the human
 * (a member to edit, a window that did not stop, a message held by what they typed).
 * Each with how many; none that is none.
 */
function groupMarks(rows, board, now) {
  const sessions = rows.flatMap((row) => row.sessions ?? [])
  const lanes = [...rows.flatMap(partsOf), ...sessions]
  const tasks = rows.flatMap((row) => rowCards(row, board, true))
  const tasksIn = (state) =>
    new Set(tasks.filter((task) => task.state === state).map((task) => task.number)).size
  // A team says what the team's line says: its member out while its backup works is not out.
  const statuses = [
    ...rows.map((row) => rowStatus(row, board, now)?.[0]),
    ...sessions.map((lane) => laneStatus(lane, board, now)?.[0]),
  ]
  const out = new Set(
    [...rows.map((row) => teamLead(row, now)), ...sessions]
      .filter((lane) => outOfQuota(lane.participant, now))
      .map((lane) => lane.participant.handle),
  )
  const windows = (state) =>
    lanes.filter((lane) => !outOfQuota(lane.participant, now) && lane.activity?.state === state)
      .length
  const counts = {
    working: windows('working'),
    held: statuses.filter((status) => status === 'held').length,
    waiting: windows('waiting'),
    question: tasksIn('waiting'),
    paused: tasksIn('paused'),
    done: tasksIn('done'),
    failed: tasksIn('failed'),
    out: out.size,
    attention: statuses.filter((status) => ['missing', 'unstopped', 'holding'].includes(status))
      .length,
  }
  return MARKS.filter(([mark]) => counts[mark] > 0).map(([mark, words]) => [
    mark,
    words(counts[mark]),
  ])
}

/** The human and the chief first, then each member with its sessions right under it. */
export function laneOrder(lanes) {
  const rank = ({ participant }) => [
    participant.role === 'chief' ? 0 : 1,
    participant.memberId ?? participant.id,
    participant.memberId === null ? 0 : participant.id,
  ]
  return [...lanes].sort((a, b) => {
    const [x, y] = [rank(a), rank(b)]
    return x[0] - y[0] || x[1] - y[1] || x[2] - y[2]
  })
}

const who = (handle) => (handle === null || handle === undefined ? 'ConsensFlow' : `@${handle}`)
/** "18:30": a time of day. */
const clock = (iso) =>
  new Date(iso).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false })
/**
 * When a member out of quota is back: "18:30" today, "Tue 11:00" within the
 * week, "Oct 13, 11:00" further off. A weekly limit's reset read "11:00"
 * two days early, as if already past (poker-lab, 2026-10-04).
 */
const backAt = (iso, now = Date.now()) => {
  const at = new Date(iso)
  const time = clock(iso)
  const today = new Date(now)
  if (at.toDateString() === today.toDateString()) return time
  if (at.getTime() - today.getTime() < 6 * 86_400_000) {
    return `${at.toLocaleDateString([], { weekday: 'short' })} ${time}`
  }
  return `${at.toLocaleDateString([], { month: 'short', day: 'numeric' })}, ${time}`
}
const outOfQuota = (participant, now) =>
  participant.outUntil !== null && Date.parse(participant.outUntil) > now
/**
 * A member out until the year 9999 is held for no time: its account refused for what no
 * wait cures (its billing or its login), or its harness moved its window's model on its own.
 * Nothing resets either, so no time is shown, only the human's way back. The daemon marks
 * the second with the later of its two times (`scheduler.rs`: `MOVED`, `NEVER`), which is
 * all the ledger keeps of why, across a restart.
 */
const MODEL_MOVED = '9999-12-31T23:59:59.999Z'
/** What a member the update could not run on what it ran on (T-1202) says, and its ways out. */
const NEEDS_EDIT =
  'Needs an edit: the update could not tell what it ran on. Edit it under Project settings, or remove it'
const unavailable = (participant) => participant.outUntil?.startsWith('9999-') === true
const modelMoved = (participant) => participant.outUntil === MODEL_MOVED
/** "@zeus · amber-pine" for a session; the member's own name otherwise. */
export const laneName = (participant) =>
  participant.member
    ? `@${participant.member} · ${participant.session}`
    : ({ human: 'You', chief: 'Chief of Staff' }[participant.handle] ?? `@${participant.handle}`)

/** "1 note", "3 notes": how many of something. */
const plural = (count, noun) => `${count} ${noun}${count === 1 ? '' : 's'}`

/**
 * A member's row: how many of its sessions' terminals (of the row's role) are open now, a team's
 * (`team`, its parts) all of its parts'; null for none.
 */
function sessionsNote(lane, board) {
  const handles = (lane.team ?? [lane]).map((part) => part.participant.handle)
  const open = board.lanes.filter(
    (other) =>
      handles.includes(other.participant.member) &&
      other.participant.role === lane.participant.role &&
      other.pane !== null,
  ).length
  return open === 0 ? null : `${plural(open, 'terminal')} open, one per task`
}

/** "T-3, T-4": task numbers in a sentence. */
const taskNumbers = (numbers) => numbers.map((number) => `T-${number}`).join(', ')

/**
 * Where a task is going or came from, on its card; what it waits for first. A task held until
 * a time is held for a quota, an account or a busy model, which look alike by their time and
 * are three things (T-1357): `busy` is until when the lane's member is out for its busy model,
 * where it is.
 */
function route(task, busy = null) {
  if (task.assignee === null) {
    const waitsFor = `for ${aPool(task)}`
    return task.blockedBy.length === 0
      ? waitsFor
      : `blocked by ${taskNumbers(task.blockedBy)} · ${waitsFor}`
  }
  if (task.state === 'paused' && task.heldUntil) {
    const busyHold = busy !== null && Date.parse(task.heldUntil) <= Date.parse(busy)
    const until =
      task.heldUntil === MODEL_MOVED
        ? 'held until you say its model may go on'
        : task.heldUntil.startsWith('9999-')
          ? 'unavailable until you say its login or billing works again: Try its member again now'
          : busyHold
            ? `model busy, tried again at ${backAt(task.heldUntil)}`
            : `out of quota until ${backAt(task.heldUntil)}; renewed early? Try its member again now`
    return `${until} · from ${who(task.requester)}`
  }
  return task.blockedBy.length === 0
    ? `from ${who(task.requester)}`
    : `blocked by ${taskNumbers(task.blockedBy)} · from ${who(task.requester)}`
}

/** What a lane says while what the human typed and did not send holds a message (T-288). */
const HOLDING = 'A message waits until you send or erase what you typed here'

/** A member between tasks: its work runs in sessions, so it has no window of its own. */
const resting = (participant, activity) =>
  isMember(participant) && (activity?.state ?? 'closed') === 'closed'

/** What a team's backup runs on, in its detail: its harness, and its model and effort where its name does not say them. */
const runsOn = (participant) =>
  [
    harnessLabel(participant),
    ...(nameApart(participant) ? [modelShown(participant), participant.effort] : []),
  ]
    .filter(Boolean)
    .join(' · ')

/** The harnesses a chief runs in, as the human knows them, in the order they are offered. */
export const HARNESS_NAMES = {
  'claude-code': 'Claude Code',
  codex: 'Codex',
  pi: 'Pi',
  devin: 'Devin',
  muse: 'Muse Code',
}

/**
 * What a member's model is called (harness-models, HMD-12): its id as its harness's list
 * names it (`claude-opus-5-5`; Pi's `provider/id`; Devin's listed id), else its id as the
 * harness takes it, else its harness's default (the image designer from before, on Codex's).
 */
export const modelShown = (participant) =>
  (participant.needsEdit && 'no model: needs an edit') ||
  participant.modelName?.trim() ||
  participant.model ||
  `${HARNESS_NAMES[participant.harness] ?? participant.harness} default`

/**
 * The model the chief runs on, as its card says it (O7): the one its window last ran on
 * where the human switched it there, else the one picked for it, said as every card says a
 * model (`modelShown`): the id its harness lists (Devin's `swe-2`), not the one its launch
 * passes (`swe-2-high`). A chief left to its harness's model says none. Display only: the
 * launch and Switch chief take the chief's `model`.
 */
const chiefModel = (participant) =>
  participant.needsEdit
    ? 'no model: needs an edit'
    : (participant.ranOn ?? (participant.model ? modelShown(participant) : null))

/** A harness as the human knows it: `claude-code` is Claude Code. */
export const harnessLabel = (participant) =>
  HARNESS_NAMES[participant.harness] ?? participant.harness

/**
 * Whether a member's name, or a session's, says nothing of the model and effort it runs on
 * (the daemon says so, `nameApart`): a designer on its harness's default, a member the update
 * could not tell the model of. Where it does, its card says them no more than the name does.
 */
export const nameApart = (participant) => participant.nameApart === true || participant.needsEdit

/**
 * What a row, or the chief's card in the dock, says its participant is, in order, each part
 * with what it is (`kind`): a session's row says whose window it is (a backup's says so, for a
 * member and its backup are one row); the member's row says
 * what it is, its roles, its tier and its harness, and what it runs on only where its name
 * does not say it (HMD-12: `@claude-opus-5-5-high` is the model and the effort). The chief's
 * card in the dock says its harness and the model it runs on.
 */
const identityParts = (participant, inLane = false) => {
  const part = (kind, text) => (text ? { kind, text } : null)
  const runs = nameApart(participant)
    ? [part('model', modelShown(participant)), part('effort', participant.effort)]
    : []
  const parts = participant.member
    ? [
        part(
          'role',
          // Under its team's lane a backup's session is marked as the backup's (T-1847); a
          // terminal's card, with no lane around it, says whose.
          participant.ofBackup && inLane
            ? `${participant.role} session`
            : `${participant.role} session of @${participant.member}${participant.ofBackup ? ', the backup' : ''}`,
        ),
        part('harness', harnessLabel(participant)),
        ...runs,
      ]
    : participant.role === 'chief'
      ? [
          part('harness', HARNESS_NAMES[participant.harness] ?? participant.harness),
          part('model', chiefModel(participant)),
        ]
      : [
          part('role', participant.roles.join('+')),
          // A member whose roles have no tier has none (an image designer's: a design task
          // has none, designer P2).
          part('tier', takesNoTier(participant.roles) ? null : participant.tier),
          part('harness', harnessLabel(participant)),
          ...runs,
        ]
  return parts.filter(Boolean)
}

/** The same, as one line of words. */
export const identity = (participant) =>
  identityParts(participant)
    .map((part) => part.text)
    .join(' · ')

/**
 * The same, drawn for a row: each part a span of its own, so the role reads first and the tier
 * has its mark, and the words between them the line's (`textContent` is [`identity`]).
 */
function identityLine(participant, className) {
  const line = element('span', className)
  identityParts(participant, true).forEach((part, at) => {
    if (at > 0) line.append(' · ')
    const span = element('span', `meta-${part.kind}`, part.text)
    if (part.kind === 'tier') span.dataset.tier = part.text
    line.append(span)
  })
  return line
}

/**
 * What a participant out of quota is waiting for, or null when it is not out: why, in the
 * engine's own words (its account refused, its model moved on its own), where it said them;
 * its model busy for a moment, which the lane says (`busy`), with no hope of a press; else what
 * the time it marked the member out until says of it, and that the human may have renewed the
 * quota early (T-1357). Said on its row, the chief's card and the staff list; with `cut` the
 * harness's words are cut as the chief's card says them (`cardWords`).
 */
export function outWords({ participant, activity, busy }, now, cut = false) {
  if (!outOfQuota(participant, now)) return null
  // Its model is busy for a moment: tried again after a back-off of ConsensFlow's own, which
  // no press shortens, so no renewal is hoped for and no button is offered (T-1357).
  if (typeof busy === 'string') {
    return `Its model is busy: ConsensFlow tries it again at ${backAt(busy, now)}`
  }
  const said = heldSaid(participant, activity, cut)
  if (modelMoved(participant)) {
    return `Held: ${said('moved: ') ?? 'its model moved on its own'}. Press Try again now once it is on the model you chose`
  }
  if (unavailable(participant)) {
    return `Unavailable: ${said('unavailable: ') ?? 'its account was refused for billing or login'}. Logged in again, or paid? Press Try again now`
  }
  return `Out of quota until ${backAt(participant.outUntil, now)}; renewed early? Try again now`
}

/**
 * What a row, or the chief's card in the dock, says its participant is
 * doing, and the state that colours it: a chief being switched or a member out of
 * quota say so before anything its window does. A member with no terminal open says nothing (null): its row has no
 * status line.
 */
export function laneStatus(lane, board, now) {
  const { participant, activity } = lane
  if (lane.switching) {
    const { harness, model } = lane.switching
    return [
      'switching',
      `Switching the chief to ${HARNESS_NAMES[harness] ?? harness} (${model}) after this turn`,
    ]
  }
  if (
    participant.role === 'chief' &&
    activity?.state === 'closed' &&
    activity.reason?.startsWith("The chief's window could not be opened: ")
  ) {
    const reason = /[.!?]$/.test(activity.reason) ? activity.reason : `${activity.reason}.`
    return ['closed', `${reason} Close then Resume the project to try again.`]
  }
  // A meeting with the human: what comes for the chief waits until its Stop.
  if (participant.role === 'chief' && board.project?.meeting) {
    const held = board.held?.length ?? 0
    return [
      'meeting',
      `In a meeting · ${held === 0 ? 'nothing held yet' : `${plural(held, 'message')} held`}`,
    ]
  }
  // A stop its window ignored in every round: the human can cancel the task to end it.
  if (lane.unstopped) {
    return ['unstopped', `Did not stop for T-${lane.unstopped.task}: still on its earlier turn`]
  }
  // What the human typed and did not send holds a message whether or not the window
  // is out of quota: the lane says both, or the hold hides behind the wait (T-288).
  const holding = HOLDING
  // A chief the update could not tell the model of (T-1202), whose harness's record named
  // none: its window stays closed until a model is picked for it, and nothing is lost.
  if (participant.needsEdit && participant.role === 'chief') {
    return [
      'missing',
      "Needs a model: ConsensFlow couldn't tell which model this chief used before the update. Pick one with Switch chief; what was written to the chief waits until then",
    ]
  }
  // A member the update could not run on what it ran on (T-1202) runs nothing until edited.
  if (participant.needsEdit && participant.role !== 'chief') return ['missing', NEEDS_EDIT]
  const out = outWords(lane, now, true)
  if (out !== null) {
    const backup = lane.backupReady?.[participant.role]
    const covered = backup ? `${out}; @${backup}, its backup, takes its tasks` : out
    return ['out', lane.holding ? `${covered}. ${holding}` : covered]
  }
  if (lane.holding) return ['holding', holding]
  const state = activity?.state ?? 'closed'
  if (state === 'waiting' && activity.reason) return [state, `Waiting: ${activity.reason}`]
  // Its task goes on in the background: the window rests, and that is not idleness.
  if (lane.held && state === 'idle') return ['held', `Waiting on ${heldWords(lane.held)}`]
  if (resting(participant, activity)) {
    const note = sessionsNote(lane, board)
    return note === null ? null : [state, note]
  }
  // A closed session's row says nothing: its lamp and its Show terminal say it.
  if (participant.member !== null && state === 'closed') return null
  return [state, ACTIVITY_LABEL[state] ?? 'No window']
}

/**
 * The human's way to try a member, or the chief, out of quota again before
 * its reset: its harness runs on another account now, or a bigger plan.
 */
export function tryAgain(participant, action) {
  const label = modelMoved(participant)
    ? `Try ${laneName(participant)} again now, once it is on the model you chose`
    : unavailable(participant)
      ? `Try ${laneName(participant)} again now, once its account works`
      : `Try ${laneName(participant)} again now, before its quota resets`
  const node = button('Try again now', 'quiet-button try-again', action, label)
  // Its tip says when to press, which a member's short words on the board leave out (T-1850).
  node.title = label
  return node
}

/** The same Vacation control on the board and in the staff dialogs. */
export const vacationButton = (name, resting, action) =>
  button(
    resting ? 'Bring back' : 'Send on Vacation',
    'quiet-button',
    action,
    resting ? `Bring ${name} back from Vacation` : `Send ${name} on Vacation`,
  )

/** A team's part as its lamp shows it: the state its words say (`partState`). */
const partLamp = (part, row, now) => ({
  participant: part.participant,
  activity: { state: partState(part, row, now)[0] },
})

/** A participant's lamp: what its window is doing, at a glance, or that it is out of quota. */
export function lamp({ participant, activity }, now) {
  const node = element('span', 'lamp')
  node.dataset.testid = 'lamp'
  node.dataset.state = outOfQuota(participant, now) ? 'out' : (activity?.state ?? 'closed')
  node.setAttribute('aria-hidden', 'true')
  return node
}

/**
 * Whether anything on a board acts: a closed project's board reads (its
 * cards open, its messages and transcripts show) and has no control that
 * would change it, for the mouse or the keyboard.
 */
const acts = (board) => board.project.state === 'open'

/**
 * The finished tasks on a board as Delete finished takes them: those that
 * may go, and those kept, each with the tasks that need it. Deleting a task
 * only takes it off the board: what waits for it reads its state as before.
 * So an accepted task goes though others need it (it is accepted for good),
 * and so does a cancelled one (it never will be); a failed one is kept while
 * a task not yet finished needs it, for the chief can reopen it and then
 * accept it, and a deleted task is not reopened. The daemon decides again
 * when it deletes them.
 */
function finishedTasks(board) {
  const tasks = [...board.open, ...board.lanes.flatMap((lane) => lane.tasks)].sort(
    (a, b) => a.number - b.number,
  )
  const unfinished = tasks.filter((task) => !FINISHED.includes(task.state))
  const finished = tasks
    .filter((task) => FINISHED.includes(task.state))
    .map((task) => ({
      number: task.number,
      state: task.state,
      neededBy: unfinished
        .filter((other) => other.needs.some((need) => need.number === task.number))
        .map((other) => other.number),
    }))
  const kept = (task) => task.state === 'failed' && task.neededBy.length > 0
  return {
    deletable: finished.filter((task) => !kept(task)).map((task) => task.number),
    kept: finished.filter(kept),
  }
}

export class BoardView {
  #root
  #actions
  /** The board drawn last: a row's button kept across redraws acts on its lane as it is now. */
  #board = null
  /** Whether the human asked to see a session's terminal, which the dock keeps out until then. */
  #shows = null
  /** The dialog that lists a crowded cell's cards, its title and its list. */
  #stack
  #stackTitle
  #stackCards
  /** The cell the stack dialog was opened on: its project, its row's handle and role, its column. */
  #stacked = null
  /** The members' rows the human unfolded, by foldKey: every other is folded. */
  #unfolded = new Set()
  /** The groups open on each project, by its id, as this browser keeps them: every other is folded. */
  #openGroups = new Map()
  #vacationErrors = new Map()
  /** What the board was drawn from last, to draw it again when a row folds. */
  #drawn = null

  constructor(root, stack, actions) {
    this.#root = root
    this.#stack = stack
    this.#stackTitle = stack.querySelector('#stack-title')
    this.#stackCards = stack.querySelector('#stack-cards')
    this.#actions = actions
    // Update the clocks in place: a tick must not redraw a focused card or the board.
    setInterval(() => this.#tick(), 1000)
    // A card chosen opens its task, as on the board, and the dialog goes.
    this.#stackCards.addEventListener('click', (event) => {
      if (event.target.closest('.card') !== null) stack.close()
    })
  }

  /** Redraw from the daemon's state; `shows(participant)` says whose terminals the human asked to see. */
  render({ board, inbox, shows, now = Date.now() }) {
    this.#drawn = { board, inbox, shows }
    this.#board = board
    this.#shows = shows
    redraw(this.#root, [
      ...(acts(board) ? [] : [this.#closed(board.resume)]),
      this.#forYou(inbox, board, now),
      this.#kanban(board, now),
    ])
    // An open stack dialog follows the board, as the cell it lists does.
    if (this.#stack.open) this.#drawStack()
  }

  /** `participant`'s lane on the board drawn last, which a kept button acts on. */
  #lane(participant) {
    return this.#board.lanes.find((lane) => lane.participant.handle === participant.handle)
  }

  /** A row's action, on its participant as the board drawn last has it. */
  #onLane(action, participant) {
    return () => action(this.#lane(participant).participant)
  }

  /**
   * What a closed project shows above its board, in one line: that it waits
   * for Resume, what Resume will start (the tasks open and the messages
   * waiting, which the daemon counts), and the one way on.
   */
  #closed(resume) {
    const banner = element('section', 'suspended')
    banner.setAttribute('role', 'status')
    banner.append(
      element('strong', null, 'Waiting for Resume.'),
      element(
        'span',
        null,
        resume
          ? ` ${plural(resume.tasks, 'open task')}, ${plural(resume.messages, 'message')} waiting.`
          : '',
      ),
      button('Resume project', 'primary-button', () => this.#actions.onResume(this.#board.project)),
    )
    return banner
  }

  /** What waits for the human. */
  #forYou(inbox, board, now) {
    // What waits for the human's approval.
    const waiting = board.gated ?? []
    // What the human's meeting with the chief holds for it, in the order it will go: the
    // human reads it here, and it goes to the chief after the Stop.
    const held = board.held ?? []
    // The chief waits for the human only in its terminal: its own question,
    // or a prompt. Messages for the chief stay its own, however long it waits.
    const chief = board.lanes.find((lane) => lane.participant.role === 'chief')
    const asking = acts(board) && chief?.activity?.state === 'waiting' ? chief : null
    const count = waiting.length + (asking === null ? 0 : 1)
    // The human's notes not yet read: each reads and is marked read, in its
    // own list. The daemon reads the newest one frame holds, and says how many
    // there are.
    const { messages: notes, total: unread, shown } = inbox
    const section = element('section', 'foryou')
    section.setAttribute('role', 'region')
    section.setAttribute('aria-label', 'For you')
    const head = element('header', 'foryou-head')
    head.append(
      element('h2', 'foryou-name', 'For you'),
      element(
        'span',
        'foryou-status',
        count === 0 && held.length === 0 && unread === 0
          ? 'Nothing waiting'
          : [
              count === 0 ? null : `${count} waiting`,
              held.length === 0 ? null : `${held.length} held`,
              unread === 0 ? null : plural(unread, 'note'),
            ]
              .filter(Boolean)
              .join(' · '),
      ),
    )
    section.append(head)
    const strips = element('ol', 'strips')
    strips.setAttribute('aria-label', 'Waiting for you')
    if (asking !== null) strips.append(this.#chiefAsks(asking))
    for (const message of waiting) strips.append(this.#messageStrip(message, board, now))
    // A gated project says what will wait here; otherwise the head's "Nothing waiting" says it all.
    if (waiting.length === 0 && board.project?.gate) {
      strips.append(
        element(
          'li',
          'bay-empty',
          'Every task, result, question and answer between your agents waits here for your approval.',
        ),
      )
    }
    if (strips.childElementCount > 0) section.append(strips)
    if (held.length > 0) {
      const list = element('ol', 'strips')
      list.setAttribute('aria-label', 'Held for your meeting')
      for (const message of held) list.append(this.#messageStrip(message, board, now, true))
      section.append(element('h3', 'foryou-sub', 'Held for your meeting'), list)
    }
    if (unread > 0) {
      const list = element('ol', 'strips')
      list.setAttribute('aria-label', 'Notes for you')
      for (const message of notes) list.append(this.#messageStrip(message, board, now))
      // Those shown, once read, make room for the earlier ones.
      const earlier = unread - shown
      if (earlier > 0) {
        list.append(
          element(
            'li',
            'strips-more',
            `${plural(earlier, 'earlier note')} not shown: mark these read to see ${earlier === 1 ? 'it' : 'them'}.`,
          ),
        )
      }
      section.append(element('h3', 'foryou-sub', 'Notes from your agents'), list)
    }
    return section
  }

  /** The chief waits for the human in its terminal: the way there, where it is answered. */
  #chiefAsks(lane) {
    const item = element('li', 'strip strip-chief')
    item.dataset.testid = 'chief-asks'
    const line = element('div', 'strip-line')
    line.append(
      element('span', 'strip-number', 'chief'),
      element('span', 'strip-title', 'The chief is asking you something in its terminal'),
      element(
        'span',
        'strip-route',
        lane.activity.reason ? `Waiting: ${lane.activity.reason}` : 'Waiting for you',
      ),
      element('span', 'strip-age', ''),
    )
    const actions = element('div', 'strip-actions')
    actions.append(
      button(
        'Show terminal',
        'primary-button',
        () =>
          this.#actions.onShowTerminal(lane.participant, {
            closed: lane.pane === null,
            hidden: lane.hidden === true,
          }),
        `Show ${laneName(lane.participant)}'s terminal`,
      ),
    )
    item.append(line, actions)
    return item
  }

  /**
   * A message's strip. One `held` by a meeting only reads: it is the chief's,
   * and one marked read would never reach it.
   */
  #messageStrip(message, board, now, held = false) {
    const item = element('li', 'strip strip-message')
    item.dataset.kind = message.kind
    item.dataset.message = String(message.id)
    const gated = message.state === 'gated'
    const line = element('div', 'strip-line')
    line.append(
      element('span', 'strip-number', `m-${message.id}`),
      // The whole message, wrapped, read where it is answered. One with
      // options says nothing here: its choices under it are the questions.
      element('span', 'strip-title', message.questions ? '' : message.body),
      element(
        'span',
        'strip-route',
        `${KIND_LABEL[message.kind] ?? message.kind} from ${who(message.sender)}${gated ? ` to ${who(message.recipient)}` : ''}${message.taskNumber ? ` · T-${message.taskNumber}` : ''}${gated ? ' · needs your approval' : ''}${held ? ' · held' : ''}`,
      ),
      element('span', 'strip-age', age(message.createdAt, now)),
    )
    if (gated) item.dataset.gated = 'true'
    if (held) item.dataset.held = 'true'
    item.append(line)
    const actions = element('div', 'strip-actions')
    if (gated && acts(board)) actions.append(...this.#gateActions(message))
    if (message.taskNumber) {
      actions.append(
        button('Open task', 'quiet-button', () => this.#actions.onOpenTask(message.taskNumber)),
      )
    }
    // A note that ConsensFlow lacks a permission Claude Code's computer use needs: the way to
    // the System Settings pane of each it names.
    for (const { pane, name } of notePanes(message)) {
      actions.append(
        button(
          'Open System Settings',
          'primary-button',
          () => this.#actions.onSystemSettings(pane),
          `Open System Settings for ${name}`,
        ),
      )
    }
    if (!gated && !held && acts(board)) {
      actions.append(
        button(
          'Mark read',
          'quiet-button',
          () => this.#actions.onRead(message),
          `Mark m-${message.id} read`,
        ),
      )
    }
    if (actions.childElementCount > 0) item.append(actions)
    return item
  }

  /**
   * What the human may do with a message that waits for approval: pass it on,
   * or decline a task or an answer (its sender is told). The human writes to
   * no agent from here: a result or a question goes on to the chief, who
   * decides and answers in its terminal.
   */
  #gateActions(message) {
    const actions = [
      button(
        'Approve',
        'primary-button',
        () => this.#actions.onApprove(message),
        `Approve m-${message.id} for ${who(message.recipient)}`,
      ),
    ]
    if (message.kind === 'task' || message.kind === 'answer') {
      actions.push(
        button(
          'Decline',
          'quiet-button',
          () => this.#actions.onDecline(message),
          `Decline m-${message.id}`,
        ),
      )
    }
    return actions
  }

  /** The grid: a row per participant in lane order, a column per state. */
  #kanban(board, now) {
    const table = element('table', 'kanban')
    table.setAttribute('aria-label', 'Tasks')
    const head = element('thead')
    const headRow = element('tr')
    headRow.append(element('th', 'kanban-staff', 'Staff'))
    for (const [state, label] of COLUMNS) {
      const cell = element('th', null, label)
      cell.dataset.state = state
      if (state === 'finished') cell.append(...this.#deleteFinished(board))
      headRow.append(cell)
    }
    head.append(headRow)
    const { chief, groups } = boardGroups(board)
    const bodies = []
    if (chief.length > 0) {
      const body = element('tbody', 'chief-rows')
      body.append(...this.#rows(chief, board, now))
      bodies.push(body)
    }
    for (const group of groups) {
      const open = this.#groupsOpen(board.project.id).has(group.role)
      const body = element('tbody', 'role-group')
      body.dataset.group = group.role
      body.dataset.open = String(open)
      body.append(this.#groupHead(group, board, now, open))
      if (open) body.append(...this.#rows(group.rows, board, now))
      bodies.push(body)
    }
    // A project with nobody on its staff looks like any other board, and every
    // task the chief hands out is refused: say it where the members would be.
    if (!board.lanes.some((lane) => isMember(lane.participant))) {
      const body = element('tbody')
      const row = element('tr', 'board-empty')
      const cell = element(
        'td',
        null,
        'No members yet: add the members this project may use under Project settings.',
      )
      cell.colSpan = COLUMNS.length + 1
      row.append(cell)
      body.append(row)
      bodies.push(body)
    }
    table.append(head, ...bodies)
    return table
  }

  /**
   * `rows` as the board draws them: each, and an unfolded member's sessions' under it. They take
   * turns in shade, odd and even, from a group's first (the owner, 2026-11-20: "lets make each
   * row a different color/shade like odd/even"): a member's lane, its backup's part and every
   * session under it are one shade, so one member is told from the next at a glance.
   */
  #rows(rows, board, now) {
    const drawn = []
    for (const [at, row] of rows.entries()) {
      const folded = this.#folded(board, row)
      const lane = [this.#row(row, board, now, folded)]
      if (folded === false) {
        for (const session of row.sessions) lane.push(this.#row(session, board, now, null))
      }
      for (const one of lane) one.dataset.shade = at % 2 === 0 ? 'odd' : 'even'
      drawn.push(...lane)
    }
    return drawn
  }

  /** The groups open on project `projectId`, read from this browser the first time it is drawn. */
  #groupsOpen(projectId) {
    if (!this.#openGroups.has(projectId)) this.#openGroups.set(projectId, keptGroups(projectId))
    return this.#openGroups.get(projectId)
  }

  /**
   * A group's head: its name and how many members it has, a button that opens or
   * folds it, kept in this browser for the project; folded, what of it needs the
   * human. Folding it hides its rows, not its terminals: a window shown in the dock
   * stays there.
   */
  #groupHead({ role, rows }, board, now, open) {
    const head = element('tr', 'group-head')
    const cell = element('th')
    cell.setAttribute('scope', 'rowgroup')
    cell.colSpan = COLUMNS.length + 1
    const line = element('div', 'group-line')
    const members = new Set(
      rows.filter((row) => row.participant.member === null).map((row) => row.participant.handle),
    ).size
    // A group of sessions alone, of a role their member no longer has, counts its sessions.
    const count = members > 0 ? plural(members, 'member') : plural(rows.length, 'session')
    const toggle = button('', 'fold-group', () => {
      const shown = this.#groupsOpen(this.#board.project.id)
      if (!shown.delete(role)) shown.add(role)
      keepGroups(this.#board.project.id, shown)
      this.render(this.#drawn)
    })
    toggle.setAttribute('aria-expanded', String(open))
    toggle.append(
      icon(ICONS.fold),
      element('span', 'group-name', groupName(role)),
      ' ',
      element('span', 'group-count', count),
    )
    line.append(toggle)
    if (!open) {
      const marks = groupMarks(rows, board, now)
      if (marks.length > 0) {
        const list = element('ul', 'group-marks')
        list.setAttribute('aria-label', `In ${groupName(role)}`)
        for (const [mark, words] of marks) {
          const item = element('li', 'group-mark', words)
          item.dataset.mark = mark
          list.append(item)
        }
        line.append(list)
      }
    }
    cell.append(line)
    head.append(cell)
    return head
  }

  /**
   * The Finished heading's control, while the column holds a task: every
   * finished task that may go leaves the board, once the human confirms, and
   * the dialog says what stays and why, or that nothing may go; nothing while
   * the project is closed.
   */
  #deleteFinished(board) {
    const { deletable, kept } = finishedTasks(board)
    if (!acts(board) || deletable.length + kept.length === 0) return []
    return [
      iconButton(
        ICONS.remove,
        'Delete finished tasks',
        () => this.#actions.onDeleteFinished(this.#board.project, finishedTasks(this.#board)),
        'Delete finished',
        'danger-button',
      ),
    ]
  }

  /**
   * Whether a member's row is folded: its sessions' rows off the board and their cards on its
   * own. Every one is until the human unfolds it. Null for a row with no sessions: a team's
   * member and backup are on its row either way (T-1847).
   */
  #folded(board, row) {
    if ((row.sessions?.length ?? 0) === 0) return null
    return !this.#unfolded.has(foldKey(board, row.participant))
  }

  #row(lane, board, now, folded) {
    const { participant } = lane
    const row = element('tr')
    row.dataset.handle = participant.handle
    row.dataset.role = participant.role
    if (participant.member) row.dataset.session = participant.member
    if (folded !== null) row.dataset.folded = String(folded)
    // A member's second role has its row too: the name is the same member's.
    if (participant.also) row.dataset.also = 'true'
    row.append(this.#rowHead(lane, board, now, folded))
    const mine = rowCards(lane, board, folded === true)
    for (const [state, label] of COLUMNS) {
      const cell = element('td')
      cell.dataset.state = state
      const held = mine.filter((task) => columnOf(task) === state)
      if (held.length > CELL_HOLDS) {
        cell.append(this.#stackTile(held.length, state, label, participant))
      } else if (held.length > 0) {
        const list = element('ol', 'cards')
        list.append(...held.map((task) => this.#card(task, lane.busy ?? null)))
        cell.append(list)
      }
      row.append(cell)
    }
    return row
  }

  #rowHead(lane, board, now, folded) {
    const { participant } = lane
    const head = element('th', 'row-head')
    head.setAttribute('scope', 'row')
    const title = element('div', 'row-title')
    if (folded !== null) title.append(this.#foldButton(lane, folded))
    // The member's model-based name changes with an edit. Make its windows read as
    // sessions, rather than more members whose models changed along with it.
    const name = participant.member ? `Session · ${participant.session}` : laneName(participant)
    // A member's lamp is its own state, its sessions' included, as its words say it; a team's
    // backup has its own under it (T-1847, T-1850): which works, and which is out, is seen
    // without unfolding. A session's lamp, and the chief's, are their window's.
    const team = Boolean(lane.backup)
    const member = isMember(participant)
    title.append(
      lamp(member ? partLamp(lane, lane, now) : lane, now),
      element('span', 'row-name', name),
    )
    head.append(title)
    // The chief's row only holds its tasks: what it is doing, what it runs on
    // and Switch chief are on its card in the dock.
    if (participant.role === 'chief') return head
    const meta = identityLine(participant, 'row-meta')
    // A backup's session is marked as the team's backup is (T-1847), not named after it again.
    if (participant.ofBackup) meta.prepend(element('span', 'team-kind', 'backup'))
    head.append(meta)
    if (team) head.append(this.#team(lane, board, now))
    else if (member) {
      // A member with no backup says what it does as a team's member does (T-1850).
      const own = element('div', 'team-part')
      own.dataset.part = 'member'
      own.append(this.#partWords(lane, lane, board, now))
      head.append(own)
    }
    const said = team
      ? teamStatus(lane, board, now)
      : member
        ? soloStatus(lane, board, now)
        : rowStatus(lane, board, now)
    if (said !== null) {
      const [state, text] = said
      const status = element('span', 'row-status', text)
      status.dataset.state = state
      head.append(status)
      // A member's Try again now is beside its own words.
      if (state === 'out' && acts(board) && !member) head.append(...this.#tryAgain(lane, now))
    }
    head.append(this.#rowTools(lane, board))
    return head
  }

  /**
   * The human's way to try a row out of quota again before its reset, where its words are its
   * status line. None for a model that is busy, which ConsensFlow tries again after a back-off
   * no press shortens (T-1357). A member's is beside its own words (`#partWords`).
   */
  #tryAgain(row, now) {
    if (!outOfQuota(row.participant, now) || row.busy) return []
    const { participant } = row
    return [tryAgain(participant, this.#onLane(this.#actions.onTryAgain, participant))]
  }

  /**
   * A team's two parts on its lane, folded or not, drawn as the staff screens draw a member and
   * its backup (T-1847): the member's own words under its name, and under them its backup, set
   * off by the team's rule, with its lamp, its mark, its name, what it runs on and its own
   * words. Each that is out says until when, with its own Try again now beside it. The
   * sessions, unfolded, are the rest of the detail.
   */
  #team(row, board, now) {
    const list = element('ul', 'row-team')
    list.setAttribute('aria-label', `${laneName(row.participant)}'s team`)
    for (const part of partsOf(row)) {
      const { participant } = part
      const backup = part !== row
      const item = element('li', 'team-part')
      item.dataset.part = backup ? 'backup' : 'member'
      if (backup) {
        const name = element('div', 'team-name')
        name.append(
          lamp(partLamp(part, row, now), now),
          element('span', 'team-kind', 'backup'),
          element('span', 'team-handle', `@${participant.handle}`),
        )
        item.append(name, element('span', 'team-meta', runsOn(participant)))
      }
      item.append(this.#partWords(part, row, board, now))
      list.append(item)
    }
    return list
  }

  /**
   * What a member, or a team's backup, does in its own few words (`partState`), coloured as its
   * lamp, with its Try again now beside them while it is out: none for a model that is busy.
   */
  #partWords(part, row, board, now) {
    const { participant } = part
    const [state, text] = partState(part, row, now)
    const words = element('div', 'team-status')
    words.dataset.state = state
    words.append(element('span', 'team-words', text))
    if (state === 'out' && !part.busy && acts(board)) {
      words.append(tryAgain(participant, this.#onLane(this.#actions.onTryAgain, participant)))
    }
    return words
  }

  /** Unfolds a member's row, its sessions' rows under it, or folds it again. */
  #foldButton({ participant, sessions }, folded) {
    const name = laneName(participant)
    const what = `${name}'s ${plural(sessions.length, 'session')}`
    const tip = 'sessions'
    const toggle = iconButton(
      ICONS.fold,
      `${folded ? 'Show' : 'Hide'} ${tip}`,
      () => {
        const key = foldKey(this.#board, participant)
        if (!this.#unfolded.delete(key)) this.#unfolded.add(key)
        this.render(this.#drawn)
      },
      `${folded ? 'Show' : 'Hide'} ${what}`,
      'fold-sessions',
    )
    toggle.setAttribute('aria-expanded', String(!folded))
    return toggle
  }

  /**
   * A session's terminal is the human's to show and hide: shown, its card is
   * in the dock; hidden, the card goes, and its window closes once nothing
   * holds it open. One whose window has closed opens again on its own
   * conversation when shown, and its copy is on its last task's card. The
   * session is deleted from here too: it leaves the board and keeps its
   * conversation. Each of these is an icon, named in its tip; a closed
   * project's rows have none.
   */
  #rowTools(lane, board) {
    const { participant } = lane
    const tools = element('div', 'row-tools')
    const session = participant.member !== null
    const acting = acts(board)
    const name = laneName(participant)
    if (isMember(participant) && acting) {
      const key = `${participant.projectId}:${participant.handle}`
      const resting = Boolean(participant.vacationSince)
      tools.append(
        vacationButton(lane.backup ? `${name} and its backup` : name, resting, async () => {
          const refused = await this.#rest(lane, resting)
          if (refused) this.#vacationErrors.set(key, refused)
          else this.#vacationErrors.delete(key)
          this.render(this.#drawn)
        }),
      )
      const refused = this.#vacationErrors.get(key)
      if (refused) tools.append(element('span', 'row-status', refused))
    }
    if (session && acting) {
      tools.append(
        this.#shows(participant)
          ? iconButton(
              ICONS.hide,
              'Hide terminal',
              this.#onLane(this.#actions.onHideTerminal, participant),
              `Hide ${name}'s terminal`,
            )
          : iconButton(
              ICONS.show,
              'Show terminal',
              () => {
                const shown = this.#lane(participant)
                this.#actions.onShowTerminal(shown.participant, {
                  closed: shown.pane === null,
                  hidden: shown.hidden === true,
                })
              },
              `Show ${name}'s terminal`,
            ),
      )
    }
    if (session && acting) {
      tools.append(
        iconButton(
          ICONS.remove,
          'Delete session',
          this.#onLane(this.#actions.onEndSession, participant),
          `Delete ${name}'s session`,
          'danger-button',
        ),
      )
    }
    return tools
  }

  /**
   * Vacation on the board rests a team, its member and its backup together, and brings it
   * back together: each part not yet as the press asks, the member first, as the board has it
   * now; a refusal stops there and is what this says. Project settings keeps each one's own.
   */
  async #rest(row, resting) {
    for (const part of partsOf(row)) {
      const participant = this.#lane(part.participant).participant
      if (Boolean(participant.vacationSince) !== resting) continue
      const refused = await this.#actions.onVacation(participant)
      if (refused) return refused
    }
    return null
  }

  /**
   * A crowded cell, `count` cards of the column `state` on `participant`'s
   * row: one tile, a stack and how many, named for what it holds. It opens
   * them in the stack dialog.
   */
  #stackTile(count, state, label, participant) {
    const tile = button(
      '',
      'stack',
      () => this.#openStack(participant, state),
      `${count} ${label.toLowerCase()} tasks of ${laneName(participant)}`,
    )
    // Across redraws its column tells it from the others, whatever its count.
    tile.dataset.stack = state
    tile.append(icon(ICONS.stack), element('span', 'stack-count', String(count)))
    return tile
  }

  /** Opens the stack dialog on the cell of the column `state` on a participant's row. */
  #openStack({ projectId, handle, role }, state) {
    this.#stacked = { project: projectId, handle, role, state }
    this.#drawStack()
    this.#stack.showModal()
    // A long list itself can take the browser's first focus; start on its first card.
    this.#stackCards.querySelector('.card')?.focus({ preventScroll: true })
  }

  /**
   * The stack dialog's cards: its cell's as the board drawn last has them,
   * the newest first, each opening its task as on the board. A cell left
   * with none (its tasks moved on, another project shown) closes it.
   */
  #drawStack() {
    const { project, handle, role, state } = this.#stacked
    const row =
      this.#board.project.id === project
        ? boardRows(this.#board)
            .flatMap((member) => [member, ...(member.sessions ?? [])])
            .find(({ participant }) => participant.handle === handle && participant.role === role)
        : undefined
    const held =
      row === undefined
        ? []
        : rowCards(row, this.#board, this.#folded(this.#board, row) === true).filter(
            (task) => columnOf(task) === state,
          )
    if (held.length === 0) {
      this.#stack.close()
      return
    }
    const [, label] = COLUMNS.find(([column]) => column === state)
    this.#stackTitle.textContent = `${label} tasks of ${laneName(row.participant)}`
    redraw(
      this.#stackCards,
      held.map((task) => this.#card(task, row.busy ?? null)),
    )
  }

  /** What a retry past its time waits for, as the drawer says it: from the board's state at `now`. */
  #retryWhy(assigneeHandle, now) {
    // The member whose task it is: a backup's task waits on the backup, folded or not.
    const lanes = this.#board.lanes
    const assignee = lanes.find((lane) => lane.participant.handle === assigneeHandle)?.participant
    const owner =
      lanes.find((lane) => lane.participant.handle === (assignee?.member ?? assignee?.handle))
        ?.participant ?? null
    return retryWaitsFor(this.#board.project.state !== 'open', owner, now)
  }

  #tick() {
    const now = Date.now()
    for (const root of [this.#root, this.#stackCards]) {
      for (const age of root.querySelectorAll('.card-age')) {
        const text = stateAge(age.dataset.since, now)
        if (age.textContent !== text) {
          age.textContent = text
          const card = age.closest('.card')
          card.setAttribute('aria-description', `${STATE_LABEL[card.dataset.state]} for ${text}`)
        }
      }
      for (const held of root.querySelectorAll('.card-held[data-retry]')) {
        // What it waits for follows the clock too: its member's quota may return meanwhile.
        const why = this.#retryWhy(held.dataset.assignee || null, now)
        const text = retryWords(JSON.parse(held.dataset.retry), false, now, why)
        if (held.textContent !== text) held.textContent = text
      }
    }
  }

  #card(task, busy = null) {
    const item = element('li', 'card-item')
    const card = button('', 'card', () => this.#actions.onOpenTask(task.number))
    card.dataset.task = String(task.number)
    card.dataset.state = task.state
    if (task.questionPending) card.dataset.pending = 'question'
    if (task.held) card.dataset.pending = 'background'
    if (task.retry) card.dataset.pending = 'retry'
    card.setAttribute(
      'aria-label',
      `T-${task.number}, ${task.title}, ${stateLabel(task)}${task.resumeWait ? `, resume waits: ${task.resumeWait.reason}` : ''}, from ${who(task.requester)}`,
    )
    const elapsed = stateAge(task.stateSince)
    const status = element('span', 'card-status')
    const elapsedNode = element('span', 'card-age', elapsed)
    elapsedNode.dataset.since = task.stateSince ?? ''
    elapsedNode.title = 'Time in this state'
    card.setAttribute('aria-description', `${STATE_LABEL[task.state]} for ${elapsed}`)
    status.append(
      element('span', 'card-state', stateLabel(task)),
      task.questionPending ? ' · Waiting ' : ' · ',
      elapsedNode,
    )
    const where = element('span', 'card-route', route(task, busy))
    // What it waits on is said on the card itself: the window's lamp says it rests.
    if (task.held) {
      where.append(' · ', element('span', 'card-held', `waiting on ${heldWords(task.held)}`))
    }
    if (task.retry) {
      const now = Date.now()
      const held = element(
        'span',
        'card-held',
        retryWords(task.retry, false, now, this.#retryWhy(task.assignee, now)),
      )
      // The card left open past its time follows the clock (`#tick`), its reason too.
      held.dataset.retry = JSON.stringify(task.retry)
      held.dataset.assignee = task.assignee ?? ''
      where.append(' · ', held)
    }
    if (task.resumeWait) {
      where.append(' · ', element('span', 'card-held', `resume waits: ${task.resumeWait.reason}`))
    }
    card.append(
      element('span', 'card-number', `T-${task.number}`),
      element('span', 'card-title', task.title),
      where,
      status,
    )
    // A result reads by its first line: its drawer has it whole.
    if (task.result) card.append(element('span', 'card-result', task.result.trim().split('\n')[0]))
    item.append(card)
    return item
  }
}

/** How a transcript item's role reads on the card. */
const TRANSCRIPT_ROLE = {
  user: 'Sent to the window',
  assistant: 'The agent',
  tool: 'Tool output',
  custom: 'Note',
}

/**
 * What a copied item is, as the drawer labels it (and the history viewer heads a
 * tool's output an older build copied): by its role, and the summary a compaction
 * wrote in the user's place as one.
 */
export const itemLabel = (item) =>
  item.compaction ? 'Compaction summary' : (TRANSCRIPT_ROLE[item.role] ?? item.role)

/**
 * One task: its story (each request, question, answer and result, in the
 * order they came), what its window wrote (ConsensFlow's own copy of the
 * conversation, kept after the window is gone), and what the human may do
 * next.
 */
export class TaskDrawer {
  #root
  #actions
  /** The task drawn, as it was read. */
  #task = null
  /** Its fold of what its window wrote, kept open or shut while the task is shown. */
  #fold = null
  /** The session whose window works on the task, if it has one: the terminal it shows. */
  #terminal = null
  /** The member whose availability may be holding this task. */
  #waitingMember = null
  /**
   * What the task is, how it stands and what the human may do with it, at the drawer's
   * top however long its story: acting on a task never takes reading down it.
   */
  #top = element('div', 'drawer-top')
  /** Its story and what its window wrote, scrolled under the top. */
  #body = element('div', 'drawer-body')

  constructor(root, actions) {
    this.#root = root
    this.#actions = actions
    root.replaceChildren(this.#top, this.#body)
  }

  /** Whether `task` is the one drawn, unchanged since. */
  shows(task) {
    return JSON.stringify(task) === JSON.stringify(this.#task)
  }

  /** Whether `task` is the one drawn, as it was then or since. */
  #drawn(task) {
    return this.#task?.projectId === task.projectId && this.#task?.number === task.number
  }

  /**
   * Draws `task`. `total` is how many items its window wrote: the fold reads
   * them when it opens, and again when the task changed while it is open.
   * `closed`: the task's project is closed, and its task reads with nothing
   * to do on it. The same task drawn again keeps whatever did not change in
   * place: each step of its story and its fold open or shut, and the drawer
   * where it was scrolled. `terminal`: the participant whose window works on
   * the task, whether that window is closed and whether the human hid it, or
   * null when it has none. `held`: what the task waits on in the background
   * (`{jobs, agents}`), or null when it waits on nothing: the drawer says it.
   * `backupOf`: the member whose backup runs the task, or null: the board draws a member and
   * its backup as one, so this is where it says which of the two has it.
   */
  show(
    task,
    {
      total,
      closed = false,
      now = Date.now(),
      terminal = null,
      held = null,
      resumeWait = null,
      backupOf = null,
      waitingMember = null,
    },
  ) {
    const same = this.#drawn(task)
    const changed = !this.shows(task)
    const head = element('header', 'drawer-head')
    const title = element('h2', 'drawer-title')
    title.append(
      element('span', 'card-number', `T-${task.number}`),
      element('span', null, task.title),
    )
    head.append(
      title,
      button('Close', 'quiet-button', () => this.#actions.onClose(), 'Close the task'),
    )
    this.#waitingMember = waitingMember
    const wait = this.#wait(task, now)
    const meta = element(
      'p',
      'drawer-meta',
      `${who(task.requester)} asked ${task.assignee === null ? `for ${aPool(task)}` : who(task.assignee)}${backupOf === null ? '' : `, the backup of @${backupOf}`} · ${stateLabel({ ...task, held })}${task.retry ? ` · ${retryWords(task.retry, true, now, retryWaitsFor(closed, waitingMember?.participant, now))}` : ''}${resumeWait === null ? '' : ` · resume waits: ${resumeWait.reason}`}${wait === null ? '' : ` · ${wait.words}`}${task.deletedAt ? ' · deleted from the board' : ''} · updated ${ago(task.updatedAt, now)}${task.needs.length === 0 ? '' : ` · needs ${task.needs.map((need) => `T-${need.number} (${need.state})`).join(', ')}`}`,
    )
    this.#terminal = terminal
    const top = [head, meta]
    const actions = element('div', 'drawer-actions')
    if (!closed) actions.append(...this.#taskActions(task, wait))
    if (actions.childElementCount > 0) top.push(actions)
    const sections = [this.#story(task, same, now)]
    // What its window wrote is read only when asked for, folded until then.
    this.#fold = total > 0 ? ((same ? this.#fold : null) ?? this.#newFold()) : null
    if (this.#fold !== null) {
      redraw(this.#fold, [transcriptHead(total), ...[...this.#fold.children].slice(1)])
      sections.push(this.#fold)
    }
    this.#task = task
    this.#root.setAttribute('aria-label', `Task T-${task.number}`)
    this.#root.dataset.state = task.state
    // Shown first: a hidden drawer takes no scroll, and keeps where the last task was read to.
    this.#root.hidden = false
    if (same) {
      redraw(this.#top, top)
      redraw(this.#body, sections)
    } else {
      this.#top.replaceChildren(...top)
      this.#body.replaceChildren(...sections)
      this.#body.scrollTop = 0
    }
    if (same && changed && this.#fold?.open) this.#actions.onTranscript(task)
  }

  /** Why a task waiting for its member cannot run, and whether the member can be tried now. */
  #wait(task, now) {
    if (task.state !== 'queued' && task.state !== 'paused') return null
    if (this.#waitingMember === null) return null
    const { participant, busy } = this.#waitingMember
    if (participant.needsEdit) return { words: 'its member needs an edit under Project settings' }
    if (!outOfQuota(participant, now)) return null
    if (busy) return { words: `its member's model is busy until ${backAt(busy, now)}` }
    if (modelMoved(participant)) return { words: "its member's model moved", retry: true }
    if (unavailable(participant)) {
      return { words: "its member's account is unavailable (login or billing)", retry: true }
    }
    return {
      words: `its member is out of quota until ${backAt(participant.outUntil, now)}`,
      retry: true,
    }
  }

  /**
   * The task's story, a step for each part of it, oldest first. Only the
   * newest step is open until the human opens or shuts one: the same task
   * drawn again keeps each step as the human left it, and a step new since
   * opens when it is the newest. The earliest messages of a story too long
   * for one frame are left out after its first request.
   */
  #story(task, same, now) {
    // Each step drawn now, open or shut, by its message ('body' for the
    // request made from the task's body), read before it is drawn again.
    const shown = new Map()
    if (same) {
      for (const step of this.#root.querySelectorAll('details.step')) {
        shown.set(step.dataset.message ?? 'body', step.open)
      }
    }
    const steps = storyOf(task)
    const list = element('ol', 'story')
    list.setAttribute('aria-label', `T-${task.number}'s story`)
    const left = task.messagesLeftOut ?? 0
    for (const [at, step] of steps.entries()) {
      const open = shown.get(String(step.id ?? 'body')) ?? at === steps.length - 1
      list.append(storyStep(step, task.number, open, now))
      if (at === 0 && left > 0) {
        list.append(
          element(
            'li',
            'story-more',
            `${plural(left, 'earlier message')} not shown: cf task get T-${task.number} shows the whole thread.`,
          ),
        )
      }
    }
    const story = element('section', 'drawer-section')
    story.dataset.section = 'story'
    story.append(sectionHead('h3', 'Story'), list)
    return story
  }

  /** The open fold read again: its window wrote more since. */
  reread() {
    if (this.#task !== null && this.#fold?.open) this.#actions.onTranscript(this.#task)
  }

  /** What the task's window wrote, read for its open fold: the last items one frame holds. */
  fill(task, { items, total, shown }) {
    if (!this.#drawn(task) || this.#fold === null) return
    const list = element('ol', 'transcript')
    list.setAttribute('aria-label', `What T-${task.number}'s window wrote`)
    for (const item of items) {
      const entry = element('li', 'transcript-item')
      entry.dataset.role = item.role
      if (item.compaction) entry.dataset.compaction = 'true'
      entry.append(
        element(
          'div',
          'transcript-head',
          `${itemLabel(item)}${item.complete ? '' : ' · still writing'}`,
        ),
        element('pre', 'transcript-body', item.text),
      )
      list.append(entry)
    }
    redraw(this.#fold, [
      transcriptHead(total),
      ...(total > shown
        ? [element('p', 'transcript-more', `The last ${shown} of ${total} items.`)]
        : []),
      list,
    ])
  }

  #newFold() {
    const fold = element('details', 'drawer-section')
    fold.dataset.section = 'transcript'
    fold.addEventListener('toggle', () => {
      if (fold.open) this.#actions.onTranscript(this.#task)
    })
    return fold
  }

  /**
   * What the human may do with a task: nothing that writes to the agent
   * (that is done in its terminal), and no accepting (that is the chief's).
   */
  #taskActions(task, wait) {
    // A button kept across redraws acts on the task as it was read last.
    const on = (action) => () => action(this.#task)
    const actions = []
    // Its window, in front in the dock (a closed one opens on its conversation).
    if (this.#terminal !== null) {
      const { participant, closed, hidden } = this.#terminal
      actions.push(
        button(
          'Show terminal',
          'quiet-button',
          () => this.#actions.onShowTerminal(participant, { closed, hidden }),
          `Show ${laneName(participant)}'s terminal`,
        ),
      )
    }
    if (PAUSABLE.includes(task.state) && task.assignee !== 'chief') {
      actions.push(button('Pause', 'quiet-button', on(this.#actions.onPause)))
    }
    if (task.state === 'paused') {
      actions.push(button('Resume', 'primary-button', on(this.#actions.onResume)))
    }
    if (wait?.retry) {
      actions.push(
        tryAgain(this.#waitingMember.participant, () =>
          this.#actions.onTryAgain(this.#waitingMember.participant),
        ),
      )
    }
    // Taken from its member and back on the board for its tier; the chief's
    // own work and work given by name have no tier to go back to.
    if (REASSIGNABLE.includes(task.state) && task.pool !== null && task.assignee !== null) {
      actions.push(
        button(
          'Reassign',
          'quiet-button',
          on(this.#actions.onReassign),
          `Reassign T-${task.number} to another member of its tier`,
        ),
      )
    }
    // Any working, waiting or paused task of a member's: what its worker said last becomes the result,
    // at the human's word (the chief has the same in `cf task finish`), whatever the window's
    // reader says of its turn. A paused task's is the answer its turn on a tell ended with,
    // which the human is told (T-442). A waiting task's is the words its turn ended on, which
    // ConsensFlow raised as the question it waits on (T-1119); the question is withdrawn. It ends nothing the turn left running, which ends with
    // its window, or outlives it where the host cannot tell what is the window's: the
    // button promises no end of it.
    if (FINISHABLE.includes(task.state) && task.assignee !== null && task.assignee !== 'chief') {
      actions.push(
        button(
          'Finish',
          'quiet-button',
          on(this.#actions.onFinish),
          `Finish T-${task.number}: what its worker said last is its result; nothing it left running is ended`,
        ),
      )
    }
    if (ACTIVE.includes(task.state)) {
      actions.push(button('Cancel task', 'danger-button', on(this.#actions.onCancel)))
    }
    // Over, it may leave the board for good, once the human confirms.
    if (FINISHED.includes(task.state) && task.deletedAt === null) {
      actions.push(button('Delete task', 'danger-button', on(this.#actions.onDelete)))
    }
    return actions
  }

  hide() {
    this.#task = null
    this.#fold = null
    this.#terminal = null
    this.#waitingMember = null
    this.#root.hidden = true
    this.#top.replaceChildren()
    this.#body.replaceChildren()
  }
}

/** A drawer section's heading: what it is, and how many when that says something. */
function sectionHead(tag, label, count) {
  const heading = element(tag, 'drawer-section-head')
  heading.append(element('span', null, label))
  if (count !== undefined) heading.append(element('span', 'drawer-count', count))
  return heading
}

const transcriptHead = (total) =>
  sectionHead('summary', 'What the agent did', plural(total, 'item'))

/** What the daemon cut to carry task T-`number` in one frame says where it reads whole. */
const cutNote = (part, number) =>
  part.bodyCut
    ? [element('p', 'drawer-cut', `Cut to fit here: cf task get T-${number} shows it whole.`)]
    : []

/**
 * Words told to the task's window before it was up (`cf tell` to a task still
 * queued): a note someone sent its window. ConsensFlow's notes have no sender,
 * and a member's go to the chief.
 */
const toldWords = (message, task) =>
  message.kind === 'note' && Boolean(message.sender) && message.recipient === task.assignee

/**
 * A task's story, oldest first: every message but a note (ConsensFlow
 * talking to the chief) and a withdrawn one (it reached nobody), each with
 * who sent it to whom and how its delivery went. Words told to its window
 * are in it, under Told, waiting for the window until it has them. A
 * request is numbered, and a result takes the number of the request it
 * answers.
 */
export function storyOf(task) {
  const told = task.messages.filter(
    (message) =>
      (message.kind !== 'note' || toldWords(message, task)) && message.state !== 'cancelled',
  )
  const steps = told.map((message) => ({
    ...message,
    route: `${who(message.sender)} → ${who(message.recipient)}`,
    told: toldWords(message, task),
    delivery:
      toldWords(message, task) && message.state === 'queued'
        ? 'waits for its window'
        : delivery(message),
  }))
  // A task sent to nobody yet (open, waiting on what it needs, the chief's
  // own) asks in its body, from its requester. It was never sent: no time.
  if (!told.some((message) => message.kind === 'task')) {
    steps.unshift({
      id: null,
      kind: 'task',
      route: `${who(task.requester)} → ${task.assignee === null ? aPool(task) : who(task.assignee)}`,
      delivery: null,
      createdAt: null,
      body: task.body,
      bodyCut: task.bodyCut,
    })
  }
  let round = 0
  return steps.map((step) => {
    if (step.kind === 'task') round += 1
    const label =
      step.kind === 'task'
        ? `Request ${round}`
        : step.kind === 'result' && round > 0
          ? `Result ${round}`
          : step.told
            ? 'Told'
            : (KIND_LABEL[step.kind] ?? step.kind)
    return { ...step, label }
  })
}

/** How a message's delivery reads while it has not reached its window. */
const DELIVERY = {
  queued: 'queued',
  delivering: 'delivering',
  gated: 'needs your approval',
  failed: 'not delivered',
}

/**
 * What a step says of its message's delivery, and why: nothing once it
 * reached its window, or was read at once (an answer picked from options).
 */
function delivery({ state, reason }) {
  if (state === 'delivered' || state === 'read') return null
  const said = DELIVERY[state] ?? state
  return reason ? `${said}: ${reason}` : said
}

/** When a message was sent: "14:05" today, "Sep 30 14:05" before; all of it on hover. */
function sentAt(iso, now) {
  const sent = new Date(iso)
  const day =
    sent.toDateString() === new Date(now).toDateString()
      ? ''
      : `${sent.toLocaleDateString([], { month: 'short', day: 'numeric' })} `
  const node = element('time', 'step-time', `${day}${clock(iso)}`)
  node.dateTime = iso
  node.title = sent.toLocaleString([], { dateStyle: 'full', timeStyle: 'medium', hour12: false })
  return node
}

/**
 * A step of a task's story: a fold whose line says what it is, who sent it
 * to whom, how its delivery went and when, then the first line of its
 * body; open, it reads whole, its markdown drawn. Its body is drawn once
 * it is open, so a long story draws only what is read.
 */
function storyStep(step, number, open, now) {
  const line = element('summary', 'step-head')
  line.append(element('span', 'step-label', step.label), element('span', 'step-route', step.route))
  if (step.delivery !== null) {
    const state = element('span', 'step-state', step.delivery)
    state.dataset.state = step.state
    line.append(state)
  }
  if (step.createdAt !== null) line.append(sentAt(step.createdAt, now))
  line.append(element('span', 'step-preview', preview(step.body)))
  const fold = element('details', 'step')
  fold.dataset.kind = step.kind
  if (step.id !== null) fold.dataset.message = String(step.id)
  fold.open = open
  fold.append(line)
  const drawBody = () => {
    const body = element('div', 'step-body')
    body.append(...render(step.body))
    fold.append(body, ...cutNote(step, number))
  }
  if (open) drawBody()
  // The click that opens it draws its body before it opens, unless it is there from before.
  line.addEventListener('click', () => {
    if (!fold.open && fold.childElementCount === 1) drawBody()
  })
  const item = element('li')
  item.append(fold)
  return item
}
