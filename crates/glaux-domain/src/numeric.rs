//! Exact, bounded numeric primitives, not complete SWE components or codecs.
//!
//! Parse finite JSON number tokens without a floating-point intermediate.
//! Preserve supplied decimal spelling separately from normalized numeric value.
//! Binary64 input means that exact binary value, not its rounded display text.
//! Component constraints, nil reasons, units and storage remain with their owners.

use std::{cmp::Ordering, fmt, hash::{Hash, Hasher}, str::FromStr};
use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};

/// Local token budget, not a claimed standards maximum.
pub const MAX_NUMERIC_TEXT_BYTES: usize = 4096;
/// Maximum absolute explicit decimal exponent; checked before power allocation.
pub const MAX_DECIMAL_EXPONENT: i32 = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NumericError {
    Syntax,
    InputLimit,
    ExponentLimit,
    NonIntegral,
    OutOfRange,
    NonFinite,
}

impl fmt::Display for NumericError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Syntax => "invalid numeric token for this parser",
            Self::InputLimit => "numeric token exceeds the local byte budget",
            Self::ExponentLimit => "numeric exponent exceeds the local magnitude budget",
            Self::NonIntegral => "Count requires an integral value",
            Self::OutOfRange => "exact integer does not fit the requested target",
            Self::NonFinite => "a finite numeric value is required",
        })
    }
}
impl std::error::Error for NumericError {}

#[derive(Clone, Debug)]
enum Source {
    Decimal(String),
    Binary64(u64),
}

/// A finite exact value and its original input spelling/bits.
///
/// Equality, ordering and hashing use value, not source spelling. Thus 1, 1.0
/// and 1e0 compare equal; signed-zero spelling/bits remain separately available.
/// No public unchecked rational constructor, arithmetic or lossy output cast.
#[derive(Clone, Debug)]
pub struct ExactNumber {
    value: BigRational,
    source: Source,
}

impl ExactNumber {
    /// Parse exactly one RFC 8259 number token, not a whole JSON document and
    /// not an XML Schema/SWE Text lexeme. Surrounding whitespace is not part of
    /// this token contract. All bounds are checked before large-number creation.
    pub fn parse_json_number(text: &str) -> Result<Self, NumericError> {
        if text.len() > MAX_NUMERIC_TEXT_BYTES {
            return Err(NumericError::InputLimit);
        }
        let bytes = text.as_bytes();
        let mut pos = usize::from(bytes.first() == Some(&b'-'));
        let negative = pos == 1;
        let start = pos;
        match bytes.get(pos) {
            Some(b'0') => pos += 1,
            Some(b'1'..=b'9') => {
                while bytes.get(pos).is_some_and(u8::is_ascii_digit) { pos += 1; }
            }
            _ => return Err(NumericError::Syntax),
        }
        let mut fraction_digits = 0_i32;
        if bytes.get(pos) == Some(&b'.') {
            pos += 1;
            let fraction_start = pos;
            while bytes.get(pos).is_some_and(u8::is_ascii_digit) { pos += 1; }
            fraction_digits = (pos - fraction_start) as i32;
            if fraction_digits == 0 { return Err(NumericError::Syntax); }
        }
        let mantissa_end = pos;
        let mut exponent = 0_i32;
        if matches!(bytes.get(pos), Some(b'e' | b'E')) {
            pos += 1;
            let exponent_negative = bytes.get(pos) == Some(&b'-');
            if matches!(bytes.get(pos), Some(b'+' | b'-')) { pos += 1; }
            let exponent_start = pos;
            while let Some(digit @ b'0'..=b'9') = bytes.get(pos) {
                exponent = exponent * 10 + i32::from(digit - b'0');
                if exponent > MAX_DECIMAL_EXPONENT {
                    return Err(NumericError::ExponentLimit);
                }
                pos += 1;
            }
            if pos == exponent_start { return Err(NumericError::Syntax); }
            if exponent_negative { exponent = -exponent; }
        }
        if pos != bytes.len() { return Err(NumericError::Syntax); }
        let digits: Vec<u8> = bytes[start..mantissa_end].iter().copied().filter(|b| *b != b'.').collect();
        let mut coefficient = BigInt::parse_bytes(&digits, 10).ok_or(NumericError::Syntax)?;
        if negative { coefficient = -coefficient; }
        let scale = exponent - fraction_digits;
        let value = if coefficient.is_zero() {
            BigRational::from_integer(coefficient)
        } else if scale >= 0 {
            BigRational::from_integer(coefficient * BigInt::from(10).pow(scale as u32))
        } else {
            BigRational::new(coefficient, BigInt::from(10).pow(scale.unsigned_abs()))
        };
        Ok(Self { value, source: Source::Decimal(text.to_owned()) })
    }

    /// Preserve the actual finite binary64 value exactly, including its source
    /// bits. This cannot recover decimal precision already lost by a caller.
    pub fn from_binary64(value: f64) -> Result<Self, NumericError> {
        Ok(Self {
            value: BigRational::from_float(value).ok_or(NumericError::NonFinite)?,
            source: Source::Binary64(value.to_bits()),
        })
    }

    pub fn decimal_lexeme(&self) -> Option<&str> {
        match &self.source { Source::Decimal(text) => Some(text), Source::Binary64(_) => None }
    }
    pub fn binary64_bits(&self) -> Option<u64> {
        match self.source { Source::Binary64(bits) => Some(bits), Source::Decimal(_) => None }
    }
    pub fn is_integer(&self) -> bool { self.value.is_integer() }
}

impl FromStr for ExactNumber {
    type Err = NumericError;
    fn from_str(text: &str) -> Result<Self, Self::Err> { Self::parse_json_number(text) }
}
impl PartialEq for ExactNumber {
    fn eq(&self, other: &Self) -> bool { self.cmp(other) == Ordering::Equal }
}
impl Eq for ExactNumber {}
impl PartialOrd for ExactNumber {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}
impl Ord for ExactNumber {
    fn cmp(&self, other: &Self) -> Ordering { self.value.cmp(&other.value) }
}
impl Hash for ExactNumber {
    fn hash<H: Hasher>(&self, state: &mut H) { self.value.hash(state); }
}

/// Integral primitive for Count, not a validated SWE Count component.
/// Signed values are permitted; component-specific ranges/nil rules are separate.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CountValue(ExactNumber);

impl TryFrom<ExactNumber> for CountValue {
    type Error = NumericError;
    fn try_from(value: ExactNumber) -> Result<Self, Self::Error> {
        if !value.is_integer() { return Err(NumericError::NonIntegral); }
        Ok(Self(value))
    }
}
impl FromStr for CountValue {
    type Err = NumericError;
    fn from_str(text: &str) -> Result<Self, Self::Err> { text.parse::<ExactNumber>()?.try_into() }
}
impl CountValue {
    pub fn number(&self) -> &ExactNumber { &self.0 }
    /// Explicit target conversions reject overflow; these are not Count limits.
    pub fn try_to_i64(&self) -> Result<i64, NumericError> {
        self.0.value.numer().to_i64().ok_or(NumericError::OutOfRange)
    }
    pub fn try_to_u64(&self) -> Result<u64, NumericError> {
        self.0.value.numer().to_u64().ok_or(NumericError::OutOfRange)
    }
}

/// Non-finite states are distinct from finite values, missing values and nil
/// sentinels. NaN is unordered, including with itself. This primitive ordering
/// is not CQL2's finite-only/NULL filtering view.
#[derive(Clone, Debug)]
pub enum NumericValue {
    Finite(ExactNumber),
    NaN,
    PositiveInfinity,
    NegativeInfinity,
}
impl NumericValue {
    /// The already-decoded NumberOrSpecial string value, not a raw JSON token.
    /// Ordinary numeric strings and XML Schema's INF spellings are not aliases.
    pub fn from_swe_special(text: &str) -> Result<Self, NumericError> {
        match text {
            "NaN" => Ok(Self::NaN),
            "Infinity" | "+Infinity" => Ok(Self::PositiveInfinity),
            "-Infinity" => Ok(Self::NegativeInfinity),
            _ => Err(NumericError::Syntax),
        }
    }
    /// Special IEEE states retain semantic category, not NaN payload/sign bits.
    pub fn from_binary64(value: f64) -> Self {
        if value.is_nan() { Self::NaN }
        else if value == f64::INFINITY { Self::PositiveInfinity }
        else if value == f64::NEG_INFINITY { Self::NegativeInfinity }
        else {
            Self::Finite(ExactNumber::from_binary64(value).expect("finite binary64 has an exact ratio"))
        }
    }
}
impl PartialEq for NumericValue {
    fn eq(&self, other: &Self) -> bool { self.partial_cmp(other) == Some(Ordering::Equal) }
}
impl PartialOrd for NumericValue {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        use NumericValue::{Finite, NaN, NegativeInfinity, PositiveInfinity};
        match (self, other) {
            (NaN, _) | (_, NaN) => None,
            (NegativeInfinity, NegativeInfinity) | (PositiveInfinity, PositiveInfinity) => Some(Ordering::Equal),
            (NegativeInfinity, _) | (_, PositiveInfinity) => Some(Ordering::Less),
            (PositiveInfinity, _) | (_, NegativeInfinity) => Some(Ordering::Greater),
            (Finite(left), Finite(right)) => Some(left.cmp(right)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn number(text: &str) -> ExactNumber { text.parse().unwrap() }

    #[test]
    fn distinct_large_integers_never_collapse() {
        let left = number("9007199254740992");
        let right = number("9007199254740993");
        assert_eq!(left.cmp(&right), Ordering::Less);
        assert_ne!(left, right);
        assert_eq!(right.value.numer().to_string(), "9007199254740993");
        assert_eq!(right.value.denom().to_string(), "1");
        assert_eq!(number("18446744073709551616").cmp(&number("18446744073709551617")), Ordering::Less);
        assert_eq!(number("-18446744073709551617").cmp(&number("-18446744073709551616")), Ordering::Less);
    }

    #[test]
    fn decimal_fractions_exponents_and_value_hashing_are_exact() {
        let tenth = number("0.1");
        assert_eq!(tenth.value.numer().to_string(), "1");
        assert_eq!(tenth.value.denom().to_string(), "10");
        assert_eq!(tenth, number("1e-1"));
        assert_eq!(number("1.2300E+2"), number("123"));
        assert_eq!(number("0.1000000000000000000000000001").cmp(&tenth), Ordering::Greater);
        assert_eq!(number("-0.1000000000000000000000000001").cmp(&number("-0.1")), Ordering::Less);
        assert_eq!([number("1"), number("1.0"), number("10e-1")].into_iter().collect::<HashSet<_>>().len(), 1);
        assert_eq!(number("1e400").cmp(&number("9e399")), Ordering::Greater);
        assert_eq!(number("1e-400").cmp(&number("0")), Ordering::Greater);
    }

    #[test]
    fn source_spelling_and_signed_zero_are_not_numeric_identity() {
        for text in ["0", "-0", "-0.000E+004", "0e-4096"] {
            let value = number(text);
            assert_eq!(value, number("0"));
            assert_eq!(value.decimal_lexeme(), Some(text));
            assert_eq!(value.binary64_bits(), None);
        }
        assert_eq!(number("1.2300E+2").decimal_lexeme(), Some("1.2300E+2"));
        let negative_zero = ExactNumber::from_binary64(-0.0).unwrap();
        assert_eq!(negative_zero, number("0"));
        assert_eq!(negative_zero.binary64_bits(), Some(0x8000_0000_0000_0000));
        assert_eq!(negative_zero.decimal_lexeme(), None);
    }

    #[test]
    fn count_integrality_and_requested_integer_ranges_are_checked() {
        for text in ["-1", "1", "1.0", "10e-1", "-0"] {
            assert!(text.parse::<CountValue>().is_ok());
        }
        assert_eq!("3.5".parse::<CountValue>(), Err(NumericError::NonIntegral));
        assert_eq!("-1e-400".parse::<CountValue>(), Err(NumericError::NonIntegral));
        assert_eq!("-9223372036854775808".parse::<CountValue>().unwrap().try_to_i64(), Ok(i64::MIN));
        assert_eq!("9223372036854775807".parse::<CountValue>().unwrap().try_to_i64(), Ok(i64::MAX));
        assert_eq!("9223372036854775808".parse::<CountValue>().unwrap().try_to_i64(), Err(NumericError::OutOfRange));
        assert_eq!("-9223372036854775809".parse::<CountValue>().unwrap().try_to_i64(), Err(NumericError::OutOfRange));
        assert_eq!("18446744073709551615".parse::<CountValue>().unwrap().try_to_u64(), Ok(u64::MAX));
        assert_eq!("18446744073709551616".parse::<CountValue>().unwrap().try_to_u64(), Err(NumericError::OutOfRange));
        assert_eq!("-1".parse::<CountValue>().unwrap().try_to_u64(), Err(NumericError::OutOfRange));
        assert!("1e400".parse::<CountValue>().is_ok());
        assert_eq!(number("3").cmp(&number("3.5")), Ordering::Less);
    }

    #[test]
    fn json_token_grammar_rejects_repairs_and_special_numbers() {
        for text in ["", "-", "+1", "01", "-01", ".5", "1.", "1e", "1e+", "1e-", " 1", "1\n",
            "1 2", "1/2", "0x10", "1_000", "NaN", "Infinity", "+Infinity", "-Infinity",
            "\"NaN\"", "null", "true", "１", "1\0", "--1", "1e2e3"] {
            assert_eq!(ExactNumber::parse_json_number(text), Err(NumericError::Syntax), "{text:?}");
        }
    }

    #[test]
    fn numeric_budgets_fail_explicitly_before_expansion() {
        let at_limit = "9".repeat(MAX_NUMERIC_TEXT_BYTES);
        assert!(number(&at_limit).is_integer());
        assert_eq!(ExactNumber::parse_json_number(&format!("{at_limit}9")), Err(NumericError::InputLimit));
        assert!(number("1e4096") > number("1e4095"));
        assert!(number("1e-4096") > number("0"));
        assert_eq!(ExactNumber::parse_json_number("1e4097"), Err(NumericError::ExponentLimit));
        assert_eq!(ExactNumber::parse_json_number("1e-4097"), Err(NumericError::ExponentLimit));
        assert_eq!(ExactNumber::parse_json_number("0e999999999999999999999"), Err(NumericError::ExponentLimit));
        assert_eq!(number("1e00000000000000000000000000000001"), number("10"));
        let tiny = format!("0.{}1e-4096", "0".repeat(MAX_NUMERIC_TEXT_BYTES-9));
        assert_eq!(tiny.len(), MAX_NUMERIC_TEXT_BYTES);
        assert!(number(&tiny) > number("0"));
    }

    #[test]
    fn binary64_preserves_exact_bits_not_shortest_decimal_display() {
        let binary = ExactNumber::from_binary64(0.1).unwrap();
        assert_eq!(binary.value.numer().to_string(), "3602879701896397");
        assert_eq!(binary.value.denom().to_string(), "36028797018963968");
        assert_eq!(binary.cmp(&number("0.1")), Ordering::Greater);
        assert_eq!(ExactNumber::from_binary64(0.5).unwrap(), number("0.5"));
        let subnormal = ExactNumber::from_binary64(f64::from_bits(1)).unwrap();
        assert_eq!(subnormal.value.numer().to_string(), "1");
        assert_eq!(subnormal.value.denom(), &(BigInt::from(1) << 1074));
        assert!(subnormal > number("0"));
        assert!(ExactNumber::from_binary64(f64::MAX).unwrap() < number("1e309"));
        assert_eq!(binary.binary64_bits(), Some(0x3fb9_9999_9999_999a));
    }

    #[test]
    fn nonfinite_states_are_explicit_and_nan_is_unordered() {
        use NumericValue::{Finite, NaN, NegativeInfinity, PositiveInfinity};
        assert!(matches!(NumericValue::from_swe_special("NaN"), Ok(NaN)));
        for text in ["Infinity", "+Infinity"] {
            assert_eq!(NumericValue::from_swe_special(text).unwrap(), PositiveInfinity);
        }
        assert_eq!(NumericValue::from_swe_special("-Infinity").unwrap(), NegativeInfinity);
        for text in ["INF", "-INF", "nan", "1", "null", " NaN", "\"NaN\""] {
            assert_eq!(NumericValue::from_swe_special(text), Err(NumericError::Syntax));
        }
        let finite = Finite(number("1e400"));
        assert!(NegativeInfinity < finite && finite < PositiveInfinity);
        for value in [NegativeInfinity, finite, PositiveInfinity, NaN] {
            assert_eq!(NaN.partial_cmp(&value), None);
            assert_eq!(value.partial_cmp(&NaN), None);
        }
        assert_ne!(NaN, NaN);
        assert!(matches!(NumericValue::from_binary64(f64::NAN), NaN));
        assert_eq!(NumericValue::from_binary64(f64::INFINITY), PositiveInfinity);
        assert_eq!(NumericValue::from_binary64(f64::NEG_INFINITY), NegativeInfinity);
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(ExactNumber::from_binary64(value), Err(NumericError::NonFinite));
        }
    }
}
