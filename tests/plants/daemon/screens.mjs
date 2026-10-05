/** The screens: the token, the order of the checks, the routes, the pages, the feed's client. */
import { DAEMON, daemon, lines } from './kit.mjs'

const SCREENS = `${DAEMON}/screens`
/** The player of Node's recordings (`tests/screens`). */
const PLAYER = ['-p', 'cf-daemon', '--test', 'screens']
const unit = daemon('screens::')

export const PLANTS = [
  {
    name: 'screens: an empty bearer is the token',
    edits: [
      [
        `${SCREENS}/mod.rs`,
        lines(
          '            .bearer()',
          '            .filter(|bearer| !bearer.is_empty())',
          '            .or_else(|| request.param("token"))',
        ),
        lines('            .bearer()', '            .or_else(|| request.param("token"))'),
      ],
    ],
    runs: [unit],
    meant: 'the_token_is_the_bearer_or_else_the_first_token_of_the_query',
  },
  {
    name: 'screens: the query’s token comes before the bearer',
    edits: [
      [
        `${SCREENS}/mod.rs`,
        lines(
          '        let presented = request',
          '            .bearer()',
          '            .filter(|bearer| !bearer.is_empty())',
          '            .or_else(|| request.param("token"))',
        ),
        lines(
          '        let presented = request',
          '            .param("token")',
          '            .or_else(|| request.bearer().filter(|bearer| !bearer.is_empty()))',
        ),
      ],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_001',
  },
  {
    name: 'screens: any token opens them',
    edits: [
      [
        `${SCREENS}/mod.rs`,
        '        !presented.is_empty() && token_matches(presented, &self.token)',
        '        !presented.is_empty()',
      ],
    ],
    runs: [PLAYER],
    meant: 'core_agents_server_001',
  },
  {
    name: 'screens: the bare 401 says why',
    edits: [
      [
        `${SCREENS}/mod.rs`,
        'return Some(Answer::json(401, json!({ "error": "unauthorized" })));',
        'return Some(Answer::json(401, json!({ "error": "unauthorized", "message": "no" })));',
      ],
    ],
    runs: [PLAYER],
    meant: 'core_agents_server_001',
  },
  {
    name: 'screens: a GET’s body is read',
    edits: [
      [
        `${SCREENS}/mod.rs`,
        lines(
          '        let body = if get {',
          '            Value::Object(Map::new())',
          '        } else {',
          '            body::read(request).await?',
          '        };',
        ),
        '        let body = body::read(request).await?;',
      ],
    ],
    runs: [unit],
    meant: 'a_get_reads_nothing_of_its_body_and_every_other_method_reads_it_before_any_route',
  },
  {
    name: 'screens: a page is not asked for its body',
    edits: [
      [
        `${SCREENS}/mod.rs`,
        '        let body = if get {',
        lines(
          '        if !get && matches!(screen, Screen::Page) {',
          '            return Ok(Answer::json(404, json!({ "error": "not found" })));',
          '        }',
          '        let body = if get {',
        ),
      ],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_002',
  },
  {
    name: 'screens: not found is said another way',
    edits: [
      [`${SCREENS}/mod.rs`, 'json!({ "error": "not found" })', 'json!({ "error": "not-found" })'],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_002',
  },
  {
    name: 'screens: an agent’s name may begin with any letter',
    edits: [
      [
        `${SCREENS}/mod.rs`,
        '.is_some_and(|first| first.is_ascii_lowercase())',
        '.is_some_and(|first| first.is_ascii_alphabetic())',
      ],
    ],
    runs: [unit],
    meant: 'any_other_path_is_the_agents_api_s',
  },
  {
    name: 'screens: an agent added is not told',
    edits: [
      [
        `${SCREENS}/agents.rs`,
        lines(
          '            .map_err(words)?;',
          '        (self.on_roster_change)()?;',
          '        Ok(Answer::created(json!({ "agent": agent })))',
        ),
        lines(
          '            .map_err(words)?;',
          '        Ok(Answer::created(json!({ "agent": agent })))',
        ),
      ],
    ],
    runs: [unit],
    meant: 'an_agent_is_added_edited_and_removed_and_each_is_told',
  },
  {
    name: 'screens: a delete tells before it removes',
    edits: [
      [
        `${SCREENS}/agents.rs`,
        '        self.agents.roster().remove(name).map_err(words)?;',
        lines(
          '        (self.on_roster_change)()?;',
          '        self.agents.roster().remove(name).map_err(words)?;',
        ),
      ],
    ],
    runs: [unit],
    meant: 'a_write_the_roster_refuses_is_a_400_in_its_words_and_is_not_told',
  },
  {
    name: 'screens: a delete removes nothing',
    edits: [
      [
        `${SCREENS}/agents.rs`,
        '        self.agents.roster().remove(name).map_err(words)?;',
        '        let _ = name;',
      ],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_005',
  },
  {
    name: 'screens: a delete answers 200',
    edits: [[`${SCREENS}/agents.rs`, 'Answer::nothing(204)', 'Answer::nothing(200)']],
    runs: [unit],
    meant: 'an_agent_is_added_edited_and_removed_and_each_is_told',
  },
  {
    name: 'screens: an agent added answers 200',
    edits: [
      [
        `${SCREENS}/agents.rs`,
        'Answer::created(json!({ "agent": agent }))',
        'Answer::ok(json!({ "agent": agent }))',
      ],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_005',
  },
  {
    name: 'screens: an agent’s edit reads the model before the tier',
    edits: [
      [
        `${SCREENS}/agents.rs`,
        'let patch = fields(body, "workTier")?;',
        'let patch = fields(body, "model")?;',
      ],
    ],
    runs: [unit],
    meant: 'a_body_that_is_no_object_is_read_as_javascript_read_it',
  },
  {
    name: 'screens: an agent that is not offered says notInstalled before hidden',
    edits: [
      [
        `${SCREENS}/agents.rs`,
        lines(
          '                fields.insert("hidden".to_owned(), Value::Bool(true));',
          '                fields.insert("notInstalled".to_owned(), Value::Bool(true));',
        ),
        lines(
          '                fields.insert("notInstalled".to_owned(), Value::Bool(true));',
          '                fields.insert("hidden".to_owned(), Value::Bool(true));',
        ),
      ],
    ],
    runs: [unit],
    meant: 'an_agent_that_cannot_be_offered_is_said_hidden_and_not_installed_in_that_order',
  },
  {
    name: 'screens: every harness is said installed',
    edits: [
      [
        `${SCREENS}/agents.rs`,
        '            .filter(|harness| !missing.contains(harness))',
        '            .filter(|_| true)',
      ],
    ],
    runs: [unit],
    meant: 'the_agents_are_the_catalog_s_as_the_pickers_offer_them',
  },
  {
    name: 'screens: the efforts leave a harness out',
    edits: [
      [
        `${SCREENS}/agents.rs`,
        '            .map(|harness| (harness.as_str().to_owned(), json!(efforts(harness))))',
        lines(
          '            .filter(|harness| *harness != Harness::Devin)',
          '            .map(|harness| (harness.as_str().to_owned(), json!(efforts(harness))))',
        ),
      ],
    ],
    runs: [PLAYER],
    meant: 'core_agents_server_006',
  },
  {
    name: 'screens: a check is looked at again for any refresh',
    edits: [
      [
        `${SCREENS}/harnesses.rs`,
        '        let refresh = property(body, "refresh")? == Some(&Value::Bool(true));',
        '        let refresh = property(body, "refresh")?.is_some();',
      ],
    ],
    runs: [unit],
    meant: 'a_check_of_one_harness_is_kept_five_minutes_and_looked_at_again_only_when_asked_to',
  },
  {
    name: 'screens: a check names no harness',
    edits: [
      [
        `${SCREENS}/harnesses.rs`,
        'self.admin.check(id.map(Harness::as_str), refresh)',
        'self.admin.check(None, refresh)',
      ],
    ],
    runs: [unit],
    meant: 'a_check_of_one_harness_is_kept_five_minutes_and_looked_at_again_only_when_asked_to',
  },
  {
    name: 'screens: a harness that is none of the five is Claude',
    edits: [
      [
        `${SCREENS}/harnesses.rs`,
        '        .ok_or_else(|| UNKNOWN.to_owned())',
        lines('        .or(Some(Harness::Claude))', '        .ok_or_else(|| UNKNOWN.to_owned())'),
      ],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_009',
  },
  {
    name: 'screens: an unknown harness is said more',
    edits: [
      [
        `${SCREENS}/harnesses.rs`,
        'const UNKNOWN: &str = "Unknown harness";',
        'const UNKNOWN: &str = "Unknown harnesses";',
      ],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_009',
  },
  {
    name: 'screens: null is read in other words than V8’s',
    edits: [
      [
        `${SCREENS}/body.rs`,
        '"Cannot read properties of null (reading \'{name}\')"',
        '"Cannot read property of null (reading \'{name}\')"',
      ],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_006',
  },
  {
    name: 'screens: no body is not an empty object',
    edits: [[`${SCREENS}/body.rs`, '    if text.is_empty() {', '    if false {']],
    runs: [PLAYER],
    meant: 'corners_screens_003',
  },
  {
    name: 'screens: null is a body a roster operation reads',
    edits: [[`${SCREENS}/body.rs`, '    property(body, first)?;', '    let _ = first;']],
    runs: [PLAYER],
    meant: 'corners_screens_006',
  },
  {
    name: 'screens: a body that is no JSON says it in no words of the daemon’s',
    edits: [
      [
        `${SCREENS}/body.rs`,
        '        .map_err(|failed| format!("the request body is not valid JSON: {failed}"))',
        '        .map_err(|failed| failed.to_string())',
      ],
    ],
    runs: [PLAYER],
    meant: 'corners_screens_002',
  },
  {
    name: 'screens: the token goes into the page as it is',
    edits: [
      [
        `${SCREENS}/pages.rs`,
        '        &js::stringify(&Value::String(token.to_owned())),',
        '        &format!("\\"{token}\\""),',
      ],
    ],
    runs: [unit],
    meant: 'a_token_that_needs_escaping_is_written_as_json_stringify_writes_it',
  },
  {
    name: 'screens: the version is not put into the page',
    edits: [
      [
        `${SCREENS}/pages.rs`,
        'AGENTS.replacen("$VERSION", VERSION, 1)',
        'AGENTS.replacen("$VERSION", "0", 1)',
      ],
    ],
    runs: [PLAYER],
    meant: 'core_agents_server_003',
  },
  {
    name: 'screens: the agents page is not the recorded one',
    edits: [
      [
        `${SCREENS}/pages/agents.html`,
        '<p class="eyebrow eyebrow--section">Define your own</p>',
        '<p class="eyebrow eyebrow--section">Define yours</p>',
      ],
    ],
    runs: [PLAYER],
    meant: 'core_agents_server_003',
  },
  {
    name: 'network: a redirect is followed',
    edits: [
      [`${SCREENS}/network.rs`, '.redirect(Policy::none())', '.redirect(Policy::limited(5))'],
    ],
    runs: [unit],
    meant: 'a_redirect_is_an_answer_like_another_and_is_not_followed',
  },
  {
    name: 'network: a connection cut in the body is said as one that failed',
    edits: [
      [
        `${SCREENS}/network.rs`,
        'Err(_) => Err(TERMINATED.to_owned()),',
        'Err(_) => Err(FETCH_FAILED.to_owned()),',
      ],
    ],
    runs: [unit],
    meant: 'a_connection_that_closes_while_the_body_comes_is_terminated',
  },
  {
    name: 'network: a connection that cannot be made is said in the client’s words',
    edits: [
      [
        `${SCREENS}/network.rs`,
        '.map_err(|_| FETCH_FAILED.to_owned())?;',
        '.map_err(|failed| failed.to_string())?;',
      ],
    ],
    runs: [unit],
    meant: 'a_connection_that_cannot_be_made_is_fetch_failed',
  },
  {
    name: 'network: the client names no one',
    edits: [
      [
        `${SCREENS}/network.rs`,
        lines('        .no_proxy()', '        .user_agent(format!("ConsensFlow/{VERSION}"))'),
        '        .no_proxy()',
      ],
    ],
    runs: [unit],
    meant: 'the_request_is_a_plain_get_that_names_its_client',
  },
  {
    name: 'network: a crypto provider is installed for the whole process',
    edits: [
      [
        `${SCREENS}/network.rs`,
        '    let provider = Arc::new(rustls::crypto::ring::default_provider());',
        lines(
          '    let provider = Arc::new(rustls::crypto::ring::default_provider());',
          '    let _ = rustls::crypto::ring::default_provider().install_default();',
        ),
      ],
    ],
    runs: [unit],
    meant: 'the_client_is_made_on_ring_and_installs_nothing_process_wide',
  },
  {
    name: 'start: the screens are given no environment',
    edits: [
      [
        `${DAEMON}/start.rs`,
        lines('        env: env.clone(),', '        agents: Rc::clone(&agents),'),
        lines('        env: Env::default(),', '        agents: Rc::clone(&agents),'),
      ],
    ],
    runs: [daemon('start::tests::screens::')],
    meant: 'the_daemon_serves_the_screens_under_its_token_over_its_own_environment',
  },
]
