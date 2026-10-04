//! Independent numeric fixtures from pinned Count/Quantity and AllowedValues,
//! Guide 4.3 and accepted IDR-022/024. No codec rounding or unit conversion.
use super::{CodeSpaceCheck, ScalarContract, ScalarError, UnitReferenceCheck};
use crate::validation::{Contract, Failure, StructuralValidator};
use glaux_domain::{
    numeric::{CountValue, ExactNumber, NumericError, NumericValue},
    scalar::{ComponentMetadata, ScalarComponent, ScalarValue, UnitReference},
};
use serde_json::json;
use std::sync::OnceLock;

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn compile(source: &[u8]) -> ScalarContract {
    ScalarContract::compile(validator(), source).expect("valid independent numeric fixture")
}

fn source(kind: &str, members: &str) -> Vec<u8> {
    format!(
        r#"{{"type":"{kind}","definition":"urn:example:property","label":"Property"{members}}}"#
    )
    .into_bytes()
}

fn quantity(members: &str) -> Vec<u8> {
    source("Quantity", &format!(r#", "uom":{{"code":"m"}}{members}"#))
}

fn finite(token: &str) -> NumericValue {
    NumericValue::Finite(token.parse().expect("independent exact numeric fixture"))
}

#[test]
fn numeric_count_exact_large_values() {
    let contract = compile(&source("Count", ""));
    let checked = contract.check_value(b"9007199254740993").unwrap();
    let ScalarValue::Count(value) = checked.value else {
        panic!("Count variant")
    };
    assert_eq!(value.try_to_u64(), Ok(9_007_199_254_740_993));
    for token in [
        "9007199254740992",
        "9007199254740993",
        "18446744073709551616",
        "1e400",
    ] {
        let input = source("Count", &format!(r#", "value":{token}"#));
        assert_eq!(validator().validate(Contract::Count, &input), Ok(()));
        let inline = compile(&input);
        let checked = contract.check_value(token.as_bytes()).unwrap();
        let ScalarValue::Count(value) = &checked.value else {
            panic!("Count must not become a Quantity or floating value");
        };
        assert_eq!(value.number().decimal_lexeme(), Some(token));
        assert_eq!(inline.component().value(), Some(checked.value.clone()));
        assert_eq!(inline.source(), input);
    }
    let checked = contract.check_value(b"9007199254740993").unwrap();
    let ScalarValue::Count(value) = checked.value else {
        panic!("Count variant")
    };
    assert_eq!(value.try_to_u64(), Ok(9_007_199_254_740_993));
    assert_ne!(value, "9007199254740992".parse::<CountValue>().unwrap());
    let checked = contract.check_value(b"1e400").unwrap();
    let ScalarValue::Count(value) = checked.value else {
        panic!("Count variant")
    };
    let expanded = format!("1{}", "0".repeat(400));
    assert_eq!(value, expanded.parse::<CountValue>().unwrap());
    assert_eq!(value.try_to_u64(), Err(NumericError::OutOfRange));

    // The original schema's integer check must not round this to 1.0.
    for token in [
        "1.0000000000000000000000000000000000000001",
        "9007199254740993.5",
        "1e-400",
    ] {
        let input = source("Count", &format!(r#", "value":{token}"#));
        assert_eq!(
            validator().validate(Contract::Count, &input),
            Err(Failure::Structure)
        );
        assert_eq!(
            ScalarContract::compile(validator(), &input).err(),
            Some(ScalarError::Structure)
        );
        assert_eq!(
            contract.check_value(token.as_bytes()),
            Err(ScalarError::Numeric(NumericError::NonIntegral))
        );
    }
    let exact_enum = compile(&source(
        "Count",
        r#", "constraint":{"values":[9007199254740993]}"#,
    ));
    assert_eq!(
        exact_enum.check_value(b"9007199254740992"),
        Err(ScalarError::ConstraintViolation)
    );
    assert_eq!(
        exact_enum.check_value(b"9007199254740993").unwrap().value,
        ScalarValue::Count("9007199254740993".parse().unwrap())
    );
    assert_eq!(
        ScalarContract::compile(validator(), &source("Count", r#", "value":1e4097"#)).err(),
        Some(ScalarError::Syntax(Failure::Numeric(
            NumericError::ExponentLimit
        )))
    );
    let oversized = source("Count", &format!(r#", "value":{}"#, "1".repeat(4097)));
    assert_eq!(
        ScalarContract::compile(validator(), &oversized).err(),
        Some(ScalarError::Syntax(Failure::Numeric(
            NumericError::InputLimit
        )))
    );
}

#[test]
fn numeric_zero_absence_and_wrong_types() {
    let count = compile(&source("Count", ""));
    let measured = compile(&quantity(""));
    assert!(matches!(
        count.component(),
        ScalarComponent::Count { value: None, .. }
    ));
    assert!(matches!(
        measured.component(),
        ScalarComponent::Quantity { value: None, .. }
    ));
    for token in ["0", "-0", "0.0", "0e-2"] {
        let input = source("Count", &format!(r#", "value":{token}"#));
        let inline = compile(&input);
        let Some(ScalarValue::Count(value)) = inline.component().value() else {
            panic!("present zero")
        };
        assert_eq!(value.try_to_i64(), Ok(0));
        assert_eq!(value.number().decimal_lexeme(), Some(token));
        let checked = count
            .check_value(format!(" \n{token}\t").as_bytes())
            .unwrap();
        assert_eq!(checked.value, ScalarValue::Count(value));
        assert_eq!(checked.unit_reference, UnitReferenceCheck::NotApplicable);
        assert_eq!(checked.code_space, CodeSpaceCheck::NotApplicable);
        assert_eq!(
            measured.check_value(token.as_bytes()).unwrap().value,
            ScalarValue::Quantity(finite("0"))
        );
    }
    for token in ["null", "false", r#""0""#, "{}", "[]"] {
        assert_eq!(
            count.check_value(token.as_bytes()),
            Err(ScalarError::ValueType)
        );
        assert_eq!(
            measured.check_value(token.as_bytes()),
            Err(ScalarError::ValueType)
        );
        for input in [
            source("Count", &format!(r#", "value":{token}"#)),
            quantity(&format!(r#", "value":{token}"#)),
        ] {
            assert_eq!(
                ScalarContract::compile(validator(), &input).err(),
                Some(ScalarError::Structure)
            );
        }
    }
    for token in [
        r#""NaN""#,
        r#""Infinity""#,
        r#""+Infinity""#,
        r#""-Infinity""#,
    ] {
        assert_eq!(
            count.check_value(token.as_bytes()),
            Err(ScalarError::ValueType)
        );
        assert_eq!(
            ScalarContract::compile(
                validator(),
                &source("Count", &format!(r#", "value":{token}"#))
            )
            .err(),
            Some(ScalarError::Structure)
        );
    }
}

#[test]
fn numeric_source_metadata_and_units_are_preserved() {
    let input = br##"{
      "type":"Quantity", "id":"LENGTH", "definition":"urn:example:length",
      "label":" Length ", "description":" supplied description ",
      "optional":false, "updatable":true, "referenceFrame":"#frame", "axisID":"x",
      "uom":{"label":" centimetres ","symbol":"cm","code":"cm"},
      "value":125.00, "vendor:note":{"keep":"original"}
    }"##;
    let contract = compile(input);
    let ScalarComponent::Quantity { uom, .. } = contract.component() else {
        panic!("Quantity")
    };
    assert_eq!(
        uom.code.as_deref(),
        Some("cm"),
        "never rewrite the submitted unit"
    );
    assert_eq!(contract.source(), input, "retain exact description bytes");
    assert_eq!(
        contract.component(),
        &ScalarComponent::Quantity {
            metadata: ComponentMetadata {
                id: Some("LENGTH".into()),
                definition: "urn:example:length".into(),
                label: " Length ".into(),
                description: Some(" supplied description ".into()),
                optional: Some(false),
                updatable: Some(true),
                reference_frame: Some("#frame".into()),
                axis_id: Some("x".into()),
            },
            constraint: None,
            uom: UnitReference {
                label: Some(" centimetres ".into()),
                symbol: Some("cm".into()),
                code: Some("cm".into()),
                href: None
            },
            value: Some(finite("125.00")),
        }
    );
    let Some(ScalarValue::Quantity(NumericValue::Finite(inline))) = contract.component().value()
    else {
        panic!("finite inline Quantity")
    };
    assert_eq!(inline.decimal_lexeme(), Some("125.00"));
    let checked = contract.check_value(b"125.00").unwrap();
    assert_eq!(
        checked.value,
        ScalarValue::Quantity(finite("125")),
        "do not convert cm to m"
    );
    assert_ne!(checked.value, ScalarValue::Quantity(finite("1.25")));
    let ScalarValue::Quantity(NumericValue::Finite(value)) = checked.value else {
        panic!("finite Quantity")
    };
    assert_eq!(value.decimal_lexeme(), Some("125.00"));
    for label in [None, Some(json!("")), Some(json!(null)), Some(json!(7))] {
        let mut invalid =
            json!({"type":"Quantity","definition":"urn:example:property","uom":{"code":"m"}});
        if let Some(label) = label {
            invalid["label"] = label;
        }
        assert_eq!(
            ScalarContract::compile(validator(), &serde_json::to_vec(&invalid).unwrap()).err(),
            Some(ScalarError::Structure)
        );
    }
}

#[test]
fn numeric_quantity_specials_and_nan_membership() {
    let contract = compile(&quantity(
        r#", "constraint":{"values":["NaN","+Infinity","-Infinity",1]}"#,
    ));
    for token in [
        r#""NaN""#,
        r#""Infinity""#,
        r#""+Infinity""#,
        r#""-Infinity""#,
    ] {
        let input = quantity(&format!(r#", "value":{token}"#));
        let inline = compile(&input);
        assert_eq!(inline.source(), input);
        for value in [
            inline.component().value().unwrap(),
            contract.check_value(token.as_bytes()).unwrap().value,
        ] {
            match token {
                r#""NaN""# => assert!(matches!(value, ScalarValue::Quantity(NumericValue::NaN))),
                r#""-Infinity""# => assert!(matches!(
                    value,
                    ScalarValue::Quantity(NumericValue::NegativeInfinity)
                )),
                _ => assert!(matches!(
                    value,
                    ScalarValue::Quantity(NumericValue::PositiveInfinity)
                )),
            }
        }
    }
    assert!(
        !NumericValue::NaN.eq(&NumericValue::NaN),
        "named membership does not redefine IEEE equality"
    );
    assert!(NumericValue::NaN.partial_cmp(&finite("0")).is_none());
    let interval = compile(&quantity(
        r#", "constraint":{"intervals":[["-Infinity","Infinity"]]}"#,
    ));
    assert_eq!(
        interval.check_value(br#""NaN""#),
        Err(ScalarError::ConstraintViolation)
    );
    assert_eq!(
        interval.check_value(br#""Infinity""#).unwrap().value,
        ScalarValue::Quantity(NumericValue::PositiveInfinity)
    );
    assert_eq!(
        interval.check_value(br#""-Infinity""#).unwrap().value,
        ScalarValue::Quantity(NumericValue::NegativeInfinity)
    );
    for token in [r#""INF""#, r#""-INF""#, r#""nan""#, r#""42""#] {
        assert_eq!(
            contract.check_value(token.as_bytes()),
            Err(ScalarError::ValueType)
        );
        assert_eq!(
            ScalarContract::compile(validator(), &quantity(&format!(r#", "value":{token}"#))).err(),
            Some(ScalarError::Structure)
        );
    }
}

#[test]
fn numeric_constraints_are_inclusive_unions() {
    let constraint =
        r#", "constraint":{"type":"AllowedValues","values":[99,99],"intervals":[[-2,2],[10,10]]}"#;
    for input in [source("Count", constraint), quantity(constraint)] {
        let contract = compile(&input);
        for token in ["-2", "0", "2", "10", "99"] {
            let value = contract.check_value(token.as_bytes()).unwrap().value;
            let expected = match contract.component() {
                ScalarComponent::Count { .. } => ScalarValue::Count(token.parse().unwrap()),
                _ => ScalarValue::Quantity(finite(token)),
            };
            assert_eq!(
                value, expected,
                "inclusive interval or enumerated value {token}"
            );
        }
        for token in ["-3", "3", "9", "11", "100"] {
            assert_eq!(
                contract.check_value(token.as_bytes()).err(),
                Some(ScalarError::ConstraintViolation)
            );
        }
    }
    for malformed in [
        r#"{"values":[99],"intervals":[[-2,2],[5,4]]}"#,
        r#"{"values":[99],"intervals":[["NaN",2]]}"#,
        r#"{"values":[99],"intervals":[[0,"NaN"]]}"#,
    ] {
        let input = quantity(&format!(r#", "constraint":{malformed}, "value":99"#));
        assert_eq!(
            ScalarContract::compile(validator(), &input).err(),
            Some(ScalarError::Constraint),
            "all intervals must be valid even when enumeration matches"
        );
    }
    for malformed in [
        r#"{"values":[]}"#,
        r#"{"intervals":[]}"#,
        r#"{"intervals":[[1]]}"#,
        r#"{"intervals":[[1,2,3]]}"#,
        r#"{}"#,
    ] {
        assert_eq!(
            ScalarContract::compile(
                validator(),
                &quantity(&format!(r#", "constraint":{malformed}"#))
            )
            .err(),
            Some(ScalarError::Structure)
        );
    }
    for invalid_count in [
        r#"{"values":[1.5]}"#,
        r#"{"values":["NaN"]}"#,
        r#"{"values":["Infinity"]}"#,
        r#"{"intervals":[[0,1.5]]}"#,
        r#"{"intervals":[["-Infinity",0]]}"#,
    ] {
        let input = source("Count", &format!(r#", "constraint":{invalid_count}"#));
        assert_eq!(
            validator().validate(Contract::Count, &input),
            Ok(()),
            "generic AllowedValues alone permits these"
        );
        assert_eq!(
            ScalarContract::compile(validator(), &input).err(),
            Some(ScalarError::Constraint)
        );
    }
    let singleton = compile(&quantity(
        r#", "constraint":{"intervals":[["Infinity","Infinity"]]}"#,
    ));
    assert_eq!(
        singleton.check_value(br#""+Infinity""#).unwrap().value,
        ScalarValue::Quantity(NumericValue::PositiveInfinity)
    );
    assert_eq!(
        singleton.check_value(b"1e400"),
        Err(ScalarError::ConstraintViolation)
    );
}

#[test]
fn numeric_significant_figures_do_not_round() {
    let contract = compile(&quantity(
        r#", "constraint":{"intervals":[["-Infinity","Infinity"]],"significantFigures":2}"#,
    ));
    for token in ["12", "1.2", "0.00052", "1.2e30", "0", "0.00", "-0.0"] {
        let checked = contract.check_value(token.as_bytes()).unwrap();
        assert_eq!(checked.value, ScalarValue::Quantity(finite(token)));
        let ScalarValue::Quantity(NumericValue::Finite(value)) = checked.value else {
            panic!("finite Quantity")
        };
        assert_eq!(value.decimal_lexeme(), Some(token));
    }
    for token in ["12.0", "1.20", "0.000520", "1.20e30", "0.000"] {
        assert_eq!(
            contract.check_value(token.as_bytes()),
            Err(ScalarError::ConstraintViolation),
            "total significant digits, including trailing zeroes: {token}"
        );
    }
    let six = compile(&quantity(
        r#", "constraint":{"values":[12.23],"significantFigures":6}, "value":12.2300"#,
    ));
    assert_eq!(
        six.component().value(),
        Some(ScalarValue::Quantity(finite("12.23")))
    );
    assert_eq!(
        six.check_value(b"12.23000"),
        Err(ScalarError::ConstraintViolation)
    );
    let forty = compile(&quantity(
        r#", "constraint":{"intervals":[[0,"Infinity"]],"significantFigures":40}"#,
    ));
    assert_eq!(
        forty.check_value("1".repeat(40).as_bytes()).unwrap().value,
        ScalarValue::Quantity(finite(&"1".repeat(40)))
    );
    assert_eq!(
        forty.check_value("1".repeat(41).as_bytes()),
        Err(ScalarError::ConstraintViolation)
    );
    for sf in ["0", "41", "1.5", r#""2""#] {
        let input = quantity(&format!(
            r#", "constraint":{{"values":[1],"significantFigures":{sf}}}"#
        ));
        assert_eq!(
            ScalarContract::compile(validator(), &input).err(),
            Some(ScalarError::Structure)
        );
    }
    let count = source(
        "Count",
        r#", "constraint":{"values":[1],"significantFigures":1}"#,
    );
    assert_eq!(
        ScalarContract::compile(validator(), &count).err(),
        Some(ScalarError::Constraint)
    );
    let sf_only = quantity(r#", "constraint":{"significantFigures":2}"#);
    assert_eq!(
        validator().validate(Contract::Quantity, &sf_only),
        Err(Failure::Structure)
    );
    assert_eq!(
        ScalarContract::compile(validator(), &sf_only).err(),
        Some(ScalarError::Structure)
    );
}

#[test]
fn numeric_unit_reference_status_is_explicit() {
    for code in ["cm", "m", "1", "Cel", "m/s"] {
        let input = source("Quantity", &format!(r#", "uom":{{"code":"{code}"}}"#));
        let checked = compile(&input).check_value(b"125").unwrap();
        assert_eq!(checked.value, ScalarValue::Quantity(finite("125")));
        assert_eq!(checked.unit_reference, UnitReferenceCheck::CodeValidated);
    }
    for href in ["urn:example:unit", "https://units.example.test/m"] {
        for code in [None, Some("cm")] {
            let mut uom = json!({"href":href,"label":" supplied label ","symbol":"custom"});
            if let Some(code) = code {
                uom["code"] = json!(code);
            }
            let input = source("Quantity", &format!(r#", "uom":{uom}"#));
            let contract = compile(&input);
            assert_eq!(
                contract.check_value(b"125").unwrap().unit_reference,
                UnitReferenceCheck::Unresolved(href.into())
            );
            let ScalarComponent::Quantity { uom, .. } = contract.component() else {
                panic!("Quantity")
            };
            assert_eq!(uom.href.as_deref(), Some(href));
            assert_eq!(uom.code.as_deref(), code);
            assert_eq!(uom.label.as_deref(), Some(" supplied label "));
        }
    }
    for invalid in [
        json!({}),
        json!({"label":"metres"}),
        json!({"code":""}),
        json!({"code":"m","symbol":""}),
        json!({"code":"m","label":null}),
        json!({"href":"relative/unit"}),
        json!({"href":"#unit"}),
    ] {
        let input = source("Quantity", &format!(r#", "uom":{invalid}"#));
        assert!(
            ScalarContract::compile(validator(), &input).is_err(),
            "invalid unit declaration {invalid}"
        );
    }
    assert_eq!(
        ScalarContract::compile(validator(), &source("Quantity", "")).err(),
        Some(ScalarError::Structure)
    );
    for code in [" m", "m ", "definitelyUnknownUnit"] {
        let input = source("Quantity", &format!(r#", "uom":{{"code":"{code}"}}"#));
        assert_eq!(
            ScalarContract::compile(validator(), &input).err(),
            Some(ScalarError::Unit(crate::units::UnitError::Invalid))
        );
    }
}

#[test]
fn numeric_generated_bounds_and_lexemes() {
    // Fixed 64 cases, no RNG or clock. The oracle uses bounded integer offsets,
    // not the production membership checker or a floating-point intermediary.
    for case in 0_i64..64 {
        let magnitude = 9_007_199_254_741_000 + case * 17;
        let low = if case % 2 == 0 { magnitude } else { -magnitude };
        let high = low + 3;
        let outlier = low + 9;
        let input = source(
            "Count",
            &format!(r#", "constraint":{{"intervals":[[{low},{high}]],"values":[{outlier}]}}"#),
        );
        let count = compile(&input);
        let q_low = case * 10 - 320;
        let q_high = q_low + 3;
        let q_outlier = q_low + 9;
        let measured = compile(&quantity(&format!(
            r#", "constraint":{{"intervals":[[{q_low}e-1,{q_high}e-1]],"values":[{q_outlier}e-1]}}"#
        )));
        for offset in [-1, 0, 1, 2, 3, 4, 9] {
            let accepted = (0..=3).contains(&offset) || offset == 9;
            let expected = low + offset;
            for token in [
                expected.to_string(),
                format!("{expected}.0"),
                format!("{expected}e0"),
            ] {
                match count.check_value(token.as_bytes()) {
                    Ok(checked) => {
                        assert!(accepted, "case {case}, forbidden Count {token}");
                        let ScalarValue::Count(value) = checked.value else {
                            panic!("Count")
                        };
                        assert_eq!(value.try_to_i64(), Ok(expected));
                        assert_eq!(value.number().decimal_lexeme(), Some(token.as_str()));
                    }
                    Err(error) => {
                        assert!(!accepted, "case {case}, permitted Count {token}");
                        assert_eq!(error, ScalarError::ConstraintViolation);
                    }
                }
            }
            let numerator = q_low + offset;
            for token in [format!("{numerator}e-1"), format!("{numerator}.0e-1")] {
                match measured.check_value(token.as_bytes()) {
                    Ok(checked) => {
                        assert!(accepted, "case {case}, forbidden Quantity {token}");
                        let ScalarValue::Quantity(NumericValue::Finite(value)) = checked.value
                        else {
                            panic!("finite Quantity")
                        };
                        assert_eq!(
                            value,
                            format!("{numerator}e-1").parse::<ExactNumber>().unwrap()
                        );
                        assert_eq!(value.decimal_lexeme(), Some(token.as_str()));
                    }
                    Err(error) => {
                        assert!(!accepted, "case {case}, permitted Quantity {token}");
                        assert_eq!(error, ScalarError::ConstraintViolation);
                    }
                }
            }
        }
    }
}
