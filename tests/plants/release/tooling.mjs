/**
 * Plants in the tools that build and release (tools/xtask, tools/cf-release): the
 * version the sources must agree on, the exit status that a command hands back
 * from what it ran, and the checkout that `cargo xtask` finds from where its own
 * manifest is. The tools' own tests must catch each.
 */

const cargo = (...args) => ['cargo', 'test', '--offline', ...args]

/** The version check's own tests: every agreement, every disagreement, every form. */
const VERSION = cargo('-p', 'cf-release', '--lib', 'version::')
/** xtask run as a process, handing its words to a stand-in for node and answering its status. */
const CLI = cargo('-p', 'xtask', '--test', 'cli')
/** xtask's process module against a child that ends as it is told. */
const PROCESS = cargo('-p', 'xtask', '--test', 'process')
/** What the tools are built with, read from the lockfile. */
const CLOSURE = cargo('-p', 'xtask', '--test', 'closure')

const VERSION_RS = 'tools/cf-release/src/version.rs'
const PROCESS_RS = 'tools/xtask/src/process.rs'
const DISPATCH_RS = 'tools/xtask/src/dispatch.rs'
const CONTEXT_RS = 'tools/xtask/src/context.rs'
const CARGO_CONFIG = '.cargo/config.toml'

const AGREE = 'if package == cargo && package == tauri {'
const NUMBER = '(*number == "0" || !number.starts_with(\'0\'))'
const ALIAS = 'xtask = ["run", "--locked", "--quiet", "--package", "xtask", "--bin", "xtask", "--"]'
const STATUS = 'status.code().unwrap_or(1)'
const HANDED_BACK = 'let status = process::run(&script.invocation(context, args), &context.env)?;'
const FROM_MANIFEST = 'Self::at(Path::new(env!("CARGO_MANIFEST_DIR")), env)'

const version = (name, edits, meant) => ({
  name: `tooling: ${name}`,
  edits: edits.map(([from, to]) => [VERSION_RS, from, to]),
  runs: [VERSION],
  meant,
})

export const PLANTS = [
  version(
    'a package.json that says another version is passed',
    [[AGREE, 'if cargo == tauri {']],
    'any_file_that_says_another_version_is_refused_with_every_file_named',
  ),
  version(
    'a Cargo.toml that says another version is passed',
    [[AGREE, 'if package == tauri {']],
    'any_file_that_says_another_version_is_refused_with_every_file_named',
  ),
  version(
    'a tauri.conf.json that says another version is passed',
    [[AGREE, 'if package == cargo {']],
    'any_file_that_says_another_version_is_refused_with_every_file_named',
  ),
  version(
    'a version with a leading zero is canonical',
    [[NUMBER, 'true']],
    'what_is_not_canonical_is_named_and_what_is_canonical_is_not',
  ),
  {
    name: 'tooling: a command that fails answers 0',
    edits: [
      [
        DISPATCH_RS,
        `${HANDED_BACK}\n                Ok(status)`,
        `process::run(&script.invocation(context, args), &context.env)?;\n                Ok(0)`,
      ],
    ],
    runs: [CLI],
    meant: 'the_exit_status_of_what_it_ran_is_its_own',
  },
  {
    name: 'tooling: a program that ends with a code answers 0',
    edits: [[PROCESS_RS, STATUS, '0']],
    runs: [PROCESS, CLI],
    meant: 'the_code_a_program_ends_with_is_the_code_run_answers',
  },
  {
    name: 'tooling: a program that ends with a code answers pass or fail',
    edits: [[PROCESS_RS, STATUS, 'i32::from(!status.success())']],
    runs: [PROCESS, CLI],
    meant: 'the_code_a_program_ends_with_is_the_code_run_answers',
  },
  {
    name: 'tooling: the checkout is the folder xtask is run in',
    edits: [
      [
        CONTEXT_RS,
        FROM_MANIFEST,
        'Self::at(&std::env::current_dir().unwrap_or_default().join("tools").join("xtask"), env)',
      ],
    ],
    runs: [CLI],
    meant: 'the_checkout_is_the_same_from_the_root_from_app_and_from_anywhere_else',
  },
  {
    // The manifest and the lockfile together, so that cargo has nothing to mend before the test reads it.
    name: 'tooling: the release tool is built with the harness',
    edits: [
      [
        'tools/cf-release/Cargo.toml',
        'cf-base.workspace = true\nserde_json.workspace = true',
        'cf-base.workspace = true\ncf-harness.workspace = true\nserde_json.workspace = true',
      ],
      [
        'Cargo.lock',
        'name = "cf-release"\nversion = "3.0.0-alpha.83"\ndependencies = [\n "cf-base",\n',
        'name = "cf-release"\nversion = "3.0.0-alpha.83"\ndependencies = [\n "cf-base",\n "cf-harness",\n',
      ],
    ],
    runs: [CLOSURE],
    meant: 'neither_tool_is_built_with_the_app_or_the_daemons_crates',
  },
  {
    name: 'tooling: an alias that finds xtask from the root alone',
    edits: [
      [
        CARGO_CONFIG,
        ALIAS,
        'xtask = ["run", "--locked", "--quiet", "--manifest-path", "tools/xtask/Cargo.toml", "--bin", "xtask", "--"]',
      ],
    ],
    runs: [CLI],
    meant: 'cargo_finds_the_xtask_alias_from_the_root_and_from_app',
  },
]
