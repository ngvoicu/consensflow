//! What an Escape the engine presses is, once the pseudoconsole has it. The pane
//! host writes the bytes of a key as they are ([`PaneTable::write`]), and a
//! program on Windows that reads the console's key events (Devin does: its paste
//! reaches it as key presses, `cf_base::text::console_text`) is given what the
//! console host makes of them. A lone `ESC` is a byte the host has to tell from
//! the start of a sequence, and the engine presses Devin's two Escapes 150 ms
//! apart, and a third a second later (`press_interrupt` of `cf-engine`): what each
//! is, when it comes, and whether two are ever taken for one is what Devin has to
//! go on. Whether it takes them for a stop is Devin's own.
//!
//! What a program that reads key events printed is read here as text, on every
//! platform; the pane it is asked of is Windows'.
//!
//! [`PaneTable::write`]: super::PaneTable::write

/// A key a program that reads the console's key events printed, as `KEY2=[Escape|27|0]#`:
/// which key it was of those it read (from 1), its name, its character and its
/// modifiers (`ConsoleModifiers`: Alt 1, Shift 2, Control 4).
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyRead {
    number: usize,
    key: String,
    character: u32,
    modifiers: u32,
}

impl KeyRead {
    /// The Escape key, plain, as the `number`th key read.
    fn escape(number: usize) -> Self {
        Self {
            number,
            key: "Escape".to_owned(),
            character: 27,
            modifiers: 0,
        }
    }
}

/// The keys `text` says were read, in the order they are said: each `KEY<n>=[…|…|…]#`
/// that is whole.
fn keys_read(text: &str) -> Vec<KeyRead> {
    text.match_indices("KEY")
        .filter_map(|(at, _)| {
            let rest = &text[at + "KEY".len()..];
            let (number, rest) = rest.split_once("=[")?;
            let (fields, _) = rest.split_once("]#")?;
            let mut fields = fields.split('|');
            Some(KeyRead {
                number: number.parse().ok()?,
                key: fields.next()?.to_owned(),
                character: fields.next()?.parse().ok()?,
                modifiers: fields.next()?.parse().ok()?,
            })
        })
        .collect()
}

/// The pane of a program that reads key events, on Windows.
#[cfg(windows)]
#[allow(clippy::expect_used)]
mod conpty {
    use std::collections::HashMap;
    use std::sync::mpsc::Receiver;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::{keys_read, KeyRead};
    use crate::pty::conpty::host_library;
    use crate::pty::conpty_test::{open, powershell, read_until, take};
    use crate::pty::{serial_pty_test, PaneKey, PaneOutput, PaneTable};

    /// The gaps of a round of Devin's, as the engine presses it: the second press
    /// 150 ms after the first, the one that closes a rewind a second after that.
    const GAPS: [Duration; 2] = [Duration::from_millis(150), Duration::from_secs(1)];

    /// How long a key is given to come after it was written: the host reads what
    /// it is given at once, so a key that takes longer is held by something.
    const ARRIVES: Duration = Duration::from_secs(1);

    /// How long after the last key is written the pane is read for what it printed.
    const SETTLE: Duration = Duration::from_secs(3);

    /// An Escape as it is written to the pane: the byte the engine presses.
    const PLAIN: &[u8] = &[27];

    /// An Escape as Windows Terminal writes one to a pseudoconsole made to take it
    /// in `win32-input-mode` (`PSEUDOCONSOLE_WIN32_INPUT_MODE`, which portable-pty
    /// creates this one with): the key going down and coming up, as
    /// `CSI Vk ; Sc ; Uc ; Kd ; Cs ; Rc _` (Escape is virtual key 27, scan code 1).
    const WIN32_INPUT: &[u8] = b"\x1b[27;1;27;1;0;1_\x1b[27;1;27;0;0;1_";

    /// A program that says READY and prints each of `keys` keys it reads from the
    /// console's key events (`ReadKey`, which is `ReadConsoleInput`'s) as
    /// `KEY<n>=[<key>|<character>|<modifiers>]#`.
    fn key_reader(keys: usize) -> Vec<String> {
        powershell(&format!(
            "Write-Output 'READY'; foreach ($n in 1..{keys}) {{ $k = [Console]::ReadKey($true); \
             Write-Output (\"KEY{{0}}=[{{1}}|{{2}}|{{3}}]#\" -f $n, $k.Key, [int]$k.KeyChar, [int]$k.Modifiers) }}"
        ))
    }

    /// That the panes here are given their keys by the console host the app ships beside
    /// its pane host (`conpty.dll`, with its `OpenConsole.exe`), which every key test here
    /// is of: the system's own makes other keys of the same bytes, a mark typed as text the
    /// Alt key carrying it (main's Windows CI, 2026-10-27: no `conpty.dll` where the tests
    /// look, and `a_mark_reaches…` read the mark as key 18, Alt's).
    fn of_the_shipped_host() {
        let library = host_library().expect("the console host's library");
        let shipped = library
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case("conpty.dll"))
            && library.with_file_name("OpenConsole.exe").is_file();
        assert!(
            shipped,
            "the panes' console host is {}, not the conpty.dll the app ships beside its OpenConsole.exe: `cargo xtask conpty --into app/src-tauri/target/debug` puts them where the tests look",
            library.display()
        );
    }

    /// A pane that is read as it goes, noting when each key was first said whole.
    struct Reading<'a> {
        table: &'a PaneTable,
        key: &'a PaneKey,
        output: &'a Receiver<PaneOutput>,
        seen: String,
        said_at: Vec<Option<Instant>>,
    }

    impl Reading<'_> {
        /// Reads what the pane prints until `until`.
        fn until(&mut self, until: Instant) {
            loop {
                let left = until.saturating_duration_since(Instant::now());
                let Ok(chunk) = self.output.recv_timeout(left) else {
                    return;
                };
                self.seen.push_str(&take(self.table, self.key, &chunk));
                let now = Instant::now();
                for key in keys_read(&self.seen) {
                    if let Some(said) = self.said_at.get_mut(key.number.saturating_sub(1)) {
                        said.get_or_insert(now);
                    }
                }
            }
        }
    }

    /// What came of the three Escapes a round of Devin's is.
    struct Pressed {
        keys: Vec<KeyRead>,
        /// How long each took to be said after it was written.
        delays: Vec<Option<Duration>>,
        seen: String,
    }

    /// Writes three Escapes into the pane of a program that reads key events, `escape`
    /// the bytes of each, as the engine writes them (the gaps of [`GAPS`]), and reads
    /// what the program makes of them.
    fn pressed(escape: &[u8]) -> Pressed {
        pressed_after(None, escape)
    }

    /// [`pressed`], in a pane that has first been given `mark`, one key, when there is one:
    /// what the human typed before the engine's round. The mark is the first key read, and
    /// the Escapes are the ones after it.
    fn pressed_after(mark: Option<&[u8]>, escape: &[u8]) -> Pressed {
        let _pty_guard = serial_pty_test();
        of_the_shipped_host();
        let table = Arc::new(PaneTable::new());
        let before = usize::from(mark.is_some());
        let (key, output) = open(&table, &key_reader(3 + before), &HashMap::new(), 1 << 20);
        read_until(&table, &key, &output, "READY");
        if let Some(mark) = mark {
            table.write(&key, mark).expect("type the mark");
            read_until(&table, &key, &output, "KEY1=");
        }
        let mut reading = Reading {
            table: &table,
            key: &key,
            output: &output,
            seen: String::new(),
            said_at: vec![None; 3 + before],
        };
        let mut written: Vec<Instant> = vec![Instant::now(); before];
        for gap in std::iter::once(Duration::ZERO).chain(GAPS) {
            if let Some(last) = written.last() {
                reading.until(*last + gap);
            }
            table.write(&key, escape).expect("press Escape");
            written.push(Instant::now());
        }
        let settled = Instant::now() + SETTLE;
        while reading.said_at.iter().any(Option::is_none) && Instant::now() < settled {
            reading.until((Instant::now() + Duration::from_millis(100)).min(settled));
        }
        let delays = written
            .iter()
            .zip(&reading.said_at)
            .skip(before)
            .map(|(written, said)| said.map(|said| said.saturating_duration_since(*written)))
            .collect();
        let keys = keys_read(&reading.seen)
            .into_iter()
            .filter(|read| read.number > before)
            .map(|read| KeyRead {
                number: read.number - before,
                ..read
            })
            .collect();
        let seen = reading.seen;
        table.kill(&key).expect("close the pane");
        Pressed { keys, delays, seen }
    }

    /// The three Escapes arrived as three Escape keys, plain, each soon after it was written.
    fn assert_three_escapes(pressed: &Pressed, what: &str) {
        let wanted: Vec<KeyRead> = (1..=3).map(KeyRead::escape).collect();
        let in_time = pressed
            .delays
            .iter()
            .all(|delay| delay.is_some_and(|delay| delay <= ARRIVES));
        assert!(
            pressed.keys == wanted && in_time,
            "{what}: a program that reads key events was given {:?}, each said {:?} after it was written; its screen: {:?}",
            pressed.keys,
            pressed.delays,
            pressed.seen
        );
    }

    /// What Devin gets of a round of the engine's Escapes: the byte, three times, as the
    /// pane host writes it. If this fails, the console host holds a lone `ESC`, or takes
    /// it with the next for an Alt key, and no number of rounds stops a window that
    /// reads key events.
    #[test]
    fn the_escapes_the_engine_presses_reach_a_program_that_reads_key_events_as_three_escape_keys() {
        assert_three_escapes(&pressed(PLAIN), "the plain byte");
    }

    /// The same three, written the way the pseudoconsole is made to take a key. If the
    /// test above fails and this one holds, it is what the pane host should write.
    #[test]
    fn the_escapes_written_in_win32_input_mode_reach_a_program_that_reads_key_events_as_three_escape_keys(
    ) {
        assert_three_escapes(&pressed(WIN32_INPUT), "the win32-input-mode form");
    }

    /// Why nothing writes a key in win32-input-mode to a pane: the console host the app
    /// ships (ConPTY 1.25) dispatches a lone `ESC` at the end of a read as the Escape key
    /// only until it has parsed one such sequence (`EncounteredWin32InputModeSequence`,
    /// never reset; Calliope, T-607), and holds every one after it. After one mark typed
    /// as a key, the engine's round of Escapes reached a program that reads key events as
    /// no key at all (zeewin, 2026-10-25), and no interrupt of Devin's or Codex's would
    /// stop it. If this fails, the host no longer holds them.
    #[test]
    fn after_one_key_in_win32_input_mode_the_console_host_holds_the_lone_escapes_the_engine_presses(
    ) {
        let pressed = pressed_after(Some(EM_DASH_AS_A_KEY), PLAIN);
        assert!(
            pressed.keys != (1..=3).map(KeyRead::escape).collect::<Vec<_>>(),
            "the host gave the three Escapes after a key as they were: {:?}",
            pressed.keys
        );
    }

    /// The engine's round after the human typed a mark as text, as the page writes what is
    /// typed: three Escape keys, as in a pane that was given nothing before them.
    #[test]
    fn the_escapes_the_engine_presses_after_a_mark_typed_as_text_reach_a_program_that_reads_key_events_as_three_escape_keys(
    ) {
        assert_three_escapes(
            &pressed_after(Some("\u{2014}".as_bytes()), PLAIN),
            "the plain byte after a mark as text",
        );
    }

    /// The first key a program that reads key events reads of `bytes`, written to its pane
    /// at once, and what its screen said.
    fn first_key(bytes: &[u8]) -> (Vec<KeyRead>, String) {
        let _pty_guard = serial_pty_test();
        of_the_shipped_host();
        let table = Arc::new(PaneTable::new());
        let (key, output) = open(&table, &key_reader(1), &HashMap::new(), 1 << 20);
        read_until(&table, &key, &output, "READY");
        let mut reading = Reading {
            table: &table,
            key: &key,
            output: &output,
            seen: String::new(),
            said_at: vec![None],
        };
        table.write(&key, bytes).expect("type the mark");
        let settled = Instant::now() + SETTLE;
        while reading.said_at[0].is_none() && Instant::now() < settled {
            reading.until((Instant::now() + Duration::from_millis(100)).min(settled));
        }
        table.kill(&key).expect("close the pane");
        (keys_read(&reading.seen), reading.seen)
    }

    /// An em dash, no key of the layout's, as Windows Terminal writes it to a pseudoconsole
    /// made to take keys: a key going down and coming up with no key code and no scan
    /// code, and the character's code unit (8212).
    const EM_DASH_AS_A_KEY: &[u8] = b"\x1b[0;0;8212;1;0;1_\x1b[0;0;8212;0;0;1_";

    /// A mark no key of the layout types reaches a program that reads key events as
    /// itself, written as text or as a key, through the console host the app ships beside
    /// its pane host (Microsoft's own, which portable-pty loads, as `cargo test` finds it
    /// beside the build). So the page writes what the human types into a Devin or Codex
    /// window as it was typed, where it spelled marks in ASCII until T-548. (The system's
    /// own host, which the app never loads, gives the same mark written as text as the Alt
    /// key carrying it: T-548, Windows 11 26200.)
    #[test]
    fn a_mark_reaches_a_program_that_reads_key_events_as_itself_written_as_text_or_as_a_key() {
        let mark = KeyRead {
            number: 1,
            key: "0".to_owned(),
            character: 0x2014,
            modifiers: 0,
        };
        let (as_text, seen) = first_key("—".as_bytes());
        assert_eq!(
            as_text,
            std::slice::from_ref(&mark),
            "written as text; its screen: {seen:?}"
        );
        let (as_key, seen) = first_key(EM_DASH_AS_A_KEY);
        assert_eq!(as_key, [mark], "written as a key; its screen: {seen:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_keys_a_program_printed_are_read_from_its_text_whole_ones_only() {
        let text = "READY\r\nKEY1=[Escape|27|0]#\r\nKEY2=[Escape|27|1]#\r\nKEY3=[Esca";
        assert_eq!(
            keys_read(text),
            [
                KeyRead::escape(1),
                KeyRead {
                    modifiers: 1,
                    ..KeyRead::escape(2)
                }
            ]
        );
        // What the console drew around the text does not matter, and a line that is none is none.
        let drawn = "\u{1b}[2;1HKEY1=[A|97|0]#\u{1b}[K KEYS=[a]# KEY4=[x|y|0]# KEY=[a|1|0]#";
        assert_eq!(
            keys_read(drawn),
            [KeyRead {
                number: 1,
                key: "A".to_owned(),
                character: 97,
                modifiers: 0
            }]
        );
        assert!(keys_read("").is_empty());
    }
}
