/**
 * Plants in the standalone verbs and in the roster they write through: the
 * recording of Node's CLI, replayed against the native `cf`, must catch each.
 */
import { GOLDENS, lines, STANDALONE } from './kit.mjs'

const AGENT = `${STANDALONE}/agent.rs`
const CATALOG = `${STANDALONE}/catalog.rs`
const MOD = `${STANDALONE}/mod.rs`
const MEANT = 'every_case_says_writes_and_exits_as_node_did'

const verb = (name, edits) => ({ name: `verbs: ${name}`, edits, runs: [GOLDENS], meant: MEANT })

export const PLANTS = [
  verb('a name of the catalog is padded to 13, not 12', [
    [CATALOG, 'pad_end(&entry.name, 12),', 'pad_end(&entry.name, 13),'],
  ]),
  verb('the table of a harness is no wider than 33 at the least', [
    [CATALOG, '.fold(34, usize::max)', '.fold(33, usize::max)'],
  ]),
  verb('the model of an agent is padded to 35, not 36', [
    [AGENT, 'pad_end(model, 36),', 'pad_end(model, 35),'],
  ]),
  verb('the home is made before an agent is refused', [
    [
      AGENT,
      lines('    let catalog = bundled()?;', '    match action {'),
      lines(
        '    let catalog = bundled()?;',
        '    if let Some(path) = roster_path(env) {',
        '        let _ = std::fs::create_dir_all(path.parent().unwrap_or(&path));',
        '    }',
        '    match action {',
      ),
    ],
  ]),
  verb('a catalog name is refused only once the harness and the model are given', [
    [
      AGENT,
      'if let Some(name) = name.filter(|name| catalog.entry(name).is_some()) {',
      'if let Some(name) = name.filter(|name| parsed.text("model").is_some() && catalog.entry(name).is_some()) {',
    ],
  ]),
  verb('a work tier of auto is kept on an added agent', [
    [AGENT, '.filter(|tier| *tier != "auto")', '.filter(|_| true)'],
  ]),
  verb('the image flag is dropped from an added agent', [
    [AGENT, 'input.insert("designer".to_owned(), Value::Bool(true));', 'let _ = &input;'],
  ]),
  verb('an edit with no name does not read the file first', [
    [AGENT, 'roster.preferences()?;', 'let _ = roster;'],
  ]),
  verb('the table goes on past an agent with no model', [
    [AGENT, 'return Err(Stop::Said(NOTHING_TO_PAD.to_owned()));', 'continue;'],
  ]),
  verb('the usage ends without its blank line', [
    [
      MOD,
      'writeln!(out, "{}", usage()).map_err(Stop::from)',
      'write!(out, "{}", usage()).map_err(Stop::from)',
    ],
  ]),
  verb('a refusal exits with 0', [[MOD, 'Ok(Some(1))', 'Ok(Some(0))']]),
  verb("an unknown command is said without JSON's escapes", [
    [MOD, 'js::stringify(&Value::from(other))', 'format!("\\"{other}\\"")'],
  ]),
  verb('the usage has no doctor', [
    [
      `${STANDALONE}/usage.txt`,
      'doctor                                    Inspect runtime, roster and bundled roles',
      '',
    ],
  ]),
  verb('an edit does not stamp the time', [
    [
      'crates/cf-catalog/src/roster/edit.rs',
      'row.set("updatedAt", Value::from(iso(clock.now_ms())));',
      'let _ = &clock;',
    ],
  ]),
  verb('an add stamps two instants', [
    [
      'crates/cf-catalog/src/roster/add.rs',
      'row.insert("updatedAt".to_owned(), now);',
      'row.insert("updatedAt".to_owned(), Value::from(iso(clock.now_ms() + 1)));',
    ],
  ]),
]
