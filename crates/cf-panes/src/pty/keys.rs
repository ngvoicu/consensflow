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
    use crate::arbiter::{EnterTiming, InputArbiter};
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

    /// The keys a crossterm program reads (Muse's TUI and Codex's read the console's key
    /// events, `ReadConsoleInputW`, in raw mode, so a Control-C is a key and no signal), as
    /// the pane host writes the Enter after a paste, the Control-C that clears Muse's box
    /// and the Escape of an interrupt: `\r`, `0x03` and `ESC`, as they are. The console
    /// host makes each the key it is (T-792: T-708 read Muse's raw CR and Control-C as
    /// failing through the system's own host, where its CR came in the same breath as the
    /// paste and its two Control-Cs cleared the box, then armed the quit, as Muse's do).
    #[test]
    fn enter_control_c_and_escape_as_the_pane_host_writes_them_reach_a_key_reader_as_those_keys() {
        let _pty_guard = serial_pty_test();
        of_the_shipped_host();
        let table = Arc::new(PaneTable::new());
        let reader = powershell(
            "[Console]::TreatControlCAsInput = $true; Write-Output 'READY'; foreach ($n in 1..3) { $k = [Console]::ReadKey($true); \
             Write-Output (\"KEY{0}=[{1}|{2}|{3}]#\" -f $n, $k.Key, [int]$k.KeyChar, [int]$k.Modifiers) }",
        );
        let (key, output) = open(&table, &reader, &HashMap::new(), 1 << 20);
        read_until(&table, &key, &output, "READY");
        let mut seen = String::new();
        for (bytes, until) in [
            (&b"\r"[..], "KEY1="),
            (&[3u8][..], "KEY2="),
            (&[27u8][..], "KEY3="),
        ] {
            table.write(&key, bytes).expect("press the key");
            seen.push_str(&read_until(&table, &key, &output, until));
        }
        let settled = Instant::now() + SETTLE;
        while keys_read(&seen).len() < 3 && Instant::now() < settled {
            if let Ok(chunk) = output.recv_timeout(Duration::from_millis(100)) {
                seen.push_str(&take(&table, &key, &chunk));
            }
        }
        table.kill(&key).expect("close the pane");
        let key_of = |number: usize, key: &str, character: u32, modifiers: u32| KeyRead {
            number,
            key: key.to_owned(),
            character,
            modifiers,
        };
        assert_eq!(
            keys_read(&seen),
            [
                key_of(1, "Enter", 13, 0),
                key_of(2, "C", 3, 4),
                KeyRead::escape(3)
            ],
            "its screen: {seen:?}"
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

    /// The keys a program that reads key events reads of `bytes`, written to its pane at once:
    /// up to `most` of them, read until none more comes for [`SETTLE`].
    fn keys_of(bytes: &[u8], most: usize) -> (Vec<KeyRead>, String) {
        let _pty_guard = serial_pty_test();
        of_the_shipped_host();
        let table = Arc::new(PaneTable::new());
        let (key, output) = open(&table, &key_reader(most), &HashMap::new(), 1 << 20);
        read_until(&table, &key, &output, "READY");
        table.write(&key, bytes).expect("write the bytes");
        let mut seen = String::new();
        let settled = Instant::now() + SETTLE;
        while keys_read(&seen).len() < most && Instant::now() < settled {
            if let Ok(chunk) = output.recv_timeout(Duration::from_millis(100)) {
                seen.push_str(&take(&table, &key, &chunk));
            }
        }
        table.kill(&key).expect("close the pane");
        (keys_read(&seen), seen)
    }

    /// Where Node is on this machine: the first `node.exe` of the `PATH`.
    #[allow(clippy::disallowed_methods)] // A test's own look at the machine's PATH.
    fn node() -> Option<std::path::PathBuf> {
        let path = std::env::var_os("PATH")?;
        std::env::split_paths(&path)
            .map(|folder| folder.join("node.exe"))
            .find(|candidate| candidate.is_file())
    }

    /// What a Node program that reads its terminal raw (as Claude Code and Pi do, which set the
    /// console's virtual-terminal input) is given of `bytes`, written to its pane at once: every
    /// chunk it was handed, as hexadecimal, joined, and what the pane showed.
    fn node_reads(bytes: &[u8]) -> (String, String) {
        let node = node().expect("a node.exe on the PATH, which the app's own Windows tests need");
        let _pty_guard = serial_pty_test();
        of_the_shipped_host();
        let table = Arc::new(PaneTable::new());
        let argv = vec![
            node.to_string_lossy().into_owned(),
            "-e".to_owned(),
            "process.stdin.setRawMode(true); console.log('READY'); \
             process.stdin.on('data', (d) => console.log('HEX=[' + d.toString('hex') + ']#'))"
                .to_owned(),
        ];
        let (key, output) = open(&table, &argv, &HashMap::new(), 1 << 20);
        read_until(&table, &key, &output, "READY");
        table.write(&key, bytes).expect("write the bytes");
        let mut seen = String::new();
        let settled = Instant::now() + Duration::from_secs(2);
        while Instant::now() < settled {
            if let Ok(chunk) = output.recv_timeout(Duration::from_millis(100)) {
                seen.push_str(&take(&table, &key, &chunk));
            }
        }
        table.kill(&key).expect("close the pane");
        let hex = seen
            .match_indices("HEX=[")
            .filter_map(|(at, _)| {
                let rest = &seen[at + "HEX=[".len()..];
                Some(rest.split_once("]#")?.0.to_owned())
            })
            .collect::<String>();
        (hex, seen)
    }

    /// How many bytes the console host reads from a pane's input at once: the size of the
    /// buffer of its input thread (`VtInputThread`, 4,096), so that a write which reaches past
    /// it is read in two.
    const CONSOLE_READ: usize = 4096;

    /// A program that says READY, prints each key it reads that is not an `x` as
    /// `KEY<n>=[<key>|<character>|<modifiers>]#` (`n` counting every key read), and says DONE
    /// once no key has come for `quiet` seconds.
    fn quiet_key_reader(quiet: u32) -> Vec<String> {
        powershell(&format!(
            "Write-Output 'READY'; $n = 0; $last = Get-Date; \
             while (((Get-Date) - $last).TotalSeconds -lt {quiet}) {{ \
               if ([Console]::KeyAvailable) {{ $k = [Console]::ReadKey($true); $n++; $last = Get-Date; \
                 if ($k.KeyChar -ne 'x') {{ Write-Output (\"KEY{{0}}=[{{1}}|{{2}}|{{3}}]#\" -f $n, $k.Key, [int]$k.KeyChar, [int]$k.Modifiers) }} }} \
               else {{ Start-Sleep -Milliseconds 20 }} }}; \
             Write-Output 'DONE'"
        ))
    }

    /// The keys, other than the `x`s, that a program which reads key events reads when `filler`
    /// `x`s and then `key` are written to its pane in one write: through the pane host's human
    /// path (the arbiter's typed write, as the page's keys are) where `human`, else straight to the
    /// pane's input; and how long the write took.
    fn keys_behind(
        filler: usize,
        key: &[u8],
        human: bool,
    ) -> (Vec<(String, u32, u32)>, String, Duration) {
        let _pty_guard = serial_pty_test();
        of_the_shipped_host();
        let table = Arc::new(PaneTable::new());
        let (pane, output) = open(&table, &quiet_key_reader(3), &HashMap::new(), 1 << 20);
        read_until(&table, &pane, &output, "READY");
        let mut bytes = vec![b'x'; filler];
        bytes.extend_from_slice(key);
        let started = Instant::now();
        if human {
            let arbiter = InputArbiter::new(EnterTiming::fixed(0));
            arbiter.register(&pane).expect("register the pane");
            arbiter
                .write_typed(&table, &pane, &bytes)
                .expect("write the keys as the human's");
        } else {
            table.write(&pane, &bytes).expect("write the keys");
        }
        let waited = started.elapsed();
        let seen = read_until(&table, &pane, &output, "DONE");
        table.kill(&pane).expect("close the pane");
        let keys = keys_read(&seen)
            .into_iter()
            .map(|read| (read.key, read.character, read.modifiers))
            .collect();
        (keys, seen, waited)
    }

    /// A key that begins with Escape must never reach a window as a lone Escape (T-1336): an Escape
    /// is the key that stops a harness's turn. The console host reads a pane's input
    /// `CONSOLE_READ` bytes at a time and takes an Escape that ends a read for the Escape key,
    /// whatever follows in the next (it holds one only after it has read a key written in
    /// win32-input-mode, which holds every lone Escape the engine presses for good:
    /// `after_one_key_in_win32_input_mode_the_console_host_holds_the_lone_escapes_the_engine_presses`).
    /// So an `ESC CR` written where a read ends was the Escape key, then Enter (measured on
    /// zeewin2, 2026-11-09). The pane host writes an Escape-led key into an input that holds nothing
    /// unread (`conpty/unread.rs`), so the console host reads its bytes together: Alt+Enter
    /// wherever the filler ends, written through the human's path or straight to the pane.
    #[test]
    fn an_escape_led_key_behind_a_full_read_reaches_a_key_reader_whole_through_the_hosts_human_path(
    ) {
        for filler in [CONSOLE_READ - 1, CONSOLE_READ - 100, CONSOLE_READ, 0] {
            for human in [true, false] {
                let (keys, seen, waited) = keys_behind(filler, b"\x1b\r", human);
                println!("ESC CR behind {filler} bytes, human {human}: written in {waited:?}");
                assert_eq!(
                    keys,
                    [("Enter".to_owned(), 13, 1)],
                    "ESC CR behind {filler} bytes, human {human}; its screen: {seen:?}"
                );
            }
        }
    }

    /// A line feed is one byte, which no read can split: it is the same key, Enter with Control,
    /// wherever it falls, and it waits for nothing.
    #[test]
    fn a_line_feed_is_the_same_key_wherever_the_console_hosts_read_ends() {
        for filler in [CONSOLE_READ - 1, CONSOLE_READ - 100, 0] {
            let (keys, seen, _) = keys_behind(filler, b"\n", true);
            assert_eq!(
                keys,
                [("Enter".to_owned(), 10, 4)],
                "a line feed after {filler} bytes; its screen: {seen:?}"
            );
        }
    }

    /// Every Escape-led key a person types goes through the same write: an arrow is one key
    /// where a read would end after its Escape, and Escape itself is the Escape key, one.
    #[test]
    fn an_arrow_and_escape_itself_behind_a_full_read_reach_a_key_reader_as_those_keys() {
        let (arrow, seen, _) = keys_behind(CONSOLE_READ - 1, b"\x1b[A", true);
        assert_eq!(
            arrow.len(),
            1,
            "an arrow is one key: {arrow:?}; its screen: {seen:?}"
        );
        assert_eq!(arrow[0].0, "UpArrow", "{arrow:?}; its screen: {seen:?}");
        let (escape, seen, _) = keys_behind(CONSOLE_READ - 1, b"\x1b", true);
        assert_eq!(
            escape,
            [("Escape".to_owned(), 27, 0)],
            "Escape itself is one Escape key; its screen: {seen:?}"
        );
        let (alt_x, seen, _) = keys_behind(CONSOLE_READ - 1, b"\x1bx", true);
        assert_eq!(
            alt_x,
            [("X".to_owned(), 120, 1)],
            "Alt+x is one key; its screen: {seen:?}"
        );
    }

    /// What the console host makes of the bytes the page writes for Shift+Enter (T-1330), for the
    /// harnesses that read the console's key events (Codex, Devin and Muse, through crossterm,
    /// which reads the Enter key with the modifiers the console says): ESC CR is the Enter key
    /// with Alt, the new line each lists; a line feed is the Enter key with Control, which none
    /// lists (their new-line keys are Shift+Enter, Alt+Enter and Ctrl+J), so the page does not
    /// write it for them; and a plain Enter, the message sent, is the Enter key alone.
    #[test]
    fn the_new_line_key_the_page_writes_reaches_a_key_reader_as_enter_with_alt() {
        let key_of = |character: u32, modifiers: u32| KeyRead {
            number: 1,
            key: "Enter".to_owned(),
            character,
            modifiers,
        };
        for (what, bytes, wanted) in [
            ("ESC CR (Alt+Enter)", &b"\x1b\r"[..], key_of(13, 1)),
            ("a line feed (Ctrl+J)", &b"\n"[..], key_of(10, 4)),
            ("a plain Enter", &b"\r"[..], key_of(13, 0)),
        ] {
            let (keys, seen) = keys_of(bytes, 1);
            assert_eq!(keys, [wanted], "{what}; its screen: {seen:?}");
        }
    }

    /// Why the page never writes the kitty protocol's Shift+Enter (`CSI 13;2u`) on Windows: the
    /// console host knows no such sequence, and a program that reads key events is given its
    /// seven characters as keys of typed text, into the message. If this fails, the host reads it,
    /// and it is worth a look whether it is the one key for every harness.
    #[test]
    fn a_csi_u_shift_enter_reaches_a_key_reader_as_seven_typed_characters() {
        let (keys, seen) = keys_of(b"\x1b[13;2u", 8);
        let characters: Vec<u32> = keys.iter().map(|read| read.character).collect();
        assert_eq!(
            characters,
            [27, 91, 49, 51, 59, 50, 117],
            "its screen: {seen:?}"
        );
        assert!(
            keys.iter().all(|read| read.modifiers == 0),
            "its keys: {keys:?}"
        );
    }

    /// What reaches the harnesses that set the console's virtual-terminal input (Claude Code and
    /// Pi: Node, in raw mode) of the new-line bytes: all of them, whole. A line feed is Pi's new
    /// line (its Alt+Enter, ESC CR, queues a follow-up); ESC CR is Claude Code's.
    #[test]
    fn the_new_line_bytes_reach_a_node_program_in_virtual_terminal_input_whole() {
        for (what, bytes, wanted) in [
            ("a line feed", &b"\n"[..], "0a"),
            ("ESC CR", &b"\x1b\r"[..], "1b0d"),
            ("a plain Enter", &b"\r"[..], "0d"),
        ] {
            let (hex, seen) = node_reads(bytes);
            assert_eq!(hex, wanted, "{what}; its screen: {seen:?}");
        }
    }

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

/// What a human's Shift+Enter becomes when the window's input is nearly full (T-1336), through
/// a real pty: the bytes the page writes for it are `sends.mac` of the fixture the page tests hold the
/// page to (`app/tests/fixtures/pane-keys/newline-cases.json`).
///
/// A program takes a lone Escape for "stop". A pty's input queue holds a limited number of bytes
/// (a Mac's, 1,024, less what the line discipline keeps), and a write that reaches past it is taken
/// in two parts, the second when the reader has read: so an `ESC CR` whose Escape is the last byte
/// the queue takes is read as an Escape, and the Enter in the next read, whatever the reader's own
/// buffer (a window that is slow to read, behind a long paste, in the shape the review T-1336 found
/// with Codex's own reader: a 1,009-character bracketed paste, a reader that waits, Shift+Enter).
#[cfg(all(test, unix))]
mod boundary {
    use std::collections::HashMap;
    use std::io::Read;
    use std::path::Path;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use portable_pty::{native_pty_system, CommandBuilder, PtySize};

    use crate::pty::{serial_pty_test, OpenedPane, PaneTable};

    const FIXTURE: &str =
        include_str!("../../../../app/tests/fixtures/pane-keys/newline-cases.json");

    fn size() -> PtySize {
        PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    /// What the page writes for Shift+Enter on a Mac, by harness: the fixture's `sends.mac`.
    fn sent_on_a_mac() -> Vec<(String, Vec<u8>)> {
        let fixture: serde_json::Value =
            serde_json::from_str(FIXTURE).expect("the fixture is JSON");
        let harnesses = fixture["harnesses"].as_object().expect("the harnesses");
        assert!(
            harnesses.len() >= 5,
            "every harness: {:?}",
            harnesses.keys()
        );
        harnesses
            .iter()
            .map(|(name, each)| {
                let bytes = each["sends"]["mac"]
                    .as_array()
                    .expect("what is sent on a Mac")
                    .iter()
                    .map(|byte| u8::try_from(byte.as_u64().expect("a byte")).expect("a byte"))
                    .collect();
                (name.clone(), bytes)
            })
            .collect()
    }

    /// The program's output until it has printed `word`, read in a thread so that a program that
    /// never does fails the test, not holds it.
    fn spawn_reader(mut reader: Box<dyn Read + Send>) -> mpsc::Receiver<Vec<u8>> {
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while let Ok(count) = reader.read(&mut buffer) {
                if count == 0 || sender.send(buffer[..count].to_vec()).is_err() {
                    return;
                }
            }
        });
        receiver
    }

    /// How many bytes this system's pty takes into its input queue of a program that reads none
    /// of them: bytes are written one at a time to a pty whose program does not read, with the
    /// master set not to block, until it refuses one.
    fn queue_limit() -> usize {
        let pair = native_pty_system().openpty(size()).expect("a pty");
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", "stty raw -echo; printf ready; sleep 5"]);
        let mut child = pair.slave.spawn_command(command).expect("the program");
        drop(pair.slave);
        let output = spawn_reader(pair.master.try_clone_reader().expect("the pty's output"));
        let mut seen = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !String::from_utf8_lossy(&seen).contains("ready") {
            let left = deadline.saturating_duration_since(Instant::now());
            seen.extend(output.recv_timeout(left).expect("the program says ready"));
        }
        let fd = pair.master.as_raw_fd().expect("the master's descriptor");
        // SAFETY: `fd` is the open master of this pty, which lives to the end of the function.
        let taken = unsafe {
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
            let mut taken = 0_usize;
            while taken < (1 << 20) && libc::write(fd, b"x".as_ptr().cast(), 1) == 1 {
                taken += 1;
            }
            taken
        };
        child.kill().expect("end the program");
        child.wait().expect("the program ended");
        assert!(taken > 16, "the pty took only {taken} bytes");
        taken
    }

    /// What a program that reads its raw terminal in blocks, after it has waited a second, reads of
    /// `filler` bytes and then `key` written to its pane in one write by the pane host: each read
    /// as the bytes it was.
    fn reads_of(filler: usize, key: &[u8]) -> Vec<Vec<u8>> {
        let _pty_guard = serial_pty_test();
        let table = PaneTable::new();
        let script = "stty raw -echo; printf ready; sleep 1; i=0; \
            while [ $i -lt 8 ]; do dd bs=4096 count=1 2>/dev/null | od -An -v -tx1 | tr -d ' \\n'; echo; i=$((i+1)); done";
        let OpenedPane { key: pane, reader } = table
            .open(
                Path::new("/tmp"),
                &["/bin/sh".to_owned(), "-c".to_owned(), script.to_owned()],
                &HashMap::new(),
                size(),
            )
            .expect("the program");
        let output = spawn_reader(reader);
        let mut seen = String::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while !seen.contains("ready") {
            let left = deadline.saturating_duration_since(Instant::now());
            seen.push_str(&String::from_utf8_lossy(
                &output.recv_timeout(left).expect("the program says ready"),
            ));
        }
        let mut bytes = vec![b'x'; filler];
        bytes.extend_from_slice(key);
        // The write is one call, as the pane host's is; where the queue is full it waits for the
        // program's first read, a second on.
        table.write(&pane, &bytes).expect("write the keys");
        let total = bytes.len();
        let mut reads: Vec<Vec<u8>> = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(20);
        while reads.iter().map(Vec::len).sum::<usize>() < total {
            let left = deadline.saturating_duration_since(Instant::now());
            let Ok(chunk) = output.recv_timeout(left) else {
                break;
            };
            seen.push_str(&String::from_utf8_lossy(&chunk));
            reads = seen
                .split("ready")
                .nth(1)
                .unwrap_or_default()
                .split_whitespace()
                .map(|line| {
                    (0..line.len() / 2)
                        .map(|at| u8::from_str_radix(&line[at * 2..at * 2 + 2], 16).unwrap_or(0))
                        .collect()
                })
                .collect();
        }
        table.kill(&pane).expect("close the pane");
        reads
    }

    /// The reads, each as how many bytes it was and the last two.
    fn shown(reads: &[Vec<u8>]) -> String {
        reads
            .iter()
            .map(|read| {
                format!(
                    "{} bytes ending {:02x?}",
                    read.len(),
                    &read[read.len().saturating_sub(2)..]
                )
            })
            .collect::<Vec<_>>()
            .join(", then ")
    }

    /// The premise, measured on this system: an `ESC CR` that begins at the last byte the queue
    /// takes is read as an Escape, and the Enter in the next read. If this fails, a read no longer
    /// splits a key and the test below proves nothing.
    #[test]
    fn an_escape_led_key_at_the_end_of_a_full_queue_is_read_as_a_lone_escape() {
        let limit = queue_limit();
        let reads = reads_of(limit - 1, b"\x1b\r");
        assert!(
            reads.iter().any(|read| read.last() == Some(&0x1b)),
            "with {} bytes waiting, ESC CR was read as {}",
            limit - 1,
            shown(&reads)
        );
        let whole = reads_of(0, b"\x1b\r");
        assert_eq!(whole, [vec![0x1b, 0x0d]], "with nothing waiting");
    }

    /// What the page writes for Shift+Enter is never read as a lone Escape, in any harness, wherever
    /// the queue's end falls: no read ends in an Escape.
    #[test]
    fn shift_enter_is_never_read_as_a_lone_escape_behind_a_full_queue() {
        let limit = queue_limit();
        for (harness, key) in sent_on_a_mac() {
            for waiting in [limit - 1, limit - 2, limit - key.len().max(1)] {
                let reads = reads_of(waiting, &key);
                assert!(
                    reads.iter().all(|read| read.last() != Some(&0x1b)),
                    "{harness}: {key:?} written behind {waiting} bytes was read as {}",
                    shown(&reads)
                );
            }
        }
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
