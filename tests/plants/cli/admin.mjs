/**
 * Plants in `cf setup` and `cf doctor`, and in the player that holds them to what
 * Node said: the recording of Node's CLI replayed against the native `cf` must
 * catch each, or a test of the module or of the process written for it. The last
 * group plants the player's own pairings, so that what is paired is shown to be
 * held and not merely written down.
 */
import { GOLDENS, lines, PROCESS, STANDALONE, UNITS } from './kit.mjs'

const SETUP = `${STANDALONE}/setup.rs`
const DOCTOR = `${STANDALONE}/doctor.rs`
const MOD = `${STANDALONE}/mod.rs`
const PLAYER = 'crates/cf/tests/cli_goldens'
const MEANT = 'every_case_says_writes_and_exits_as_node_did'

const PARSE = 'args::parse(words, &[], Positionals::Refused).map_err(Stop::Said)?;'
const RUNTIME = lines(
  '    if let Some(wiring) = runtime(env, &cf, &Places::default()).map_err(Stop::Said)? {',
  '        writeln!(out, "{}", wiring.report())?;',
  '    }',
  '',
)

const golden = (name, edits) => ({
  name: `setup and doctor: ${name}`,
  edits,
  runs: [GOLDENS],
  meant: MEANT,
})
const unit = (name, edits, meant) => ({
  name: `setup and doctor: ${name}`,
  edits,
  runs: [UNITS],
  meant,
})

export const PLANTS = [
  golden('the roster is read before the launcher is made', [
    [
      SETUP,
      'let prepared = prepare_app(env, &own_cf()?);',
      'let saved = agents_saved(env)?;\n    let prepared = prepare_app(env, &own_cf()?);',
    ],
    [SETUP, lines('        agents_saved(env)?', '    )?;'), lines('        saved', '    )?;')],
  ]),
  golden('the words of setup are read after the launcher is made', [
    [SETUP, `    ${PARSE}\n`, ''],
    [
      SETUP,
      'let prepared = prepare_app(env, &own_cf()?);',
      `let prepared = prepare_app(env, &own_cf()?);\n    ${PARSE}`,
    ],
  ]),
  golden('setup takes a word that is no option', [
    [SETUP, 'Positionals::Refused', 'Positionals::Allowed'],
  ]),
  golden('setup says what doctor says of no harness', [
    [SETUP, '"none found on PATH".to_owned()', '"none on PATH".to_owned()'],
  ]),
  golden('setup says nothing of a launcher it could not make', [
    [
      SETUP,
      lines('    for line in &prepared.report {', '        writeln!(out, "{line}")?;', '    }'),
      '    let _ = &prepared.report;',
    ],
  ]),
  golden('the agents are counted one short', [
    [
      MOD,
      'Ok(roster(env, &catalog)?.list()?.len())',
      'Ok(roster(env, &catalog)?.list()?.len() - 1)',
    ],
  ]),
  golden('doctor reads its words', [
    [
      MOD,
      'Some("doctor") => doctor::run(env, out),',
      'Some("doctor") => cf_base::args::parse(rest, &[], cf_base::args::Positionals::Refused).map_err(Stop::Said).and_then(|_| doctor::run(env, out)),',
    ],
  ]),
  golden('doctor says the home before the version', [
    [
      DOCTOR,
      lines(
        '    writeln!(out, "consensflow {}", env!("CARGO_PKG_VERSION"))?;',
        '    writeln!(out, "home:         {}", home.to_string_lossy())?;',
      ),
      lines(
        '    writeln!(out, "home:         {}", home.to_string_lossy())?;',
        '    writeln!(out, "consensflow {}", env!("CARGO_PKG_VERSION"))?;',
      ),
    ],
  ]),
  golden('doctor says setup’s words of no harness', [
    [DOCTOR, '"none on PATH".to_owned()', '"none found on PATH".to_owned()'],
  ]),
  golden('doctor words the roles another way', [
    [DOCTOR, 'prepared when a window launches', 'prepared when a pane launches'],
  ]),
  golden('doctor leaves out the hooks of an older version', [
    [
      DOCTOR,
      'if let Some(line) = stale_hooks(env).report() {',
      'if let Some(line) = stale_hooks(env).report().filter(|_| false) {',
    ],
  ]),
  golden('doctor says the hooks before the command', [
    [DOCTOR, RUNTIME, ''],
    [DOCTOR, lines('    Ok(())', '}'), lines(RUNTIME.trimEnd(), '    Ok(())', '}')],
  ]),
  unit(
    'doctor does not say what stops it in reading the command',
    [
      [
        DOCTOR,
        'runtime(env, &cf, &Places::default()).map_err(Stop::Said)?',
        'runtime(env, &cf, &Places::default()).unwrap_or(None)',
      ],
    ],
    'doctor_says_what_stops_it_after_the_lines_it_has_said',
  ),
  unit(
    'setup goes on without a home to keep its things in',
    [[SETUP, '    home(env)?;\n', '']],
    'with_no_folder_to_keep_the_agents_in_a_verb_that_needs_it_says_so',
  ),
  unit(
    'setup is an unknown command',
    [[MOD, '        Some("setup") => setup::run(env, rest, out),\n', '']],
    'with_no_folder_to_keep_the_agents_in_a_verb_that_needs_it_says_so',
  ),
  unit(
    'doctor is an unknown command',
    [
      [
        MOD,
        lines(
          '        // Whatever words follow it are no matter, as in Node.',
          '        Some("doctor") => doctor::run(env, out),',
          '',
        ),
        '',
      ],
    ],
    'with_no_folder_to_keep_the_agents_in_a_verb_that_needs_it_says_so',
  ),
  {
    name: 'setup and doctor: the cf the command names is not the one that runs',
    edits: [
      [
        MOD,
        'Ok(exe) => Ok(machine::bundle_of(&exe).cf),',
        'Ok(_) => Ok(PathBuf::from("/nowhere/cf")),',
      ],
    ],
    runs: [PROCESS],
    meant: 'the_command_setup_makes_is_the_one_doctor_says_runs_this_cf',
  },
  {
    name: 'setup and doctor: an extension is written a byte short, seen by the player',
    edits: [
      [
        'crates/cf-harness/src/shared/private_bundle.rs',
        'write_file(target, bytes, 0o600).map_err(said)?;',
        'write_file(target, &bytes[..bytes.len() - 1], 0o600).map_err(said)?;',
      ],
    ],
    runs: [GOLDENS],
    meant: MEANT,
  },
  golden('paired: the launcher Node wrote is held to Node’s own text', [
    [
      `${PLAYER}/paired.rs`,
      'entry["text"] = Value::String(native(text));',
      'entry["text"] = Value::String(text.to_owned());',
    ],
  ]),
  golden('paired: a command of another runtime is held to Node’s word of it', [
    [
      `${PLAYER}/paired.rs`,
      'expected.stdout = expected.stdout.as_deref().map(this_copy);',
      'expected.stdout = expected.stdout.clone();',
    ],
  ]),
  golden('paired: the runtime a launcher names is not put in its place', [
    [
      `${PLAYER}/names.rs`,
      '            .replace("$NODE", &self.runtime)\n            .replace("$ROOT", root)',
      '            .replace("$ROOT", root)',
    ],
  ]),
  golden('paired: the cf.mjs a launcher runs is not put in its place', [
    [
      `${PLAYER}/names.rs`,
      'text.replace("$REPO/bin/cf.mjs", &self.mjs)\n            .replace("$NODE", &self.runtime)',
      'text.replace("$NODE", &self.runtime)',
    ],
  ]),
  golden('paired: the cf a launcher runs is not written back as its name', [
    [`${PLAYER}/names.rs`, '            .replace(&self.cf, "$CF")\n', ''],
  ]),
]
