//! The words and the folders of the privacy dialogs (T-1194): what a service the system names
//! guards, which folders a window is told of when it opens on them, and what is said.

use super::*;

#[test]
fn a_service_the_system_names_is_said_in_what_it_guards() {
    assert_eq!(
        service_words("MediaLibrary"),
        "your music and media library (Apple Music)"
    );
    assert_eq!(service_words("Photos"), "your Photos library");
    assert_eq!(
        service_words("SystemPolicyDesktopFolder"),
        "the Desktop folder"
    );
    assert_eq!(
        service_words("SystemPolicyDocumentsFolder"),
        "the Documents folder"
    );
    assert_eq!(
        service_words("SystemPolicyDownloadsFolder"),
        "the Downloads folder"
    );
    assert_eq!(
        service_words("SystemPolicyRemovableVolumes"),
        "files on a removable drive"
    );
    assert_eq!(
        service_words("SystemPolicyNetworkVolumes"),
        "files on a network volume"
    );
    assert_eq!(
        service_words("SystemPolicyAppDataDetailed"),
        "the data of other apps"
    );
    // One this table does not know is named as the system names it, not guessed at.
    assert_eq!(
        service_words("SomethingNew"),
        "\u{201c}SomethingNew\u{201d}"
    );
}

#[test]
fn only_the_home_folder_and_the_music_folder_are_told_of_when_a_window_opens_on_them() {
    let home = "/Users/dmitry";
    assert_eq!(gated_folder("/Users/dmitry", home), Some(Opened::Home));
    assert_eq!(gated_folder("/Users/dmitry/", home), Some(Opened::Home));
    assert_eq!(
        gated_folder("/Users/dmitry", "/Users/dmitry/"),
        Some(Opened::Home)
    );
    assert_eq!(
        gated_folder("/Users/dmitry/Music", home),
        Some(Opened::Music)
    );
    assert_eq!(
        gated_folder("/Users/dmitry/Music/Logic/song", home),
        Some(Opened::Music)
    );
    // A project in Desktop, Documents or Downloads asks once and is answered once: not told.
    for folder in ["Desktop", "Documents", "Downloads", "Projects", "Pictures"] {
        assert_eq!(
            gated_folder(&format!("/Users/dmitry/{folder}"), home),
            None,
            "{folder}"
        );
    }
    // A folder whose name starts as another's, and another user's home, are neither.
    assert_eq!(gated_folder("/Users/dmitry/Musical", home), None);
    assert_eq!(gated_folder("/Users/dmitry2", home), None);
    assert_eq!(gated_folder("/Users", home), None);
    // No home is no folder to compare with: a root is not a home.
    assert_eq!(gated_folder("/", ""), None);
    assert_eq!(gated_folder("/Music", "/"), None);
}

#[test]
fn what_the_human_is_told_names_the_window_the_program_and_what_the_dialog_guards() {
    let prompt = TccPrompt {
        id: None,
        generation: None,
        service: "SystemPolicyDocumentsFolder".to_owned(),
        pid: 4711,
        program: "git".to_owned(),
    };
    let window = told_in_window("zeus", &prompt);
    assert!(window
        .starts_with("@zeus's window waits on a macOS permission dialog: `git` (process 4711)"));
    assert!(
        window.contains("asked for the Documents folder"),
        "{window}"
    );
    assert!(
        window.ends_with("unless the task needs the Documents folder."),
        "{window}"
    );
    let loose = told_in_no_window(&prompt);
    assert!(
        loose.starts_with("A process of ConsensFlow's that no window holds (`git`, process 4711)")
    );
    assert!(loose.contains("not in a window"), "{loose}");
}
