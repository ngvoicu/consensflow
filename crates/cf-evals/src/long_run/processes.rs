//! The system's word on the processes the run follows: whether each is there, and
//! when it began (its identity, which a number taken again by another process does
//! not have), asked of many numbers at once. It is for what the report says of a
//! process the run found left behind, and nothing more: the run ends no process it
//! did not start, and none by its number. On Unix `ps` says when a process began
//! (`lstart`); on Windows the creation time CIM keeps (`Get-CimInstance
//! Win32_Process`, in the 100 ns since 1601), asked by one PowerShell for every
//! number of a look, since one takes ten seconds and more to start.
//!
//! An asking that failed (the program could not be run, ran out of its time, or said
//! why it could not answer) says nothing of what is there: it is an error, never an
//! answer that names none.

use std::collections::BTreeMap;
use std::time::Duration;

use cf_e2e::process::{Ran, Run};

/// How many numbers one asking names: a filter of CIM's, or `ps`'s list, stays short.
const AT_ONCE: usize = 64;

/// How long one asking is given: a PowerShell on a loaded machine takes its time.
const ASKING: Duration = Duration::from_secs(120);

/// When each of `pids` that is there began, by its number: one that is not there is
/// not named. Why the system could not be asked, where it could not.
pub fn identities(pids: &[u32]) -> Result<BTreeMap<u32, String>, String> {
    let mut found = BTreeMap::new();
    for chunk in pids.chunks(AT_ONCE) {
        let ran = if cfg!(windows) {
            Run::new("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command"])
                .arg(cim_script(chunk))
                .inheriting_env()
                .limit(ASKING)
                .run()
        } else {
            Run::new("ps")
                .args(["-o", "pid=,lstart=", "-p"])
                .arg(numbers(chunk))
                .inheriting_env()
                .limit(ASKING)
                .run()
        };
        found.extend(answer(
            ran.map_err(|failed| failed.to_string()),
            cfg!(windows),
        )?);
    }
    Ok(found)
}

/// What one asking said: the identities, or why it said nothing. `ps` exits 1 when a
/// number it was given is no process's, and says so on no output; anything it says
/// on its error output is its failure. The PowerShell answers 0 when it answered.
fn answer(ran: Result<Ran, String>, windows: bool) -> Result<BTreeMap<u32, String>, String> {
    let ran = ran?;
    let said = ran.stderr.trim();
    if ran.timed_out || ran.code.is_none() {
        return Err(format!(
            "the asking ran out of its time or was ended: {said}"
        ));
    }
    if (windows && ran.code != Some(0)) || !said.is_empty() {
        return Err(format!("the asking failed ({:?}): {said}", ran.code));
    }
    Ok(parse(&ran.stdout))
}

/// The earliest moment, in milliseconds since the epoch, a process the system says
/// began at `began` can have begun, where `began` says it to the moment: Windows's
/// creation time, in 100 ns since 1601, asked by a PowerShell that takes seconds to
/// answer. None for `ps`'s `lstart`, which says the second alone, and comes a few
/// milliseconds after the look on a system that gives a number again only once it
/// has given every other.
pub fn began_at(began: &str) -> Option<i64> {
    let ticks: i64 = began.parse().ok().filter(|ticks| *ticks >= 0)?;
    Some(ticks / 10_000 - 11_644_473_600_000)
}

/// The numbers as `ps -p` takes them.
fn numbers(pids: &[u32]) -> String {
    pids.iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

/// The PowerShell that says the number and the creation time of each of `pids`
/// that is there, a line each.
pub fn cim_script(pids: &[u32]) -> String {
    let filter = pids
        .iter()
        .map(|pid| format!("ProcessId={pid}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    format!(
        "Get-CimInstance Win32_Process -Filter '{filter}' | ForEach-Object {{ '{{0}} {{1}}' -f $_.ProcessId, $_.CreationDate.ToFileTimeUtc() }}"
    )
}

/// What `ps -o pid=,lstart=` or the PowerShell of [`cim_script`] said: a number,
/// then when it began, a line each.
pub fn parse(said: &str) -> BTreeMap<u32, String> {
    said.lines()
        .filter_map(|line| {
            let (pid, began) = line.trim().split_once(char::is_whitespace)?;
            let began = crate::js::squeeze(began.trim());
            (!began.is_empty()).then_some((pid.parse().ok()?, began))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_ps_says_is_each_number_with_when_it_began_its_spaces_kept_as_one() {
        let said =
            "  412 Fri Oct 16 00:29:15 2026\n 9134 Thu Oct 15  21:32:01 2026\nnot a line\n77\n";
        assert_eq!(
            parse(said),
            BTreeMap::from([
                (412, "Fri Oct 16 00:29:15 2026".to_owned()),
                (9134, "Thu Oct 15 21:32:01 2026".to_owned()),
            ])
        );
    }

    #[test]
    fn what_windows_says_is_each_number_with_its_creation_time() {
        assert_eq!(
            parse("4120 134051234567890123\r\n88 134051234500000000\r\n"),
            BTreeMap::from([
                (88, "134051234500000000".to_owned()),
                (4120, "134051234567890123".to_owned()),
            ])
        );
        assert_eq!(
            cim_script(&[4120, 88]),
            "Get-CimInstance Win32_Process -Filter 'ProcessId=4120 OR ProcessId=88' | ForEach-Object { '{0} {1}' -f $_.ProcessId, $_.CreationDate.ToFileTimeUtc() }"
        );
        assert_eq!(numbers(&[1, 22, 333]), "1,22,333");
    }

    #[test]
    fn windows_says_to_the_moment_when_a_process_began_and_ps_only_to_the_second() {
        // 2025-10-15T10:00:00.123Z, as CIM's creation time in 100 ns since 1601.
        let at = 1_760_522_400_123_i64;
        let ticks = (at + 11_644_473_600_000) * 10_000 + 4_567;
        assert_eq!(began_at(&ticks.to_string()), Some(at));
        assert_eq!(began_at("Fri Oct 16 00:29:15 2026"), None);
        assert_eq!(began_at("-5"), None);
    }

    #[cfg(unix)]
    #[test]
    fn this_process_is_there_with_when_it_began_and_a_number_no_process_has_is_not() {
        let own = std::process::id();
        // A process that has ended: its number is one no process has now (macOS's `ps`
        // refuses a number no process could have, and says nothing of the others).
        let mut ended = Run::new("true").inheriting_env().spawn().unwrap();
        let gone = ended.id();
        assert!(ended.wait(Duration::from_secs(30)).unwrap().is_some());
        let said = identities(&[own, gone]).unwrap();
        assert!(said.contains_key(&own), "{said:?}");
        assert!(!said.contains_key(&gone));
        // Asked again, the same process began at the same time.
        assert_eq!(identities(&[own]).unwrap().get(&own), said.get(&own));
    }

    fn ran(code: Option<i32>, stdout: &str, stderr: &str, timed_out: bool) -> Ran {
        Ran {
            code,
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
            timed_out,
            held: false,
        }
    }

    #[test]
    fn an_asking_that_failed_says_nothing_of_what_is_there() {
        // `ps` named one of the two it was asked, and exits 1 for the other: an answer.
        assert_eq!(
            answer(
                Ok(ran(Some(1), " 412 Fri Oct 16 00:29:15 2026\n", "", false)),
                false
            )
            .unwrap()
            .len(),
            1
        );
        // A PowerShell that found none answers 0 and says nothing: none is there.
        assert!(answer(Ok(ran(Some(0), "", "", false)), true)
            .unwrap()
            .is_empty());
        // What could not be run, ran out of its time, or said why it failed is no
        // answer that names none.
        for failed in [
            Err("powershell: not found".to_owned()),
            Ok(ran(None, "", "", true)),
            Ok(ran(Some(1), "", "Get-CimInstance : Access denied", false)),
            Ok(ran(
                Some(1),
                "",
                "ps: process id too large: 2147483646",
                false,
            )),
        ] {
            let windows = matches!(&failed, Ok(said) if said.stderr.contains("Cim"));
            assert!(answer(failed.clone(), windows).is_err(), "{failed:?}");
        }
    }
}
