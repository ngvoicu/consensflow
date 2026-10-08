/**
 * Every `cf` verb and sub-verb answers `--help` and `-h` with its usage, exit
 * code 0, and asks the board nothing and reads nothing of the home (the chief's
 * report of 2026-10-08: `cf note --help` posted a note saying "--help"). The
 * word is text unless it stands where only a flag could: it is the only word the
 * command was given that is no flag, after the number the command takes first,
 * with no `--` ahead of it. One plant takes one rule out; a test of it fails.
 */
import { cargo, lines, STANDALONE } from './kit.mjs'

const BOARD = 'crates/cf/src/board'
const MOD = `${BOARD}/mod.rs`
const TASK = `${BOARD}/task.rs`
const WORDS = `${BOARD}/words.rs`
const USAGE = `${BOARD}/usage.rs`

/** The board's words: which word asks for help, and which is text. */
const words = cargo('-p', 'cf', '--lib', 'board::words')
/** The usage each board command answers with. */
const usage = cargo('-p', 'cf', '--lib', 'board::usage')
/** The commands as a window runs them (`help`), and the standalone verbs as a process (`help_standalone`). */
const help = cargo('-p', 'cf', '--test', 'help', '--test', 'help_standalone')
/** What the standalone module answers for the words. */
const standalone = cargo('-p', 'cf', '--lib', 'standalone')

/** What a board command does when the words ask for its usage. */
const answers = (path) => `        return Ok(help_of(&[${path}]));`

/** A plant that takes the answer of one board command out of `file`. */
const forgets = (name, file, path, run, meant) => ({
  name: `help: ${name}`,
  edits: [[file, answers(path), '        {}']],
  runs: [help, run],
  meant,
})

export const PLANTS = [
  {
    name: 'help: -h is no word that asks for help',
    edits: [
      [
        WORDS,
        'const HELP: [&str; 2] = ["--help", "-h"];',
        'const HELP: [&str; 2] = ["--help", "--help"];',
      ],
    ],
    runs: [words, help],
    meant: 'help_is_asked_by_the_one_word_that_is_no_flag_and_by_nothing_else',
  },
  {
    name: 'help: the word after a double dash asks for help',
    edits: [
      [
        WORDS,
        lines('        !self.literal', '            && self.operands.len() <= self.leading + 1'),
        '        self.operands.len() <= self.leading + 1',
      ],
    ],
    runs: [words, help],
    meant: 'help_is_asked_by_the_one_word_that_is_no_flag_and_by_nothing_else',
  },
  {
    name: 'help: the word among other words asks for help',
    edits: [[WORDS, '            && self.operands.len() <= self.leading + 1\n', '']],
    runs: [words, help],
    meant: 'help_is_asked_by_the_one_word_that_is_no_flag_and_by_nothing_else',
  },
  {
    name: 'help: a command that takes a number first asks for help only as the first word',
    edits: [[WORDS, 'self.operands.len() <= self.leading + 1', 'self.operands.len() <= 1']],
    runs: [words, help],
    meant: 'a_number_a_command_takes_first_may_stand_before_the_word_that_asks_for_help',
  },
  {
    name: 'help: a double dash before the text is text',
    edits: [
      [
        WORDS,
        '        if word == "--" && split.operands.len() <= shape.leading {',
        '        if false {',
      ],
    ],
    runs: [words, help],
    meant: 'a_double_dash_before_the_text_ends_the_flags_and_is_not_text_and_one_after_it_is',
  },
  {
    name: 'help: a double dash after the text ends the flags',
    edits: [
      [
        WORDS,
        '        if word == "--" && split.operands.len() <= shape.leading {',
        '        if word == "--" {',
      ],
    ],
    runs: [words],
    meant: 'a_double_dash_before_the_text_ends_the_flags_and_is_not_text_and_one_after_it_is',
  },
  {
    name: 'help: the number a command takes first is no word before a double dash',
    edits: [
      [
        WORDS,
        '        if word == "--" && split.operands.len() <= shape.leading {',
        '        if word == "--" && split.operands.is_empty() {',
      ],
    ],
    runs: [words, help],
    meant: 'a_double_dash_before_the_text_ends_the_flags_and_is_not_text_and_one_after_it_is',
  },
  {
    name: 'help: the number a command takes first is part of its text',
    edits: [[WORDS, '        .get(shape.leading..)', '        .get(0..)']],
    runs: [words, help],
    meant: 'a_double_dash_before_the_text_ends_the_flags_and_is_not_text_and_one_after_it_is',
  },
  forgets(
    'cf note posts --help as a note',
    MOD,
    '"note"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf ask asks --help of the chief',
    MOD,
    '"ask"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf tell puts --help to a window',
    MOD,
    '"tell"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf answer answers --help',
    MOD,
    '"answer"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf staff reads the staff for --help',
    MOD,
    '"staff"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf whoami reads who it is for --help',
    MOD,
    '"whoami"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf history reads the history for --help',
    MOD,
    '"history"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf inbox lists the inbox for --help',
    MOD,
    '"inbox"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf task list reads the board for --help',
    TASK,
    '"task", "list"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  forgets(
    'cf task add adds --help as a task',
    TASK,
    '"task", "add"',
    help,
    'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  ),
  {
    name: 'help: cf task get, done and the others take --help for their task',
    edits: [[TASK, '            return Ok(help_of(&["task", verb]));', '            {}']],
    runs: [help],
    meant: 'every_board_command_answers_help_with_its_usage_and_asks_the_board_nothing',
  },
  {
    name: 'help: cf task get is asked for help without the flags it takes',
    edits: [
      [
        TASK,
        '            ..if verb == "get" { GET } else { Shape::TEXT }',
        '            ..Shape::TEXT',
      ],
    ],
    runs: [help],
    meant: 'the_word_may_stand_after_the_number_a_command_takes_first_or_beside_its_flags',
  },
  {
    name: 'help: cf inbox read is asked for help as the list is',
    edits: [
      [
        MOD,
        lines('    let asked = if reading {', '        Shape::NUMBERED', '    } else {'),
        lines('    let asked = if reading {', '        Shape::TEXT', '    } else {'),
      ],
    ],
    runs: [help],
    meant: 'the_word_may_stand_after_the_number_a_command_takes_first_or_beside_its_flags',
  },
  {
    name: 'help: a command takes its text as the words that were no flag, a double dash among them',
    edits: [
      [
        MOD,
        '    let question = require_text(text_of(words.text, input)?, "cf ask \\"your question\\"")?;',
        '    let question = require_text(text_of(rest.join(" "), input)?, "cf ask \\"your question\\"")?;',
      ],
    ],
    runs: [help],
    meant: 'a_text_that_holds_the_word_among_others_or_after_a_double_dash_is_text',
  },
  {
    name: 'help: the usage of a command goes on after the commands',
    edits: [
      [
        USAGE,
        lines(
          '        .skip_while(|line| !line.starts_with("  cf "))',
          '        .take_while(|line| !line.is_empty());',
        ),
        '        .skip_while(|line| !line.starts_with("  cf "));',
      ],
    ],
    runs: [usage, help],
    meant: 'what_the_usage_says_after_its_commands_is_no_commands',
  },
  {
    name: 'help: the lines of what cf task add takes besides are none of its usage',
    edits: [[USAGE, '        return path == ["task", "add"];', '        return false;']],
    runs: [usage, help],
    meant: 'task_add_takes_the_lines_that_say_what_it_takes_besides_and_none_of_the_other_commands',
  },
  {
    name: 'help: a line that names two commands is the usage of the first',
    edits: [
      [
        USAGE,
        "            .is_some_and(|word| word.split('|').any(|each| each == *wanted))",
        '            .is_some_and(|word| word == *wanted)',
      ],
    ],
    runs: [usage, help],
    meant: 'a_line_that_names_two_commands_is_the_usage_of_each',
  },
  {
    name: 'help: the lines that go on from a usage line are not its own',
    edits: [
      [
        USAGE,
        lines(
          '        if kept {',
          '            lines.push(line);',
          '        }',
          '    }',
          '    lines.join("\\n")',
        ),
        lines(
          '        if kept && !line.starts_with(&" ".repeat(CONTINUED)) {',
          '            lines.push(line);',
          '        }',
          '    }',
          '    lines.join("\\n")',
        ),
      ],
    ],
    runs: [usage, help],
    meant: 'a_command_is_its_line_of_the_list_with_the_lines_that_go_on_from_it',
  },
  {
    name: 'help: a verb of cf outside a window takes -h for no word that asks',
    edits: [[`${STANDALONE}/mod.rs`, ' || word == "-h"', '']],
    runs: [standalone, help],
    meant: 'help_is_asked_by_the_word_among_a_verbs_words_ahead_of_a_double_dash',
  },
  {
    name: 'help: a verb of cf outside a window takes the word after a double dash for help',
    edits: [[`${STANDALONE}/mod.rs`, '        .take_while(|word| *word != "--")\n', '']],
    runs: [standalone, help],
    meant: 'a_double_dash_makes_the_word_a_positional_as_it_was',
  },
  {
    name: 'help: cf -h is an unknown command',
    edits: [
      [
        `${STANDALONE}/mod.rs`,
        '        None | Some("help" | "--help" | "-h") =>',
        '        None | Some("help" | "--help") =>',
      ],
    ],
    runs: [standalone, help],
    meant: 'the_usage_is_the_one_node_printed_with_this_builds_version_and_a_blank_line_at_its_end',
  },
  ...['catalog', 'agent', 'setup', 'doctor', 'ui'].map((verb) => ({
    name: `help: cf ${verb} takes --help for a word it refuses or runs on`,
    edits: [
      [
        `${STANDALONE}/mod.rs`,
        '        Some(verb @ ("catalog" | "agent" | "setup" | "doctor" | "ui")) if asks_for_help(rest) => {',
        `        Some(verb @ (${['catalog', 'agent', 'setup', 'doctor', 'ui']
          .filter((other) => other !== verb)
          .map((other) => JSON.stringify(other))
          .join(' | ')})) if asks_for_help(rest) => {`,
      ],
    ],
    runs: [standalone, help],
    meant: 'a_verb_answers_help_before_it_reads_its_words_or_looks_at_the_home',
  })),
  {
    name: 'help: cf agent remove --help is the usage of agent alone',
    edits: [
      [
        `${STANDALONE}/mod.rs`,
        '        .filter(|word| ["add", "list", "edit", "remove"].contains(word));',
        '        .filter(|word| ["add", "list", "edit"].contains(word));',
      ],
    ],
    runs: [help],
    meant: 'every_standalone_verb_answers_help_with_its_usage_and_reads_and_writes_nothing',
  },
  {
    name: 'help: an action the verb has not is the usage of that action',
    edits: [[`${STANDALONE}/mod.rs`, '        .filter(|_| verb == "agent")\n', '']],
    runs: [standalone],
    meant: 'the_usage_of_a_verb_is_its_lines_with_those_that_go_on_from_them',
  },
  {
    name: 'help: the lines that go on from a verb of cf outside a window are not its own',
    edits: [
      [
        `${STANDALONE}/mod.rs`,
        '            Some(_) => {}\n',
        '            Some(_) => kept = false,\n',
      ],
    ],
    runs: [standalone, help],
    meant: 'the_usage_of_a_verb_is_its_lines_with_those_that_go_on_from_them',
  },
  {
    name: 'help: cf ui --help starts the daemon',
    edits: [
      [
        'crates/cf/src/lib.rs',
        lines(
          '    let asked = first == "ui"',
          '        && env.text("CONSENSFLOW_TOKEN").is_none()',
          '        && !standalone::asks_for_help(&words);',
        ),
        lines(
          '    let asked = first == "ui"',
          '        && env.text("CONSENSFLOW_TOKEN").is_none();',
          '    let _ = &words;',
        ),
      ],
    ],
    runs: [help],
    meant: 'ui_answers_help_with_its_usage_and_starts_nothing',
  },
]
