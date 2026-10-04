//! Independent SWE range fixtures: two inclusive scalar bounds, typed nils,
//! explicit ordering evidence, and the qualified CountRange schema correction.
use super::{
    CategoryOrder, CountNilSchema, OrderCheck, RangeContract, RangeError, RangeOptions,
    SourceValidation,
};
use crate::{
    scalar::{CodeSpaceCheck, ScalarContract, ScalarError, UnitReferenceCheck},
    validation::{Contract, Failure, StructuralValidator},
};
use glaux_domain::{
    numeric::{NumericError, NumericValue},
    range::RangeKind,
    scalar::{
        CalendarTime, ScalarComponent, ScalarValue, TimeFrame, TimePosition,
        UnsupportedTimeConversion,
    },
};
use serde_json::json;
use std::sync::OnceLock;

const GREGORIAN: &str = "http://www.opengis.net/def/uom/ISO-8601/0/Gregorian";
const GPS: &str = "http://www.opengis.net/def/trs/USNO/0/GPS";
const REASON: &str = "urn:example:nil:missing";

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn source(kind: &str, members: &str) -> Vec<u8> {
    format!(r#"{{"type":"{kind}","definition":"urn:example:extent","label":"Extent"{members}}}"#)
        .into_bytes()
}

fn quantity(members: &str) -> Vec<u8> {
    source(
        "QuantityRange",
        &format!(r#", "uom":{{"code":"m"}}{members}"#),
    )
}

fn calendar(members: &str) -> Vec<u8> {
    source(
        "TimeRange",
        &format!(r#", "uom":{{"href":"{GREGORIAN}"}}{members}"#),
    )
}

fn compile(input: &[u8]) -> RangeContract {
    RangeContract::compile(validator(), input, RangeOptions::default())
        .expect("valid independent range fixture")
}

fn finite(token: &str) -> NumericValue {
    NumericValue::Finite(token.parse().expect("independent exact number"))
}

fn category_options(order: &CategoryOrder) -> RangeOptions<'_> {
    RangeOptions {
        category_order: Some(order),
        ..RangeOptions::default()
    }
}

#[test]
fn range_pair_cardinality_rejects_extra_values() {
    let contract = compile(&source("CountRange", ""));
    assert_eq!(
        contract.check_value(b"[1,2,3]").err(),
        Some(RangeError::Cardinality)
    );
    for input in ["[]", "[1]", "[1,2,3,4]", "null", "1", "{}", "false"] {
        assert_eq!(
            contract.check_value(input.as_bytes()).err(),
            Some(RangeError::Cardinality),
            "exactly two endpoints, not a scalar or absent value: {input}"
        );
    }
    for input in ["[1,null]", "[null,1]", "[false,1]", r#"[1,"2"]"#, "[[],1]"] {
        assert_eq!(
            contract.check_value(input.as_bytes()).err(),
            Some(RangeError::Endpoint(ScalarError::ValueType))
        );
    }
    for value in ["[]", "[1]", "[1,2,3]", "null", "[1,null]"] {
        let input = source("CountRange", &format!(r#", "value":{value}"#));
        assert_eq!(
            validator().validate(Contract::CountRange, &input),
            Err(Failure::Structure)
        );
        assert_eq!(
            RangeContract::compile(validator(), &input, RangeOptions::default()).err(),
            Some(RangeError::Structure)
        );
    }
    assert!(contract.inline_value().is_none());
    assert!(contract.component().value.is_none());
    assert_eq!(
        contract.check_value(b"[0,0]").unwrap().order,
        OrderCheck::Established
    );
}

#[test]
fn range_count_exact_pairs_and_endpoint_constraints() {
    let contract = compile(&source("CountRange", ""));
    let pair = contract
        .check_value(b"[9007199254740992,9007199254740993]")
        .unwrap();
    let ScalarValue::Count(high) = &pair.endpoints[1].value else {
        panic!("integer upper bound")
    };
    assert_eq!(high.try_to_u64(), Ok(9_007_199_254_740_993));
    assert_eq!(pair.order, OrderCheck::Established);
    assert_eq!(
        contract
            .check_value(b"[9007199254740993,9007199254740992]")
            .err(),
        Some(RangeError::ReversedBounds)
    );
    for (low, high) in [
        ("18446744073709551616", "18446744073709551617"),
        ("1e400", "2e400"),
        ("-0", "0.0"),
    ] {
        let input = source("CountRange", &format!(r#", "value":[{low},{high}]"#));
        assert_eq!(validator().validate(Contract::CountRange, &input), Ok(()));
        let inline = compile(&input);
        assert_eq!(inline.source_validation(), SourceValidation::Original);
        for (endpoint, token) in inline
            .inline_value()
            .unwrap()
            .endpoints
            .iter()
            .zip([low, high])
        {
            let ScalarValue::Count(value) = &endpoint.value else {
                panic!("Count")
            };
            assert_eq!(value.number().decimal_lexeme(), Some(token));
            assert!(endpoint.nil_reason.is_none());
        }
        assert_eq!(inline.source(), input);
        assert_eq!(compile(inline.source()).component(), inline.component());
    }
    assert_eq!(
        contract
            .check_value(b"[1,1.0000000000000000000000000001]")
            .err(),
        Some(RangeError::Endpoint(ScalarError::Numeric(
            NumericError::NonIntegral
        )))
    );
    for pair in [r#"[1,"NaN"]"#, r#"["1",2]"#] {
        assert_eq!(
            contract.check_value(pair.as_bytes()).err(),
            Some(RangeError::Endpoint(ScalarError::ValueType))
        );
    }
    let constrained = compile(&source(
        "CountRange",
        r#", "constraint":{"values":[9],"intervals":[[0,2]]}"#,
    ));
    for pair in ["[0,2]", "[2,9]", "[9,9]"] {
        assert_eq!(
            constrained.check_value(pair.as_bytes()).unwrap().order,
            OrderCheck::Established
        );
    }
    for pair in ["[-1,2]", "[0,3]", "[3,9]"] {
        assert_eq!(
            constrained.check_value(pair.as_bytes()).err(),
            Some(RangeError::Endpoint(ScalarError::ConstraintViolation))
        );
    }
    assert_eq!(
        constrained.check_value(b"[9,2]").err(),
        Some(RangeError::ReversedBounds)
    );
}

#[test]
fn range_quantity_source_units_and_nil_endpoints() {
    let input = br##"{
      "type":"QuantityRange", "definition":"urn:example:length", "label":" Length extent ",
      "description":" original metadata ", "optional":false, "referenceFrame":"#frame", "axisID":"x",
      "uom":{"code":"cm","label":" centimetres ","symbol":"cm"},
      "value":[125.00,150.00], "vendor:note":{"keep":"original"}
    }"##;
    let contract = compile(input);
    assert_eq!(contract.source(), input);
    assert_eq!(contract.component().kind, RangeKind::Quantity);
    let ScalarComponent::Quantity {
        metadata,
        uom,
        value,
        ..
    } = &contract.component().endpoint
    else {
        panic!("shared Quantity endpoint descriptor")
    };
    assert_eq!(metadata.label, " Length extent ");
    assert_eq!(metadata.description.as_deref(), Some(" original metadata "));
    assert_eq!(metadata.reference_frame.as_deref(), Some("#frame"));
    assert_eq!(metadata.axis_id.as_deref(), Some("x"));
    assert_eq!(metadata.optional, Some(false));
    assert_eq!(uom.code.as_deref(), Some("cm"));
    assert_eq!(uom.label.as_deref(), Some(" centimetres "));
    assert!(value.is_none(), "the pair is not a scalar inline value");
    for (endpoint, expected) in contract
        .inline_value()
        .unwrap()
        .endpoints
        .iter()
        .zip(["125.00", "150.00"])
    {
        assert_eq!(
            endpoint.value,
            ScalarValue::Quantity(finite(expected)),
            "no implicit cm to m conversion"
        );
        let ScalarValue::Quantity(NumericValue::Finite(value)) = &endpoint.value else {
            panic!("finite bound")
        };
        assert_eq!(value.decimal_lexeme(), Some(expected));
        assert_eq!(endpoint.unit_reference, UnitReferenceCheck::CodeValidated);
    }
    let recompiled = compile(contract.source());
    assert_eq!(recompiled.component(), contract.component());
    assert_eq!(recompiled.inline_value(), contract.inline_value());
    assert_eq!(
        contract.component().value.as_ref().unwrap()[0].value,
        ScalarValue::Quantity(finite("125"))
    );

    let input = quantity(&format!(
        r#", "constraint":{{"intervals":[[0,10]]}}, "nilValues":[{{"reason":"{REASON}","value":-999.00}}]"#
    ));
    let absent = compile(&input);
    assert!(absent.inline_value().is_none());
    assert!(absent.component().value.is_none());
    assert_eq!(absent.component().nil_values.len(), 1);
    assert_eq!(absent.component().nil_values[0].reason, REASON);
    for (pair, reasons) in [
        ("[-999,5]", [Some(REASON), None]),
        ("[5,-999]", [None, Some(REASON)]),
        ("[-999,-999]", [Some(REASON), Some(REASON)]),
    ] {
        let checked = absent.check_value(pair.as_bytes()).unwrap();
        assert_eq!(checked.order, OrderCheck::NilEndpoint);
        for (endpoint, reason) in checked.endpoints.iter().zip(reasons) {
            assert_eq!(endpoint.nil_reason.as_deref(), reason);
        }
    }
    for pair in ["[-999,11]", "[-1,-999]"] {
        assert_eq!(
            absent.check_value(pair.as_bytes()).err(),
            Some(RangeError::Endpoint(ScalarError::ConstraintViolation))
        );
    }
    assert_eq!(
        absent.check_value(b"[0,10]").unwrap().order,
        OrderCheck::Established
    );
    let mut inline_source: serde_json::Value = serde_json::from_slice(&input).unwrap();
    inline_source["value"] = json!([-999, 5]);
    let inline_source = serde_json::to_vec(&inline_source).unwrap();
    let inline = compile(&inline_source);
    assert_eq!(inline.source(), inline_source);
    assert_eq!(
        inline.component().value.as_ref().unwrap()[0]
            .nil_reason
            .as_deref(),
        Some(REASON)
    );
    assert_eq!(compile(inline.source()).component(), inline.component());
}

#[test]
fn range_count_nil_correction_is_explicit_and_narrow() {
    let input = source(
        "CountRange",
        &format!(
            r#", "nilValues":[{{"reason":"{REASON}","value":-999}}], "constraint":{{"intervals":[[0,2]]}}, "value":[-999,1]"#
        ),
    );
    assert_eq!(
        validator().validate(Contract::CountRange, &input),
        Err(Failure::Structure)
    );
    assert_eq!(
        RangeContract::compile(validator(), &input, RangeOptions::default()).err(),
        Some(RangeError::Structure)
    );
    let corrected = || RangeOptions {
        count_nil_schema: CountNilSchema::IntegerNilCorrection,
        ..RangeOptions::default()
    };
    let contract = RangeContract::compile(validator(), &input, corrected()).unwrap();
    assert_eq!(
        contract.source_validation(),
        SourceValidation::CountIntegerNilCorrection
    );
    assert_eq!(contract.source(), input);
    let pair = contract.inline_value().unwrap();
    assert_eq!(pair.order, OrderCheck::NilEndpoint);
    assert_eq!(
        pair.endpoints[0].value,
        ScalarValue::Count("-999".parse().unwrap())
    );
    assert_eq!(pair.endpoints[0].nil_reason.as_deref(), Some(REASON));
    assert_eq!(
        pair.endpoints[1].value,
        ScalarValue::Count("1".parse().unwrap())
    );
    assert!(pair.endpoints[1].nil_reason.is_none());
    assert_eq!(
        RangeContract::compile(validator(), contract.source(), corrected())
            .unwrap()
            .component(),
        contract.component()
    );
    assert_eq!(
        validator().validate(Contract::CountRange, &input),
        Err(Failure::Structure),
        "adaptation must not mutate the original catalog"
    );
    assert_eq!(
        contract.check_value(br#"["-999",1]"#).err(),
        Some(RangeError::Endpoint(ScalarError::ValueType))
    );
    assert_eq!(
        contract.check_value(b"[-999,3]").err(),
        Some(RangeError::Endpoint(ScalarError::ConstraintViolation))
    );
    let ordinary =
        RangeContract::compile(validator(), &source("CountRange", ""), corrected()).unwrap();
    assert_eq!(ordinary.source_validation(), SourceValidation::Original);
    let catalog = crate::validation::catalog().unwrap();
    let original = &catalog["https://schemas.opengis.net/sweCommon/3.0/json/CountRange.json"];
    let mut altered = original.clone();
    assert_eq!(super::correct_count_schema(&mut altered), Ok(()));
    assert_eq!(
        super::correct_count_schema(&mut altered),
        Err(RangeError::AdaptationSourceChanged)
    );
    assert_eq!(
        original
            .pointer("/allOf/1/properties/nilValues/$ref")
            .and_then(serde_json::Value::as_str),
        Some("basicTypes.json#/$defs/NilValuesText")
    );

    let text_nil = source(
        "CountRange",
        &format!(r#", "nilValues":[{{"reason":"{REASON}","value":"-999"}}]"#),
    );
    assert_eq!(
        validator().validate(Contract::CountRange, &text_nil),
        Ok(())
    );
    assert_eq!(
        RangeContract::compile(validator(), &text_nil, corrected()).err(),
        Some(RangeError::Endpoint(ScalarError::Structure)),
        "original structural acceptance is not compatible integer meaning"
    );
    for defect in 0..5 {
        let mut invalid: serde_json::Value = serde_json::from_slice(&input).unwrap();
        match defect {
            0 => {
                invalid.as_object_mut().unwrap().remove("label");
            }
            1 => invalid["value"] = json!([-999, 1, 2]),
            2 => invalid["nilValues"][0]["value"] = json!(-999.5),
            3 => invalid["nilValues"][0]["extra"] = json!(true),
            _ => invalid["value"] = json!([-999, null]),
        }
        assert_eq!(
            RangeContract::compile(
                validator(),
                &serde_json::to_vec(&invalid).unwrap(),
                corrected()
            )
            .err(),
            Some(RangeError::Structure),
            "correction is not general shape relaxation: defect {defect}"
        );
    }
}

#[test]
fn range_category_order_uses_supplied_evidence() {
    let input = source(
        "CategoryRange",
        r#", "codeSpace":"urn:example:severity", "constraint":{"values":["m-high","z-low"]}"#,
    );
    let mut order = CategoryOrder {
        code_space: Some("urn:example:severity".into()),
        ascending_tokens: ["z-low", "a-mid", "m-high"].map(str::to_owned).into(),
    };
    let contract = RangeContract::compile(validator(), &input, category_options(&order)).unwrap();
    order.ascending_tokens.reverse();
    let pair = contract.check_value(br#"["z-low","m-high"]"#).unwrap();
    assert_eq!(
        pair.order,
        OrderCheck::Established,
        "captured semantic order, not lexical/enum order or mutable caller evidence"
    );
    assert_eq!(
        pair.endpoints[0].value,
        ScalarValue::Category("z-low".into())
    );
    assert_eq!(
        pair.endpoints[0].code_space,
        CodeSpaceCheck::Unresolved("urn:example:severity".into())
    );
    assert_eq!(
        contract.check_value(br#"["m-high","z-low"]"#).err(),
        Some(RangeError::ReversedBounds)
    );
    assert_eq!(
        contract.check_value(br#"["z-low","z-low"]"#).unwrap().order,
        OrderCheck::Established
    );
    for pair in [r#"["a-mid","m-high"]"#, r#"["z-low","a-mid"]"#] {
        assert_eq!(
            contract.check_value(pair.as_bytes()).err(),
            Some(RangeError::Endpoint(ScalarError::ConstraintViolation))
        );
    }
    assert_eq!(
        compile(&input)
            .check_value(br#"["m-high","z-low"]"#)
            .unwrap()
            .order,
        OrderCheck::UnresolvedCategory
    );
    let local = source(
        "CategoryRange",
        r#", "constraint":{"values":["z-low","m-high"]}, "value":["z-low","m-high"]"#,
    );
    let local = compile(&local);
    assert_eq!(local.component().kind, RangeKind::Category);
    assert_eq!(
        local.inline_value().unwrap().order,
        OrderCheck::UnresolvedCategory
    );
    assert_eq!(compile(local.source()).component(), local.component());

    for evidence in [
        CategoryOrder {
            code_space: Some("urn:example:other".into()),
            ascending_tokens: vec!["z-low".into(), "m-high".into()],
        },
        CategoryOrder {
            code_space: Some("urn:example:severity".into()),
            ascending_tokens: vec!["z-low".into(), "z-low".into(), "m-high".into()],
        },
        CategoryOrder {
            code_space: Some("urn:example:severity".into()),
            ascending_tokens: vec!["z-low".into()],
        },
        CategoryOrder {
            code_space: Some("urn:example:severity".into()),
            ascending_tokens: vec![],
        },
    ] {
        assert_eq!(
            RangeContract::compile(validator(), &input, category_options(&evidence)).err(),
            Some(RangeError::CategoryOrder)
        );
    }
    assert_eq!(
        RangeContract::compile(
            validator(),
            &source("CountRange", ""),
            category_options(&order)
        )
        .err(),
        Some(RangeError::CategoryOrder)
    );
    order.ascending_tokens.reverse();
    let nil_input = source(
        "CategoryRange",
        &format!(
            r#", "codeSpace":"urn:example:severity", "nilValues":[{{"reason":"{REASON}","value":"missing"}}]"#
        ),
    );
    let with_nil =
        RangeContract::compile(validator(), &nil_input, category_options(&order)).unwrap();
    let pair = with_nil.check_value(br#"["missing","m-high"]"#).unwrap();
    assert_eq!(pair.order, OrderCheck::NilEndpoint);
    assert_eq!(pair.endpoints[0].nil_reason.as_deref(), Some(REASON));
    assert_eq!(
        with_nil.check_value(br#"["missing","foreign"]"#).err(),
        Some(RangeError::CategoryOrder),
        "nil cannot excuse unknown ordinary sibling membership"
    );
    let nil_enum = source(
        "CategoryRange",
        &format!(
            r#", "codeSpace":"urn:example:severity", "nilValues":[{{"reason":"{REASON}","value":"missing"}}], "constraint":{{"values":["missing","z-low"]}}"#
        ),
    );
    let nil_enum =
        RangeContract::compile(validator(), &nil_enum, category_options(&order)).unwrap();
    assert_eq!(
        nil_enum
            .check_value(br#"["missing","z-low"]"#)
            .unwrap()
            .order,
        OrderCheck::NilEndpoint
    );
}

#[test]
fn range_time_preserves_context_and_exact_calendar_bounds() {
    let input = calendar(
        r#", "value":["2000-01-01T00:00:00.1234567890123456789Z","2000-01-01T00:00:00.1234567890123456790Z"]"#,
    );
    assert_eq!(validator().validate(Contract::TimeRange, &input), Ok(()));
    let contract = compile(&input);
    let pair = contract.inline_value().unwrap();
    let ScalarValue::Time(low) = &pair.endpoints[0].value else {
        panic!("Time")
    };
    assert_eq!(
        low.utc_instant().unwrap().fraction_decimal(),
        "0.1234567890123456789"
    );
    assert_eq!(low.utc_instant().unwrap().civil_second(), 946_684_800);
    assert_eq!(pair.order, OrderCheck::Established);
    assert_eq!(contract.component().kind, RangeKind::Time);
    assert_eq!(contract.source(), input);
    assert_eq!(compile(contract.source()).component(), contract.component());
    assert_eq!(
        contract
            .check_value(br#"["2000-01-01T01:00:00+01:00","2000-01-01T00:00:00Z"]"#)
            .unwrap()
            .order,
        OrderCheck::Established
    );
    assert_eq!(contract.check_value(br#"["2000-01-01T00:00:00.1234567890123456790Z","2000-01-01T00:00:00.1234567890123456789Z"]"#).err(), Some(RangeError::ReversedBounds));

    let input = source(
        "TimeRange",
        r#", "uom":{"code":"ms"}, "referenceTime":"2000-01-01T00:00:00Z", "localFrame":"urn:example:clock", "constraint":{"values":[9],"intervals":[[0,2]]}, "value":[0.00,2e0]"#,
    );
    let numeric = compile(&input);
    for (endpoint, token) in numeric
        .inline_value()
        .unwrap()
        .endpoints
        .iter()
        .zip(["0.00", "2e0"])
    {
        let ScalarValue::Time(bound) = &endpoint.value else {
            panic!("Time")
        };
        assert_eq!(bound.utc_instant().err(), Some(UnsupportedTimeConversion));
        assert_eq!(bound.position, TimePosition::Numeric(finite(token)));
        let TimePosition::Numeric(NumericValue::Finite(value)) = &bound.position else {
            panic!("numeric coordinate")
        };
        assert_eq!(value.decimal_lexeme(), Some(token));
        assert_eq!(bound.reference.frame, TimeFrame::DefaultUtc);
        assert_eq!(bound.reference.uom.code.as_deref(), Some("ms"));
        assert_eq!(
            bound.reference.local_frame.as_deref(),
            Some("urn:example:clock")
        );
        let Some(CalendarTime::Utc(origin)) = &bound.reference.origin else {
            panic!("explicit UTC origin")
        };
        assert_eq!(origin.source_lexeme(), "2000-01-01T00:00:00Z");
        assert_eq!(origin.civil_second(), 946_684_800);
    }
    assert_eq!(numeric.source(), input);
    assert_eq!(compile(numeric.source()).component(), numeric.component());
    assert_eq!(
        numeric.check_value(b"[2,9]").unwrap().order,
        OrderCheck::Established
    );
    for pair in ["[-1,2]", "[0,3]"] {
        assert_eq!(
            numeric.check_value(pair.as_bytes()).err(),
            Some(RangeError::Endpoint(ScalarError::ConstraintViolation))
        );
    }
    assert_eq!(
        numeric.check_value(b"[2,0]").err(),
        Some(RangeError::ReversedBounds)
    );
}

#[test]
fn range_special_endpoints_are_not_implicit_nil() {
    let quantity = compile(&quantity(""));
    for pair in [r#"["-Infinity","Infinity"]"#, r#"["Infinity","+Infinity"]"#] {
        let checked = quantity.check_value(pair.as_bytes()).unwrap();
        assert_eq!(checked.order, OrderCheck::Established);
        assert!(
            checked
                .endpoints
                .iter()
                .all(|endpoint| endpoint.nil_reason.is_none())
        );
    }
    let checked = quantity.check_value(br#"["NaN",0]"#).unwrap();
    assert_eq!(checked.order, OrderCheck::UnorderedSpecial);
    assert!(matches!(
        checked.endpoints[0].value,
        ScalarValue::Quantity(NumericValue::NaN)
    ));
    assert!(checked.endpoints[0].nil_reason.is_none());
    assert_eq!(
        quantity.check_value(br#"["Infinity",0]"#).err(),
        Some(RangeError::ReversedBounds)
    );

    let time = compile(&calendar(""));
    let enumerated = compile(&calendar(r#", "constraint":{"values":["NaN"]}"#));
    assert_eq!(
        enumerated.check_value(br#"["NaN","NaN"]"#).unwrap().order,
        OrderCheck::UnorderedSpecial
    );
    assert_eq!(
        enumerated
            .check_value(br#"["NaN","2000-01-01T00:00:00Z"]"#)
            .err(),
        Some(RangeError::Endpoint(ScalarError::ConstraintViolation))
    );
    let scalar_enum = source(
        "Time",
        &format!(r#", "uom":{{"href":"{GREGORIAN}"}}, "constraint":{{"values":["NaN"]}}"#),
    );
    assert_eq!(
        ScalarContract::compile(validator(), &scalar_enum).err(),
        Some(ScalarError::ValueType)
    );
    for pair in [
        r#"["-Infinity","2000-01-01T00:00:00Z"]"#,
        r#"["2000-01-01T00:00:00Z","+Infinity"]"#,
        r#"["-Infinity","Infinity"]"#,
    ] {
        let input = calendar(&format!(r#", "value":{pair}"#));
        assert_eq!(validator().validate(Contract::TimeRange, &input), Ok(()));
        assert_eq!(
            compile(&input).inline_value().unwrap().order,
            OrderCheck::Established
        );
    }
    assert_eq!(
        time.check_value(br#"["Infinity","2000-01-01T00:00:00Z"]"#)
            .err(),
        Some(RangeError::ReversedBounds)
    );
    let checked = time.check_value(br#"["NaN","NaN"]"#).unwrap();
    assert_eq!(checked.order, OrderCheck::UnorderedSpecial);
    assert!(
        checked
            .endpoints
            .iter()
            .all(|endpoint| endpoint.nil_reason.is_none())
    );
    for endpoint in &checked.endpoints {
        let ScalarValue::Time(value) = &endpoint.value else {
            panic!("Time")
        };
        assert!(matches!(
            value.position,
            TimePosition::Numeric(NumericValue::NaN)
        ));
    }
    for input in [
        source(
            "QuantityRange",
            &format!(
                r#", "uom":{{"code":"m"}}, "constraint":{{"intervals":[[0,10]]}}, "nilValues":[{{"reason":"{REASON}","value":"NaN"}}]"#
            ),
        ),
        calendar(&format!(
            r#", "nilValues":[{{"reason":"{REASON}","value":"NaN"}}]"#
        )),
    ] {
        let nil = compile(&input).check_value(br#"["NaN","NaN"]"#).unwrap();
        assert_eq!(nil.order, OrderCheck::NilEndpoint);
        assert!(
            nil.endpoints
                .iter()
                .all(|endpoint| endpoint.nil_reason.as_deref() == Some(REASON))
        );
    }
    let scalar = source("Time", &format!(r#", "uom":{{"href":"{GREGORIAN}"}}"#));
    let scalar = ScalarContract::compile(validator(), &scalar).unwrap();
    for value in [br#""Infinity""#.as_slice(), br#""NaN""#.as_slice()] {
        assert_eq!(
            scalar.check_value(value).err(),
            Some(ScalarError::ValueType),
            "range-only specials do not widen ordinary Gregorian Time"
        );
    }
    assert_eq!(
        time.check_value(b"[0,1]").err(),
        Some(RangeError::Endpoint(ScalarError::ValueType))
    );
}

#[test]
fn range_unknown_time_frames_remain_unresolved() {
    let input = calendar(&format!(
        r#", "referenceFrame":"{GPS}", "referenceTime":"2000-01-01T00:00:00Z", "value":["2000-01-01T00:00:00Z","2000-01-02T00:00:00Z"]"#
    ));
    let contract = compile(&input);
    let pair = contract.inline_value().unwrap();
    assert_eq!(pair.order, OrderCheck::UnresolvedTimeFrame);
    let ScalarValue::Time(low) = &pair.endpoints[0].value else {
        panic!("Time")
    };
    assert_eq!(low.utc_instant().err(), Some(UnsupportedTimeConversion));
    assert_eq!(
        low.position,
        TimePosition::Calendar(CalendarTime::Unresolved("2000-01-01T00:00:00Z".into()))
    );
    assert_eq!(low.reference.frame, TimeFrame::Declared(GPS.into()));
    assert_eq!(
        low.reference.origin,
        Some(CalendarTime::Unresolved("2000-01-01T00:00:00Z".into()))
    );
    assert_eq!(contract.source(), input);
    assert_eq!(compile(contract.source()).component(), contract.component());
    assert_eq!(
        contract
            .check_value(br#"["2000-01-01T00:00:00Z","2000-01-01T00:00:00Z"]"#)
            .unwrap()
            .order,
        OrderCheck::Established
    );
    let numeric = compile(&source(
        "TimeRange",
        &format!(r#", "uom":{{"code":"s"}}, "referenceFrame":"{GPS}""#),
    ));
    let numeric = numeric
        .check_value(b"[9007199254740992,9007199254740993]")
        .unwrap();
    assert_eq!(numeric.order, OrderCheck::Established);
    let ScalarValue::Time(high) = &numeric.endpoints[1].value else {
        panic!("Time")
    };
    assert_eq!(
        high.position,
        TimePosition::Numeric(finite("9007199254740993"))
    );
    assert_eq!(high.utc_instant().err(), Some(UnsupportedTimeConversion));
}

#[test]
fn range_generated_exact_boundaries() {
    // Fixed 64 cases; the oracle is bounded integer arithmetic above 2^53,
    // independent of production range membership and ordering algorithms.
    for case in 0_i64..64 {
        let magnitude = 9_007_199_254_741_000 + case * 17;
        let low = if case % 2 == 0 { magnitude } else { -magnitude };
        let high = low + 3;
        let outlier = low + 9;
        let contract = compile(&source(
            "CountRange",
            &format!(r#", "constraint":{{"intervals":[[{low},{high}]],"values":[{outlier}]}}"#),
        ));
        for offset in [-1, 0, 1, 2, 3, 4, 9] {
            let expected = low + offset;
            let accepted = (0..=3).contains(&offset) || offset == 9;
            for upper in [
                expected.to_string(),
                format!("{expected}.0"),
                format!("{expected}e0"),
            ] {
                let checked = contract.check_value(format!("[{low},{upper}]").as_bytes());
                if accepted {
                    let checked = checked.unwrap();
                    assert_eq!(checked.order, OrderCheck::Established);
                    let ScalarValue::Count(value) = &checked.endpoints[1].value else {
                        panic!("Count")
                    };
                    assert_eq!(value.try_to_i64(), Ok(expected));
                    assert_eq!(value.number().decimal_lexeme(), Some(upper.as_str()));
                    if offset > 0 {
                        assert_eq!(
                            contract
                                .check_value(format!("[{upper},{low}]").as_bytes())
                                .err(),
                            Some(RangeError::ReversedBounds)
                        );
                    }
                } else {
                    assert_eq!(
                        checked.err(),
                        Some(RangeError::Endpoint(ScalarError::ConstraintViolation)),
                        "case {case}, forbidden endpoint {upper}"
                    );
                }
            }
        }
    }
}
