//! Bounded exact-number checks for task 1.2.4; no codec or storage claim.
//!
//! The oracle is signed i128 integer arithmetic over coefficients / 10^scale.
//! Coefficients are bounded below 2^81 and scales at six, so cross products
//! remain below 2^102. No production parser or floating point computes the
//! expected ordering. The fixed generator is deliberately small and pinned.

use std::cmp::Ordering;
use std::collections::BTreeSet;

use glaux_domain::numeric::{
    CountValue, ExactNumber, MAX_DECIMAL_EXPONENT, MAX_NUMERIC_TEXT_BYTES,
};

const SEED: u64 = 0x0123_4567_89ab_cdef;
const CASES: usize = 1024;

struct Generator(u64);

impl Generator {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    fn decimal(&mut self) -> Decimal {
        let offset = i128::from((self.next() >> 32) % 2_000_001) - 1_000_000;
        let coefficient = match (self.next() >> 32) % 4 {
            0 => offset,
            1 => (1_i128 << 80) + offset,
            2 => -((1_i128 << 80) + offset),
            _ => (1_i128 << 53) + offset,
        };
        let scale = u32::try_from((self.next() >> 32) % 7).expect("scale <= 6");
        Decimal { coefficient, scale }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Decimal {
    coefficient: i128,
    scale: u32,
}

impl Decimal {
    fn denominator(self) -> i128 {
        10_i128.pow(self.scale)
    }

    fn fixed(self) -> String {
        if self.scale == 0 {
            return self.coefficient.to_string();
        }
        let places = usize::try_from(self.scale).expect("small scale");
        let digits = format!(
            "{:0width$}",
            self.coefficient.unsigned_abs(),
            width = places + 1
        );
        let split = digits.len() - places;
        let sign = if self.coefficient < 0 { "-" } else { "" };
        format!("{sign}{}.{}", &digits[..split], &digits[split..])
    }

    fn scientific(self) -> String {
        format!("{}e-{}", self.coefficient, self.scale)
    }

    fn compare(self, other: Self) -> Ordering {
        (self.coefficient * other.denominator()).cmp(&(other.coefficient * self.denominator()))
    }
}

fn parse(lexeme: &str) -> ExactNumber {
    ExactNumber::parse_json_number(lexeme)
        .unwrap_or_else(|error| panic!("valid fixture {lexeme:?}: {error:?}"))
}

#[test]
fn numeric_property_generator_is_pinned() {
    let mut generator = Generator(SEED);
    assert_eq!(
        [
            generator.next(),
            generator.next(),
            generator.next(),
            generator.next(),
        ],
        [
            0x2ce3_2d23_35df_4552,
            0xca18_dd5a_e3c4_5eb9,
            0x860f_5366_7996_eed4,
            0xe212_99d8_28ce_a893,
        ],
        "changing the generator requires a reviewed seed/corpus update"
    );
    let mut first = Generator(SEED);
    let mut replay = Generator(SEED);
    let mut different = Generator(SEED ^ 1);
    let original: Vec<_> = (0..CASES).map(|_| first.decimal()).collect();
    let repeated: Vec<_> = (0..CASES).map(|_| replay.decimal()).collect();
    let changed: Vec<_> = (0..CASES).map(|_| different.decimal()).collect();
    assert_eq!(original, repeated);
    assert_ne!(original, changed);
    let distinct: BTreeSet<_> = original
        .iter()
        .map(|case| (case.coefficient, case.scale))
        .collect();
    assert!(distinct.len() > CASES / 2, "degenerate generated corpus");
    assert!(original.iter().any(|case| case.coefficient < 0));
    assert!(original.iter().any(|case| case.coefficient > 0));
    assert!(original.iter().any(|case| case.scale == 0));
    assert!(original.iter().any(|case| case.scale == 6));
}

#[test]
fn numeric_property_equivalent_lexemes_and_integer_oracle() {
    let mut generator = Generator(SEED);
    let mut integers = 0;
    let mut fractions = 0;
    for index in 0..CASES {
        let case = if index.is_multiple_of(32) {
            Decimal {
                coefficient: 0,
                scale: 6,
            }
        } else {
            generator.decimal()
        };
        let fixed = case.fixed();
        let scientific = case.scientific();
        let padded = format!("{}E-{}", case.coefficient * 10, case.scale + 1);
        let number = parse(&fixed);
        for lexeme in [&fixed, &scientific, &padded] {
            let alternative = parse(lexeme);
            assert_eq!(alternative, number, "seed={SEED:#x} case={index}");
            assert_eq!(alternative.decimal_lexeme(), Some(lexeme.as_str()));
            assert_eq!(lexeme.parse::<ExactNumber>().unwrap(), alternative);
        }
        let integral = case.coefficient % case.denominator() == 0;
        assert_eq!(number.is_integer(), integral, "case={case:?}");
        let count = CountValue::try_from(parse(&fixed));
        if integral {
            integers += 1;
            let expected = case.coefficient / case.denominator();
            let count = count.expect("integer-valued lexeme is a Count");
            assert_eq!(count.number(), &number);
            assert_eq!(count.try_to_i64().ok(), i64::try_from(expected).ok());
            assert_eq!(count.try_to_u64().ok(), u64::try_from(expected).ok());
        } else {
            fractions += 1;
            assert!(count.is_err(), "fraction must not round to Count: {fixed}");
        }
    }
    assert!(integers > 0 && fractions > 0);
    for lexeme in ["0", "-0", "-0.000", "0e+4096", "-0E-4096"] {
        let zero = parse(lexeme);
        assert_eq!(zero, parse("0"));
        assert_eq!(zero.decimal_lexeme(), Some(lexeme));
        assert_eq!(CountValue::try_from(zero).unwrap().try_to_i64(), Ok(0));
    }
}

#[test]
fn numeric_property_order_matches_integer_rationals() {
    let mut generator = Generator(SEED ^ 0x5a5a_0102);
    let mut less = 0;
    let mut equal = 0;
    let mut greater = 0;
    for index in 0..CASES {
        let a = generator.decimal();
        let b = if index.is_multiple_of(8) {
            a
        } else {
            generator.decimal()
        };
        let c = generator.decimal();
        let parsed_a = parse(&a.fixed());
        let parsed_b = parse(&b.scientific());
        let parsed_c = parse(&c.fixed());
        let expected = a.compare(b);
        assert_eq!(parsed_a.cmp(&parsed_b), expected, "a={a:?} b={b:?}");
        assert_eq!(parsed_b.cmp(&parsed_a), expected.reverse());
        assert_eq!(parsed_a == parsed_b, expected == Ordering::Equal);
        assert_eq!(parsed_b.cmp(&parsed_c), b.compare(c));
        assert_eq!(parsed_a.cmp(&parsed_c), a.compare(c));
        if a.compare(b) != Ordering::Greater && b.compare(c) != Ordering::Greater {
            assert!(parsed_a <= parsed_c, "transitivity: {a:?}, {b:?}, {c:?}");
        }
        match expected {
            Ordering::Less => less += 1,
            Ordering::Equal => equal += 1,
            Ordering::Greater => greater += 1,
        }
        // Adjacent integers above binary64's exact range must not collapse.
        let large = (1_i128 << 80) + i128::try_from(index).unwrap();
        assert!(parse(&large.to_string()) < parse(&(large + 1).to_string()));
    }
    assert!(less > 0 && equal > 0 && greater > 0);
}

#[test]
fn numeric_property_invalid_partitions_and_limits() {
    for invalid in [
        "",
        "+1",
        "01",
        "-01",
        ".1",
        "1.",
        "1e",
        "1e+",
        "1e-",
        "--1",
        " 1",
        "1 ",
        "1\n",
        "1\0",
        "1_000",
        "0x10",
        "NaN",
        "Infinity",
        "+Infinity",
        "-Infinity",
        "\"1\"",
        "\"NaN\"",
        "1,2",
        "[1]",
        "true",
        "１",
        "−1",
        "1e２",
    ] {
        assert!(
            ExactNumber::parse_json_number(invalid).is_err(),
            "{invalid:?}"
        );
        assert!(invalid.parse::<ExactNumber>().is_err(), "{invalid:?}");
    }
    assert_eq!(MAX_NUMERIC_TEXT_BYTES, 4096);
    assert_eq!(MAX_DECIMAL_EXPONENT, 4096);
    let at_limit = "9".repeat(MAX_NUMERIC_TEXT_BYTES);
    assert_eq!(parse(&at_limit).decimal_lexeme(), Some(at_limit.as_str()));
    let fraction = format!("0.{}1", "0".repeat(MAX_NUMERIC_TEXT_BYTES - 3));
    assert_eq!(fraction.len(), MAX_NUMERIC_TEXT_BYTES);
    let tiny = parse(&fraction);
    assert!(parse("0") < tiny && tiny < parse("1"));
    assert!(ExactNumber::parse_json_number(&"9".repeat(MAX_NUMERIC_TEXT_BYTES + 1)).is_err());
    assert!(parse("1e4096") > parse("1"));
    assert!(parse("1e-4096") > parse("0"));
    assert!(parse("1e-4096") < parse("1"));
    for invalid in [
        "1e4097",
        "1e-4097",
        "0e4097",
        "0e-4097",
        "1e999999999999999999999",
    ] {
        assert!(
            ExactNumber::parse_json_number(invalid).is_err(),
            "{invalid}"
        );
    }
}
