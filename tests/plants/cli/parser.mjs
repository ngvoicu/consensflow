/**
 * Plants in the parser the verbs read their words with (`cf_base::args`), and
 * in the padding of their tables: the recording of Node's `parseArgs` and of the
 * CLI must catch each.
 */
import { ARGS, GOLDENS } from './kit.mjs'

const ARGS_RS = 'crates/cf-base/src/args.rs'
const REPLAY = 'every_list_of_words_is_read_as_node_reads_it'
const HELD_TO_NODE = 'every_case_says_writes_and_exits_as_node_did'

const parser = (name, edits) => ({ name: `parser: ${name}`, edits, runs: [ARGS], meant: REPLAY })

export const PLANTS = [
  parser('a value of two units is no option-like value, only one of three', [
    [
      ARGS_RS,
      "value.len() > 1 && value.starts_with('-')",
      "value.len() > 2 && value.starts_with('-')",
    ],
  ]),
  parser('an equals right after the dashes splits the name', [
    [ARGS_RS, "if after[first..].contains('=') {", "if after.contains('=') {"],
  ]),
  parser('a letter past U+FFFF is named whole', [
    [ARGS_RS, 'if letter.len_utf16() == 2 {', 'if letter.len_utf16() == 99 {'],
  ]),
  parser('the suggestion is made where no positional is allowed, and not where one is', [
    [ARGS_RS, 'Positionals::Allowed => format!(', 'Positionals::Refused => format!('],
    [ARGS_RS, 'Positionals::Refused => String::new(),', 'Positionals::Allowed => String::new(),'],
  ]),
  parser('a text option takes only a word that does not look like an option', [
    [
      ARGS_RS,
      'let value = if takes_text(after) && at < words.len() {',
      'let value = if takes_text(after) && at < words.len() && !looks_like_an_option(&words[at]) {',
    ],
  ]),
  parser('the first of an option given twice is kept, not the last', [
    [ARGS_RS, 'Some(slot) => slot.1 = value,', 'Some(_) => {}'],
  ]),
  parser('the refusal of a positional is cut short', [
    [
      ARGS_RS,
      '"Unexpected argument \'{word}\'. This command does not take positional arguments"',
      '"Unexpected argument \'{word}\'"',
    ],
  ]),
  parser('a flag given a value is taken', [
    [ARGS_RS, '(Takes::Nothing, Some(_)) => {', '(Takes::Nothing, Some(_)) if false => {'],
  ]),
  {
    name: 'text: padEnd counts bytes, not UTF-16 units',
    edits: [
      [
        'crates/cf-base/src/text.rs',
        'width.saturating_sub(utf16_len(text)),',
        'width.saturating_sub(text.len()),',
      ],
    ],
    runs: [GOLDENS],
    meant: HELD_TO_NODE,
  },
]
