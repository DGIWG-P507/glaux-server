//! Independent SWE nil fixtures: UML req33/34, original NilValues types,
//! accepted IDR-022 nil-first order and the documented Gregorian interpretation.
use std::sync::OnceLock;

use glaux_domain::{
    numeric::NumericValue,
    scalar::{CalendarTime, ScalarValue, TimePosition},
};
use serde_json::json;

use super::{MAX_NIL_DECLARATIONS, ScalarContract, ScalarError, UnitReferenceCheck};
use crate::validation::{Contract, StructuralValidator};

const REASON: &str = "urn:example:nil:missing";
const GREGORIAN: &str = "http://www.opengis.net/def/uom/ISO-8601/0/Gregorian";

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().unwrap())
}

fn source(kind: &str, members: &str) -> Vec<u8> {
    format!(
        r#"{{"type":"{kind}","definition":"urn:example:property","label":"Property"{members}}}"#
    )
    .into_bytes()
}

fn compile(kind: &str, members: &str) -> ScalarContract {
    ScalarContract::compile(validator(), &source(kind, members)).unwrap()
}

#[test]
fn nil_tokens_override_constraints_without_relaxing_other_values() {
    for kind in ["Text", "Category"] {
        let input = source(kind, &format!(
            r#", "constraint":{{"values":["Ready"]}}, "nilValues":[{{"value":"NA","reason":"{REASON}"}}], "value":"NA", "extension":{{"retained":true}}"#
        ));
        let contract = ScalarContract::compile(validator(), &input).unwrap();
        assert_eq!(contract.source(), input);
        assert_eq!(contract.nil_declarations().len(), 1);
        let expected = if kind == "Text" {
            ScalarValue::Text("NA".into())
        } else {
            ScalarValue::Category("NA".into())
        };
        assert_eq!(contract.nil_declarations()[0].value, expected);
        assert_eq!(contract.nil_declarations()[0].reason, REASON);
        let inline = contract.inline_value().unwrap();
        assert_eq!(inline.value, expected);
        assert_eq!(inline.nil_reason.as_deref(), Some(REASON));
        assert_eq!(contract.component().value(), Some(expected));
        assert_eq!(contract.check_value(br#""NA""#).unwrap(), inline);
        assert_eq!(contract.check_value(br#""Ready""#).unwrap().nil_reason, None);
        assert_eq!(contract.check_value(br#""Other""#), Err(ScalarError::ConstraintViolation));
    }
    let pattern = compile("Text", &format!(
        r#", "constraint":{{"pattern":"^[A-Z]+$"}}, "nilValues":[{{"value":"","reason":"{REASON}"}}]"#
    ));
    assert_eq!(pattern.check_value(br#""""#).unwrap().nil_reason.as_deref(), Some(REASON));
    assert_eq!(pattern.check_value(br#""lower""#), Err(ScalarError::ConstraintViolation));
}

#[test]
fn nil_numeric_sentinels_preserve_exact_values_and_lexemes() {
    for token in ["9007199254740993", "18446744073709551616", "-0", "-9.9900e2"] {
        for kind in ["Count", "Quantity"] {
            let uom = if kind == "Quantity" { r#", "uom":{"code":"m"}"# } else { "" };
            let input = source(kind, &format!(
                r#"{uom}, "constraint":{{"values":[10]}}, "nilValues":[{{"value":{token},"reason":"{REASON}"}}], "value":{token}"#
            ));
            let contract = ScalarContract::compile(validator(), &input).unwrap();
            assert_eq!(contract.source(), input);
            let declaration = &contract.nil_declarations()[0];
            let number = match &declaration.value {
                ScalarValue::Count(value) => value.number(),
                ScalarValue::Quantity(NumericValue::Finite(value)) => value,
                _ => panic!("exact typed numeric sentinel"),
            };
            assert_eq!(number.decimal_lexeme(), Some(token));
            let inline = contract.inline_value().unwrap();
            assert_eq!(inline.nil_reason.as_deref(), Some(REASON));
            assert_eq!(inline.value, declaration.value);
            let checked = contract.check_value(token.as_bytes()).unwrap();
            assert_eq!(checked, inline);
            assert_eq!(contract.check_value(b"10").unwrap().nil_reason, None);
            assert_eq!(contract.check_value(b"11"), Err(ScalarError::ConstraintViolation));
        }
    }
    let count = compile("Count", &format!(
        r#", "constraint":{{"values":[10]}}, "nilValues":[{{"value":9007199254740993,"reason":"{REASON}"}}]"#
    ));
    assert_eq!(count.check_value(b"9007199254740992"), Err(ScalarError::ConstraintViolation));
    assert_eq!(count.check_value(br#""9007199254740993""#), Err(ScalarError::ValueType));
}

#[test]
fn nil_specials_are_reserved_states_not_numeric_coercions() {
    for kind in ["Quantity", "Time"] {
        let code = if kind == "Time" { "s" } else { "m" };
        let contract = compile(kind, &format!(
            r#", "uom":{{"code":"{code}"}}, "constraint":{{"intervals":[[0,1]]}}, "nilValues":[{{"value":"NaN","reason":"{REASON}"}},{{"value":"Infinity","reason":"urn:example:nil:above"}},{{"value":"-Infinity","reason":"urn:example:nil:below"}}], "value":"NaN""#
        ));
        for (token, reason) in [
            ("NaN", REASON),
            ("Infinity", "urn:example:nil:above"),
            ("+Infinity", "urn:example:nil:above"),
            ("-Infinity", "urn:example:nil:below"),
        ] {
            let checked = contract.check_value(json!(token).to_string().as_bytes()).unwrap();
            assert_eq!(checked.nil_reason.as_deref(), Some(reason));
            if token == "NaN" {
                let numeric = match checked.value {
                    ScalarValue::Quantity(value) => value,
                    ScalarValue::Time(value) => match value.position {
                        TimePosition::Numeric(value) => value,
                        _ => panic!("numeric Time nil"),
                    },
                    _ => panic!("numeric sentinel"),
                };
                assert!(matches!(numeric, NumericValue::NaN));
            }
        }
        assert_eq!(contract.inline_value().unwrap().nil_reason.as_deref(), Some(REASON));
        assert_eq!(contract.check_value(b"1").unwrap().nil_reason, None);
        assert_eq!(contract.check_value(b"2"), Err(ScalarError::ConstraintViolation));
        assert_eq!(contract.check_value(br#""1""#), Err(ScalarError::ValueType));
        assert_eq!(contract.check_value(b"null"), Err(ScalarError::ValueType));
    }
    // A nonfinite state is not implicitly nil merely because it exists.
    let ordinary = compile("Quantity", r#", "uom":{"code":"m"}"#);
    assert_eq!(ordinary.check_value(br#""NaN""#).unwrap().nil_reason, None);
}

#[test]
fn nil_invalid_declarations_and_ambiguity_fail() {
    let boolean = source("Boolean", &format!(
        r#", "nilValues":[{{"value":false,"reason":"{REASON}"}}]"#
    ));
    assert_eq!(validator().validate(Contract::Boolean, &boolean), Ok(()));
    assert_eq!(ScalarContract::compile(validator(), &boolean).err(), Some(ScalarError::NilDeclaration));
    for (kind, uom, first, second) in [
        ("Text", "", r#""NA""#, r#""NA""#),
        ("Count", "", "1", "1.0"),
        ("Quantity", r#", "uom":{"code":"m"}"#, "0", "-0"),
        ("Quantity", r#", "uom":{"code":"m"}"#, r#""NaN""#, r#""NaN""#),
        ("Quantity", r#", "uom":{"code":"m"}"#, r#""Infinity""#, r#""+Infinity""#),
        ("Time", r#", "uom":{"code":"s"}"#, "0", "-0"),
    ] {
        for second_reason in [REASON, "urn:example:conflicting"] {
            let input = source(kind, &format!(
                r#"{uom}, "nilValues":[{{"value":{first},"reason":"{REASON}"}},{{"value":{second},"reason":"{second_reason}"}}]"#
            ));
            assert_eq!(ScalarContract::compile(validator(), &input).err(), Some(ScalarError::DuplicateNilValue));
        }
    }
    for (kind, members) in [
        ("Text", r#", "nilValues":[{"value":1,"reason":"urn:example:missing"}]"#),
        ("Count", r#", "nilValues":[{"value":"1","reason":"urn:example:missing"}]"#),
        ("Count", r#", "nilValues":[{"value":1.5,"reason":"urn:example:missing"}]"#),
        ("Count", r#", "nilValues":[{"value":"NaN","reason":"urn:example:missing"}]"#),
        ("Text", r#", "nilValues":[{"value":null,"reason":"urn:example:missing"}]"#),
        ("Text", r#", "nilValues":[{"value":"NA"}]"#),
        ("Text", r#", "nilValues":[]"#),
        ("Text", r#", "nilValues":[{"value":"NA","reason":"urn:example:missing","extra":true}]"#),
    ] {
        assert_eq!(ScalarContract::compile(validator(), &source(kind, members)).err(), Some(ScalarError::Structure));
    }
    let invalid_uri = source("Text", r#", "nilValues":[{"value":"NA","reason":"relative-reason"}]"#);
    assert_eq!(ScalarContract::compile(validator(), &invalid_uri).err(), Some(ScalarError::NilDeclaration));
}

#[test]
fn nil_calendar_time_and_range_context_remain_distinct() {
    let members = format!(
        r#", "uom":{{"href":"{GREGORIAN}"}}, "constraint":{{"values":["2000-01-01T00:00:00Z"]}}, "nilValues":[{{"value":"NaN","reason":"{REASON}"}}], "value":"NaN""#
    );
    let contract = compile("Time", &members);
    let inline = contract.inline_value().unwrap();
    assert_eq!(inline.nil_reason.as_deref(), Some(REASON));
    assert_eq!(inline.unit_reference, UnitReferenceCheck::CalendarEncoding);
    let ScalarValue::Time(value) = inline.value else { panic!("Time nil retains reference") };
    assert!(matches!(value.position, TimePosition::Numeric(NumericValue::NaN)));
    assert!(value.utc_instant().is_err());
    assert_eq!(contract.check_value(br#""Infinity""#), Err(ScalarError::ValueType));
    assert_eq!(contract.check_value(br#""2001-01-01T00:00:00Z""#), Err(ScalarError::ConstraintViolation));
    assert_eq!(contract.check_value(br#""2000-01-01T00:00:00Z""#).unwrap().nil_reason, None);

    let ordinary = compile("Time", &format!(r#", "uom":{{"href":"{GREGORIAN}"}}"#));
    for token in [r#""NaN""#, r#""Infinity""#, r#""-Infinity""#] {
        assert_eq!(ordinary.check_value(token.as_bytes()), Err(ScalarError::ValueType));
        assert_eq!(ordinary.check_range_endpoint(token.as_bytes()).unwrap().nil_reason, None);
    }
    let finite_nil = source("Time", &format!(
        r#", "uom":{{"href":"{GREGORIAN}"}}, "nilValues":[{{"value":-999,"reason":"{REASON}"}}]"#
    ));
    assert_eq!(validator().validate(Contract::Time, &finite_nil), Ok(()));
    assert_eq!(ScalarContract::compile(validator(), &finite_nil).err(), Some(ScalarError::UnsupportedTimeMeaning));

    let timestamp = "1970-01-01T00:00:00.1234567890123456789Z";
    let calendar = compile("Time", &format!(
        r#", "uom":{{"href":"{GREGORIAN}"}}, "nilValues":[{{"value":"{timestamp}","reason":"{REASON}"}}]"#
    ));
    let nil = calendar.check_value(json!(timestamp).to_string().as_bytes()).unwrap();
    assert_eq!(nil.nil_reason.as_deref(), Some(REASON));
    let ScalarValue::Time(value) = nil.value else { panic!("calendar sentinel") };
    let TimePosition::Calendar(CalendarTime::Utc(instant)) = &value.position else { panic!("exact UTC calendar sentinel") };
    assert_eq!(instant.source_lexeme(), timestamp);

    let range_enum = source("Time", &format!(
        r#", "uom":{{"href":"{GREGORIAN}"}}, "constraint":{{"values":["NaN"]}}"#
    ));
    assert_eq!(ScalarContract::compile(validator(), &range_enum).err(), Some(ScalarError::ValueType));
    let endpoint = ScalarContract::compile_range_endpoint_descriptor(validator(), &range_enum).unwrap();
    assert_eq!(endpoint.check_range_endpoint(br#""NaN""#).unwrap().nil_reason, None);
    assert_eq!(endpoint.check_value(br#""NaN""#), Err(ScalarError::ValueType));
    assert_eq!(endpoint.check_range_endpoint(br#""2000-01-01T00:00:00Z""#), Err(ScalarError::ConstraintViolation));
}

#[test]
fn nil_absence_null_and_limits_stay_distinct() {
    let absent = compile("Text", &format!(
        r#", "optional":true, "nilValues":[{{"value":"","reason":"{REASON}"}}]"#
    ));
    assert_eq!(absent.inline_value(), None);
    assert_eq!(absent.component().value(), None);
    assert_eq!(absent.check_value(br#""""#).unwrap().nil_reason.as_deref(), Some(REASON));
    for input in [b"null".as_slice(), b"false", b"0"] {
        assert_eq!(absent.check_value(input), Err(ScalarError::ValueType));
    }
    let ordinary_false = compile("Boolean", r#", "value":false"#);
    assert_eq!(ordinary_false.inline_value().unwrap().value, ScalarValue::Boolean(false));
    assert_eq!(ordinary_false.inline_value().unwrap().nil_reason, None);
    let ordinary_empty = compile("Text", r#", "value":"""#);
    assert_eq!(ordinary_empty.inline_value().unwrap().value, ScalarValue::Text(String::new()));
    assert_eq!(ordinary_empty.inline_value().unwrap().nil_reason, None);

    for (count, expected) in [(MAX_NIL_DECLARATIONS, None), (MAX_NIL_DECLARATIONS + 1, Some(ScalarError::NilLimit))] {
        let declarations: Vec<_> = (0..count).map(|value| json!({"value":value,"reason":REASON})).collect();
        let input = source("Count", &format!(r#", "nilValues":{}"#, json!(declarations)));
        let result = ScalarContract::compile(validator(), &input);
        assert_eq!(result.as_ref().err().copied(), expected);
        if let Ok(contract) = result {
            assert_eq!(contract.nil_declarations().len(), MAX_NIL_DECLARATIONS);
        }
    }
}
