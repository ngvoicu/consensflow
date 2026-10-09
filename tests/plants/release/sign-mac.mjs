/**
 * Plants in the Mac signing (tools/cf-release, `cf-release sign-mac`): the order
 * an app is signed in, what is cleaned up when a call fails, what is kept
 * secret, and what the notary and Gatekeeper are asked and held to. The
 * crate's tests, which run the steps against a script in place of Apple's
 * tools, must catch each.
 */

const cargo = (...args) => ['cargo', 'test', '--offline', '-p', 'cf-release', ...args]

/** The steps against the script: the order of the calls, the cleanup, the secrets. */
const SIGN_MAC = cargo('--lib', 'sign_mac::')
/** The command as a process, with the tools of the system. */
const WITH_APPLES_TOOLS = cargo('--test', 'sign_mac')

const DIR = 'tools/cf-release/src/sign_mac'
const SIGN_MAC_RS = 'tools/cf-release/src/sign_mac.rs'
const SIGNING_RS = `${DIR}/signing.rs`
const KEYCHAIN_RS = `${DIR}/keychain.rs`
const TOOLS_RS = `${DIR}/tools.rs`
const NOTARY_RS = `${DIR}/notary.rs`
const DMG_RS = `${DIR}/dmg.rs`
const MACHO_RS = `${DIR}/macho.rs`
const VERIFY_RS = `${DIR}/verify.rs`

const APP_SIGNED = '    codesign(tools, signing, app, &["--options", "runtime"])\n}'
const LOOP = '    for path in macho::mach_os(app)?\n'
const RESTORED = '    tools.attempt("security", restored);\n'
const DELETED = '    tools.attempt("security", args!["delete-keychain", &keychain]);\n'

const plant = (name, file, from, to, runs, meant) => ({
  name: `sign-mac: ${name}`,
  edits: [[file, from, to]],
  runs,
  meant,
})

export const PLANTS = [
  {
    // A keychain with the Developer ID's key left on the machine of whoever ran this.
    name: 'sign-mac: the keychain is not removed, nor the search list put back, when a call fails',
    edits: [
      [
        KEYCHAIN_RS,
        RESTORED + DELETED,
        `    if result.is_ok() {\n    ${RESTORED}    ${DELETED}    }\n`,
      ],
    ],
    runs: [SIGN_MAC],
    meant:
      'whichever_call_fails_the_search_list_is_back_the_keychain_is_gone_and_the_run_ends_there',
  },
  plant(
    'the keychain is deleted only when the run went well',
    KEYCHAIN_RS,
    DELETED,
    `    if result.is_ok() {\n    ${DELETED}    }\n`,
    [SIGN_MAC],
    'whichever_call_fails_the_search_list_is_back_the_keychain_is_gone_and_the_run_ends_there',
  ),
  plant(
    'the search list is put back only when the run went well',
    KEYCHAIN_RS,
    RESTORED,
    `    if result.is_ok() {\n    ${RESTORED}    }\n`,
    [SIGN_MAC],
    'whichever_call_fails_the_search_list_is_back_the_keychain_is_gone_and_the_run_ends_there',
  ),
  {
    // The bundle's seal covers what is in it: a file signed after it breaks the seal.
    name: 'sign-mac: an inner binary is signed after the app that contains it',
    edits: [
      [SIGNING_RS, APP_SIGNED, '    Ok(())\n}'],
      [SIGNING_RS, LOOP, `    codesign(tools, signing, app, &["--options", "runtime"])?;\n${LOOP}`],
    ],
    runs: [SIGN_MAC],
    meant:
      'what_a_bundle_seals_is_signed_before_the_bundle_and_the_ticket_is_on_the_app_before_the_dmg_holds_it',
  },
  plant(
    'a password is in an error',
    SIGN_MAC_RS,
    'Failure::Failed(text) => Failure::Failed(secrets.redact(&text)),',
    'Failure::Failed(text) => Failure::Failed(text),',
    [SIGN_MAC],
    'a_secret_that_a_tool_says_in_a_failure_of_the_certificates_import_is_blanked_there',
  ),
  plant(
    'a password is in a line the run speaks',
    TOOLS_RS,
    'self.secrets.redact(line)',
    'line',
    [SIGN_MAC],
    'whichever_call_fails_its_failure_and_every_line_are_told_with_the_secrets_blanked_out',
  ),
  plant(
    'a password is in the notary log the run shows',
    TOOLS_RS,
    'self.secrets.redact(text)',
    'text',
    [SIGN_MAC],
    'the_notarys_log_is_shown_with_the_secrets_blanked_out_of_it',
  ),
  plant(
    'the app is signed without the hardened runtime',
    SIGNING_RS,
    APP_SIGNED,
    '    codesign(tools, signing, app, &[])\n}',
    [SIGN_MAC, WITH_APPLES_TOOLS],
    'an_ad_hoc_release_signs_the_code_then_the_app_then_makes_the_dmg_again_with_no_notary',
  ),
  plant(
    'a binary inside the app is signed without the hardened runtime',
    SIGNING_RS,
    '&["--options", "runtime", "--identifier", &identifier],',
    '&["--identifier", &identifier],',
    [SIGN_MAC, WITH_APPLES_TOOLS],
    'an_ad_hoc_release_signs_the_code_then_the_app_then_makes_the_dmg_again_with_no_notary',
  ),
  plant(
    'the main executable is signed on its own as well',
    SIGNING_RS,
    '.filter(|path| *path != main)',
    '.filter(|_| true)',
    [SIGN_MAC],
    'the_main_executable_is_signed_with_the_bundle_and_what_is_not_code_is_not_signed_at_all',
  ),
  plant(
    'a Java class file is signed as code',
    MACHO_RS,
    '(UNIVERSAL.contains(&magic) && next < FIRST_CLASS_VERSION)',
    'UNIVERSAL.contains(&magic)',
    [SIGN_MAC],
    'a_universal_binary_is_code_but_a_class_file_that_shares_its_magic_is_not',
  ),
  plant(
    'an ad hoc signature asks for a timestamp',
    SIGNING_RS,
    'Signing::AdHoc => words.extend(args!["--timestamp=none", "--sign", "-"]),',
    'Signing::AdHoc => words.extend(args!["--sign", "-"]),',
    [SIGN_MAC],
    'an_ad_hoc_release_signs_the_code_then_the_app_then_makes_the_dmg_again_with_no_notary',
  ),
  plant(
    'the notary is waited for 20 minutes, not 60',
    NOTARY_RS,
    'const WAIT: &str = "60m";',
    'const WAIT: &str = "20m";',
    [SIGN_MAC],
    'the_app_goes_to_the_notary_as_a_zip_and_the_dmg_as_it_is_and_each_waits_an_hour',
  ),
  plant(
    'a refused app is stapled and put in a DMG all the same',
    NOTARY_RS,
    'if status != Some("Accepted") {',
    'if false {',
    [SIGN_MAC],
    'a_refused_app_is_neither_stapled_nor_put_in_a_dmg_and_the_notarys_log_is_shown_whole',
  ),
  plant(
    'the notary is not asked why it refused',
    NOTARY_RS,
    'if let Some(id) = answer.get("id").and_then(Value::as_str) {',
    'if let Some(id) = None::<&str> {',
    [SIGN_MAC],
    'a_refused_app_is_neither_stapled_nor_put_in_a_dmg_and_the_notarys_log_is_shown_whole',
  ),
  plant(
    'Gatekeeper is not asked whether the file is notarized',
    VERIFY_RS,
    'if assessed.code != 0 || !assessed.stderr.contains(NOTARIZED) {',
    'if assessed.code != 0 {',
    [SIGN_MAC],
    'a_file_gatekeeper_does_not_take_as_notarized_ends_the_run_naming_it',
  ),
  plant(
    'the DMG volume stays mounted when copying into it fails',
    DMG_RS,
    'let detached = detach(tools, &volume);',
    'let detached = if replaced.is_ok() { detach(tools, &volume) } else { Ok(()) };',
    [SIGN_MAC],
    'a_volume_that_copying_into_failed_is_let_go_and_the_failure_is_the_copys',
  ),
  plant(
    'the volume is not waited for between tries to detach it',
    DMG_RS,
    '            tools.wait(SETTLE);\n',
    '',
    [SIGN_MAC],
    'a_volume_that_stays_busy_is_asked_three_times_two_seconds_apart_and_then_forced',
  ),
  plant(
    'the app in the DMG is left writable by others',
    DMG_RS,
    '    tools.run("chmod", args!["-R", "go-w", &inside])?;\n',
    '',
    [SIGN_MAC],
    'a_release_under_the_developer_id_is_signed_notarized_and_stapled_in_this_order',
  ),
  plant(
    'a certificate that holds two Developer ID identities is taken',
    KEYCHAIN_RS,
    '[only] => Ok(only.to_string()),',
    '[only, ..] => Ok(only.to_string()),',
    [SIGN_MAC],
    'a_certificate_with_no_such_identity_or_with_several_is_refused_by_the_count',
  ),
  plant(
    'the run leaves its own folder behind',
    SIGN_MAC_RS,
    'if let Err(cause) = file::remove_all(&scratch) {',
    'if let Err(cause) = Ok::<(), cf_base::file::FileError>(()) {',
    [SIGN_MAC, WITH_APPLES_TOOLS],
    'what_a_run_says_is_progress_on_the_error_stream_and_it_leaves_nothing_behind',
  ),
]
