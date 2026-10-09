// biome-ignore-all lint/suspicious/noTemplateCurlyInString: a plant's text is the source it replaces, `${...}` and all, not a template
/**
 * Plants in the tool that makes the update feed's entry (tools/cf-release,
 * `prepare-update`): what holds an archive to its bundle (a path, a kind, a
 * mode, a size, a hash), what keeps an archive from being unpacked against the
 * machine, the versions that must agree, the shape of the signature, the
 * channel, the date and the notes, and the workflow's use of the tool. The
 * tool's own tests (its unit tests, and the command run on bundles and archives
 * made in the test) must catch each, and the workflow's text test the last.
 */
import { RELEASE_YML } from './kit.mjs'

const cargo = (...args) => ['cargo', 'test', '--offline', ...args]

/** The pure parts: the signature, the date, the channel, the paths, the trees, the entry. */
const UNITS = cargo('-p', 'cf-release', '--lib')
/** The command, on a bundle, an archive, a signature and notes made for each test. */
const COMMAND = cargo('-p', 'cf-release', '--test', 'prepare_update')
/** The workflow's text: where the tool is built, and what runs it. */
const WORKFLOW = [process.execPath, '--test', 'tests/release-tool.test.mjs']

const SRC = 'tools/cf-release/src'
const TREE_RS = `${SRC}/update/tree.rs`
const ARCHIVE_RS = `${SRC}/update/archive.rs`
const BUNDLE_RS = `${SRC}/update/bundle.rs`
const SIGNATURE_RS = `${SRC}/update/signature.rs`
const DATE_RS = `${SRC}/update/date.rs`
const CHANNEL_RS = `${SRC}/update/channel.rs`
const FEED_RS = `${SRC}/update/feed.rs`
const UPDATE_RS = `${SRC}/update.rs`
const ARGS_RS = `${SRC}/args.rs`

const plant = (name, file, from, to, meant, runs = [UNITS, COMMAND]) => ({
  name: `update: ${name}`,
  edits: [[file, from, to]],
  runs,
  meant,
})

export const PLANTS = [
  // What an archive is held to.
  plant(
    'an archive entry with a different mode passes',
    TREE_RS,
    '(Some(in_bundle), Some(in_archive)) if in_bundle == in_archive => None,',
    '(Some(in_bundle), Some(in_archive)) if in_bundle.kind == in_archive.kind => None,',
    'rejects_an_entry_whose_mode_is_not_the_bundles',
    [COMMAND, UNITS],
  ),
  plant(
    'a set-user-id or sticky bit that the bundle does not have passes',
    ARCHIVE_RS,
    'let mode = entry.header().mode()? & PERMISSION_BITS;',
    'let mode = entry.header().mode()? & 0o777;',
    'rejects_a_set_user_id_or_sticky_bit_that_the_bundle_does_not_have',
    [COMMAND],
  ),
  plant(
    'a file with other bytes of the same length passes',
    TREE_RS,
    'Ok((size, hex))',
    'Ok((size, String::new()))',
    'rejects_bytes_that_differ_and_are_as_many',
    [COMMAND],
  ),
  plant(
    'an archive with a path more than the bundle has passes',
    TREE_RS,
    '(None, Some(_)) => Some(format!("{name} is in the archive and not in the bundle")),',
    '(None, Some(_)) => None,',
    'rejects_a_path_the_archive_has_and_the_bundle_has_not_and_the_other_way',
    [COMMAND, UNITS],
  ),
  plant(
    'a bundle with a symlink in it passes',
    TREE_RS,
    'if file_type.is_symlink() {',
    'if false && file_type.is_symlink() {',
    'rejects_a_bundle_with_a_symlink_or_a_special_file_in_it',
    [COMMAND, UNITS],
  ),
  // What keeps an archive from being unpacked against the machine.
  plant(
    'a symlink or a hard link in the archive passes',
    ARCHIVE_RS,
    'if kind.is_symlink() || kind.is_hard_link() {',
    'if kind.is_symlink() && kind.is_hard_link() {',
    'rejects_a_link_of_either_kind_wherever_it_points',
    [COMMAND],
  ),
  plant(
    'a pipe or a device in the archive is a file',
    ARCHIVE_RS,
    '} else if kind.is_file() {',
    '} else if kind.is_file() || !kind.is_dir() {',
    'rejects_an_entry_that_is_a_pipe_or_a_device',
    [COMMAND],
  ),
  plant(
    'a path with .. in it passes',
    ARCHIVE_RS,
    'if parts.first() != Some(&ROOT) || parts.contains(&"..") {',
    'if parts.first() != Some(&ROOT) {',
    'a_path_that_leaves_the_root_or_never_was_under_it_is_unsafe',
  ),
  plant(
    'a path outside the root passes',
    ARCHIVE_RS,
    'if parts.first() != Some(&ROOT) || parts.contains(&"..") {',
    'if parts.contains(&"..") {',
    'a_path_that_leaves_the_root_or_never_was_under_it_is_unsafe',
  ),
  plant(
    'an absolute path passes',
    ARCHIVE_RS,
    "if raw.starts_with('/') {",
    "if false && raw.starts_with('/') {",
    'a_path_that_leaves_the_root_or_never_was_under_it_is_unsafe',
  ),
  plant(
    'a path given twice passes',
    ARCHIVE_RS,
    'if self.tree.contains_key(&name) {',
    'if false && self.tree.contains_key(&name) {',
    'rejects_a_path_given_twice_however_it_is_written',
    [COMMAND],
  ),
  plant(
    'an archive with no root passes',
    ARCHIVE_RS,
    'if !found.tree.contains_key(ROOT) {',
    'if false && !found.tree.contains_key(ROOT) {',
    'rejects_an_archive_with_no_root_or_no_info_plist',
    [COMMAND],
  ),
  plant(
    'an archive that is empty is read',
    ARCHIVE_RS,
    'if size == 0 {',
    'if false {',
    'rejects_missing_and_empty_archives',
    [COMMAND],
  ),
  plant(
    'an archive named for no version passes',
    ARCHIVE_RS,
    'if safe && name.ends_with(".app.tar.gz") && name.contains(version) {',
    'if safe && name.ends_with(".app.tar.gz") {',
    'a_filename_is_a_safe_asset_name_that_ends_as_an_app_archive_and_names_the_version',
  ),
  plant(
    'an Info.plist of any size is read into memory',
    ARCHIVE_RS,
    'if name != INFO_PLIST || entry.size() > INFO_PLIST_LIMIT {',
    'if name != INFO_PLIST {',
    'reads_an_info_plist_as_large_as_an_app_has_one_and_no_larger',
    [COMMAND],
  ),
  // The versions that must agree.
  plant(
    'a version mismatch passes: the sources say another',
    UPDATE_RS,
    'if source != bundle.version {',
    'if false && source != bundle.version {',
    'rejects_a_bundle_that_disagrees_with_the_source_versions',
    [COMMAND],
  ),
  plant(
    'a version mismatch passes: the archive says another',
    ARCHIVE_RS,
    'if packaged != bundle.version {',
    'if false && packaged != bundle.version {',
    'rejects_an_archive_whose_packaged_version_differs',
    [COMMAND],
  ),
  plant(
    'a bundle that is no .app folder passes',
    BUNDLE_RS,
    'if !named_app || !path.is_dir() {',
    'if !path.is_dir() {',
    'rejects_what_is_not_an_app_folder',
    [COMMAND],
  ),
  plant(
    'an executable name that leaves its folder passes',
    BUNDLE_RS,
    "if executable.contains(['/', '\\\\', '\\0']) {",
    "if false && executable.contains(['/', '\\\\', '\\0']) {",
    'rejects_an_executable_name_that_leaves_the_folder_it_is_in',
    [COMMAND],
  ),
  plant(
    "a bundle without a window's cf is read",
    BUNDLE_RS,
    'for (what, required) in [("native executable", &binary), ("a window\'s cf", &cf)] {',
    'for (what, required) in [("native executable", &binary)] {',
    'rejects_a_bundle_without_the_cf_a_window_runs',
    [COMMAND],
  ),
  // The channel, the date, the notes, the entry.
  plant(
    'the stable channel takes a prerelease',
    CHANNEL_RS,
    'Self::Stable => Err(ChannelError::NotStable),',
    'Self::Stable => Ok(()),',
    'only_a_stable_release_goes_to_the_stable_channel',
  ),
  plant(
    'the alpha channel takes any prerelease',
    CHANNEL_RS,
    'Self::Alpha if prerelease.split(\'.\').next() == Some("alpha") => Ok(()),',
    "Self::Alpha if prerelease.split('.').next().is_some() => Ok(()),",
    'the_alpha_channel_takes_prereleases_that_begin_with_alpha',
  ),
  plant(
    'a channel in other letters is one',
    CHANNEL_RS,
    '"alpha" => Ok(Self::Alpha),',
    '"alpha" | "Alpha" => Ok(Self::Alpha),',
    'a_channel_is_alpha_or_stable_and_nothing_else',
  ),
  plant(
    'a date that does not exist passes',
    DATE_RS,
    '&& (1..=days_in_month(year, month)).contains(&day)',
    '&& (1..=31).contains(&day)',
    'every_month_has_the_days_it_has',
  ),
  plant(
    'a date with no zone passes',
    DATE_RS,
    'offset(rest)?;',
    'let _ = offset(rest);',
    'what_is_not_in_the_form_is_refused',
  ),
  plant(
    'a date is not checked at all',
    UPDATE_RS,
    'date::check(&options.date).map_err(failed)?;',
    'let _ = date::check(&options.date);',
    'rejects_invalid_channels_and_dates',
    [COMMAND],
  ),
  plant(
    'notes of any length pass',
    FEED_RS,
    'if notes.len() > NOTES_LIMIT {',
    'if false && notes.len() > NOTES_LIMIT {',
    'notes_of_64_kib_are_the_most_there_can_be_counted_in_bytes',
  ),
  plant(
    'the notes keep the blank around them',
    FEED_RS,
    'let notes = js::trim(&text);',
    'let notes = &*text;',
    'the_notes_are_the_text_of_the_file_without_javascripts_blank_around_it',
  ),
  plant(
    'the entry names the release without its v',
    FEED_RS,
    '"url": format!("{DOWNLOADS}/v{}/{}", build.version, build.archive),',
    '"url": format!("{DOWNLOADS}/{}/{}", build.version, build.archive),',
    'what_the_script_this_replaced_wrote_for_a_build_is_what_this_writes',
  ),
  plant(
    'the entry has no line break at its end',
    FEED_RS,
    'format!("{}\\n", js::stringify(&entry))',
    'js::stringify(&entry)',
    'the_entry_is_one_line_in_the_order_the_updater_documents',
  ),
  plant(
    'an archive is not inspected at all',
    UPDATE_RS,
    'let asset = archive::inspect(&options.archive, &bundle).map_err(failed)?;',
    'let asset = options.archive.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();',
    'rejects_an_archive_whose_file_bytes_differ_from_the_supplied_bundle',
    [COMMAND],
  ),
  // The signature's shape.
  plant(
    'a signature cut short passes as text',
    SIGNATURE_RS,
    '.filter(|bytes| bytes.len() >= SHORTEST)',
    '.filter(|_| true)',
    'text_too_short_or_not_text_is_refused_before_its_lines_are_counted',
  ),
  plant(
    'a signature that is no UTF-8 passes as text',
    SIGNATURE_RS,
    'String::from_utf8(bytes).map_err(|_| SignatureError::NotBase64)?',
    'String::from_utf8_lossy(&bytes).into_owned()',
    'text_too_short_or_not_text_is_refused_before_its_lines_are_counted',
  ),
  plant(
    'a signature of more than four lines passes',
    SIGNATURE_RS,
    'let &[untrusted, key, trusted, global] = lines.as_slice() else {',
    'let &[untrusted, key, trusted, global, ..] = lines.as_slice() else {',
    'four_lines_of_the_right_kinds_make_the_envelope',
  ),
  plant(
    'a signature with any first line passes',
    SIGNATURE_RS,
    'let envelope = untrusted == UNTRUSTED_COMMENT',
    'let envelope = !untrusted.is_empty()',
    'four_lines_of_the_right_kinds_make_the_envelope',
  ),
  plant(
    'a signature with any trusted comment passes',
    SIGNATURE_RS,
    '&& is_trusted_comment(trusted)',
    '&& !trusted.is_empty()',
    'the_trusted_comment_has_a_timestamp_a_gap_and_a_file_name',
  ),
  plant(
    'a signature with the blank of its file around it is refused',
    SIGNATURE_RS,
    'let signature = js::trim(&text);',
    'let signature = &*text;',
    'the_file_is_read_without_the_blank_around_it',
  ),
  // The words of the command line.
  plant(
    'a flag given twice passes',
    ARGS_RS,
    'if values.insert(name, value.clone()).is_some() {',
    'if values.insert(name, value.clone()).is_some() && false {',
    'a_flag_without_a_value_or_given_twice_is_refused',
  ),
  plant(
    'a flag is given the next flag as its value',
    ARGS_RS,
    '.filter(|value| !value.as_encoded_bytes().starts_with(b"--"));',
    '.filter(|_| true);',
    'a_flag_without_a_value_or_given_twice_is_refused',
  ),
  // The workflow's use of the tool.
  {
    name: 'update: the feed entry is made through cargo run',
    edits: [
      [
        RELEASE_YML,
        '"$release_tool" prepare-update --repo "$PWD" \\',
        'cargo run --release --locked -p cf-release -- prepare-update --repo "$PWD" \\',
      ],
    ],
    runs: [WORKFLOW],
    meant: 'is what the notes step runs, by its path',
  },
  {
    name: 'update: the version comes from node again',
    edits: [
      [
        RELEASE_YML,
        'version=$("$release_tool" version)',
        `version=$(node -p "require('./package.json').version")`,
      ],
    ],
    runs: [WORKFLOW],
    meant: 'is what the notes step runs, by its path',
  },
  {
    name: 'update: the tool is built in the step that signs the update bundle, where the key is',
    edits: [
      [
        RELEASE_YML,
        '"$GITHUB_WORKSPACE/app/node_modules/.bin/tauri" signer sign ConsensFlow.app.tar.gz',
        'cargo build --release --locked -p cf-release\n          "$GITHUB_WORKSPACE/app/node_modules/.bin/tauri" signer sign ConsensFlow.app.tar.gz',
      ],
    ],
    runs: [WORKFLOW],
    meant: 'never has a step that holds a key build or run anything through cargo',
  },
  {
    name: 'update: the tool is built in a step that holds a key',
    edits: [
      [
        RELEASE_YML,
        '      - name: Build the release tool\n        run: cargo build',
        '      - name: Build the release tool\n        env:\n          APPLE_API_KEY: ${{ secrets.APPLE_API_KEY }}\n        run: cargo build',
      ],
    ],
    runs: [WORKFLOW],
    meant: 'is built with the lockfile in a step of its own that holds no key',
  },
  {
    name: 'update: the tool is built without the lockfile',
    edits: [
      [
        RELEASE_YML,
        'cargo build --release --locked -p cf-release',
        'cargo build --release -p cf-release',
      ],
    ],
    runs: [WORKFLOW],
    meant: 'is built with the lockfile in a step of its own that holds no key',
  },
]
