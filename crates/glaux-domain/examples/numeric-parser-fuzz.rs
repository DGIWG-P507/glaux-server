//! Dependency-free, deterministic bounded parser campaign for task 1.2.4.
//!
//! This is a reproducible seed corpus, not exhaustive fuzzing or a codec test.
//! The coefficient/scale oracle uses only bounded integer arithmetic; no
//! floating point or production numeric output supplies its expected values.

use std::collections::BTreeSet;

use glaux_domain::numeric::{
    CountValue, ExactNumber, MAX_DECIMAL_EXPONENT, MAX_NUMERIC_TEXT_BYTES,
};

const SEED: u64 = 0x0123_4567_89ab_cdef;
const CASES: usize = 2048;

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Expected {
    Rational { coefficient: i128, scale: u32 },
    PositiveLarge,
    PositiveTiny,
    Rejected,
    MalformedUtf8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Case {
    bytes: Vec<u8>,
    expected: Expected,
    boundary: bool,
}

fn text_case(text: String, expected: Expected, boundary: bool) -> Case {
    Case {
        bytes: text.into_bytes(),
        expected,
        boundary,
    }
}

fn rational(coefficient: i128, scale: u32) -> Expected {
    Expected::Rational { coefficient, scale }
}

fn generate(seed: u64) -> Vec<Case> {
    let mut generator = Generator(seed);
    let invalid = [
        "",
        "+1",
        "01",
        "-01",
        ".5",
        "5.",
        "1e",
        "1e+",
        "1e-",
        "--1",
        " 1",
        "1 ",
        "1\n",
        "1\0",
        "1_0",
        "NaN",
        "Infinity",
        "+Infinity",
        "-Infinity",
        "\"1\"",
        "１",
        "−1",
        "1e２",
        "[1]",
    ];
    let boundary_cases = [
        text_case("1e4096".into(), Expected::PositiveLarge, true),
        text_case("1e-4096".into(), Expected::PositiveTiny, true),
        text_case("0e4096".into(), rational(0, 0), true),
        text_case("1e4097".into(), Expected::Rejected, true),
        text_case("1e-4097".into(), Expected::Rejected, true),
        text_case("0e4097".into(), Expected::Rejected, true),
        text_case(
            "9".repeat(MAX_NUMERIC_TEXT_BYTES),
            Expected::PositiveLarge,
            true,
        ),
        text_case(
            "9".repeat(MAX_NUMERIC_TEXT_BYTES + 1),
            Expected::Rejected,
            true,
        ),
        text_case(
            format!("0.{}1", "0".repeat(MAX_NUMERIC_TEXT_BYTES - 3)),
            Expected::PositiveTiny,
            true,
        ),
        text_case("1e999999999999999999999".into(), Expected::Rejected, true),
    ];
    (0..CASES)
        .map(|index| {
            let offset = i128::from((generator.next() >> 32) % 2_000_001) - 1_000_000;
            let coefficient = match (generator.next() >> 32) % 3 {
                0 => offset,
                1 => (1_i128 << 80) + offset,
                _ => -((1_i128 << 80) + offset),
            };
            let scale = u32::try_from((generator.next() >> 32) % 7).unwrap();
            match index % 8 {
                0 => text_case(
                    format!("{coefficient}e-{scale}"),
                    rational(coefficient, scale),
                    false,
                ),
                1 => text_case(
                    format!("{}E-{}", coefficient * 10, scale + 1),
                    rational(coefficient, scale),
                    false,
                ),
                2 => text_case(
                    invalid[(index / 8) % invalid.len()].into(),
                    Expected::Rejected,
                    false,
                ),
                3 => Case {
                    bytes: match (index / 8) % 4 {
                        0 => vec![0xff, b'1'],
                        1 => vec![0xc0, 0xaf],
                        2 => vec![0xe2, 0x82],
                        _ => vec![0xed, 0xa0, 0x80],
                    },
                    expected: Expected::MalformedUtf8,
                    boundary: false,
                },
                4 => text_case(
                    format!("{coefficient}e-{scale}!"),
                    Expected::Rejected,
                    false,
                ),
                5 => text_case(
                    match (index / 8) % 4 {
                        0 => "-0".into(),
                        1 => "0.000".into(),
                        2 => "-0e+4096".into(),
                        _ => "0E-4096".into(),
                    },
                    rational(0, 0),
                    false,
                ),
                6 => boundary_cases[(index / 8) % boundary_cases.len()].clone(),
                _ => text_case(
                    format!("{coefficient}e+{scale}"),
                    rational(coefficient * 10_i128.pow(scale), 0),
                    false,
                ),
            }
        })
        .collect()
}

fn parse(lexeme: &str) -> ExactNumber {
    ExactNumber::parse_json_number(lexeme)
        .unwrap_or_else(|error| panic!("valid seed input {lexeme:?}: {error:?}"))
}

fn assert_rational(number: &ExactNumber, coefficient: i128, scale: u32) {
    let denominator = 10_i128.pow(scale);
    let integral = coefficient % denominator == 0;
    assert_eq!(number.is_integer(), integral);
    let floor = coefficient.div_euclid(denominator);
    let lower = parse(&floor.to_string());
    if integral {
        assert_eq!(number, &lower);
        let count = CountValue::try_from(parse(number.decimal_lexeme().unwrap())).unwrap();
        assert_eq!(count.try_to_i64().ok(), i64::try_from(floor).ok());
        assert_eq!(count.try_to_u64().ok(), u64::try_from(floor).ok());
    } else {
        let upper = parse(&(floor + 1).to_string());
        assert!(number > &lower && number < &upper);
        assert!(CountValue::try_from(parse(number.decimal_lexeme().unwrap())).is_err());
    }
    // Independently known adjacent rational values must stay distinct, even
    // when their numerators exceed binary64's exact integer range.
    let next = parse(&format!("{}e-{scale}", coefficient + 1));
    assert!(number < &next);
}

fn main() {
    assert_eq!(MAX_NUMERIC_TEXT_BYTES, 4096);
    assert_eq!(MAX_DECIMAL_EXPONENT, 4096);
    let mut pinned = Generator(SEED);
    assert_eq!(pinned.next(), 0x2ce3_2d23_35df_4552);
    assert_eq!(pinned.next(), 0xca18_dd5a_e3c4_5eb9);
    let cases = generate(SEED);
    assert_eq!(cases.len(), CASES);
    assert_eq!(cases, generate(SEED), "fixed seed must reproduce inputs");
    assert_ne!(cases, generate(SEED ^ 1), "generator must use its seed");
    let distinct_inputs: BTreeSet<_> = cases.iter().map(|case| &case.bytes).collect();
    assert!(distinct_inputs.len() > CASES / 4);
    let mut distinct_rationals = BTreeSet::new();
    let mut valid = 0;
    let mut rejected = 0;
    let mut malformed_utf8 = 0;
    let mut boundary = 0;
    let mut positive = 0;
    let mut negative = 0;
    let mut zero = 0;
    for (index, case) in cases.iter().enumerate() {
        boundary += usize::from(case.boundary);
        if case.expected == Expected::MalformedUtf8 {
            assert!(std::str::from_utf8(&case.bytes).is_err());
            // The public parser accepts &str: malformed UTF-8 cannot enter it.
            // A lossy replacement diagnostic must not turn these bytes into
            // an accepted number; this is not a production decoding policy.
            let diagnostic = String::from_utf8_lossy(&case.bytes);
            assert!(ExactNumber::parse_json_number(&diagnostic).is_err());
            malformed_utf8 += 1;
            continue;
        }
        let text = std::str::from_utf8(&case.bytes).expect("UTF-8 seed partition");
        let actual = ExactNumber::parse_json_number(text);
        let repeat = ExactNumber::parse_json_number(text);
        assert_eq!(actual.is_ok(), repeat.is_ok(), "case={index}");
        if case.expected == Expected::Rejected {
            assert!(actual.is_err(), "seed={SEED:#x} case={index} text={text:?}");
            assert!(text.parse::<ExactNumber>().is_err());
            rejected += 1;
            continue;
        }
        let number = actual.unwrap_or_else(|error| panic!("case={index}: {error:?}"));
        assert_eq!(number, repeat.unwrap(), "parse determinism case={index}");
        assert_eq!(number, text.parse::<ExactNumber>().unwrap());
        assert_eq!(number.decimal_lexeme(), Some(text));
        valid += 1;
        match case.expected {
            Expected::Rational {
                mut coefficient,
                mut scale,
            } => {
                assert_rational(&number, coefficient, scale);
                positive += usize::from(coefficient > 0);
                negative += usize::from(coefficient < 0);
                zero += usize::from(coefficient == 0);
                while scale > 0 && coefficient % 10 == 0 {
                    coefficient /= 10;
                    scale -= 1;
                }
                distinct_rationals.insert((coefficient, scale));
            }
            Expected::PositiveLarge => {
                assert!(number > parse("1"));
                assert!(number.is_integer());
                let count = CountValue::try_from(number).unwrap();
                assert!(count.try_to_i64().is_err());
                assert!(count.try_to_u64().is_err());
            }
            Expected::PositiveTiny => {
                assert!(number > parse("0") && number < parse("1"));
                assert!(!number.is_integer());
                assert!(CountValue::try_from(number).is_err());
            }
            Expected::Rejected | Expected::MalformedUtf8 => unreachable!("handled above"),
        }
    }
    assert_eq!(valid + rejected + malformed_utf8, CASES);
    assert!(valid > 0 && rejected > 0);
    assert_eq!(malformed_utf8, CASES / 8);
    assert_eq!(boundary, CASES / 8);
    assert!(positive > 0 && negative > 0 && zero > 0);
    assert!(distinct_rationals.len() > CASES / 4);
    println!(
        "Numeric parser corpus: seed={SEED:#018x}; valid={valid}; rejected={rejected}; \
         malformed_utf8={malformed_utf8}; boundary={boundary}; positive={positive}; \
         negative={negative}; zero={zero}; distinct_inputs={}; distinct_values={}",
        distinct_inputs.len(),
        distinct_rationals.len()
    );
    println!("Required numeric-parser fuzz invariants passed: 2048 cases.");
}
