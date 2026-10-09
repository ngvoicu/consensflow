//! The release from start to end, call by call: which programs run, in what order
//! and with what, ad hoc and under the Developer ID.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::{
    credentials, Fake, Trial, API_ISSUER, API_KEY_ID, API_KEY_LINES, CERTIFICATE_PASSWORD, ID,
};

/// The calls a text lists, one to a line, with the names of this run's places.
fn calls(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| {
            line.replace("$ID", ID)
                .replace("$KEY_ID", API_KEY_ID)
                .replace("$ISSUER", API_ISSUER)
                .replace("$CERT_PASSWORD", CERTIFICATE_PASSWORD)
        })
        .collect()
}

/// The Mach-Os of the test bundle that are signed on their own, in the order they are.
const CODE: &str = "
    Frameworks/libz.dylib
    MacOS/helper
    Resources/cli/bin/cf
    Resources/fat
";

/// The `codesign` calls that sign the bundle's own code, one to a line of `CODE`.
fn code_signed(prefix: &str) -> String {
    CODE.lines()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(|path| {
            let name = path.rsplit('/').next().unwrap();
            format!("{prefix} --options runtime --identifier $ID.{name} $APP/Contents/{path}\n")
        })
        .collect()
}

#[test]
fn an_ad_hoc_release_signs_the_code_then_the_app_then_makes_the_dmg_again_with_no_notary() {
    let mut trial = Trial::new(Fake::new());
    trial.sign(None).unwrap();

    let ad_hoc = "codesign --force --timestamp=none --sign -";
    let expected = format!(
        "
        plutil -extract CFBundleIdentifier raw -o - $APP/Contents/Info.plist
        plutil -extract CFBundleExecutable raw -o - $APP/Contents/Info.plist
        {code}
        {ad_hoc} --options runtime $APP
        hdiutil convert $DMG -format UDRW -ov -o $SCRATCH/writable.dmg
        hdiutil attach $SCRATCH/writable.dmg -readwrite -noverify -noautoopen -nobrowse -mountpoint $SCRATCH/volume
        ditto $APP $SCRATCH/volume/ConsensFlow.app
        chmod -R go-w $SCRATCH/volume/ConsensFlow.app
        hdiutil detach $SCRATCH/volume
        hdiutil convert $SCRATCH/writable.dmg -format UDZO -imagekey zlib-level=9 -ov -o $DMG
        {ad_hoc} $DMG
        codesign --verify --deep --strict $APP
        codesign --verify --strict $DMG
        ",
        code = code_signed(ad_hoc),
    );
    assert_eq!(trial.transcript(), calls(&expected));
}

#[test]
fn a_release_under_the_developer_id_is_signed_notarized_and_stapled_in_this_order() {
    let mut trial = Trial::new(Fake::new());
    trial.sign(Some(credentials())).unwrap();

    let signed = "codesign --force --timestamp --sign 0123456789ABCDEF0123456789ABCDEF01234567 \
                  --keychain $SCRATCH/signing.keychain-db";
    let notarized = "--key $SCRATCH/notary.p8 --key-id $KEY_ID --issuer $ISSUER \
                     --wait --timeout 60m --output-format json";
    let expected = format!(
        "
        security list-keychains -d user
        security create-keychain -p $PASSWORD $SCRATCH/signing.keychain-db
        security set-keychain-settings -lut 21600 $SCRATCH/signing.keychain-db
        security unlock-keychain -p $PASSWORD $SCRATCH/signing.keychain-db
        security import $SCRATCH/certificate.p12 -k $SCRATCH/signing.keychain-db -f pkcs12 -P $CERT_PASSWORD -T /usr/bin/codesign
        security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k $PASSWORD $SCRATCH/signing.keychain-db
        security list-keychains -d user -s $SCRATCH/signing.keychain-db {searched}
        security find-identity -v -p codesigning $SCRATCH/signing.keychain-db
        plutil -extract CFBundleIdentifier raw -o - $APP/Contents/Info.plist
        plutil -extract CFBundleExecutable raw -o - $APP/Contents/Info.plist
        {code}
        {signed} --options runtime $APP
        ditto -c -k --keepParent $APP $SCRATCH/ConsensFlow.app.zip
        xcrun notarytool submit $SCRATCH/ConsensFlow.app.zip {notarized}
        xcrun stapler staple $APP
        xcrun stapler validate $APP
        hdiutil convert $DMG -format UDRW -ov -o $SCRATCH/writable.dmg
        hdiutil attach $SCRATCH/writable.dmg -readwrite -noverify -noautoopen -nobrowse -mountpoint $SCRATCH/volume
        ditto $APP $SCRATCH/volume/ConsensFlow.app
        chmod -R go-w $SCRATCH/volume/ConsensFlow.app
        hdiutil detach $SCRATCH/volume
        hdiutil convert $SCRATCH/writable.dmg -format UDZO -imagekey zlib-level=9 -ov -o $DMG
        {signed} $DMG
        xcrun notarytool submit $DMG {notarized}
        xcrun stapler staple $DMG
        xcrun stapler validate $DMG
        codesign --verify --deep --strict $APP
        codesign --verify --strict $DMG
        spctl --assess --verbose=4 --type execute $APP
        spctl --assess --verbose=4 --type open --context context:primary-signature $DMG
        security list-keychains -d user -s {searched}
        security delete-keychain $SCRATCH/signing.keychain-db
        ",
        code = code_signed(signed),
        searched = Fake::SEARCHED.join(" "),
    );
    assert_eq!(trial.transcript(), calls(&expected));
}

#[test]
fn what_a_bundle_seals_is_signed_before_the_bundle_and_the_ticket_is_on_the_app_before_the_dmg_holds_it(
) {
    let mut trial = Trial::new(Fake::new());
    trial.sign(Some(credentials())).unwrap();
    let transcript = trial.transcript();
    let at = |start: &str, end: &str| {
        transcript
            .iter()
            .position(|line| line.starts_with(start) && line.ends_with(end))
            .unwrap_or_else(|| panic!("no call {start}…{end} in {transcript:#?}"))
    };

    let app = at("codesign --force", "--options runtime $APP");
    for (place, line) in transcript.iter().enumerate() {
        if line.starts_with("codesign --force") && line.contains("$APP/") {
            assert!(place < app, "{line} is signed after the app that seals it");
        }
    }
    // The ticket goes on the app, and the DMG is made around the app with it.
    assert!(app < at("xcrun stapler staple", "$APP"));
    assert!(
        at("xcrun stapler validate", "$APP") < at("hdiutil convert $DMG", "$SCRATCH/writable.dmg")
    );
    // The DMG is signed once it is made again, and notarized once it is signed.
    let made = at("hdiutil convert $SCRATCH/writable.dmg", "-o $DMG");
    let dmg = at("codesign --force", "$DMG");
    assert!(made < dmg && dmg < at("xcrun notarytool submit $DMG", "json"));
    // And what a download meets is checked last of all that signs.
    assert!(at("xcrun stapler validate", "$DMG") < at("codesign --verify --deep", "$APP"));
}

#[test]
fn the_main_executable_is_signed_with_the_bundle_and_what_is_not_code_is_not_signed_at_all() {
    let mut trial = Trial::new(Fake::new());
    trial.sign(None).unwrap();
    let signs: Vec<_> = trial
        .transcript()
        .into_iter()
        .filter(|line| line.starts_with("codesign --force"))
        .collect();
    assert_eq!(signs.len(), 4 + 1 + 1, "{signs:#?}");
    for left_alone in [
        "MacOS/app",
        "Foo.class",
        "cf.json",
        "notes.txt",
        "Info.plist",
    ] {
        assert!(
            !signs.iter().any(|line| line.contains(left_alone)),
            "{left_alone} is signed on its own: {signs:#?}"
        );
    }
}

#[test]
fn what_a_run_says_is_progress_on_the_error_stream_and_it_leaves_nothing_behind() {
    let mut trial = Trial::new(Fake::new());
    trial.sign(Some(credentials())).unwrap();
    assert_eq!(trial.out, "");
    assert_eq!(
        trial.err,
        "sign-mac: signing ConsensFlow.app\n\
         sign-mac: the notary checks ConsensFlow.app\n\
         sign-mac: making ConsensFlow_3.0.0-alpha.99_aarch64.dmg again around it\n\
         sign-mac: the notary checks ConsensFlow_3.0.0-alpha.99_aarch64.dmg\n\
         sign-mac: ConsensFlow.app and ConsensFlow_3.0.0-alpha.99_aarch64.dmg signed\n"
    );
    // The certificate, the notary's key and the keychain went with the run's folder.
    assert_eq!(trial.left_behind(), Vec::<String>::new());
    assert!(!trial.scratch().exists());
    assert_eq!(trial.fake.waits(), []);
}

/// A file as a tool finds it when it is called: what it holds, and who may read it
/// where the system says (Windows does not).
type Found = (Vec<u8>, Option<u32>);

#[cfg(unix)]
fn mode(path: &Path) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(fs::metadata(path).unwrap().permissions().mode() & 0o777)
}

#[cfg(not(unix))]
fn mode(_: &Path) -> Option<u32> {
    None
}

#[test]
fn the_certificate_and_the_notarys_key_are_written_for_the_tools_to_read_and_for_this_user_alone() {
    let found: Rc<RefCell<BTreeMap<&str, Found>>> = Rc::default();
    // The file is the argument after `before`, looked at when the call is made.
    let look = |label: &'static str, before: &'static str| {
        let found = Rc::clone(&found);
        move |args: &[OsString]| {
            let at = args.iter().position(|arg| arg == before).unwrap();
            let path = PathBuf::from(&args[at + 1]);
            let held = (fs::read(&path).unwrap(), mode(&path));
            found.borrow_mut().insert(label, held);
        }
    };
    let fake = Fake::new()
        .watching("security import", look("certificate", "import"))
        .watching("xcrun notarytool submit", look("key", "--key"));
    Trial::new(fake).sign(Some(credentials())).unwrap();

    let found = found.borrow();
    let key = format!("{}\n", API_KEY_LINES.join("\n"));
    let alone = if cfg!(unix) { Some(0o600) } else { None };
    // The certificate is decoded, as the keychain's tool takes it; the key is as it was given.
    assert_eq!(
        found["certificate"],
        (b"pkcs12-bytes-of-the-certificate".to_vec(), alone)
    );
    assert_eq!(found["key"], (key.into_bytes(), alone));
}
