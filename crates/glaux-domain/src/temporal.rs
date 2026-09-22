//! Exact RFC 3339 instant ordering with source context, not duration arithmetic.
//!
//! The key is (UTC civil second, leap slot, exact fractional second). A positive
//! leap uses the preceding 23:59:59 civil second with a separate slot, so it can
//! never compare equal to either neighbour. Civil seconds alone are NOT elapsed
//! SI seconds or a lossless Unix timestamp for a leap instant.

use crate::numeric::ExactNumber;
use std::{cmp::Ordering, fmt, hash::{Hash, Hasher}, str::FromStr};

/// Local resource budget, not a standards precision restriction.
pub const MAX_TIMESTAMP_BYTES: usize = 4096;

/// Historical positive leap dates, in UTC. Source and update limits: docs/exact-time.md.
/// This immutable basis contains no negative leap event; future announcements
/// require a reviewed basis update, not inference from a month ending.
const POSITIVE_LEAP_DATES: [(u32, u32, u32); 27] = [
    (1972, 6, 30), (1972, 12, 31), (1973, 12, 31), (1974, 12, 31),
    (1975, 12, 31), (1976, 12, 31), (1977, 12, 31), (1978, 12, 31),
    (1979, 12, 31), (1981, 6, 30), (1982, 6, 30), (1983, 6, 30),
    (1985, 6, 30), (1987, 12, 31), (1989, 12, 31), (1990, 12, 31),
    (1992, 6, 30), (1993, 6, 30), (1994, 6, 30), (1995, 12, 31),
    (1997, 6, 30), (1998, 12, 31), (2005, 12, 31), (2008, 12, 31),
    (2012, 6, 30), (2015, 6, 30), (2016, 12, 31),
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeError {
    Syntax,
    Calendar,
    InputLimit,
    UnrecognizedLeapSecond,
    InconsistentStorage,
}

impl fmt::Display for TimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "invalid RFC 3339 instant syntax",
            Self::Calendar => "invalid Gregorian date or clock field",
            Self::InputLimit => "timestamp exceeds the local byte budget",
            Self::UnrecognizedLeapSecond => "leap second is not established by the pinned basis",
            Self::InconsistentStorage => "stored instant key does not match retained source",
        })
    }
}
impl std::error::Error for TimeError {}

/// A bounded RFC 3339 instant and the exact supplied text.
///
/// Equality/order/hash ignore source spelling and use all three key fields.
/// No unchecked constructor, automatic timestamp cast, interval or duration API.
#[derive(Clone, Debug)]
pub struct ExactInstant {
    civil_second: i64,
    leap: bool,
    fraction: ExactNumber,
    source: String,
    fraction_start: usize,
    fraction_digits: usize,
    offset_seconds: i32,
    offset_known: bool,
}

impl ExactInstant {
    pub fn parse_rfc3339(text: &str) -> Result<Self, TimeError> {
        if text.len() > MAX_TIMESTAMP_BYTES { return Err(TimeError::InputLimit); }
        let bytes = text.as_bytes();
        if bytes.len() < 20 || !bytes.is_ascii()
            || bytes[4] != b'-' || bytes[7] != b'-'
            || !matches!(bytes[10], b'T' | b't')
            || bytes[13] != b':' || bytes[16] != b':' {
            return Err(TimeError::Syntax);
        }
        let year = digits(&bytes[0..4])?;
        let month = digits(&bytes[5..7])?;
        let day = digits(&bytes[8..10])?;
        let hour = digits(&bytes[11..13])?;
        let minute = digits(&bytes[14..16])?;
        let second = digits(&bytes[17..19])?;
        if !(1..=12).contains(&month) || day == 0 || day > month_days(year, month)
            || hour > 23 || minute > 59 || second > 60 {
            return Err(TimeError::Calendar);
        }
        let mut pos = 19;
        let mut fraction_start = pos;
        let mut fraction_digits = 0;
        if bytes.get(pos) == Some(&b'.') {
            pos += 1;
            fraction_start = pos;
            while bytes.get(pos).is_some_and(u8::is_ascii_digit) { pos += 1; }
            fraction_digits = pos - fraction_start;
            if fraction_digits == 0 { return Err(TimeError::Syntax); }
        }
        let (offset_seconds, offset_known) = match bytes.get(pos..) {
            Some([b'Z' | b'z']) => (0, false),
            Some([sign @ (b'+' | b'-'), h1, h2, b':', m1, m2]) => {
                let hours = digits(&[*h1, *h2])?;
                let minutes = digits(&[*m1, *m2])?;
                if hours > 23 || minutes > 59 { return Err(TimeError::Calendar); }
                let magnitude = (hours * 3600 + minutes * 60) as i32;
                let value = if *sign == b'-' { -magnitude } else { magnitude };
                (value, !(*sign == b'-' && magnitude == 0))
            }
            _ => return Err(TimeError::Syntax),
        };
        let leap = second == 60;
        let local_second = civil_days(year, month, day) * 86_400
            + i64::from(hour * 3600 + minute * 60 + second.min(59));
        let civil_second = local_second - i64::from(offset_seconds);
        if leap && !POSITIVE_LEAP_DATES.iter().any(|&(y, m, d)| {
            civil_days(y, m, d) * 86_400 + 86_399 == civil_second
        }) {
            return Err(TimeError::UnrecognizedLeapSecond);
        }
        let fraction_text = if fraction_digits == 0 { "0".to_owned() }
            else { format!("0.{}", &text[fraction_start..pos]) };
        let fraction = ExactNumber::parse_json_number(&fraction_text)
            .map_err(|_| TimeError::Syntax)?;
        Ok(Self {
            civil_second, leap, fraction, source: text.to_owned(),
            fraction_start, fraction_digits, offset_seconds, offset_known,
        })
    }

    /// Validates a stored comparison key against its retained source. No lossy
    /// cast or normalization can silently override a mismatch.
    pub fn from_storage_parts(civil_second: i64, leap: bool, fraction: &str, source: &str)
        -> Result<Self, TimeError> {
        let parsed = Self::parse_rfc3339(source)?;
        let stored_fraction = ExactNumber::parse_json_number(fraction)
            .map_err(|_| TimeError::InconsistentStorage)?;
        if civil_second != parsed.civil_second || leap != parsed.leap
            || stored_fraction != parsed.fraction {
            return Err(TimeError::InconsistentStorage);
        }
        Ok(parsed)
    }

    /// UTC civil-second coordinate. Always retain leap slot and fraction with it.
    pub fn civil_second(&self) -> i64 { self.civil_second }
    pub fn is_leap_second(&self) -> bool { self.leap }
    pub fn fraction(&self) -> &ExactNumber { &self.fraction }
    /// Finite decimal suitable for an unconstrained PostgreSQL numeric column.
    pub fn fraction_decimal(&self) -> String {
        if self.fraction_digits == 0 { "0".to_owned() }
        else { format!("0.{}", &self.source[self.fraction_start..self.fraction_start+self.fraction_digits]) }
    }
    pub fn source_lexeme(&self) -> &str { &self.source }
    /// Number of supplied fractional digits, not a claim about clock accuracy.
    pub fn fraction_digits(&self) -> usize { self.fraction_digits }
    pub fn offset_seconds(&self) -> i32 { self.offset_seconds }
    /// An explicit known numeric local offset was supplied. Z/z and -00:00
    /// are false (RFC 9557 section 2); the UTC instant is known in every case.
    /// Original spelling distinguishes Z from -00:00 without invented context.
    pub fn offset_known(&self) -> bool { self.offset_known }
}

impl FromStr for ExactInstant {
    type Err = TimeError;
    fn from_str(text: &str) -> Result<Self, Self::Err> { Self::parse_rfc3339(text) }
}
impl PartialEq for ExactInstant {
    fn eq(&self, other: &Self) -> bool { self.cmp(other) == Ordering::Equal }
}
impl Eq for ExactInstant {}
impl PartialOrd for ExactInstant {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for ExactInstant {
    fn cmp(&self, other: &Self) -> Ordering {
        self.civil_second.cmp(&other.civil_second)
            .then_with(|| self.leap.cmp(&other.leap))
            .then_with(|| self.fraction.cmp(&other.fraction))
    }
}
impl Hash for ExactInstant {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.civil_second.hash(state);
        self.leap.hash(state);
        self.fraction.hash(state);
    }
}

fn digits(bytes: &[u8]) -> Result<u32, TimeError> {
    bytes.iter().try_fold(0, |value, byte| {
        if byte.is_ascii_digit() { Ok(value * 10 + u32::from(byte - b'0')) }
        else { Err(TimeError::Syntax) }
    })
}
fn leap_year(year: u32) -> bool {
    year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400))
}
fn month_days(year: u32, month: u32) -> u32 {
    match month {
        2 if leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 0,
    }
}
fn days_before_year(year: u32) -> i64 {
    // Include Gregorian year zero, a leap year, without unsigned underflow.
    let leaps = if year == 0 { 0 } else {
        let prior = year - 1;
        prior / 4 - prior / 100 + prior / 400 + 1
    };
    i64::from(year) * 365 + i64::from(leaps)
}
fn civil_days(year: u32, month: u32, day: u32) -> i64 {
    days_before_year(year) - days_before_year(1970)
        + (1..month).map(|m| i64::from(month_days(year, m))).sum::<i64>()
        + i64::from(day) - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    fn instant(text: &str) -> ExactInstant { text.parse().unwrap() }

    #[test]
    fn calendar_and_epoch_values_are_independent() {
        for (source, expected) in [
            ("1970-01-01T00:00:00Z", 0),
            ("1969-12-31T23:59:59Z", -1),
            ("2000-01-01T00:00:00Z", 946_684_800),
            ("2000-02-29T00:00:00Z", 951_782_400),
            ("1900-03-01T00:00:00Z", -2_203_891_200),
            ("0000-01-01T00:00:00Z", -62_167_219_200),
            ("9999-12-31T23:59:59Z", 253_402_300_799),
        ] { assert_eq!(instant(source).civil_second(), expected, "{source}"); }
        assert!(instant("0000-02-29T00:00:00Z") < instant("0000-03-01T00:00:00Z"));
        for source in ["1900-02-29T00:00:00Z", "2100-02-29T00:00:00Z",
            "2024-02-30T00:00:00Z", "2025-04-31T00:00:00Z", "2025-00-01T00:00:00Z",
            "2025-13-01T00:00:00Z", "2025-01-00T00:00:00Z", "2025-01-01T24:00:00Z",
            "2025-01-01T00:60:00Z", "2025-01-01T00:00:61Z", "2025-01-01T00:00:00+24:00",
            "2025-01-01T00:00:00+00:60"] {
            assert_eq!(ExactInstant::parse_rfc3339(source), Err(TimeError::Calendar));
        }
    }

    #[test]
    fn offset_sign_and_unknown_offset_preserve_instant() {
        assert_eq!(instant("1970-01-01T01:00:00+01:00").civil_second(), 0);
        assert_eq!(instant("1969-12-31T23:00:00-01:00").civil_second(), 0);
        assert_eq!(instant("1996-12-19T16:39:57-08:00"), instant("1996-12-20T00:39:57Z"));
        assert_eq!(instant("2000-03-01T00:00:00+00:01"), instant("2000-02-29T23:59:00Z"));
        assert_eq!(instant("1970-01-01T23:59:00+23:59").civil_second(), 0);
        let unknown = instant("1970-01-01t00:00:00.000-00:00");
        assert!(!unknown.offset_known());
        assert_eq!(unknown.offset_seconds(), 0);
        assert_eq!(unknown, instant("1970-01-01T00:00:00Z"));
        assert_eq!(unknown.fraction_digits(), 3);
        assert_eq!(unknown.source_lexeme(), "1970-01-01t00:00:00.000-00:00");
        assert!(instant("1970-01-01T00:00:00+00:00").offset_known());
        assert!(!instant("1970-01-01T00:00:00z").offset_known());
        assert!(instant("0000-01-01T00:00:00+23:59") < instant("0000-01-01T00:00:00Z"));
        assert!(instant("9999-12-31T23:59:59-23:59") > instant("9999-12-31T23:59:59Z"));
    }

    #[test]
    fn fractional_boundaries_are_not_rounded() {
        let first = instant("1970-01-01T00:00:00.1234561Z");
        let second = instant("1970-01-01T00:00:00.1234562Z");
        assert_eq!(first.cmp(&second), Ordering::Less);
        assert_eq!(first.fraction(), &"0.1234561".parse::<ExactNumber>().unwrap());
        assert_eq!(first.fraction_decimal(), "0.1234561");
        assert!(instant("1969-12-31T23:59:59.9999999999999999999Z") < instant("1970-01-01T00:00:00Z"));
        assert!(instant("1970-01-01T00:00:00.0000000000000000001Z") > instant("1970-01-01T00:00:00Z"));
        let variants = [instant("1970-01-01T00:00:00.1Z"), instant("1970-01-01t01:00:00.100+01:00")];
        assert_eq!(variants[0], variants[1]);
        assert_ne!(variants[0].source_lexeme(), variants[1].source_lexeme());
        assert_eq!(variants.into_iter().collect::<HashSet<_>>().len(), 1);
    }

    #[test]
    fn leap_slot_is_distinct_and_offsets_are_checked_in_utc() {
        let before = instant("2016-12-31T23:59:59.9999999999999999999Z");
        let leap = instant("2016-12-31T23:59:60Z");
        let after = instant("2017-01-01T00:00:00Z");
        assert_eq!(before.cmp(&leap), Ordering::Less);
        assert_eq!(leap.cmp(&after), Ordering::Less);
        assert_eq!(leap.civil_second(), 1_483_228_799);
        assert!(leap.is_leap_second());
        assert_eq!(leap, instant("2017-01-01T00:59:60+01:00"));
        assert_eq!(leap, instant("2016-12-31T15:59:60-08:00"));
        assert_eq!(instant("1990-12-31T23:59:60Z"), instant("1990-12-31T15:59:60-08:00"));
        assert!(instant("2016-12-31T23:59:60.999999999999Z") < after);
        for source in ["2016-12-30T23:59:60Z", "2017-12-31T23:59:60Z",
            "2016-12-31T23:59:60+01:00", "2016-12-31T12:59:60Z", "2099-12-31T23:59:60Z"] {
            assert_eq!(ExactInstant::parse_rfc3339(source), Err(TimeError::UnrecognizedLeapSecond));
        }
        assert_eq!(POSITIVE_LEAP_DATES.len(), 27);
        for (y,m,d) in POSITIVE_LEAP_DATES {
            assert!(instant(&format!("{y:04}-{m:02}-{d:02}T23:59:60Z")).is_leap_second());
        }
    }

    #[test]
    fn syntax_and_input_budgets_fail_without_repair() {
        for source in ["", "1970-01-01", "1970-01-01 00:00:00Z", "1970-01-01T00:00:00",
            "1970-01-01T00:00:00+0000", "1970-01-01T00:00:00+00:00:00",
            "1970-01-01T00:00:00.Z", "1970-01-01T00:00:00,1Z", "1970-01-01T00:00:00.1e2Z",
            "1970-01-01T00:00:00Z ", " 1970-01-01T00:00:00Z", "1970-01-01T00:00:00Z\0",
            "1970-01-01T00:00:00Z/..", "now", "１９７０-01-01T00:00:00Z", "+1970-01-01T00:00:00Z"] {
            assert_eq!(ExactInstant::parse_rfc3339(source), Err(TimeError::Syntax), "{source:?}");
        }
        let boundary = format!("1970-01-01T00:00:00.{}1Z", "0".repeat(MAX_TIMESTAMP_BYTES-22));
        assert_eq!(boundary.len(), MAX_TIMESTAMP_BYTES);
        assert!(instant(&boundary) > instant("1970-01-01T00:00:00Z"));
        assert_eq!(instant(&boundary).source_lexeme(), boundary);
        assert_eq!(ExactInstant::parse_rfc3339(&format!("{boundary}Z")), Err(TimeError::InputLimit));
    }

    #[test]
    fn storage_parts_must_match_source_without_normalizing_away_errors() {
        let source = "1970-01-01T01:00:00.1234567890123456789+01:00";
        let parsed = instant(source);
        let restored = ExactInstant::from_storage_parts(0, false, "0.123456789012345678900", source).unwrap();
        assert_eq!(restored, parsed);
        assert_eq!(restored.source_lexeme(), source);
        assert_eq!(restored.fraction_digits(), 19);
        for (second, leap, fraction) in [(1,false,"0.1234567890123456789"), (0,true,"0.1234567890123456789"),
            (0,false,"0.123457"), (0,false,"NaN"), (0,false,"-0.1"), (0,false,"1"), (i64::MAX,false,"0")] {
            assert_eq!(ExactInstant::from_storage_parts(second, leap, fraction, source), Err(TimeError::InconsistentStorage));
        }
    }
}
