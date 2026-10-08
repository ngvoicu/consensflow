/**
 * The board keeps every task in view: a task no lane has is among the open
 * ones, whatever its state, and the page and `cf task list` draw it. Three
 * cases the owner decided on (2026-10-07) stay in view: a task paused before
 * any member had it, one called off or failed before any member had it, and
 * the tasks of a member the human removed. One plant takes one case out of the
 * board, or one rule out of what draws it; a test of it fails: the ledger's own,
 * which holds the board to the placements, or the page's, the eval's and the
 * list's. A run written `{ node: [...] }` is `node --test`.
 */
import { lines } from './kit.mjs'

const RUST = 'crates/cf-ledger/src/page_reads.rs'
const PAGE = 'app/ui/core/board.js'
const EVAL = 'evals/measure.mjs'
const LIST = 'crates/cf/src/board/task.rs'

/** The ledger's board, on a ledger of its own: held to the placements. */
const ledgerBoard = ['-p', 'cf-ledger', '--test', 'cancelled']
/** The page's rules for the cards it is given. */
const page = { node: ['tests/board-ui.test.mjs'] }
/** The eval's count of the tasks the board places, from a ledger. */
const placed = { node: ['tests/evals-measure.test.mjs'] }
/** `cf task list`, over what waits for a member and what does not. */
const listing = ['-p', 'cf', '--lib', 'board::task']

/** The test that holds the ledger's board to each case. */
const RUST_MEANT =
  'a_task_no_lane_has_is_among_the_open_ones_whatever_its_state_as_node_draws_the_board'

/** The open list of the ledger's board: the tasks no lane has. */
const RUST_OPEN = '.filter(|(_, lane)| !has_lane(lane))'

export const PLANTS = [
  {
    name: 'board: the ledger leaves a task paused before any member had it off the board',
    edits: [
      [
        RUST,
        RUST_OPEN,
        '.filter(|(card, lane)| !has_lane(lane) && !(card.state == "paused" && card.assignee.is_none()))',
      ],
    ],
    runs: [ledgerBoard],
    meant: RUST_MEANT,
  },
  {
    name: 'board: the ledger leaves a task called off or failed before any member had it off the board',
    edits: [
      [
        RUST,
        RUST_OPEN,
        '.filter(|(card, lane)| !has_lane(lane) && !(matches!(card.state.as_str(), "cancelled" | "failed") && card.assignee.is_none()))',
      ],
    ],
    runs: [ledgerBoard],
    meant: RUST_MEANT,
  },
  {
    name: "board: the ledger leaves a removed member's tasks off the board",
    edits: [
      [RUST, RUST_OPEN, '.filter(|(card, lane)| !has_lane(lane) && card.assignee.is_none())'],
    ],
    runs: [ledgerBoard],
    meant: RUST_MEANT,
  },
  {
    name: 'board: the page queues a paused task that no lane has, where it belongs in the backlog',
    edits: [
      [
        PAGE,
        lines('        ? task.laneless', "          ? 'open'", "          : 'queued'"),
        "        ? 'queued'",
      ],
    ],
    runs: [page],
    meant: 'stays in the backlog when paused, where a paused task in a window waits in the queue',
  },
  {
    name: 'board: the page draws only the open tasks that no lane has, on their requester`s row',
    edits: [
      [
        PAGE,
        '.filter((task) => task.requester === lane.participant.handle)',
        ".filter((task) => task.requester === lane.participant.handle && task.state === 'open')",
      ],
    ],
    runs: [page],
    meant: "is on its requester's row, in the column of its state",
  },
  {
    name: 'board: the eval counts as placed only the tasks waiting for a member',
    edits: [
      [
        EVAL,
        "placed: count('SELECT COUNT(*) AS n FROM task WHERE deleted_at IS NULL'),",
        'placed: count("SELECT COUNT(*) AS n FROM task WHERE deleted_at IS NULL AND state = \'open\' AND assignee_id IS NULL"),',
      ],
    ],
    runs: [placed],
    meant:
      'are every one it lists: one no lane has, whatever its state, and not one the human deleted',
  },
  {
    name: 'board: cf task list puts every task no lane has under the words for those that wait',
    edits: [[LIST, '.partition(|task| waits_for_a_member(task));', '.partition(|_| true);']],
    runs: [listing],
    meant: 'what_waits_for_a_member_is_listed_apart_from_the_other_tasks_no_lane_has',
  },
  {
    name: 'board: cf task list takes a removed member`s open task for one that waits for a member',
    edits: [[LIST, ' && matches!(task.get("assignee"), Some(Value::Null))', '']],
    runs: [listing],
    meant: 'what_waits_for_a_member_is_listed_apart_from_the_other_tasks_no_lane_has',
  },
]
