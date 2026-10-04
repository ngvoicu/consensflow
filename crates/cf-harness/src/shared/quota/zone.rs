//! The zone a refusal names, as `Intl.DateTimeFormat` takes a `timeZone`
//! (`resetAt`, `hosts/lib/quota.js`): in any ASCII case, never trimmed.
//!
//! `Intl` reads ICU's zones. Most are the bundled database's, by the same
//! names; ICU also keeps names tzdata has not (Java's three-letter ids, two
//! names tzdata dropped, the SystemV zones it dropped in 2020), takes an
//! offset for a zone, and refuses two names the database has: `Factory`, and
//! `Etc/Unknown`, which jiff answers with a zone of no offset. Each list here
//! is what Node 26 (ICU 78) takes that jiff's tzdata 2026c has not.
//!
//! Kept from Node on purpose: a SystemV zone with daylight time keeps it from
//! the last Sunday in April to the last in October, every year; ICU's start
//! it on January 6 in 1974 and February 23 in 1975.

use jiff::tz::{Offset, TimeZone, TimeZoneDatabase};

/// ICU's names for a zone of the database, each with the zone's name there.
const LINKS: [(&str, &str); 27] = [
    ("ACT", "Australia/Darwin"),
    ("AET", "Australia/Sydney"),
    ("AGT", "America/Buenos_Aires"),
    ("ART", "Africa/Cairo"),
    ("AST", "America/Anchorage"),
    ("BET", "America/Sao_Paulo"),
    ("BST", "Asia/Dhaka"),
    ("CAT", "Africa/Maputo"),
    ("CNT", "America/St_Johns"),
    ("CST", "America/Chicago"),
    ("CTT", "Asia/Shanghai"),
    ("EAT", "Africa/Nairobi"),
    ("ECT", "Europe/Paris"),
    ("IET", "America/Indianapolis"),
    ("IST", "Asia/Calcutta"),
    ("JST", "Asia/Tokyo"),
    ("MIT", "Pacific/Apia"),
    ("NET", "Asia/Yerevan"),
    ("NST", "Pacific/Auckland"),
    ("PLT", "Asia/Karachi"),
    ("PNT", "America/Phoenix"),
    ("PRT", "America/Puerto_Rico"),
    ("PST", "America/Los_Angeles"),
    ("SST", "Pacific/Guadalcanal"),
    ("VST", "Asia/Saigon"),
    ("Canada/East-Saskatchewan", "America/Regina"),
    ("US/Pacific-New", "America/Los_Angeles"),
];

/// ICU's SystemV zones, each as the POSIX rule that reads it.
const SYSTEM_V: [(&str, &str); 13] = [
    ("SystemV/AST4", "AST4"),
    ("SystemV/AST4ADT", "AST4ADT,M4.5.0,M10.5.0"),
    ("SystemV/CST6", "CST6"),
    ("SystemV/CST6CDT", "CST6CDT,M4.5.0,M10.5.0"),
    ("SystemV/EST5", "EST5"),
    ("SystemV/EST5EDT", "EST5EDT,M4.5.0,M10.5.0"),
    ("SystemV/HST10", "HST10"),
    ("SystemV/MST7", "MST7"),
    ("SystemV/MST7MDT", "MST7MDT,M4.5.0,M10.5.0"),
    ("SystemV/PST8", "PST8"),
    ("SystemV/PST8PDT", "PST8PDT,M4.5.0,M10.5.0"),
    ("SystemV/YST9", "YST9"),
    ("SystemV/YST9YDT", "YST9YDT,M4.5.0,M10.5.0"),
];

/// The names the database has and `Intl` refuses.
const REFUSED: [&str; 2] = ["Factory", "Etc/Unknown"];

/// The zone `name` names, as `Intl` takes it; none where `Intl` throws.
pub(super) fn time_zone(name: &str) -> Option<TimeZone> {
    if let Some(offset) = offset(name) {
        return Some(TimeZone::fixed(offset));
    }
    let named = |(id, _): &&(&str, &str)| id.eq_ignore_ascii_case(name);
    if let Some((_, rule)) = SYSTEM_V.iter().find(named) {
        return TimeZone::posix(rule).ok();
    }
    if REFUSED
        .iter()
        .any(|refused| refused.eq_ignore_ascii_case(name))
    {
        return None;
    }
    let name = LINKS.iter().find(named).map_or(name, |(_, zone)| zone);
    TimeZoneDatabase::bundled().get(name).ok()
}

/// An offset as `Intl` takes one for a zone: `±HH`, `±HHMM` or `±HH:MM`, to
/// 23 hours and 59 minutes, its sign `+`, `-` or U+2212.
fn offset(name: &str) -> Option<Offset> {
    let (negative, digits) = match name.strip_prefix('+') {
        Some(digits) => (false, digits),
        None => (true, name.strip_prefix(['-', '\u{2212}'])?),
    };
    let two = |tens: u8, ones: u8| {
        (tens.is_ascii_digit() && ones.is_ascii_digit())
            .then(|| i32::from(tens - b'0') * 10 + i32::from(ones - b'0'))
    };
    let (hours, minutes) = match *digits.as_bytes() {
        [tens, ones] => (two(tens, ones)?, 0),
        [h_tens, h_ones, m_tens, m_ones] | [h_tens, h_ones, b':', m_tens, m_ones] => {
            (two(h_tens, h_ones)?, two(m_tens, m_ones)?)
        }
        _ => return None,
    };
    if hours > 23 || minutes > 59 {
        return None;
    }
    let seconds = (hours * 60 + minutes) * 60;
    Offset::from_seconds(if negative { -seconds } else { seconds }).ok()
}
