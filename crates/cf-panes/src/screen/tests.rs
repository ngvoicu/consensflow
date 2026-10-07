//! What a screen shows after a program printed: each case is a thing a real
//! harness does to its terminal, in the bytes it does it with.

use std::sync::Arc;

use super::*;

/// A screen of `rows` by `cols` that was fed `printed`, in one chunk.
fn after(rows: u16, cols: u16, printed: &str) -> Screen {
    let mut screen = Screen::new(rows, cols);
    screen.feed(printed.as_bytes());
    screen
}

/// Every line of the screen, as it lies, empty ones too.
fn lines(screen: &Screen) -> Vec<String> {
    screen.grid.lines()
}

#[test]
fn text_lands_where_the_cursor_is_and_a_line_ends_where_its_text_does() {
    let screen = after(4, 20, "hello\r\nworld");
    assert_eq!(lines(&screen), ["hello", "world", "", ""]);
}

#[test]
fn a_line_feed_alone_moves_down_and_not_back_to_the_first_column() {
    // The pty turns a program's \n into \r\n; a program that sets its terminal
    // raw does not get that, and writes the \r itself.
    let screen = after(3, 20, "ab\ncd");
    assert_eq!(lines(&screen), ["ab", "  cd", ""]);
}

#[test]
fn a_carriage_return_overwrites_and_a_backspace_steps_back() {
    assert_eq!(lines(&after(2, 20, "hello\rJ"))[0], "Jello");
    assert_eq!(lines(&after(2, 20, "ab\x08c"))[0], "ac");
}

#[test]
fn colours_titles_and_the_other_strings_are_not_text() {
    let printed = "\x1b[1;31mred\x1b[0m \x1b]0;a title\x07ok \x1b]2;other\x1b\\fine \
                   \x1bP1$r0m\x1b\\done \x1b_app\x1b\\!";
    assert_eq!(lines(&after(2, 40, printed))[0], "red ok fine done !");
}

#[test]
fn cursor_addressing_puts_text_at_a_place_and_erasing_blanks_it() {
    let screen = after(4, 20, "\x1b[2;5Hx\x1b[4;1Hlast");
    assert_eq!(lines(&screen), ["", "    x", "", "last"]);
    let mut screen = screen;
    screen.feed(b"\x1b[2J");
    assert_eq!(lines(&screen), ["", "", "", ""]);
    screen.feed(b"\x1b[Hagain");
    assert_eq!(lines(&screen)[0], "again");
}

#[test]
fn erasing_from_and_to_the_cursor_clears_that_part_of_a_line() {
    assert_eq!(lines(&after(2, 20, "abcdef\r\x1b[3C\x1b[K"))[0], "abc");
    assert_eq!(lines(&after(2, 20, "abcdef\r\x1b[2C\x1b[1K"))[0], "   def");
    assert_eq!(lines(&after(2, 20, "abcdef\x1b[2K"))[0], "");
    let screen = after(3, 20, "one\r\ntwo\r\nthree\x1b[2;2H\x1b[J");
    assert_eq!(lines(&screen), ["one", "t", ""]);
    let screen = after(3, 20, "one\r\ntwo\r\nthree\x1b[2;2H\x1b[1J");
    assert_eq!(lines(&screen), ["", "  o", "three"]);
}

#[test]
fn characters_are_inserted_deleted_and_erased_in_place() {
    assert_eq!(lines(&after(1, 20, "abcdef\r\x1b[2C\x1b[2P"))[0], "abef");
    assert_eq!(
        lines(&after(1, 20, "abcdef\r\x1b[2C\x1b[2@"))[0],
        "ab  cdef"
    );
    assert_eq!(lines(&after(1, 20, "abcdef\r\x1b[2C\x1b[2X"))[0], "ab  ef");
}

#[test]
fn lines_scroll_off_the_top_when_text_goes_past_the_bottom() {
    let screen = after(3, 20, "1\r\n2\r\n3\r\n4\r\n5");
    assert_eq!(lines(&screen), ["3", "4", "5"]);
}

#[test]
fn a_scroll_region_keeps_the_rows_outside_it() {
    // A header and a footer stay while the middle scrolls.
    let screen = after(5, 20, "head\x1b[5;1Hfoot\x1b[2;4r\x1b[2;1Ha\r\nb\r\nc\r\nd");
    assert_eq!(lines(&screen), ["head", "b", "c", "d", "foot"]);
}

#[test]
fn lines_are_inserted_and_deleted_within_the_region() {
    let screen = after(4, 20, "a\r\nb\r\nc\r\nd\x1b[2;1H\x1b[L");
    assert_eq!(lines(&screen), ["a", "", "b", "c"]);
    let screen = after(4, 20, "a\r\nb\r\nc\r\nd\x1b[2;1H\x1b[M");
    assert_eq!(lines(&screen), ["a", "c", "d", ""]);
}

#[test]
fn scrolling_and_reverse_index_move_the_screen_under_the_cursor() {
    let screen = after(3, 20, "a\r\nb\r\nc\x1b[S");
    assert_eq!(lines(&screen), ["b", "c", ""]);
    let screen = after(3, 20, "a\r\nb\r\nc\x1b[T");
    assert_eq!(lines(&screen), ["", "a", "b"]);
    let screen = after(3, 20, "a\r\nb\r\nc\x1b[H\x1bM");
    assert_eq!(lines(&screen), ["", "a", "b"]);
}

#[test]
fn text_past_the_last_column_wraps_and_the_wrapped_rows_are_one_line() {
    let long: String = ('a'..='z').collect();
    let screen = after(4, 10, &long);
    assert_eq!(screen.tail(5), std::slice::from_ref(&long));
    let screen = after(4, 10, &format!("{long}\r\nnext"));
    assert_eq!(screen.tail(5), [long, "next".to_owned()]);
}

#[test]
fn with_autowrap_off_the_last_column_is_overwritten() {
    let screen = after(2, 5, "\x1b[?7labcdefgh");
    assert_eq!(lines(&screen), ["abcdh", ""]);
}

#[test]
fn a_character_or_a_sequence_split_between_chunks_is_whole() {
    let mut screen = Screen::new(2, 20);
    screen.feed(&[b'c', b'a', b'f', 0xC3]);
    screen.feed(&[0xA9, 0x1b]);
    screen.feed(b"[3");
    screen.feed(b"1mred");
    assert_eq!(lines(&screen)[0], "caf\u{e9}red");
    // Four bytes of an emoji, a byte at a time.
    let mut screen = Screen::new(1, 20);
    for byte in "\u{1F600}".bytes() {
        screen.feed(&[byte]);
    }
    assert_eq!(lines(&screen)[0], "\u{1F600}");
}

#[test]
fn bytes_that_are_not_text_show_as_the_replacement_character() {
    let mut screen = Screen::new(1, 20);
    screen.feed(&[b'a', 0xFF, b'b', 0xC3, b'c', 0x80]);
    assert_eq!(lines(&screen)[0], "a\u{FFFD}b\u{FFFD}c\u{FFFD}");
}

#[test]
fn the_alternate_screen_parks_the_main_one_and_gives_it_back() {
    let mut screen = after(3, 20, "main\r\ntext\x1b[?1049h\x1b[Halt screen");
    assert_eq!(lines(&screen), ["alt screen", "", ""]);
    screen.feed(b"\x1b[?1049l");
    assert_eq!(lines(&screen), ["main", "text", ""]);
    // A program that dies on the alternate screen leaves it showing.
    screen.feed(b"\x1b[?1049h\x1b[Hcrashed");
    assert_eq!(lines(&screen)[0], "crashed");
}

#[test]
fn the_cursor_is_saved_and_restored_with_the_escape_and_the_csi_forms() {
    let screen = after(3, 20, "\x1b[2;3H\x1b7\x1b[H\x1b8x");
    assert_eq!(lines(&screen), ["", "  x", ""]);
    let screen = after(3, 20, "\x1b[2;3H\x1b[s\x1b[H\x1b[ux");
    assert_eq!(lines(&screen), ["", "  x", ""]);
}

#[test]
fn tabs_and_repeats_and_relative_moves() {
    assert_eq!(lines(&after(1, 20, "a\tb"))[0], "a       b");
    assert_eq!(lines(&after(1, 20, "x\x1b[3b"))[0], "xxxx");
    assert_eq!(lines(&after(2, 20, "ab\x1b[B\x1b[2Dc"))[0], "ab");
    assert_eq!(lines(&after(2, 20, "ab\x1b[B\x1b[2Dc"))[1], "c");
    assert_eq!(lines(&after(3, 20, "x\x1b[3;4H\x1b[2A\x1b[Gy"))[0], "y");
}

#[test]
fn a_wide_character_takes_two_cells_and_shows_once() {
    let screen = after(1, 6, "\u{6F22}\u{5B57}a");
    assert_eq!(lines(&screen)[0], "\u{6F22}\u{5B57}a");
    // At the end of a row it does not fit and goes to the next, whole, in
    // the same line.
    let screen = after(2, 5, "abcd\u{6F22}");
    assert_eq!(lines(&screen), ["abcd\u{6F22}"]);
    // A combining mark takes no cell of its own.
    assert_eq!(lines(&after(1, 6, "e\u{301}x"))[0], "ex");
}

#[test]
fn a_windows_pseudoconsole_redraw_lands_in_place() {
    // ConPTY paints with absolute positions, erases to the end of a line, and
    // jumps by columns where a Unix terminal prints blanks.
    let screen = after(
        6,
        40,
        "\x1b[?25l\x1b[2J\x1b[m\x1b[H\x1b[1;1HNo API key found\x1b[K\
         \x1b[2;1H\x1b[5CUse /login\x1b[K\x1b[3;1H\x1b[?25h",
    );
    assert_eq!(
        screen.tail(5),
        ["No API key found", "     Use /login"],
        "the jump by columns is the blanks it stands for"
    );
}

#[test]
fn the_tail_leaves_out_what_only_draws_and_keeps_the_last_lines_asked() {
    let screen = after(
        8,
        30,
        "\u{250C}\u{2500}\u{2500}\u{2510}\r\n\u{2502} hi \u{2502}\r\n\u{2514}\u{2500}\u{2500}\u{2518}\r\n$ \r\n\r\none\r\ntwo\r\nthree",
    );
    assert_eq!(
        screen.tail(10),
        ["\u{2502} hi \u{2502}", "one", "two", "three"]
    );
    assert_eq!(screen.tail(2), ["two", "three"]);
    assert_eq!(screen.tail(0), Vec::<String>::new());
    assert_eq!(Screen::new(24, 80).tail(5), Vec::<String>::new());
}

#[test]
fn a_screen_resized_keeps_its_text_and_its_cursor_on_it() {
    let mut screen = after(4, 10, "one\r\ntwo\r\nthree\r\nfour");
    screen.resize(2, 6);
    assert_eq!(lines(&screen), ["three", "four"]);
    screen.feed(b"\r\nmore");
    assert_eq!(lines(&screen), ["four", "more"]);
    screen.resize(3, 8);
    assert_eq!(lines(&screen), ["four", "more", ""]);
}

#[test]
fn a_string_that_never_ends_does_not_hide_what_follows_it() {
    let mut printed = String::from("\x1b]0;");
    printed.push_str(&"x".repeat(10_000));
    printed.push_str("\r\nvisible");
    let screen = after(4, 40, &printed);
    assert_eq!(screen.tail(1), ["visible"]);
}

#[test]
fn cancelling_a_sequence_and_a_new_escape_in_one_leave_no_trace_on_the_screen() {
    assert_eq!(lines(&after(1, 20, "a\x1b[12\x18b"))[0], "ab");
    assert_eq!(lines(&after(1, 20, "a\x1b[12\x1b[31mb"))[0], "ab");
    assert_eq!(lines(&after(1, 20, "a\x1b[?1;2<xb"))[0], "ab");
}

#[test]
fn reset_blanks_the_screen_and_the_modes() {
    let screen = after(2, 10, "text\x1b[?7l\x1bcnew");
    assert_eq!(lines(&screen), ["new", ""]);
}

#[test]
fn any_bytes_at_all_leave_a_screen_that_can_be_read() {
    // Every byte, then a long run of a simple generator's: nothing may panic,
    // whatever the state a sequence was left in.
    let mut screen = Screen::new(5, 12);
    let every: Vec<u8> = (0..=255).collect();
    for _ in 0..4 {
        screen.feed(&every);
    }
    let mut state: u32 = 0x9E37_79B9;
    let noise: Vec<u8> = (0..200_000)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            state.to_be_bytes()[0]
        })
        .collect();
    for chunk in noise.chunks(97) {
        screen.feed(chunk);
        screen.resize(2 + u16::from(chunk[0] % 9), 3 + u16::from(chunk[1] % 40));
    }
    assert!(screen.tail(usize::MAX).len() <= 11);
}

#[test]
fn a_pty_the_size_of_a_page_is_kept_to_the_size_of_a_screen() {
    let mut screen = Screen::new(4096, 4096);
    assert_eq!(screen.grid.size(), (400, 1000));
    screen.resize(5000, 5000);
    assert_eq!(screen.grid.size(), (400, 1000));
    screen.resize(0, 0);
    assert_eq!(screen.grid.size(), (1, 1), "a screen has a cell at least");
}

#[test]
fn a_shared_screen_is_fed_resized_and_read_from_any_thread() {
    let screen = Arc::new(PaneScreen::new(3, 20));
    let feeding = Arc::clone(&screen);
    std::thread::spawn(move || feeding.feed(b"from a thread"))
        .join()
        .unwrap();
    assert_eq!(screen.tail(3), ["from a thread"]);
    screen.resize(2, 4);
    assert_eq!(
        screen.tail(3),
        ["from a thread".chars().take(4).collect::<String>()]
    );
}
