//! Independent SWE Time fixtures: pinned req26-28/56/60, Time/AllowedTimes
//! schemas, Guide 4.3/4.7 and the existing exact-time precision contract.
use super::{CodeSpaceCheck, ScalarContract, ScalarError, UnitReferenceCheck};
use crate::validation::{Contract, Failure, StructuralValidator};
use glaux_domain::{
    numeric::NumericValue,
    scalar::{
        BoundTimeValue, CalendarTime, ScalarComponent, ScalarValue, TimeComponent, TimeFrame,
        TimePosition, UnitReference, UnsupportedTimeConversion,
    },
    temporal::{MAX_TIMESTAMP_BYTES, TimeError},
};
use serde_json::json;
use std::sync::OnceLock;

const GREGORIAN: &str = "http://www.opengis.net/def/uom/ISO-8601/0/Gregorian";
const UTC: &str = "http://www.opengis.net/def/trs/BIPM/0/UTC";
const GPS: &str = "http://www.opengis.net/def/trs/USNO/0/GPS";

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn compile(source: &[u8]) -> ScalarContract {
    ScalarContract::compile(validator(), source).expect("valid independent Time fixture")
}

fn source(uom: &str, members: &str) -> Vec<u8> {
    format!(
        r#"{{"type":"Time","definition":"urn:example:time","label":"Time","uom":{uom}{members}}}"#
    )
    .into_bytes()
}

fn calendar(members: &str) -> Vec<u8> {
    source(&format!(r#"{{"href":"{GREGORIAN}"}}"#), members)
}

fn numeric(code: &str, members: &str) -> Vec<u8> {
    source(&format!(r#"{{"code":"{code}"}}"#), members)
}

fn component(contract: &ScalarContract) -> &TimeComponent {
    let ScalarComponent::Time(component) = contract.component() else {
        panic!("Time component must retain its own typed context")
    };
    component
}

fn checked(contract: &ScalarContract, token: &str) -> BoundTimeValue {
    let ScalarValue::Time(value) = contract.check_value(token.as_bytes()).unwrap().value else {
        panic!("checked Time must not become a bare number or instant")
    };
    *value
}

fn date(contract: &ScalarContract, text: &str) -> BoundTimeValue {
    checked(contract, &json!(text).to_string())
}

#[test]
fn time_calendar_defaults_preserve_exact_instants() {
    let text = "1970-01-01T01:00:00.1234567890123456789+01:00";
    let input = calendar(&format!(
        r#", "id":"TIME", "description":" supplied description ", "optional":false,
        "updatable":true, "axisID":"T", "value":"{text}", "vendor:note":{{"keep":true}}"#
    ));
    let contract = compile(&input);
    let value = date(&contract, text);
    let instant = value.utc_instant().unwrap();
    // First assertion is the targeted precision-loss fault oracle.
    assert_eq!(instant.fraction_decimal(), "0.1234567890123456789");
    assert_eq!(instant.civil_second(), 0);
    assert_eq!(instant.fraction_digits(), 19);
    assert_eq!(instant.source_lexeme(), text);
    assert_eq!(instant.offset_seconds(), 3600);
    assert_eq!(contract.source(), input);
    assert_eq!(component(&contract).reference.frame, TimeFrame::DefaultUtc);
    assert_eq!(component(&contract).metadata.reference_frame, None);
    assert_eq!(component(&contract).metadata.id.as_deref(), Some("TIME"));
    assert_eq!(component(&contract).metadata.definition, "urn:example:time");
    assert_eq!(component(&contract).metadata.label, "Time");
    assert_eq!(component(&contract).metadata.description.as_deref(), Some(" supplied description "));
    assert_eq!(component(&contract).metadata.optional, Some(false));
    assert_eq!(component(&contract).metadata.updatable, Some(true));
    assert_eq!(component(&contract).metadata.axis_id.as_deref(), Some("T"));
    assert_eq!(component(&contract).reference.origin, None);
    assert_eq!(component(&contract).reference.uom, UnitReference {
        label: None, symbol: None, code: None, href: Some(GREGORIAN.into()),
    });
    assert_eq!(contract.component().value(), Some(ScalarValue::Time(Box::new(value.clone()))));
    let equivalent = date(&contract, "1970-01-01T00:00:00.123456789012345678900Z");
    assert_eq!(instant, equivalent.utc_instant().unwrap());
    assert_ne!(instant.source_lexeme(), equivalent.utc_instant().unwrap().source_lexeme());

    let explicit = compile(&calendar(&format!(r#", "referenceFrame":"{UTC}""#)));
    let declared = date(&explicit, text);
    assert_eq!(declared.utc_instant().unwrap(), instant);
    assert_eq!(declared.reference.frame, TimeFrame::Declared(UTC.into()));
    assert_eq!(component(&explicit).metadata.reference_frame.as_deref(), Some(UTC));
    let result = explicit.check_value(json!(text).to_string().as_bytes()).unwrap();
    assert_eq!(result.unit_reference, UnitReferenceCheck::CalendarEncoding);
    assert_eq!(result.code_space, CodeSpaceCheck::NotApplicable);
}

#[test]
fn time_numeric_coordinates_preserve_origin_and_context() {
    let input = source(
        r#"{"code":"ms","label":" milliseconds ","symbol":"ms"}"#,
        r#", "referenceTime":"2000-01-01T00:00:00.000000001Z",
        "localFrame":"urn:example:scan-start", "value":9007199254740993"#,
    );
    let contract = compile(&input);
    let value = checked(&contract, "9007199254740993");
    // First conversion assertion targets inventing a Unix instant for a coordinate.
    assert_eq!(value.utc_instant().err(), Some(UnsupportedTimeConversion));
    let Some(CalendarTime::Utc(origin)) = &value.reference.origin else {
        panic!("explicit UTC origin must be retained")
    };
    assert_eq!(origin.source_lexeme(), "2000-01-01T00:00:00.000000001Z");
    assert_eq!(origin.civil_second(), 946_684_800);
    assert_eq!(origin.fraction_decimal(), "0.000000001");
    assert_eq!(value.reference.frame, TimeFrame::DefaultUtc);
    assert_eq!(value.reference.local_frame.as_deref(), Some("urn:example:scan-start"));
    assert_eq!(value.reference.uom, UnitReference {
        label: Some(" milliseconds ".into()), symbol: Some("ms".into()),
        code: Some("ms".into()), href: None,
    });
    let TimePosition::Numeric(NumericValue::Finite(number)) = &value.position else {
        panic!("numeric Time must remain an exact coordinate")
    };
    assert_eq!(number, &"9007199254740993".parse().unwrap());
    assert_ne!(number, &"9007199254740992".parse().unwrap());
    assert_eq!(number.decimal_lexeme(), Some("9007199254740993"));
    assert_eq!(contract.component().value(), Some(ScalarValue::Time(Box::new(value))));
    assert_eq!(contract.source(), input);

    let unanchored = compile(&numeric("s", ""));
    assert_eq!(component(&unanchored).value, None);
    for token in ["0", "-0.00", "1e400", "1.0000000000000000001e-9"] {
        let value = checked(&unanchored, token);
        assert_eq!(value.utc_instant().err(), Some(UnsupportedTimeConversion));
        assert_eq!(value.reference.origin, None, "UTC default does not invent an epoch");
        let TimePosition::Numeric(NumericValue::Finite(number)) = value.position else {
            panic!("finite coordinate")
        };
        assert_eq!(number.decimal_lexeme(), Some(token));
        assert_eq!(number, token.parse().unwrap());
    }
}

#[test]
fn time_foreign_frames_do_not_invent_utc() {
    for frame in [GPS, "urn:example:mission-clock"] {
        let text = "2009-11-05T16:29:26.1234567890123456789Z";
        let input = calendar(&format!(
            r#", "referenceFrame":"{frame}", "referenceTime":"2000-01-01T00:00:00Z", "value":"{text}""#
        ));
        let contract = compile(&input);
        let value = date(&contract, text);
        assert_eq!(value.position, TimePosition::Calendar(CalendarTime::Unresolved(text.into())));
        assert_eq!(value.reference.frame, TimeFrame::Declared(frame.into()));
        assert_eq!(value.reference.origin, Some(CalendarTime::Unresolved("2000-01-01T00:00:00Z".into())));
        assert_eq!(value.utc_instant().err(), Some(UnsupportedTimeConversion));
        assert_eq!(contract.source(), input);
        assert_eq!(contract.check_value(br#""2016-12-31T23:59:60Z""#), Err(ScalarError::UnsupportedTimeMeaning));
    }
    let coordinate = compile(&numeric("s", &format!(
        r#", "referenceFrame":"{GPS}", "referenceTime":"2000-01-01T00:00:00Z""#
    )));
    let value = checked(&coordinate, "1.25");
    assert_eq!(value.reference.origin, Some(CalendarTime::Unresolved("2000-01-01T00:00:00Z".into())));
    assert_eq!(value.utc_instant().err(), Some(UnsupportedTimeConversion));

    let unresolved_range = calendar(&format!(
        r#", "referenceFrame":"{GPS}", "constraint":{{"intervals":[["2000-01-01T00:00:00Z","2000-01-02T00:00:00Z"]]}}"#
    ));
    assert_eq!(ScalarContract::compile(validator(), &unresolved_range).err(), Some(ScalarError::UnsupportedTimeMeaning));
    let named = compile(&calendar(&format!(
        r#", "referenceFrame":"{GPS}", "constraint":{{"values":["2000-01-01T00:00:00Z"]}}"#
    )));
    assert!(matches!(date(&named, "2000-01-01T00:00:00Z").position, TimePosition::Calendar(CalendarTime::Unresolved(_))));
    assert_eq!(named.check_value(br#""2000-01-01T01:00:00+01:00""#), Err(ScalarError::UnsupportedTimeMeaning), "do not infer equivalence in an unresolved frame");
}

#[test]
fn time_original_schema_and_value_representation() {
    let calendar_contract = compile(&calendar(""));
    let numeric_contract = compile(&numeric("s", ""));
    assert_eq!(validator().validate(Contract::Time, &calendar(r#", "value":"2024-02-29T12:00:00Z""#)), Ok(()));
    for token in [r#""NaN""#, r#""Infinity""#, r#""+Infinity""#, r#""-Infinity""#] {
        let input = numeric("s", &format!(r#", "value":{token}"#));
        assert_eq!(validator().validate(Contract::Time, &input), Ok(()), "original oneOf must distinguish named specials from date-time");
        let inline = compile(&input);
        let value = checked(&numeric_contract, token);
        match token {
            r#""NaN""# => assert!(matches!(value.position, TimePosition::Numeric(NumericValue::NaN))),
            r#""-Infinity""# => assert_eq!(value.position, TimePosition::Numeric(NumericValue::NegativeInfinity)),
            _ => assert_eq!(value.position, TimePosition::Numeric(NumericValue::PositiveInfinity)),
        }
        assert!(inline.component().value().is_some());
        let forbidden = calendar(&format!(r#", "value":{token}"#));
        assert_eq!(validator().validate(Contract::Time, &forbidden), Ok(()), "schema is broader than req56 calendar semantics");
        assert_eq!(ScalarContract::compile(validator(), &forbidden).err(), Some(ScalarError::ValueType));
        assert_eq!(calendar_contract.check_value(token.as_bytes()), Err(ScalarError::ValueType));
    }
    for token in ["null", "false", "{}", "[]", r#""INF""#, r#""-INF""#, r#""nan""#, r#""123""#, r#""2024-02-29""#, r#""2024-02-30T12:00:00Z""#] {
        assert_eq!(validator().validate(Contract::Time, &calendar(&format!(r#", "value":{token}"#))), Err(Failure::Structure));
    }
    let wrong_numeric_representation = numeric("s", r#", "value":"2024-02-29T12:00:00Z""#);
    assert_eq!(validator().validate(Contract::Time, &wrong_numeric_representation), Ok(()));
    assert_eq!(ScalarContract::compile(validator(), &wrong_numeric_representation).err(), Some(ScalarError::ValueType));
    assert_eq!(calendar_contract.check_value(b"0"), Err(ScalarError::ValueType));
    for missing in ["type", "definition", "label", "uom"] {
        let mut input = json!({"type":"Time","definition":"urn:example:time","label":"Time","uom":{"code":"s"}});
        input.as_object_mut().unwrap().remove(missing);
        assert_eq!(validator().validate(Contract::Time, &serde_json::to_vec(&input).unwrap()), Err(Failure::Structure));
    }
}

#[test]
fn time_calendar_constraints_are_inclusive_unions() {
    let members = r#", "constraint":{"values":["1970-01-02T00:00:00Z"],"intervals":[["1970-01-01T00:00:00.1234567890123456789Z","1970-01-01T00:00:00.1234567890123456791Z"]]}"#;
    let contract = compile(&calendar(members));
    for text in ["1970-01-01T00:00:00.1234567890123456789Z", "1970-01-01T01:00:00.1234567890123456790+01:00", "1970-01-01T00:00:00.1234567890123456791Z", "1970-01-02T00:00:00Z"] {
        assert_eq!(date(&contract, text).utc_instant().unwrap().source_lexeme(), text);
    }
    for text in ["1970-01-01T00:00:00.1234567890123456788Z", "1970-01-01T00:00:00.1234567890123456792Z", "1970-01-03T00:00:00Z"] {
        assert_eq!(contract.check_value(json!(text).to_string().as_bytes()), Err(ScalarError::ConstraintViolation));
    }
    let invalid_inline = calendar(&format!(r#"{members}, "value":"1970-01-03T00:00:00Z""#));
    assert_eq!(ScalarContract::compile(validator(), &invalid_inline).err(), Some(ScalarError::ConstraintViolation));
    let singleton = compile(&calendar(r#", "constraint":{"intervals":[["1970-01-01T01:00:00+01:00","1970-01-01T00:00:00Z"]]}"#));
    assert_eq!(date(&singleton, "1970-01-01T00:00:00Z").utc_instant().unwrap().civil_second(), 0);
    let open = compile(&calendar(r#", "constraint":{"intervals":[["-Infinity","2000-01-01T00:00:00Z"],["2001-01-01T00:00:00Z","+Infinity"]]}"#));
    assert_eq!(date(&open, "1970-01-01T00:00:00Z").utc_instant().unwrap().civil_second(), 0);
    assert!(date(&open, "2024-01-01T00:00:00Z").utc_instant().is_ok());
    assert_eq!(open.check_value(br#""2000-06-01T00:00:00Z""#), Err(ScalarError::ConstraintViolation));
    assert_eq!(open.check_value(br#""+Infinity""#), Err(ScalarError::ValueType));

    let empty_input = calendar(r#", "constraint":{"intervals":[]}"#);
    assert_eq!(validator().validate(Contract::Time, &empty_input), Ok(()));
    let empty = compile(&empty_input);
    assert_eq!(empty.check_value(br#""1970-01-01T00:00:00Z""#), Err(ScalarError::ConstraintViolation));
    let enumerated = compile(&calendar(r#", "constraint":{"values":["1970-01-01T00:00:00Z"],"intervals":[]}"#));
    assert_eq!(date(&enumerated, "1970-01-01T00:00:00Z").utc_instant().unwrap().civil_second(), 0);
    let reversed = calendar(r#", "constraint":{"values":["1970-01-01T00:00:00Z"],"intervals":[["2001-01-01T00:00:00Z","2000-01-01T00:00:00Z"]]}, "value":"1970-01-01T00:00:00Z""#);
    assert_eq!(ScalarContract::compile(validator(), &reversed).err(), Some(ScalarError::Constraint), "validate every interval even if enumeration matches");
    for constraint in [r#"{"intervals":[["NaN","2000-01-01T00:00:00Z"]]}"#, r#"{"intervals":[[0,"2000-01-01T00:00:00Z"]]}"#] {
        assert_eq!(ScalarContract::compile(validator(), &calendar(&format!(r#", "constraint":{constraint}"#))).err(), Some(ScalarError::ValueType));
    }
}

#[test]
fn time_numeric_constraints_and_special_states() {
    let contract = compile(&numeric("s", r#", "constraint":{"values":[-1,"NaN"],"intervals":[[0,2]],"significantFigures":2}"#));
    for token in ["-1", "0", "1.2", "2", "0.00"] {
        let value = checked(&contract, token);
        assert_eq!(value.position, TimePosition::Numeric(NumericValue::Finite(token.parse().unwrap())));
        assert_eq!(value.utc_instant().err(), Some(UnsupportedTimeConversion));
    }
    assert!(matches!(checked(&contract, r#""NaN""#).position, TimePosition::Numeric(NumericValue::NaN)));
    for token in ["-2", "3", "1.20", "0.000", r#""Infinity""#] {
        assert_eq!(contract.check_value(token.as_bytes()), Err(ScalarError::ConstraintViolation));
    }
    let interval = compile(&numeric("s", r#", "constraint":{"intervals":[["-Infinity","Infinity"]]}"#));
    assert_eq!(checked(&interval, r#""+Infinity""#).position, TimePosition::Numeric(NumericValue::PositiveInfinity));
    assert_eq!(interval.check_value(br#""NaN""#), Err(ScalarError::ConstraintViolation));
    for invalid in [r#"{"intervals":[[2,1]]}"#, r#"{"intervals":[["NaN",2]]}"#] {
        assert_eq!(ScalarContract::compile(validator(), &numeric("s", &format!(r#", "constraint":{invalid}"#))).err(), Some(ScalarError::Constraint));
    }
    for invalid in [r#"{"values":[]}"#, r#"{"intervals":[[1]]}"#, r#"{"intervals":[[1,2,3]]}"#, r#"{"significantFigures":2}"#, r#"{"values":[1],"significantFigures":41}"#] {
        assert_eq!(ScalarContract::compile(validator(), &numeric("s", &format!(r#", "constraint":{invalid}"#))).err(), Some(ScalarError::Structure));
    }
    let calendar_precision = calendar(r#", "constraint":{"values":["2000-01-01T00:00:00Z"],"significantFigures":2}"#);
    assert_eq!(validator().validate(Contract::Time, &calendar_precision), Ok(()));
    assert_eq!(ScalarContract::compile(validator(), &calendar_precision).err(), Some(ScalarError::UnsupportedTimeMeaning));
}

#[test]
fn time_units_and_frame_metadata_are_validated() {
    for code in ["s", "ms", "min", "h", "d"] {
        let contract = compile(&numeric(code, ""));
        let result = contract.check_value(b"125").unwrap();
        assert_eq!(result.unit_reference, UnitReferenceCheck::CodeValidated);
        let ScalarValue::Time(value) = result.value else { panic!("Time") };
        assert_eq!(value.reference.uom.code.as_deref(), Some(code));
        assert_eq!(value.position, TimePosition::Numeric(NumericValue::Finite("125".parse().unwrap())));
    }
    for code in ["m", "m/s", " s", "s "] {
        assert!(ScalarContract::compile(validator(), &numeric(code, "")).is_err(), "not an accepted temporal declaration: {code}");
    }
    let href = "urn:example:unresolved-time-unit";
    let unresolved = compile(&source(&format!(r#"{{"href":"{href}","label":" custom unit "}}"#), ""));
    let result = unresolved.check_value(b"125").unwrap();
    assert_eq!(result.unit_reference, UnitReferenceCheck::Unresolved(href.into()));
    assert_eq!(component(&unresolved).reference.uom.label.as_deref(), Some(" custom unit "));
    for uom in [r#"{}"#.to_owned(), r#"{"label":"seconds"}"#.to_owned(), r##"{"href":"#unit"}"##.to_owned(), format!(r#"{{"href":"{GREGORIAN}","code":"s"}}"#)] {
        assert!(ScalarContract::compile(validator(), &source(&uom, "")).is_err());
    }
    for members in [format!(r#", "localFrame":"{UTC}""#), format!(r#", "referenceFrame":"{UTC}", "localFrame":"{UTC}""#), r#", "referenceFrame":"urn:example:clock", "localFrame":"urn:example:clock""#.into()] {
        assert_eq!(ScalarContract::compile(validator(), &numeric("s", &members)).err(), Some(ScalarError::Metadata));
    }
    for members in [r##", "localFrame":"#clock""##, r#", "referenceTime":"2024-02-30T00:00:00Z""#, r#", "referenceTime":"2024-02-29""#] {
        assert_eq!(ScalarContract::compile(validator(), &numeric("s", members)).err(), Some(ScalarError::Structure));
    }
}

#[test]
fn time_invalid_calendar_and_bounded_inputs() {
    let contract = compile(&calendar(""));
    for text in ["2023-02-29T00:00:00Z", "2024-02-30T00:00:00Z", "2024-13-01T00:00:00Z", "2024-01-01T24:00:00Z", "2024-01-01T00:00:61Z", "2024-01-01T00:00:00+24:00"] {
        assert_eq!(contract.check_value(json!(text).to_string().as_bytes()), Err(ScalarError::Time(TimeError::Calendar)));
    }
    for text in ["2024-02-29", "2024-02-29T12:00:00", "2024-02-29T12:00:00.Z", " 2024-02-29T12:00:00Z", "2024-02-29T12:00:00Z[UTC]"] {
        assert_eq!(contract.check_value(json!(text).to_string().as_bytes()), Err(ScalarError::Time(TimeError::Syntax)));
    }
    let before = date(&contract, "2016-12-31T23:59:59.9999999999999999999Z");
    let leap = date(&contract, "2016-12-31T23:59:60.1234567890123456789Z");
    let after = date(&contract, "2017-01-01T00:00:00Z");
    assert!(leap.utc_instant().unwrap().is_leap_second());
    assert_eq!(leap.utc_instant().unwrap().civil_second(), 1_483_228_799);
    assert!(before.utc_instant().unwrap() < leap.utc_instant().unwrap());
    assert!(leap.utc_instant().unwrap() < after.utc_instant().unwrap());
    assert_eq!(contract.check_value(br#""2017-12-31T23:59:60Z""#), Err(ScalarError::Time(TimeError::UnrecognizedLeapSecond)));
    let at_limit = format!("1970-01-01T00:00:00.{}Z", "1".repeat(MAX_TIMESTAMP_BYTES - 21));
    assert_eq!(date(&contract, &at_limit).utc_instant().unwrap().source_lexeme(), at_limit);
    let over_limit = format!("1970-01-01T00:00:00.{}Z", "1".repeat(MAX_TIMESTAMP_BYTES - 20));
    assert_eq!(contract.check_value(json!(over_limit).to_string().as_bytes()), Err(ScalarError::Time(TimeError::InputLimit)));
}

#[test]
fn time_generated_calendar_boundaries() {
    // Fixed 64 cases at a known day rollover, with 19/20 decimal digits. The
    // independent oracle uses integer indices, not production time arithmetic.
    for case in 0_u32..64 {
        let low_fraction = format!("{case:019}");
        let high_fraction = format!("{:019}", case + 1);
        let low = format!("1970-01-01T23:59:59.{low_fraction}Z");
        let high = format!("1970-01-01T23:59:59.{high_fraction}Z");
        let input = calendar(&format!(r#", "constraint":{{"intervals":[["{low}","{high}"]]}}"#));
        let contract = compile(&input);
        let same = format!("1970-01-02T00:59:59.{low_fraction}+01:00");
        let negative_offset = format!("1970-01-01T22:59:59.{low_fraction}-01:00");
        let middle = format!("1970-01-01T23:59:59.{low_fraction}5Z");
        let lower = date(&contract, &low);
        assert_eq!(lower.utc_instant().unwrap().civil_second(), 86_399);
        assert_eq!(lower.utc_instant().unwrap().fraction_decimal(), format!("0.{low_fraction}"));
        for text in [&same, &negative_offset] {
            let value = date(&contract, text);
            assert_eq!(value.utc_instant().unwrap(), lower.utc_instant().unwrap());
            assert_eq!(value.utc_instant().unwrap().source_lexeme(), text);
        }
        let midpoint = date(&contract, &middle);
        let upper = date(&contract, &high);
        assert!(lower.utc_instant().unwrap() < midpoint.utc_instant().unwrap());
        assert!(midpoint.utc_instant().unwrap() < upper.utc_instant().unwrap());
        assert_eq!(midpoint.utc_instant().unwrap().fraction_digits(), 20);
        assert_eq!(midpoint.utc_instant().unwrap().source_lexeme(), middle);
        for text in ["1970-01-01T23:59:58.9999999999999999999Z".to_owned(), format!("1970-01-01T23:59:59.{:019}Z", case + 2), "1970-01-02T00:00:00Z".to_owned()] {
            assert_eq!(contract.check_value(json!(text).to_string().as_bytes()), Err(ScalarError::ConstraintViolation), "case {case}, outside exact interval");
        }
        assert_eq!(contract.source(), input);
    }
}
