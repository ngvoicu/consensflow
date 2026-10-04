use super::*;

#[test]
fn a_dot_dot_takes_off_the_name_before_it_by_the_text_alone() {
    // The folder `missing` need not be there.
    assert_eq!(
        posix::join(&["/r/base/missing/..", "agents.json"]),
        "/r/base/agents.json"
    );
    assert_eq!(posix::normalize("/r/./a"), "/r/a");
    assert_eq!(posix::normalize("../a/./b/.."), "../a");
    assert_eq!(posix::normalize("a/.."), ".");
    assert_eq!(posix::normalize("a/../"), "./");
    assert_eq!(posix::normalize("/../x"), "/x");
    assert_eq!(win32::normalize(r"C:\..\x"), r"C:\x");
    assert_eq!(win32::normalize(r"..\a\.\b\.."), r"..\a");
    assert_eq!(win32::normalize(r"a\.."), ".");
}

#[test]
fn a_drive_stays_as_it_is_written_and_a_bare_one_is_given_its_root() {
    assert_eq!(
        win32::join(&[r"C:..\cf", "agents.json"]),
        r"C:..\cf\agents.json"
    );
    assert_eq!(win32::join(&["C:", "agents.json"]), r"C:\agents.json");
    assert_eq!(
        win32::join(&[r"C:\Users\me", ".consensflow"]),
        r"C:\Users\me\.consensflow"
    );
}

#[test]
fn nothing_to_join_is_a_dot() {
    for parts in [&[][..], &[""], &["", ""]] {
        assert_eq!(posix::join(parts), ".");
        assert_eq!(win32::join(parts), ".");
    }
}

#[test]
fn join_is_the_flavour_of_the_system_this_is_built_for() {
    let expected = if cfg!(windows) { r"a\c" } else { "a/c" };
    assert_eq!(join(&["a", "b/../c"]), expected);
}

#[test]
fn a_dot_dot_after_text_outside_ascii_takes_it_off_whole() {
    // `é` is one UTF-16 unit and two bytes, `日本` two units and six, `😀` two
    // units and four: a `..` empties each, and takes `x` off `日本/x` alone.
    for segment in ["é", "日本", "😀"] {
        assert_eq!(posix::join(&[segment, ".."]), ".");
        assert_eq!(win32::join(&[segment, ".."]), ".");
    }
    assert_eq!(posix::join(&["日本/x", ".."]), "日本");
    assert_eq!(win32::normalize("é"), "é");
}

#[test]
fn a_reserved_name_and_one_unit_more_is_a_device_when_the_path_has_no_colon() {
    // Node looks the name up with the path's last UTF-16 unit cut off: a
    // letter, or `é`, but not half of an emoji, which no name ends in.
    assert_eq!(win32::normalize("CONa"), r".\CONa");
    assert_eq!(win32::normalize("CONé"), r".\CONé");
    assert_eq!(win32::normalize("NULL"), r".\NULL");
    assert_eq!(win32::normalize("CON😀"), "CON😀");
    assert_eq!(win32::normalize("CON"), "CON");
}

#[test]
fn only_windows_takes_a_backslash_for_a_separator() {
    assert_eq!(posix::normalize(r"a\..\b"), r"a\..\b");
    assert_eq!(win32::normalize(r"a/b\c"), r"a\b\c");
    assert_eq!(posix::normalize("//a"), "/a");
}

#[test]
fn a_unc_root_is_kept_and_a_drive_with_no_separator_after_it_is_relative() {
    assert_eq!(win32::join(&["//server", "share"]), r"\\server\share\");
    assert_eq!(win32::normalize(r"C:\a\..\..\b"), r"C:\b");
    assert_eq!(win32::normalize(r"C:a\..\..\b"), r"C:..\b");
}

#[test]
fn a_relative_path_that_would_read_as_a_drive_is_given_a_dot_before_it() {
    // CVE-2024-36139: `C:..\cf` after `.` must not become an absolute path.
    assert_eq!(win32::join(&[".", r"C:..\cf"]), r".\C:..\cf");
}

#[test]
fn a_reserved_name_with_a_colon_is_a_device_root_and_joined_it_is_left_alone() {
    assert_eq!(win32::normalize("COM¹:x"), r".\COM¹:x");
    assert_eq!(win32::normalize(r"\\?\COM¹:x"), r"\\?\COM¹:\x");
    assert_eq!(win32::normalize(r"\\.\COM1:x"), r"\\?\COM1:\x");
    assert_eq!(win32::join(&["a/b", "CON:/c"]), r"a\b\CON:\c");
}
