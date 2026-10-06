/**
 * What the three players of Node's recordings (`crates/cf-daemon/tests/{api,page,screens}`)
 * once let pass, which Astraeus found at its first look at the daemon: a bug
 * of each kind is planted, and the player that was blind to it must fail a
 * trace. The bugs that no recorded trace can show are not here (see
 * `support.mjs` for what bugs in the players' own parts can do).
 *
 * - The API's traces went through its own checks, not `api::handle` with the
 *   screens in front: a screen that took an agent's route passed.
 * - `cf`'s requests were compared by method, path and bearer: a query, a body
 *   or a trailing request that no answer showed passed.
 * - The page's operations were not held to the files they wrote, and their
 *   replies were read off the bridge and written again, so spaces and escapes
 *   did not show.
 * - The screens' world masked every time of every file, the recorder only a
 *   roster's `createdAt` and `updatedAt` (a test of the mask is what catches
 *   that: no recorded file holds a time outside them, so no daemon's bug is
 *   seen by the narrow mask alone).
 * - The API's and the screens' traces were not held to reach the ledger's
 *   `close`, where the database is compared, as the page's were.
 */
import { DAEMON, lines } from './kit.mjs'

const TESTS = 'crates/cf-daemon/tests'
const player = (name) => ['-p', 'cf-daemon', '--test', name]
const API = player('api')
const PAGE = player('page')
const SCREENS = player('screens')

/** The test of each player that holds what a bug breaks. */
const API_TRACES = 'every_trace_of_the_api_is_answered_as_node_answered_it'
const CF_RUNS = 'every_run_of_cf_against_the_api_prints_what_it_printed_against_node'
const PAGE_TRACES = 'every_page_trace_is_answered_as_node_answered'

/** A trace is read with its last step, the ledger's close, taken off. */
const NO_CLOSE = [
  `${TESTS}/support/trace.rs`,
  '    serde_json::from_str(&text).expect("a trace that is JSON")',
  lines(
    '    let mut trace: Value = serde_json::from_str(&text).expect("a trace that is JSON");',
    '    if let Some(steps) = trace["steps"].as_array_mut() {',
    '        if steps.last().is_some_and(|step| step["method"] == "close") {',
    '            steps.pop();',
    '        }',
    '    }',
    '    trace',
  ),
]

export const PLANTS = [
  {
    name: 'players: the screens take an agent’s route (/api/whoami)',
    edits: [
      [
        `${DAEMON}/screens/mod.rs`,
        '        "/api/preferences" => Some(Screen::Preferences),',
        '        "/api/preferences" | "/api/whoami" => Some(Screen::Preferences),',
      ],
    ],
    runs: [API],
    meant: API_TRACES,
  },
  {
    name: 'players: a screen’s path without the UI token falls through to the API',
    edits: [
      [
        `${DAEMON}/screens/mod.rs`,
        'return Some(Answer::json(401, json!({ "error": "unauthorized" })));',
        'return None;',
      ],
    ],
    runs: [API],
    meant: API_TRACES,
  },
  {
    name: 'players: the API’s traces go through the routes with no screens in front',
    edits: [
      [
        `${TESTS}/api/rig.rs`,
        'let answer = handle(&context, &screens, request).await;',
        lines(
          'let answer = match cf_daemon::api::callers::caller_of(&context, &request) {',
          '                    Ok(caller) => match cf_daemon::api::routes::recognize(&request.method, &request.path) {',
          '                        Some(route) => {',
          '                            cf_daemon::api::routes::dispatch(&context, &caller, route, request).await',
          '                        }',
          '                        None => Err(request.unknown_route()),',
          '                    },',
          '                    Err(failure) => Err(failure),',
          '                };',
        ),
      ],
    ],
    runs: [API],
    meant: API_TRACES,
  },
  {
    name: 'players: cf asks the door to wait a second longer',
    edits: [
      [
        'crates/cf-board/src/door.rs',
        'const POLL_WAIT: Duration = Duration::from_secs(20);',
        'const POLL_WAIT: Duration = Duration::from_secs(21);',
      ],
    ],
    runs: [API],
    meant: CF_RUNS,
  },
  {
    name: 'players: cf asks for a transcript of ten items whatever it was asked for',
    edits: [
      [
        'crates/cf/src/board/task.rs',
        'let transcript = format!("/api/tasks/{number}/transcript?last={last}");',
        'let transcript = format!("/api/tasks/{number}/transcript?last=10");',
      ],
    ],
    runs: [API],
    meant: CF_RUNS,
  },
  {
    name: 'players: cf writes the body of a task with its text first',
    edits: [
      [
        'crates/cf/src/board/task.rs',
        lines('    let mut body = Map::new();', '    if mine {'),
        lines(
          '    let mut body = Map::new();',
          '    body.insert("body".into(), String::new().into());',
          '    if mine {',
        ),
      ],
    ],
    runs: [API],
    meant: CF_RUNS,
  },
  {
    name: 'players: cf asks who it is twice',
    edits: [
      [
        'crates/cf/src/board/mod.rs',
        '    let answer = board.get("/api/whoami")?;',
        lines(
          '    let _ = board.get("/api/whoami");',
          '    let answer = board.get("/api/whoami")?;',
        ),
      ],
    ],
    runs: [API],
    meant: CF_RUNS,
  },
  {
    name: 'players: a page operation leaves a file behind',
    edits: [
      [
        `${DAEMON}/page/mod.rs`,
        '    let kick = Rc::clone(&page.kick);',
        lines(
          '    let kick = Rc::clone(&page.kick);',
          '    let home = page.env.text("CONSENSFLOW_HOME").map(str::to_owned);',
        ),
      ],
      [
        `${DAEMON}/page/mod.rs`,
        '            if operation.kicks() {',
        lines(
          '            if let Some(home) = &home {',
          '                let _ = std::fs::write(std::path::Path::new(home).join("left-behind"), "x");',
          '            }',
          '            if operation.kicks() {',
        ),
      ],
    ],
    runs: [PAGE],
    meant: PAGE_TRACES,
  },
  {
    name: 'players: the bridge writes a space after "body":',
    edits: [
      [
        'crates/cf-bridge/src/local/serve.rs',
        '        inner.write(line);',
        lines(
          '        let line = String::from_utf8_lossy(&line)',
          '            .replacen("\\"body\\":", "\\"body\\": ", 1)',
          '            .into_bytes();',
          '        inner.write(line);',
        ),
      ],
    ],
    runs: [PAGE],
    meant: PAGE_TRACES,
  },
  {
    name: 'players: the world masks every time of every file',
    edits: [
      [
        `${TESTS}/support/wrote.rs`,
        'Regex::new(r#"("(?:createdAt|updatedAt)": )"\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}\\.\\d{3}Z""#)',
        'Regex::new(r#"()"\\d{4}-\\d{2}-\\d{2}T\\d{2}:\\d{2}:\\d{2}\\.\\d{3}Z""#)',
      ],
      [
        `${TESTS}/support/wrote.rs`,
        `    if path.rsplit('/').next() == Some("agents.json") {`,
        `    if path.rsplit('/').next().is_some() {`,
      ],
    ],
    runs: [SCREENS],
    meant: 'the_world_masks_what_the_recorder_masks_and_nothing_else',
  },
  {
    name: 'players: the API’s traces are not played to the close of their ledger',
    edits: [NO_CLOSE],
    runs: [API],
    meant: API_TRACES,
  },
  {
    name: 'players: the screens’ traces are not played to the close of their ledger',
    edits: [NO_CLOSE],
    runs: [SCREENS],
    meant: 'corners_screens_004',
  },
  {
    name: 'players: the page’s traces are not played to the close of their ledger',
    edits: [NO_CLOSE],
    runs: [PAGE],
    meant: PAGE_TRACES,
  },
]
