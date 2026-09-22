//! Bounded deterministic task 1.2.5 parser campaign, not elapsed-time arithmetic.
//!
//! Integer year/month walking supplies expected normalized civil seconds.
//! Explicit Gregorian century, offset, leap-second and grammar partitions are
//! supplemented by generated dates/fractions. This is not exhaustive fuzzing.

use std::collections::BTreeSet;

use glaux_domain::numeric::ExactNumber;
use glaux_domain::temporal::{ExactInstant, MAX_TIMESTAMP_BYTES};

const SEED: u64 = 0x0123_4567_89ab_cdef;
const CASES: usize = 2048;
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
struct Expected {
    civil_second: i64,
    leap: bool,
    fraction: String,
    offset_seconds: i32,
    known_numeric_offset: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Case {
    bytes: Vec<u8>,
    expected: Option<Expected>,
    malformed_utf8: bool,
    boundary: bool,
}

fn rejected(text: &str) -> Case {
    Case {
        bytes: text.as_bytes().to_vec(),
        expected: None,
        malformed_utf8: false,
        boundary: false,
    }
}

fn valid(
    date: (i32, u32, u32),
    time: (u32, u32, u32),
    offset_minutes: i32,
    digits: &str,
    zone: &str,
    leap: bool,
) -> Case {
    let (year, month, day) = date;
    let (hour, minute, second) = time;
    let decimal = if digits.is_empty() {
        String::new()
    } else {
        format!(".{digits}")
    };
    let text =
        format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{decimal}{zone}");
    Case {
        bytes: text.into_bytes(),
        expected: Some(Expected {
            civil_second: days_from_epoch(year, month, day) * 86_400
                + i64::from(hour * 3600 + minute * 60 + second.min(59))
                - i64::from(offset_minutes) * 60,
            leap,
            fraction: digits.into(),
            offset_seconds: offset_minutes * 60,
            // RFC 9557's clarification: Z is a known UTC instant but does not
            // assert a known numeric local offset; +00:00 does assert one.
            known_numeric_offset: !matches!(zone, "Z" | "z" | "-00:00"),
        }),
        malformed_utf8: false,
        boundary: false,
    }
}

fn zone(offset: i32) -> String {
    let absolute = offset.unsigned_abs();
    let sign = if offset < 0 { '-' } else { '+' };
    format!("{sign}{:02}:{:02}", absolute / 60, absolute % 60)
}

fn generate(seed: u64) -> Vec<Case> {
    let mut generator = Generator(seed);
    let invalid = [
        "",
        "1970-01-01",
        "1970-01-01T00:00:00",
        "1970-01-01 00:00:00Z",
        "1970-01-01T24:00:00Z",
        "1970-01-01T00:00:61Z",
        "1970-00-01T00:00:00Z",
        "1970-13-01T00:00:00Z",
        "1970-01-00T00:00:00Z",
        "2000-04-31T00:00:00Z",
        "1970-01-01T00:00:00.Z",
        "1970-01-01T00:00:00,1Z",
        "1970-01-01T00:00:00+0000",
        "1970-01-01T00:00:00+24:00",
        "1970-01-01T00:00:00+00:60",
        "1970-01-01T00:60:00Z",
        " 1970-01-01T00:00:00Z",
        "1970-01-01T00:00:00Z\n",
        "1970-01-01T00:00:00Z\0",
        "1970-01-01T00:00:00Z/..",
        "１０００-01-01T00:00:00Z",
        "10000-01-01T00:00:00Z",
    ];
    let longest_digits = format!("{}1", "0".repeat(MAX_TIMESTAMP_BYTES - 22));
    let mut boundaries = [
        valid((1970, 1, 1), (0, 0, 0), 0, &longest_digits, "Z", false),
        rejected(&format!("1970-01-01T00:00:00.{longest_digits}0Z")),
        valid((0, 1, 1), (0, 0, 0), 1439, "", "+23:59", false),
        valid((9999, 12, 31), (23, 59, 59), -1439, "", "-23:59", false),
        valid(
            (1969, 12, 31),
            (23, 59, 59),
            0,
            "999999999999999999999999999999",
            "Z",
            false,
        ),
        valid(
            (1970, 1, 1),
            (0, 0, 0),
            0,
            "000000000000000000000000000001",
            "Z",
            false,
        ),
    ];
    for case in &mut boundaries {
        case.boundary = true;
    }
    (0..CASES)
        .map(|index| {
            let year = i32::try_from(generator.bounded(10_000)).unwrap();
            let month = 1 + generator.bounded(12);
            let day = 1 + generator.bounded(month_days(year, month));
            let hour = generator.bounded(24);
            let minute = generator.bounded(60);
            let second = generator.bounded(60);
            let offset = i32::try_from(generator.bounded(2879)).unwrap() - 1439;
            let digits = format!("{:018}1", generator.next() % 1_000_000_000_000_000_000);
            match index % 8 {
                0 => valid(
                    (year, month, day),
                    (hour, minute, second),
                    offset,
                    "",
                    &zone(offset),
                    false,
                ),
                1 => valid((1969, 12, 31), (23, 59, 59), 0, &digits, "Z", false),
                2 => rejected(invalid[(index / 8) % invalid.len()]),
                3 => Case {
                    bytes: match (index / 8) % 4 {
                        0 => vec![0xff],
                        1 => vec![0xc0, 0xaf],
                        2 => vec![0xe2, 0x82],
                        _ => vec![0xed, 0xa0, 0x80],
                    },
                    expected: None,
                    malformed_utf8: true,
                    boundary: false,
                },
                4 => match (index / 8) % 6 {
                    0 => valid((2016, 12, 31), (23, 59, 60), 0, "", "Z", true),
                    1 => valid((2017, 1, 1), (0, 59, 60), 60, "125", "+01:00", true),
                    2 => valid((1972, 6, 30), (23, 59, 60), 0, "5", "-00:00", true),
                    3 => rejected("2017-12-31T23:59:60Z"),
                    4 => rejected("2016-12-31T22:59:60Z"),
                    _ => rejected("2016-12-31T23:59:60+01:00"),
                },
                5 => {
                    let century = i32::try_from((index / 8) % 100).unwrap();
                    let year = century * 100;
                    if century % 4 == 0 {
                        valid((year, 2, 29), (0, 0, 0), 0, "", "Z", false)
                    } else {
                        rejected(&format!("{year:04}-02-29T00:00:00Z"))
                    }
                }
                6 => boundaries[(index / 8) % boundaries.len()].clone(),
                _ => {
                    let offset = if (index / 8).is_multiple_of(4) {
                        0
                    } else {
                        offset % 720
                    };
                    let local_minutes = 720 + offset;
                    let zone_text = if offset == 0 {
                        ["Z", "z", "-00:00", "+00:00"][(index / 32) % 4].to_owned()
                    } else {
                        zone(offset)
                    };
                    valid(
                        (year, month, day),
                        (
                            u32::try_from(local_minutes / 60).unwrap(),
                            u32::try_from(local_minutes % 60).unwrap(),
                            second,
                        ),
                        offset,
                        &digits,
                        &zone_text,
                        false,
                    )
                }
            }
        })
        .collect()
}

fn parse(text: &str) -> ExactInstant {
    ExactInstant::parse_rfc3339(text)
        .unwrap_or_else(|error| panic!("valid seed {text:?}: {error:?}"))
}

fn main() {
    assert_eq!(MAX_TIMESTAMP_BYTES, 4096);
    let mut pinned = Generator(SEED);
    assert_eq!(pinned.next(), 0x2ce3_2d23_35df_4552);
    assert_eq!(pinned.next(), 0xca18_dd5a_e3c4_5eb9);
    assert_eq!(days_from_epoch(0, 1, 1), -719_528);
    assert_eq!(days_from_epoch(2000, 1, 1), 10_957);
    let cases = generate(SEED);
    assert_eq!(cases.len(), CASES);
    assert_eq!(cases, generate(SEED));
    assert_ne!(cases, generate(SEED ^ 1));
    let distinct_inputs: BTreeSet<_> = cases.iter().map(|case| &case.bytes).collect();
    assert!(distinct_inputs.len() > CASES / 4);
    let mut distinct_values = BTreeSet::new();
    let mut valid = 0;
    let mut rejected = 0;
    let mut malformed = 0;
    let mut boundary = 0;
    let mut pre_epoch = 0;
    let mut leap = 0;
    let mut known_offsets = 0;
    let mut unknown_offsets = 0;
    let mut positive_offsets = 0;
    let mut negative_offsets = 0;
    let origin = parse("1970-01-01T00:00:00Z");
    for (index, case) in cases.iter().enumerate() {
        boundary += usize::from(case.boundary);
        if case.malformed_utf8 {
            assert!(std::str::from_utf8(&case.bytes).is_err());
            // Invalid UTF-8 cannot enter the &str API. A lossy diagnostic must
            // not become a valid timestamp; no lossy ingestion is authorized.
            let diagnostic = String::from_utf8_lossy(&case.bytes);
            assert!(ExactInstant::parse_rfc3339(&diagnostic).is_err());
            malformed += 1;
            continue;
        }
        let text = std::str::from_utf8(&case.bytes).unwrap();
        let actual = ExactInstant::parse_rfc3339(text);
        let repeated = ExactInstant::parse_rfc3339(text);
        assert_eq!(actual.is_ok(), repeated.is_ok(), "case={index}");
        let Some(expected) = &case.expected else {
            assert!(
                actual.is_err(),
                "seed={SEED:#x} case={index} input={text:?}"
            );
            assert!(text.parse::<ExactInstant>().is_err());
            rejected += 1;
            continue;
        };
        let instant = actual.unwrap_or_else(|error| panic!("case={index}: {error:?}"));
        assert_eq!(instant, repeated.unwrap());
        assert_eq!(instant, text.parse::<ExactInstant>().unwrap());
        assert_eq!(
            instant.civil_second(),
            expected.civil_second,
            "case={index}"
        );
        assert_eq!(instant.is_leap_second(), expected.leap);
        assert_eq!(instant.source_lexeme(), text);
        assert_eq!(instant.fraction_digits(), expected.fraction.len());
        assert_eq!(instant.offset_seconds(), expected.offset_seconds);
        assert_eq!(instant.offset_known(), expected.known_numeric_offset);
        let fraction_text = if expected.fraction.is_empty() {
            "0".to_owned()
        } else {
            format!("0.{}", expected.fraction)
        };
        let expected_fraction = ExactNumber::parse_json_number(&fraction_text).unwrap();
        assert_eq!(instant.fraction(), &expected_fraction);
        assert_eq!(instant.fraction_decimal(), fraction_text);
        assert_eq!(instant, parse(&text.to_ascii_lowercase()));
        if expected.civil_second < 0 {
            pre_epoch += 1;
            assert!(instant < origin);
        } else if expected.civil_second > 0 {
            assert!(instant > origin);
        } else if expected.fraction.bytes().any(|digit| digit != b'0') {
            assert!(instant > origin, "nonzero precision must not underflow");
        } else {
            assert_eq!(instant, origin);
        }
        known_offsets += usize::from(expected.known_numeric_offset);
        unknown_offsets += usize::from(!expected.known_numeric_offset);
        positive_offsets += usize::from(expected.offset_seconds > 0);
        negative_offsets += usize::from(expected.offset_seconds < 0);
        leap += usize::from(expected.leap);
        let canonical_fraction = expected.fraction.trim_end_matches('0').to_owned();
        distinct_values.insert((expected.civil_second, expected.leap, canonical_fraction));
        valid += 1;
    }
    assert_eq!(valid + rejected + malformed, CASES);
    assert!(valid > CASES / 4 && rejected > 0);
    assert_eq!(malformed, CASES / 8);
    assert_eq!(boundary, CASES / 8);
    assert!(pre_epoch > 0 && leap > 0);
    assert!(known_offsets > 0 && unknown_offsets > 0);
    assert!(positive_offsets > 0 && negative_offsets > 0);
    assert!(distinct_values.len() > CASES / 4);
    println!(
        "Time parser corpus: generator={GENERATOR_VERSION}; seed={SEED:#018x}; \
         valid={valid}; rejected={rejected}; malformed_utf8={malformed}; boundary={boundary}; \
         pre_epoch={pre_epoch}; leap={leap}; known_offsets={known_offsets}; unknown_offsets={unknown_offsets}; \
         distinct_inputs={}; distinct_values={}",
        distinct_inputs.len(),
        distinct_values.len()
    );
    println!("Required time-parser fuzz invariants passed: 2048 cases.");
}
