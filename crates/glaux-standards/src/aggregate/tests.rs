//! Independent description fixtures from SWE 3.0 record/vector requirements.
//! These checks do not exercise or claim aggregate payload codecs.
use super::{AggregateContract, AggregateError};
use crate::{
    range::{OrderCheck, RangeError},
    scalar::{ScalarError, UnitReferenceCheck},
    validation::{self, Contract, Failure, StructuralValidator},
};
use glaux_domain::{
    aggregate::{AggregateComponent, Component},
    numeric::NumericValue,
    range::RangeKind,
    scalar::{
        CalendarTime, ScalarComponent, ScalarValue, TimeFrame, TimePosition,
        UnsupportedTimeConversion,
    },
};
use serde_json::{Value, json};
use std::sync::OnceLock;

const RECORD: &[u8] = include_bytes!("../../fixtures/aggregate/record-mixed.json");
const VECTOR: &[u8] = include_bytes!("../../fixtures/aggregate/vector-mixed.json");
const FRAME: &str = "urn:example:spatiotemporal-frame";
const UTC: &str = "http://www.opengis.net/def/trs/BIPM/0/UTC";
const GREGORIAN: &str = "http://www.opengis.net/def/uom/ISO-8601/0/Gregorian";

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn compile(input: &[u8]) -> AggregateContract {
    AggregateContract::compile(validator(), input).expect("valid independent aggregate fixture")
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn count(name: &str) -> Value {
    json!({"name":name,"type":"Count","definition":"urn:example:count","label":"Count"})
}

fn record(fields: Vec<Value>) -> Vec<u8> {
    bytes(&json!({"type":"DataRecord","fields":fields}))
}

fn finite(token: &str) -> NumericValue {
    NumericValue::Finite(token.parse().expect("independent exact number"))
}

#[test]
fn aggregate_record_preserves_declared_field_order() {
    let contract = compile(RECORD);
    assert_eq!(
        contract
            .children()
            .iter()
            .map(|child| child.name())
            .collect::<Vec<_>>(),
        ["zCount", "aBand", "mNested"]
    );
    let AggregateComponent::Record { fields, .. } = contract.component() else {
        panic!("record")
    };
    assert_eq!(
        fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["zCount", "aBand", "mNested"]
    );
    assert!(
        matches!(&fields[0].component, Component::Scalar(component) if matches!(component.as_ref(), ScalarComponent::Count { .. }))
    );
    assert!(matches!(fields[1].component, Component::Range(_)));
    assert!(matches!(fields[2].component, Component::Aggregate(_)));
    let count = contract.children()[0].scalar().unwrap();
    let Some(ScalarValue::Count(value)) = count.component().value() else {
        panic!("inline count")
    };
    assert_eq!(value.try_to_u64(), Ok(9_007_199_254_740_993));
    assert_eq!(value.number().decimal_lexeme(), Some("9007199254740993"));
    let nested = contract.children()[2].aggregate().unwrap();
    assert_eq!(
        nested
            .children()
            .iter()
            .map(|child| child.name())
            .collect::<Vec<_>>(),
        ["zCount", "aText", "mStatus"]
    );
    assert_eq!(
        nested.children()[0].scalar().unwrap().component().value(),
        Some(ScalarValue::Boolean(false))
    );
    assert_eq!(contract.source(), RECORD);
    assert_eq!(compile(contract.source()).component(), contract.component());
}

#[test]
fn aggregate_metadata_optional_and_nested_names() {
    let contract = compile(RECORD);
    let AggregateComponent::Record { metadata, .. } = contract.component() else {
        panic!("record")
    };
    assert_eq!(metadata.id.as_deref(), Some("MIXED"));
    assert_eq!(
        metadata.definition, None,
        "do not invent a scalar-style definition requirement"
    );
    assert_eq!(metadata.label.as_deref(), Some(" Mixed values "));
    assert_eq!(
        metadata.description.as_deref(),
        Some("Independent ordered record fixture")
    );
    assert_eq!(metadata.optional, Some(true));
    assert_eq!(metadata.updatable, Some(false));
    let minimal = compile(&record(vec![count("A"), count("a")]));
    let AggregateComponent::Record { metadata, .. } = minimal.component() else {
        panic!("record")
    };
    assert_eq!(metadata.definition, None);
    assert_eq!(metadata.label, None);
    assert_eq!(metadata.optional, None);
    assert_eq!(metadata.updatable, None);
    assert!(
        minimal.children()[0]
            .scalar()
            .unwrap()
            .component()
            .value()
            .is_none()
    );
    assert_eq!(
        minimal
            .children()
            .iter()
            .map(|child| child.name())
            .collect::<Vec<_>>(),
        ["A", "a"]
    );
    assert_eq!(
        AggregateContract::compile(validator(), &record(vec![count("same"), count("same")])).err(),
        Some(AggregateError::DuplicateName)
    );
    for name in ["", "1bad", "with space", "_prefix", "é"] {
        let input = record(vec![count(name)]);
        assert_eq!(
            validator().validate(Contract::DataRecord, &input),
            Err(Failure::Structure)
        );
        assert_eq!(
            AggregateContract::compile(validator(), &input).err(),
            Some(AggregateError::Structure)
        );
    }
    assert_eq!(
        AggregateContract::compile(validator(), &record(vec![])).err(),
        Some(AggregateError::Structure)
    );
    for label in [json!(""), json!(null), json!(7)] {
        let mut input: Value = serde_json::from_slice(&record(vec![count("one")])).unwrap();
        input["label"] = label;
        assert_eq!(
            AggregateContract::compile(validator(), &bytes(&input)).err(),
            Some(AggregateError::Structure)
        );
    }
}

#[test]
fn aggregate_nested_scalar_and_range_checks() {
    let input = format!(
        r#"{{"type":"DataRecord","fields":[
      {{"name":"bounded","type":"Count","definition":"urn:example:count","label":"Count","constraint":{{"intervals":[[0,2]]}},"value":1}},
      {{"name":"counts","type":"CountRange","definition":"urn:example:count","label":"Count extent","value":[9007199254740992,9007199254740993]}},
      {{"name":"categories","type":"CategoryRange","definition":"urn:example:class","label":"Classes","constraint":{{"values":["z","a"]}},"value":["z","a"]}},
      {{"name":"calendar","type":"TimeRange","definition":"urn:example:time","label":"Time extent","uom":{{"href":"{GREGORIAN}"}},"value":["2000-01-01T00:00:00Z","+Infinity"]}},
      {{"name":"nilTime","type":"Time","definition":"urn:example:time","label":"Time","uom":{{"href":"{GREGORIAN}"}},"nilValues":[{{"reason":"urn:example:nil:missing","value":"NaN"}}],"value":"NaN"}}
    ]}}"#
    );
    assert_eq!(
        validator().validate(Contract::DataRecord, input.as_bytes()),
        Ok(())
    );
    assert_eq!(
        validator().validate(Contract::SweRecord, input.as_bytes()),
        Err(Failure::Structure),
        "legacy format-disabled original diagnostic is unchanged"
    );
    let contract = compile(input.as_bytes());
    assert_eq!(
        contract.children()[0]
            .scalar()
            .unwrap()
            .check_value(b"3")
            .err(),
        Some(ScalarError::ConstraintViolation)
    );
    let counts = contract.children()[1].range().unwrap();
    assert_eq!(counts.component().kind, RangeKind::Count);
    assert_eq!(
        counts.inline_value().unwrap().order,
        OrderCheck::Established
    );
    assert_eq!(
        counts
            .check_value(b"[9007199254740993,9007199254740992]")
            .err(),
        Some(RangeError::ReversedBounds)
    );
    let categories = contract.children()[2].range().unwrap();
    assert_eq!(categories.component().kind, RangeKind::Category);
    assert_eq!(
        categories.inline_value().unwrap().order,
        OrderCheck::UnresolvedCategory
    );
    assert_eq!(
        categories.check_value(br#"["foreign","a"]"#).err(),
        Some(RangeError::Endpoint(ScalarError::ConstraintViolation))
    );
    let time = contract.children()[3].range().unwrap();
    assert_eq!(time.component().kind, RangeKind::Time);
    assert_eq!(time.inline_value().unwrap().order, OrderCheck::Established);
    let nil = contract.children()[4]
        .scalar()
        .unwrap()
        .inline_value()
        .unwrap();
    assert_eq!(nil.nil_reason.as_deref(), Some("urn:example:nil:missing"));
    let ScalarValue::Time(nil) = nil.value else {
        panic!("Time")
    };
    assert!(matches!(
        nil.position,
        TimePosition::Numeric(NumericValue::NaN)
    ));
    let mixed = compile(RECORD);
    let band = mixed.children()[1].range().unwrap();
    for pair in ["[99,150]", "[125,201]"] {
        assert_eq!(
            band.check_value(pair.as_bytes()).err(),
            Some(RangeError::Endpoint(ScalarError::ConstraintViolation))
        );
    }
    let mut invalid: Value = serde_json::from_str(&input).unwrap();
    invalid["fields"][0]["value"] = json!(3);
    assert_eq!(
        AggregateContract::compile(validator(), &bytes(&invalid)).err(),
        Some(AggregateError::Scalar(ScalarError::ConstraintViolation))
    );
    invalid["fields"][0]["value"] = json!(1);
    invalid["fields"][1]["value"] = json!([2, 1]);
    assert_eq!(
        AggregateContract::compile(validator(), &bytes(&invalid)).err(),
        Some(AggregateError::Range(RangeError::ReversedBounds))
    );
    let malformed_calendar = input.replace("2000-01-01T00:00:00Z", "not-a-calendar");
    assert_eq!(
        AggregateContract::compile(validator(), malformed_calendar.as_bytes()).err(),
        Some(AggregateError::Structure)
    );
}

#[test]
fn aggregate_vector_numeric_members_and_frame_binding() {
    let contract = compile(VECTOR);
    assert_eq!(validator().validate(Contract::Vector, VECTOR), Ok(()));
    let AggregateComponent::Vector {
        metadata,
        reference_frame,
        local_frame,
        coordinates,
    } = contract.component()
    else {
        panic!("vector")
    };
    assert_eq!(reference_frame, FRAME);
    assert_eq!(local_frame.as_deref(), Some("#platform"));
    assert_eq!(
        metadata.optional,
        Some(true),
        "the whole vector may be optional"
    );
    assert_eq!(
        coordinates
            .iter()
            .map(|coordinate| coordinate.name.as_str())
            .collect::<Vec<_>>(),
        ["zIndex", "aLength", "mTime"]
    );
    let count = contract.children()[0].scalar().unwrap();
    let ScalarComponent::Count { metadata, .. } = count.component() else {
        panic!("Count coordinate")
    };
    assert_eq!(metadata.axis_id.as_deref(), Some("I"));
    assert_eq!(metadata.optional, Some(false));
    assert_eq!(metadata.reference_frame, None);
    let length = contract.children()[1].scalar().unwrap();
    let ScalarComponent::Quantity { metadata, uom, .. } = length.component() else {
        panic!("Quantity coordinate")
    };
    assert_eq!(metadata.axis_id.as_deref(), Some("X"));
    assert_eq!(uom.code.as_deref(), Some("cm"));
    assert_eq!(
        length.component().value(),
        Some(ScalarValue::Quantity(finite("125.00")))
    );
    assert_eq!(
        length.inline_value().unwrap().unit_reference,
        UnitReferenceCheck::CodeValidated
    );
    let time = contract.children()[2].scalar().unwrap();
    let ScalarComponent::Time(component) = time.component() else {
        panic!("Time coordinate")
    };
    assert_eq!(component.metadata.axis_id.as_deref(), Some("T"));
    assert_eq!(
        component.metadata.reference_frame, None,
        "do not fabricate a supplied child frame"
    );
    assert_eq!(component.reference.frame, TimeFrame::Declared(FRAME.into()));
    assert_eq!(
        component.reference.origin,
        Some(CalendarTime::Unresolved("2000-01-01T00:00:00Z".into()))
    );
    let Some(ScalarValue::Time(bound)) = time.component().value() else {
        panic!("inline Time")
    };
    assert_eq!(bound.utc_instant().err(), Some(UnsupportedTimeConversion));
    assert_eq!(
        bound.position,
        TimePosition::Calendar(CalendarTime::Unresolved(
            "2000-01-01T00:00:00.1234567890123456789Z".into()
        ))
    );
    let child_source: Value = serde_json::from_slice(contract.children()[2].source()).unwrap();
    assert!(child_source.get("referenceFrame").is_none());
    assert_eq!(contract.source(), VECTOR);
    assert_eq!(compile(contract.source()).component(), contract.component());

    let mut utc: Value = serde_json::from_slice(VECTOR).unwrap();
    utc["referenceFrame"] = json!(UTC);
    let utc = compile(&bytes(&utc));
    let Some(ScalarValue::Time(bound)) = utc.children()[2].scalar().unwrap().component().value()
    else {
        panic!("Time")
    };
    assert_eq!(
        bound.utc_instant().unwrap().fraction_decimal(),
        "0.1234567890123456789"
    );
    let mut numeric: Value = serde_json::from_slice(VECTOR).unwrap();
    numeric["coordinates"][2]["uom"] = json!({"code":"ms"});
    numeric["coordinates"][2]["value"] = json!(125);
    let numeric = compile(&bytes(&numeric));
    let Some(ScalarValue::Time(bound)) =
        numeric.children()[2].scalar().unwrap().component().value()
    else {
        panic!("Time")
    };
    assert_eq!(bound.position, TimePosition::Numeric(finite("125")));
    assert_eq!(bound.reference.uom.code.as_deref(), Some("ms"));
    assert_eq!(bound.reference.frame, TimeFrame::Declared(FRAME.into()));
    assert_eq!(bound.utc_instant().err(), Some(UnsupportedTimeConversion));
    let mut relative: Value = serde_json::from_slice(VECTOR).unwrap();
    relative["referenceFrame"] = json!("#world");
    // SWE requires unique coordinate names, not an invented axis-ID uniqueness rule.
    relative["coordinates"][1]["axisID"] = json!("I");
    assert!(AggregateContract::compile(validator(), &bytes(&relative)).is_ok());
}

#[test]
fn aggregate_vector_semantics_exceed_original_schema() {
    for (defect, expected) in [
        (0, AggregateError::EmptyVector),
        (1, AggregateError::DuplicateName),
        (2, AggregateError::CoordinateAxis),
        (3, AggregateError::CoordinateReferenceFrame),
        (4, AggregateError::OptionalCoordinate),
        (5, AggregateError::Metadata),
    ] {
        let mut input: Value = serde_json::from_slice(VECTOR).unwrap();
        match defect {
            0 => input["coordinates"] = json!([]),
            1 => input["coordinates"][1]["name"] = json!("zIndex"),
            2 => {
                input["coordinates"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("axisID");
            }
            3 => input["coordinates"][0]["referenceFrame"] = json!(FRAME),
            4 => input["coordinates"][0]["optional"] = json!(true),
            _ => input["localFrame"] = json!(FRAME),
        }
        let input = bytes(&input);
        assert_eq!(
            validator().validate(Contract::Vector, &input),
            Ok(()),
            "upstream artifact is not silently repaired: defect {defect}"
        );
        assert_eq!(
            AggregateContract::compile(validator(), &input).err(),
            Some(expected)
        );
    }
    for kind in [
        "Boolean",
        "Text",
        "Category",
        "CountRange",
        "QuantityRange",
        "TimeRange",
        "DataRecord",
        "Vector",
    ] {
        let mut input: Value = serde_json::from_slice(VECTOR).unwrap();
        input["coordinates"][0]["type"] = json!(kind);
        assert_eq!(
            AggregateContract::compile(validator(), &bytes(&input)).err(),
            Some(AggregateError::CoordinateType)
        );
    }
    for axis in [json!(""), json!(null), json!(3)] {
        let mut input: Value = serde_json::from_slice(VECTOR).unwrap();
        input["coordinates"][0]["axisID"] = axis;
        assert_eq!(
            AggregateContract::compile(validator(), &bytes(&input)).err(),
            Some(AggregateError::CoordinateAxis)
        );
    }
    for frame in [None, Some(json!(null)), Some(json!("not a URI"))] {
        let mut input: Value = serde_json::from_slice(VECTOR).unwrap();
        if let Some(frame) = frame {
            input["referenceFrame"] = frame;
        } else {
            input.as_object_mut().unwrap().remove("referenceFrame");
        }
        assert_eq!(
            AggregateContract::compile(validator(), &bytes(&input)).err(),
            Some(AggregateError::Structure)
        );
    }
    let mut input: Value = serde_json::from_slice(VECTOR).unwrap();
    input["localFrame"] = json!("not a URI");
    assert_eq!(
        AggregateContract::compile(validator(), &bytes(&input)).err(),
        Some(AggregateError::Structure)
    );
    for member in ["definition", "label"] {
        let mut input: Value = serde_json::from_slice(VECTOR).unwrap();
        input.as_object_mut().unwrap().remove(member);
        assert_eq!(
            AggregateContract::compile(validator(), &bytes(&input)).err(),
            Some(AggregateError::Structure)
        );
    }
}

#[test]
fn aggregate_invalid_children_and_references_fail_explicitly() {
    for href in [
        "#local",
        "https://example.invalid/component",
        "file:///forbidden/component",
    ] {
        let input = record(vec![json!({"name":"linked","href":href})]);
        assert_eq!(validator().validate(Contract::DataRecord, &input), Ok(()));
        assert_eq!(
            AggregateContract::compile(validator(), &input).err(),
            Some(AggregateError::UnsupportedComponent),
            "do not fetch or discard a required reference"
        );
    }
    for kind in ["DataStream", "Unknown"] {
        let input = record(vec![json!({"name":"deferred","type":kind})]);
        assert_eq!(
            AggregateContract::compile(validator(), &input).err(),
            Some(AggregateError::UnsupportedComponent)
        );
    }
    for member in ["value", "quality", "nilValues", "constraint"] {
        let mut input: Value = serde_json::from_slice(&record(vec![count("one")])).unwrap();
        input[member] = json!([]);
        assert_eq!(
            AggregateContract::compile(validator(), &bytes(&input)).err(),
            Some(AggregateError::UnsupportedFeature),
            "known unimplemented aggregate meaning must not vanish as an extension"
        );
    }
    let mut invalid: Value = serde_json::from_slice(RECORD).unwrap();
    invalid["fields"][1]["uom"]["code"] = json!("unknownUnit");
    assert_eq!(
        AggregateContract::compile(validator(), &bytes(&invalid)).err(),
        Some(AggregateError::Range(RangeError::Endpoint(
            ScalarError::Unit(crate::units::UnitError::Invalid)
        )))
    );
    invalid["fields"][1]["uom"]["code"] = json!("cm");
    invalid["fields"][0]["value"] = json!(1.5);
    assert_eq!(
        AggregateContract::compile(validator(), &bytes(&invalid)).err(),
        Some(AggregateError::Structure)
    );
    let count_nil = record(vec![
        json!({"name":"count","type":"CountRange","definition":"urn:example:count","label":"Count","nilValues":[{"reason":"urn:example:nil:missing","value":-999}]}),
    ]);
    assert_eq!(
        AggregateContract::compile(validator(), &count_nil).err(),
        Some(AggregateError::Structure),
        "nesting must not opt into the separate CountRange schema correction"
    );
}

#[test]
fn aggregate_exact_source_and_nil_states() {
    let zero = r#"{"name":"zero","type":"Count","definition":"urn:example:count","label":"Count","nilValues":[{"reason":"urn:example:nil:missing","value":-0}],"value":0.00}"#;
    let band = r#"{"name":"band","type":"QuantityRange","definition":"urn:example:extent","label":"Band","uom":{"code":"cm"},"constraint":{"intervals":[[0,200]]},"nilValues":[{"reason":"urn:example:nil:missing","value":-999.00}],"value":[-999,125.00]}"#;
    let absent = r#"{"name":"absent","type":"Text","definition":"urn:example:text","label":"Text","optional":true}"#;
    let input = format!(
        "{{\n \"type\":\"DataRecord\", \"fields\":[{zero}, {band}, {absent}], \"vendor:note\":\"keep\\u0020escape\"\n}}"
    );
    let contract = compile(input.as_bytes());
    assert_eq!(contract.source(), input.as_bytes());
    for (child, expected) in contract.children().iter().zip([zero, band, absent]) {
        assert_eq!(
            child.source(),
            expected.as_bytes(),
            "retain each exact child slice, not a Value serialization"
        );
    }
    let zero = contract.children()[0].scalar().unwrap();
    let checked = zero.inline_value().unwrap();
    assert_eq!(
        checked.nil_reason.as_deref(),
        Some("urn:example:nil:missing")
    );
    let ScalarValue::Count(value) = checked.value else {
        panic!("Count")
    };
    assert_eq!(value.try_to_i64(), Ok(0));
    assert_eq!(value.number().decimal_lexeme(), Some("0.00"));
    let ScalarValue::Count(sentinel) = &zero.nil_declarations()[0].value else {
        panic!("Count nil")
    };
    assert_eq!(sentinel.number().decimal_lexeme(), Some("-0"));
    let band = contract.children()[1]
        .range()
        .unwrap()
        .inline_value()
        .unwrap();
    assert_eq!(band.order, OrderCheck::NilEndpoint);
    assert_eq!(
        band.endpoints[0].nil_reason.as_deref(),
        Some("urn:example:nil:missing")
    );
    assert!(band.endpoints[1].nil_reason.is_none());
    assert_eq!(
        band.endpoints[1].value,
        ScalarValue::Quantity(finite("125.00")),
        "retain centimetres without conversion"
    );
    let absent = contract.children()[2].scalar().unwrap();
    assert!(absent.component().value().is_none());
    assert!(absent.inline_value().is_none());
    assert_eq!(
        absent.check_value(b"null").err(),
        Some(ScalarError::ValueType)
    );
    assert_eq!(compile(contract.source()).component(), contract.component());
    let mixed = compile(RECORD);
    let nested = mixed.children()[2].aggregate().unwrap();
    assert_eq!(
        nested.children()[1]
            .scalar()
            .unwrap()
            .inline_value()
            .unwrap()
            .value,
        ScalarValue::Text(String::new())
    );
    assert_eq!(
        nested.children()[2]
            .scalar()
            .unwrap()
            .inline_value()
            .unwrap()
            .nil_reason
            .as_deref(),
        Some("urn:example:nil:missing")
    );
}

#[test]
fn aggregate_limits_fail_before_unbounded_traversal() {
    assert_eq!(
        AggregateContract::compile(validator(), &vec![b' '; validation::MAX_BYTES + 1]).err(),
        Some(AggregateError::Syntax(Failure::Size))
    );
    let mut deep = count("leaf");
    for _ in 0..validation::MAX_DEPTH {
        deep = json!({"name":"nested","type":"DataRecord","fields":[deep]});
    }
    assert_eq!(
        AggregateContract::compile(validator(), &bytes(&deep)).err(),
        Some(AggregateError::Syntax(Failure::Depth))
    );
    let wide = record(
        (0..900)
            .map(|index| count(&format!("field{index}")))
            .collect(),
    );
    assert!(wide.len() < validation::MAX_BYTES);
    assert_eq!(
        AggregateContract::compile(validator(), &wide).err(),
        Some(AggregateError::Syntax(Failure::Members))
    );
    let mut long = count("field");
    long["label"] = json!("x".repeat(validation::MAX_STRING_BYTES + 1));
    assert_eq!(
        AggregateContract::compile(validator(), &record(vec![long])).err(),
        Some(AggregateError::Syntax(Failure::String))
    );
    let duplicate_key = br#"{"type":"DataRecord","fields":[],"fields":[]}"#;
    assert_eq!(
        AggregateContract::compile(validator(), duplicate_key).err(),
        Some(AggregateError::Syntax(Failure::DuplicateKey))
    );
}

#[test]
fn aggregate_generated_order_and_identity() {
    // Fixed 64 cases; expected association order and exact values come from
    // bounded integer arithmetic, never a sorted/generated production model.
    for case in 0_u64..64 {
        let mut names = [format!("z{case}"), format!("a{case}"), format!("m{case}")];
        names.rotate_left((case % 3) as usize);
        let fields = names
            .iter()
            .enumerate()
            .map(|(index, name)| {
                let mut field = count(name);
                field["value"] = json!(9_007_199_254_740_993 + case * 3 + index as u64);
                field
            })
            .collect();
        let mut input = record(fields);
        let contract = compile(&input);
        assert_eq!(
            contract
                .children()
                .iter()
                .map(|child| child.name())
                .collect::<Vec<_>>(),
            names.iter().map(String::as_str).collect::<Vec<_>>()
        );
        for (index, child) in contract.children().iter().enumerate() {
            let Some(ScalarValue::Count(value)) = child.scalar().unwrap().component().value()
            else {
                panic!("Count")
            };
            assert_eq!(
                value.try_to_u64(),
                Ok(9_007_199_254_740_993 + case * 3 + index as u64)
            );
        }
        let expected_source = input.clone();
        input.fill(b' ');
        assert_eq!(
            contract.source(),
            expected_source,
            "caller mutation cannot change the captured contract"
        );
        assert_eq!(compile(contract.source()).component(), contract.component());
    }
}
