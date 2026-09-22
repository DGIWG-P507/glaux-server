//! Bounded task 1.2.5 properties with an integer calendar oracle.
//!
//! The oracle walks years/months from 1970; it does not call a production
//! calendar conversion or infer expected seconds from a parsed timestamp.
//! Fractions use the separately tested exact-number primitive, never floats.

use std::collections::BTreeSet;

use glaux_domain::numeric::ExactNumber;
use glaux_domain::temporal::{ExactInstant, MAX_TIMESTAMP_BYTES};

const SEED: u64 = 0x0123_4567_89ab_cdef;
const CASES: usize = 1024;
const GENERATOR_VERSION: &str = "lcg64-time-v1";

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn bounded(&mut self, limit: u32) -> u32 {
        u32::try_from((self.next() >> 32) % u64::from(limit)).unwrap()
    }
}

fn leap_year(year: i32) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

fn month_days(year: i32, month: u32) -> u32 {
    match month {
        2 => {
            if leap_year(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_epoch(year: i32, month: u32, day: u32) -> i64 {
    let days_in_year = |value| if leap_year(value) { 366_i64 } else { 365_i64 };
    let years = if year >= 1970 {
        (1970..year).map(days_in_year).sum::<i64>()
    } else {
        -(year..1970).map(days_in_year).sum::<i64>()
    };
    years
        + (1..month)
            .map(|value| i64::from(month_days(year, value)))
            .sum::<i64>()
        + i64::from(day - 1)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CivilCase {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    offset_minutes: i32,
    unknown_offset: bool,
}

impl CivilCase {
    fn expected_second(&self) -> i64 {
        days_from_epoch(self.year, self.month, self.day) * 86_400
            + i64::from(self.hour * 3600 + self.minute * 60 + self.second)
            - i64::from(self.offset_minutes) * 60
    }

    fn lexeme(&self) -> String {
        let offset = self.offset_minutes.unsigned_abs();
        let sign = if self.offset_minutes < 0 || self.unknown_offset {
            '-'
        } else {
            '+'
        };
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            offset / 60,
            offset % 60
        )
    }
}

fn generate(seed: u64) -> Vec<CivilCase> {
    let mut generator = Generator(seed);
    (0..CASES)
        .map(|index| {
            let year = i32::try_from(generator.bounded(10_000)).unwrap();
            let month = 1 + generator.bounded(12);
            let unknown_offset = index.is_multiple_of(17);
            CivilCase {
                year,
                month,
                day: 1 + generator.bounded(month_days(year, month)),
                hour: generator.bounded(24),
                minute: generator.bounded(60),
                second: generator.bounded(60),
                // Year-edge offset crossings are covered only by the lexical
                // year contract; no artificial operational +/-14h limit.
                offset_minutes: if unknown_offset {
                    0
                } else {
                    i32::try_from(generator.bounded(2879)).unwrap() - 1439
                },
                unknown_offset,
            }
        })
        .collect()
}

fn parse(text: &str) -> ExactInstant {
    ExactInstant::parse_rfc3339(text)
        .unwrap_or_else(|error| panic!("valid fixture {text:?}: {error:?}"))
}

fn fraction(digits: &str) -> ExactNumber {
    let text = if digits.is_empty() {
        "0".to_owned()
    } else {
        format!("0.{digits}")
    };
    ExactNumber::parse_json_number(&text).unwrap()
}

#[test]
fn time_property_generator_is_pinned() {
    let mut generator = Generator(SEED);
    assert_eq!(GENERATOR_VERSION, "lcg64-time-v1");
    assert_eq!(generator.next(), 0x2ce3_2d23_35df_4552);
    assert_eq!(generator.next(), 0xca18_dd5a_e3c4_5eb9);
    let cases = generate(SEED);
    assert_eq!(cases, generate(SEED));
    assert_ne!(cases, generate(SEED ^ 1));
    assert_eq!(cases.len(), CASES);
    let distinct: BTreeSet<_> = cases.iter().map(CivilCase::lexeme).collect();
    assert!(distinct.len() > CASES / 2);
    assert!(cases.iter().any(|case| case.year < 1970));
    assert!(cases.iter().any(|case| case.year > 2000));
    assert!(cases.iter().any(|case| case.offset_minutes < 0));
    assert!(cases.iter().any(|case| case.offset_minutes > 0));
    assert!(cases.iter().any(|case| case.unknown_offset));
    assert!(cases.iter().any(|case| !case.unknown_offset));
    assert_eq!(days_from_epoch(1970, 1, 1), 0);
    assert_eq!(days_from_epoch(1969, 12, 31), -1);
    assert_eq!(days_from_epoch(2000, 1, 1), 10_957);
    assert_eq!(days_from_epoch(0, 1, 1), -719_528);
    assert_eq!(
        days_from_epoch(2400, 1, 1) - days_from_epoch(2000, 1, 1),
        146_097
    );
}

#[test]
fn time_property_calendar_and_offsets_match_integer_oracle() {
    let cases = generate(SEED);
    for (index, case) in cases.iter().enumerate() {
        let source = case.lexeme();
        let instant = parse(&source);
        assert_eq!(
            instant.civil_second(),
            case.expected_second(),
            "case={index}: {source}"
        );
        assert!(!instant.is_leap_second());
        assert_eq!(instant.fraction(), &fraction(""));
        assert_eq!(instant.fraction_digits(), 0);
        assert_eq!(instant.offset_seconds(), case.offset_minutes * 60);
        assert_eq!(instant.offset_known(), !case.unknown_offset);
        assert_eq!(instant.source_lexeme(), source);
        assert_eq!(source.parse::<ExactInstant>().unwrap(), instant);
        assert_eq!(parse(&source.to_ascii_lowercase()), instant);

        // UTC noon and the shifted local clock are independently equivalent;
        // restricting this generated offset avoids needing a second inverse
        // date algorithm just to manufacture the comparison timestamp.
        let offset = case.offset_minutes % 720;
        let local_minutes = 720 + offset;
        let shifted = CivilCase {
            hour: u32::try_from(local_minutes / 60).unwrap(),
            minute: u32::try_from(local_minutes % 60).unwrap(),
            offset_minutes: offset,
            unknown_offset: false,
            ..case.clone()
        };
        let utc = format!(
            "{:04}-{:02}-{:02}T12:00:{:02}Z",
            case.year, case.month, case.day, case.second
        );
        assert_eq!(parse(&shifted.lexeme()), parse(&utc));
        if index > 0 {
            let previous = &cases[index - 1];
            assert_eq!(
                instant.cmp(&parse(&previous.lexeme())),
                case.expected_second().cmp(&previous.expected_second())
            );
        }
    }
}

#[test]
fn time_property_fraction_order_and_source_are_exact() {
    let mut generator = Generator(SEED ^ 0x55aa);
    for index in 0..CASES {
        let prefix = format!("{:018}", generator.next() % 1_000_000_000_000_000_000);
        let low_digits = format!("{prefix}0");
        let high_digits = format!("{prefix}1");
        let low_text = format!("1969-12-31T23:59:59.{low_digits}Z");
        let high_text = format!("1969-12-31T23:59:59.{high_digits}Z");
        let low = parse(&low_text);
        let high = parse(&high_text);
        assert_eq!(low.civil_second(), -1);
        assert_eq!(high.civil_second(), -1);
        assert_eq!(low.fraction(), &fraction(&low_digits));
        assert_eq!(high.fraction(), &fraction(&high_digits));
        assert_eq!(low.fraction_digits(), 19);
        assert_eq!(low.source_lexeme(), low_text);
        assert!(
            low < high && high < parse("1970-01-01T00:00:00Z"),
            "case={index}"
        );
        assert_eq!(low.cmp(&high), high.cmp(&low).reverse());
        let padded = format!("1969-12-31t23:59:59.{low_digits}00-00:00");
        let equivalent = parse(&padded);
        assert_eq!(equivalent, low);
        assert!(!equivalent.offset_known());
        assert_eq!(equivalent.offset_seconds(), 0);
        assert_eq!(equivalent.fraction_digits(), 21);
        assert_eq!(equivalent.source_lexeme(), padded);
        assert_eq!(
            ExactNumber::parse_json_number(&low.fraction_decimal()).unwrap(),
            fraction(&low_digits)
        );
    }
    let before = parse("2016-12-31T23:59:59.999999999999999999999999999999Z");
    let leap = parse("2016-12-31T23:59:60Z");
    let leap_end = parse("2016-12-31T23:59:60.999999999999999999999999999999Z");
    let after = parse("2017-01-01T00:00:00Z");
    assert!(before < leap && leap < leap_end && leap_end < after);
    assert_eq!(leap.civil_second(), 1_483_228_799);
    assert!(leap.is_leap_second());
    assert_eq!(leap, parse("2017-01-01T00:59:60+01:00"));
    assert_eq!(leap, parse("2016-12-31T18:59:60-05:00"));
}

#[test]
fn time_property_calendar_leap_and_grammar_boundaries() {
    for century in 0..100 {
        let year = century * 100;
        let february = format!("{year:04}-02-29T00:00:00Z");
        assert_eq!(
            ExactInstant::parse_rfc3339(&february).is_ok(),
            century % 4 == 0,
            "{february}"
        );
        let march = parse(&format!("{year:04}-03-01T00:00:00Z"));
        assert_eq!(march.civil_second(), days_from_epoch(year, 3, 1) * 86_400);
    }
    for text in [
        "",
        "1970-01-01",
        "1970-01-01T00:00:00",
        "1970-01-01 00:00:00Z",
        "1970-01-01T24:00:00Z",
        "1970-01-01T23:60:00Z",
        "1970-01-01T00:00:61Z",
        "1970-00-01T00:00:00Z",
        "1970-13-01T00:00:00Z",
        "1970-01-00T00:00:00Z",
        "2001-02-29T00:00:00Z",
        "2000-04-31T00:00:00Z",
        "2016-12-31T22:59:60Z",
        "2017-12-31T23:59:60Z",
        "2016-12-31T23:59:60+01:00",
        "1969-12-31T23:59:60Z",
        "1970-01-01T00:00:00.Z",
        "1970-01-01T00:00:00,1Z",
        "1970-01-01T00:00:00+0000",
        "1970-01-01T00:00:00+24:00",
        "1970-01-01T00:00:00+00:60",
        " 1970-01-01T00:00:00Z",
        "1970-01-01T00:00:00Z\n",
        "1970-01-01T00:00:00Z\0",
        "1970-01-01T00:00:00Z/..",
        "10000-01-01T00:00:00Z",
        "１９７０-01-01T00:00:00Z",
    ] {
        assert!(ExactInstant::parse_rfc3339(text).is_err(), "{text:?}");
        assert!(text.parse::<ExactInstant>().is_err(), "{text:?}");
    }
    assert_eq!(
        parse("0000-01-01T00:00:00Z").civil_second(),
        -62_167_219_200
    );
    assert_eq!(
        parse("9999-12-31T23:59:59Z").civil_second(),
        253_402_300_799
    );
    assert_eq!(parse("1970-01-01T00:00:00+23:59").civil_second(), -86_340);
    assert_eq!(parse("1970-01-01T00:00:00-23:59").civil_second(), 86_340);
    assert_eq!(MAX_TIMESTAMP_BYTES, 4096);
    let digits = format!("{}1", "0".repeat(MAX_TIMESTAMP_BYTES - 22));
    let at_limit = format!("1970-01-01T00:00:00.{digits}Z");
    assert_eq!(at_limit.len(), MAX_TIMESTAMP_BYTES);
    let instant = parse(&at_limit);
    assert_eq!(instant.fraction_digits(), digits.len());
    assert_eq!(instant.fraction(), &fraction(&digits));
    assert!(instant > parse("1970-01-01T00:00:00Z"));
    let over_limit = format!("1970-01-01T00:00:00.{digits}0Z");
    assert!(ExactInstant::parse_rfc3339(&over_limit).is_err());
}
