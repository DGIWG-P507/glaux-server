//! Independent SWE block-description cases, not array payload or graph tests.
use super::{ArrayContract, ArrayOptions, CountReferenceSchema, SourceValidation};
use crate::{
    aggregate::{AggregateContract, AggregateError},
    choice::{
        CheckedComponentValue, ChoiceContract, ComponentErrorKind, ComponentValue, NamedValue,
    },
    scalar::ScalarError,
    validation::{self, Contract, Failure, StructuralValidator},
};
use glaux_domain::{
    aggregate::AggregateComponent,
    array::{ArrayKind, ElementCount},
    numeric::NumericValue,
    scalar::{CalendarTime, ScalarComponent, ScalarValue, TimeFrame, UnsupportedTimeConversion},
};
use serde_json::{Value, json};
use std::sync::OnceLock;

const FIXED: &[u8] = include_bytes!("../../fixtures/array/fixed-nested.json");
const REFERENCE: &[u8] = include_bytes!("../../fixtures/array/count-reference.json");
const MATRIX: &[u8] = include_bytes!("../../fixtures/array/matrix-time.json");

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn compile(input: &[u8]) -> ArrayContract {
    ArrayContract::compile(validator(), input, ArrayOptions::default())
        .expect("valid independent array descriptor")
}

fn correction() -> ArrayOptions {
    ArrayOptions {
        count_reference_schema: CountReferenceSchema::DisjointReferenceCorrection,
    }
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn count(name: &str) -> Value {
    json!({"name":name,"type":"Count","definition":"urn:example:count","label":"Count"})
}

fn array(count: Value, element: Value) -> Value {
    json!({"type":"DataArray","elementCount":count,"elementType":element})
}

fn dimension_lexemes(contract: &ArrayContract) -> Vec<Option<&str>> {
    contract
        .dimensions()
        .into_iter()
        .map(|dimension| match dimension {
            ElementCount::Inline(count) => count
                .value
                .as_ref()
                .and_then(|value| value.number().decimal_lexeme()),
            ElementCount::Reference(_) => None,
        })
        .collect()
}

fn error(input: &Value) -> ComponentErrorKind {
    ArrayContract::compile(validator(), &bytes(input), ArrayOptions::default())
        .err()
        .expect("invalid descriptor")
        .kind
}

#[test]
fn array_fixed_dimensions_preserve_exact_order() {
    let contract = compile(FIXED);
    assert_eq!(
        dimension_lexemes(&contract),
        [Some("9007199254740993"), Some("7")]
    );
    assert_eq!(validator().validate(Contract::DataArray, FIXED), Ok(()));
    assert_eq!(contract.source_validation(), SourceValidation::Original);
    let AggregateComponent::Array {
        kind,
        metadata,
        element_count,
        element_type,
        ..
    } = contract.component()
    else {
        panic!("array")
    };
    assert_eq!(*kind, ArrayKind::DataArray);
    assert_eq!(metadata.id.as_deref(), Some("image"));
    assert_eq!(metadata.definition, None);
    assert_eq!(metadata.label.as_deref(), Some(" Ordered samples "));
    assert_eq!(metadata.optional, Some(true));
    assert_eq!(metadata.updatable, Some(false));
    assert_eq!(element_type.name, "zRow");
    let ElementCount::Inline(count) = element_count else {
        panic!("fixed inline count")
    };
    assert_eq!(
        count.value.as_ref().unwrap().try_to_u64(),
        Ok(9_007_199_254_740_993)
    );
    assert_eq!(
        count.metadata.definition.as_deref(),
        Some("urn:example:rows")
    );
    assert_eq!(
        contract.element_count_source(),
        br#"{ "definition": "urn:example:rows", "value": 9007199254740993 }"#
    );
    let row = contract.element().aggregate().unwrap();
    assert_eq!(row.children()[0].name(), "aSample");
    let sample = row.children()[0].scalar().unwrap();
    let ScalarComponent::Quantity {
        uom, constraint, ..
    } = sample.component()
    else {
        panic!("quantity descriptor")
    };
    assert_eq!(uom.code.as_deref(), Some("cm"));
    let NumericValue::Finite(low) = &constraint.as_ref().unwrap().intervals[0][0] else {
        panic!("finite lower bound")
    };
    assert_eq!(low.decimal_lexeme(), Some("0.1234567890123456789"));
    assert_eq!(
        sample.check_value(b"0.1234567890123456788").err(),
        Some(ScalarError::ConstraintViolation)
    );
    assert_eq!(
        sample.check_value(b"-999").unwrap().nil_reason.as_deref(),
        Some("urn:example:nil:missing")
    );
    let ScalarValue::Quantity(NumericValue::Finite(nil)) = &sample.nil_declarations()[0].value
    else {
        panic!("exact declared nil sentinel")
    };
    assert_eq!(nil.decimal_lexeme(), Some("-999.00"));
    assert!(sample.component().value().is_none());
    assert_eq!(contract.source(), FIXED);
    assert_eq!(compile(contract.source()).component(), contract.component());
}

#[test]
fn array_variable_count_and_original_requiredness() {
    for descriptor in [json!({}), json!({"label":"Implicit size"})] {
        let input = bytes(&array(descriptor, count("element")));
        let contract = compile(&input);
        assert_eq!(dimension_lexemes(&contract), [None]);
        let ElementCount::Inline(count) = contract.dimensions()[0] else {
            panic!("inline variable count")
        };
        assert!(count.value.is_none());
        assert!(count.metadata.definition.is_none());
        assert_eq!(contract.source(), input);
    }
    let mut missing = array(json!({}), count("element"));
    missing.as_object_mut().unwrap().remove("elementCount");
    let input = bytes(&missing);
    assert_eq!(validator().validate(Contract::DataArray, &input), Ok(()));
    assert_eq!(
        error(&missing),
        ComponentErrorKind::Compile(AggregateError::MissingElementCount)
    );
    for fixed in [0, -1] {
        assert_eq!(
            error(&array(json!({"value":fixed}), count("element"))),
            ComponentErrorKind::Compile(AggregateError::ElementCount)
        );
    }
    let constrained = compile(&bytes(&array(
        json!({"constraint":{"intervals":[[-1,9]]}}),
        count("element"),
    )));
    assert_eq!(dimension_lexemes(&constrained), [None]);
}

#[test]
fn array_count_reference_correction_preserves_original_verdict() {
    assert_eq!(
        validator().validate(Contract::DataArray, REFERENCE),
        Err(Failure::Structure)
    );
    assert_eq!(
        ArrayContract::compile(validator(), REFERENCE, ArrayOptions::default())
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::Structure)
    );
    let contract = ArrayContract::compile(validator(), REFERENCE, correction()).unwrap();
    assert_eq!(
        contract.source_validation(),
        SourceValidation::CountReferenceCorrection
    );
    let ElementCount::Reference(reference) = contract.dimensions()[0] else {
        panic!("unresolved count reference")
    };
    assert_eq!(
        reference.href,
        "https://example.invalid/description#sampleCount"
    );
    assert_eq!(reference.role.as_deref(), Some("urn:example:count"));
    assert_eq!(reference.arcrole.as_deref(), Some("urn:example:has-count"));
    assert_eq!(reference.title.as_deref(), Some(" Unresolved count "));
    assert_eq!(contract.source(), REFERENCE);
    assert_eq!(
        ArrayContract::compile(validator(), contract.source(), correction())
            .unwrap()
            .component(),
        contract.component()
    );
    assert_eq!(
        validator().validate(Contract::DataArray, contract.source()),
        Err(Failure::Structure),
        "qualified support must not rewrite the original-schema verdict"
    );
    assert_eq!(
        ArrayContract::compile(validator(), FIXED, correction())
            .unwrap()
            .source_validation(),
        SourceValidation::Original
    );
    for href in ["#unresolved", "other.json#count", "urn:example:count"] {
        let input = bytes(&array(json!({"href":href}), count("element")));
        let contract = ArrayContract::compile(validator(), &input, correction()).unwrap();
        let ElementCount::Reference(reference) = contract.dimensions()[0] else {
            panic!("reference")
        };
        assert_eq!(reference.href, href);
    }
    for descriptor in [
        json!({"href":""}),
        json!({"href":"#"}),
        json!({"href":"#n","value":3}),
        json!({"href":"#n","constraint":{"values":[3]}}),
    ] {
        let input = bytes(&array(descriptor, count("element")));
        assert_eq!(
            ArrayContract::compile(validator(), &input, correction())
                .err()
                .unwrap()
                .kind,
            ComponentErrorKind::Compile(AggregateError::CountReference)
        );
    }
    let invalid_uri = bytes(&array(json!({"href":"not a URI"}), count("element")));
    // The original oneOf admits this through its open inline-count branch,
    // even though the association branch rejects the URI. Semantic count
    // validation must still reject it; an original structural pass is not enough.
    assert_eq!(
        validator().validate(Contract::DataArray, &invalid_uri),
        Ok(())
    );
    assert_eq!(
        ArrayContract::compile(validator(), &invalid_uri, correction())
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::CountReference)
    );
    let mut invalid: Value = serde_json::from_slice(REFERENCE).unwrap();
    invalid["elementType"]
        .as_object_mut()
        .unwrap()
        .remove("label");
    let failure = ArrayContract::compile(validator(), &bytes(&invalid), correction())
        .err()
        .unwrap();
    assert_eq!(
        failure.kind,
        ComponentErrorKind::Compile(AggregateError::Scalar(ScalarError::Structure))
    );
    assert_eq!(failure.path, [0]);
    let original: Value = serde_json::from_str(include_str!(
        "../../corpus/originals/csapi/swecommon/schemas/json/DataArray.json"
    ))
    .unwrap();
    let mut adapted = original.clone();
    assert_eq!(super::correct_count_schema(&mut adapted), Ok(()));
    assert_ne!(adapted, original);
    assert_eq!(
        super::correct_count_schema(&mut adapted),
        Err(AggregateError::AdaptationSourceChanged)
    );
}

#[test]
fn array_counts_reject_inconsistent_and_invalid_values() {
    for token in ["18446744073709551616", "1e400", "7.000", "70e-1"] {
        let input = format!(
            r#"{{"type":"DataArray","elementCount":{{"value":{token}}},"elementType":{{"name":"n","type":"Count","definition":"urn:example:n","label":"N"}}}}"#
        );
        let contract = compile(input.as_bytes());
        assert_eq!(dimension_lexemes(&contract), [Some(token)]);
        assert_eq!(contract.source(), input.as_bytes());
    }
    for descriptor in [
        json!(null),
        json!({"value":null}),
        json!({"value":"3"}),
        json!({"value":true}),
        json!({"value":1.5}),
        json!({"value":"NaN"}),
        json!({"type":"Quantity","value":3}),
        json!({"constraint":{"values":[1.5]}}),
        json!({"constraint":{"intervals":[[3,1]]}}),
        json!({"constraint":{"values":[3],"significantFigures":1}}),
    ] {
        assert!(
            ArrayContract::compile(
                validator(),
                &bytes(&array(descriptor, count("element"))),
                ArrayOptions::default()
            )
            .is_err()
        );
    }
    let constraint = json!({"values":[2],"intervals":[[5,7]]});
    for value in [2, 5, 7] {
        let contract = compile(&bytes(&array(
            json!({"constraint":constraint,"value":value}),
            count("element"),
        )));
        let expected = value.to_string();
        assert_eq!(dimension_lexemes(&contract), [Some(expected.as_str())]);
    }
    for value in [1, 3, 4, 8] {
        assert_eq!(
            error(&array(
                json!({"constraint":constraint,"value":value}),
                count("element")
            )),
            ComponentErrorKind::Compile(AggregateError::Scalar(ScalarError::ConstraintViolation))
        );
    }
}

#[test]
fn array_matrix_members_frames_and_inheritance() {
    let contract = compile(MATRIX);
    assert_eq!(validator().validate(Contract::Matrix, MATRIX), Ok(()));
    assert_eq!(dimension_lexemes(&contract), [Some("3"), None]);
    let AggregateComponent::Array {
        kind,
        reference_frame,
        local_frame,
        ..
    } = contract.component()
    else {
        panic!("matrix")
    };
    assert_eq!(*kind, ArrayKind::Matrix);
    assert_eq!(
        reference_frame.as_deref(),
        Some("urn:example:unknown-time-frame")
    );
    assert_eq!(local_frame.as_deref(), Some("#clock-array"));
    let inner = contract.element().aggregate().unwrap();
    let time = inner.children()[0].scalar().unwrap();
    let ScalarComponent::Time(component) = time.component() else {
        panic!("Time matrix element")
    };
    assert_eq!(component.metadata.reference_frame, None);
    assert_eq!(component.metadata.axis_id, None);
    assert_eq!(
        component.reference.frame,
        TimeFrame::Declared("urn:example:unknown-time-frame".into())
    );
    assert_eq!(
        component.reference.origin,
        Some(CalendarTime::Unresolved("2000-01-01T00:00:00Z".into()))
    );
    let checked = time
        .check_value(br#""2000-01-01T00:00:00.1234567890123456789Z""#)
        .unwrap();
    let ScalarValue::Time(checked) = checked.value else {
        panic!("bound Time")
    };
    assert_eq!(checked.utc_instant().err(), Some(UnsupportedTimeConversion));
    let mut explicit: Value = serde_json::from_slice(MATRIX).unwrap();
    explicit["elementType"]["elementType"]["referenceFrame"] = json!("urn:example:explicit");
    let explicit = compile(&bytes(&explicit));
    let time = explicit.element().aggregate().unwrap().children()[0]
        .scalar()
        .unwrap();
    let ScalarComponent::Time(component) = time.component() else {
        panic!("explicit Time frame")
    };
    assert_eq!(
        component.metadata.reference_frame.as_deref(),
        Some("urn:example:explicit")
    );
    assert_eq!(
        component.reference.frame,
        TimeFrame::Declared("urn:example:explicit".into())
    );
    for element in [
        count("number"),
        json!({"name":"length","type":"Quantity","definition":"urn:example:length","label":"Length","uom":{"code":"cm"}}),
    ] {
        let mut input = array(json!({"value":1}), element);
        input["type"] = json!("Matrix");
        let contract = compile(&bytes(&input));
        let AggregateComponent::Array {
            reference_frame,
            local_frame,
            ..
        } = contract.component()
        else {
            panic!("matrix without frame")
        };
        assert_eq!(reference_frame, &None);
        assert_eq!(local_frame, &None);
    }
    for kind in [
        "Boolean",
        "Text",
        "Category",
        "CountRange",
        "DataRecord",
        "Vector",
        "DataChoice",
        "DataArray",
    ] {
        let mut input = array(json!({"value":1}), count("element"));
        input["type"] = json!("Matrix");
        input["elementType"]["type"] = json!(kind);
        assert_eq!(
            error(&input),
            ComponentErrorKind::Compile(AggregateError::MatrixElement)
        );
    }
    let mut invalid: Value = serde_json::from_slice(MATRIX).unwrap();
    invalid["referenceFrame"] = json!("not a URI");
    assert!(
        ArrayContract::compile(validator(), &bytes(&invalid), ArrayOptions::default()).is_err()
    );
}

#[test]
fn array_element_descriptors_reject_inline_payloads() {
    let mut direct = count("element");
    direct["value"] = json!(3);
    let input = array(json!({"value":2}), direct);
    let failure = ArrayContract::compile(validator(), &bytes(&input), ArrayOptions::default())
        .err()
        .unwrap();
    assert_eq!(
        failure.kind,
        ComponentErrorKind::Compile(AggregateError::InlineElementValue)
    );
    let nested = json!({"name":"record","type":"DataRecord","fields":[{"name":"flag","type":"Boolean","definition":"urn:example:flag","label":"Flag","value":false}]});
    assert_eq!(
        error(&array(json!({"value":2}), nested)),
        ComponentErrorKind::Compile(AggregateError::InlineElementValue)
    );
    for member in ["values", "encoding"] {
        let mut input = array(json!({"value":2}), count("element"));
        input[member] = if member == "values" {
            json!([1, 2])
        } else {
            json!({"type":"JSONEncoding"})
        };
        assert_eq!(
            error(&input),
            ComponentErrorKind::Compile(AggregateError::UnsupportedFeature)
        );
    }
    let elements = [
        json!({"name":"flag","type":"Boolean","definition":"urn:example:flag","label":"Flag"}),
        json!({"name":"text","type":"Text","definition":"urn:example:text","label":"Text"}),
        json!({"name":"class","type":"Category","definition":"urn:example:class","label":"Class","constraint":{"values":["z","a"]}}),
        json!({"name":"band","type":"CountRange","definition":"urn:example:band","label":"Band"}),
        json!({"name":"record","type":"DataRecord","fields":[count("n")]}),
        json!({"name":"vector","type":"Vector","definition":"urn:example:vector","label":"Vector","referenceFrame":"urn:example:frame","coordinates":[{"name":"n","type":"Count","definition":"urn:example:n","label":"N","axisID":"X"}]}),
        json!({"name":"choice","type":"DataChoice","items":[count("z"),count("a")]}),
    ];
    for element in elements {
        let name = element["name"].as_str().unwrap().to_owned();
        let input = bytes(&array(json!({}), element));
        assert_eq!(compile(&input).element().name(), name);
    }
    let mut extension = count("element");
    extension["fields"] = json!([{"id":"fake","type":"Text","value":"extension only"}]);
    let input = bytes(&array(json!({}), extension));
    assert_eq!(compile(&input).source(), input);
    let linked = array(json!({}), json!({"name":"element","href":"#external"}));
    assert_eq!(
        error(&linked),
        ComponentErrorKind::Compile(AggregateError::UnsupportedComponent)
    );
}

#[test]
fn array_nested_record_choice_and_source_retention() {
    let mut referenced: Value = serde_json::from_slice(REFERENCE).unwrap();
    referenced["name"] = json!("samples");
    referenced["elementCount"]["href"] = json!("#n");
    let mut size = count("size");
    size["id"] = json!("n");
    size["value"] = json!(5);
    let input = bytes(&json!({"type":"DataRecord","fields":[size,referenced]}));
    assert_eq!(
        validator().validate(Contract::DataRecord, &input),
        Err(Failure::Structure)
    );
    let record =
        AggregateContract::compile_with_options(validator(), &input, correction()).unwrap();
    assert_eq!(
        record.source_validation(),
        SourceValidation::CountReferenceCorrection
    );
    assert_eq!(record.children()[0].name(), "size");
    assert_eq!(record.children()[1].name(), "samples");
    let child = record.children()[1].aggregate().unwrap();
    let AggregateComponent::Array { element_count, .. } = child.component() else {
        panic!("nested array")
    };
    let ElementCount::Reference(reference) = element_count else {
        panic!("preserved local reference")
    };
    assert_eq!(reference.href, "#n");
    assert_eq!(record.source(), input);
    assert_eq!(
        AggregateContract::compile_with_options(validator(), record.source(), correction())
            .unwrap()
            .component(),
        record.component()
    );
    let mut wrong: Value = serde_json::from_slice(&input).unwrap();
    wrong["fields"][0]["type"] = json!("Text");
    wrong["fields"][0].as_object_mut().unwrap().remove("value");
    assert_eq!(
        AggregateContract::compile_with_options(validator(), &bytes(&wrong), correction()).err(),
        Some(AggregateError::CountReference)
    );
    let mut extension: Value = serde_json::from_slice(&input).unwrap();
    extension["fields"][0]["fields"] = json!([{"id":"shadow","type":"Text"}]);
    extension["fields"][1]["elementCount"]["href"] = json!("#shadow");
    let extension =
        AggregateContract::compile_with_options(validator(), &bytes(&extension), correction())
            .unwrap();
    let AggregateComponent::Array { element_count, .. } =
        extension.children()[1].aggregate().unwrap().component()
    else {
        panic!("array with unresolved extension-like ID")
    };
    let ElementCount::Reference(reference) = element_count else {
        panic!("reference")
    };
    assert_eq!(reference.href, "#shadow");
    let mut linked_arm: Value = serde_json::from_slice(REFERENCE).unwrap();
    linked_arm["name"] = json!("linked");
    let linked_choice = bytes(&json!({"type":"DataChoice","items":[linked_arm,count("n")]}));
    assert_eq!(
        validator().validate(Contract::DataChoice, &linked_choice),
        Err(Failure::Structure)
    );
    let linked_choice =
        ChoiceContract::compile_with_options(validator(), &linked_choice, correction()).unwrap();
    assert_eq!(
        linked_choice.alternatives()[0]
            .aggregate()
            .unwrap()
            .source_validation(),
        SourceValidation::CountReferenceCorrection
    );
    let mut invalid_wrapper: Value = serde_json::from_slice(linked_choice.source()).unwrap();
    invalid_wrapper["label"] = json!("");
    let invalid_wrapper = bytes(&invalid_wrapper);
    assert_eq!(
        validator().validate(Contract::DataChoice, &invalid_wrapper),
        Err(Failure::Structure)
    );
    assert_eq!(
        ChoiceContract::compile_with_options(validator(), &invalid_wrapper, correction())
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::Structure),
        "the count-reference correction must not bypass enclosing metadata"
    );
    let mut array_arm: Value = serde_json::from_slice(FIXED).unwrap();
    array_arm["name"] = json!("array");
    let input = bytes(&json!({"type":"DataChoice","items":[array_arm,count("ordinary")]}));
    let choice = ChoiceContract::compile(validator(), &input).unwrap();
    let checked = choice
        .check_value(&[NamedValue {
            name: "ordinary",
            value: ComponentValue::ScalarJson(b"7"),
        }])
        .unwrap();
    assert_eq!(checked.name, "ordinary");
    let CheckedComponentValue::Scalar(value) = checked.value else {
        panic!("ordinary scalar remains usable")
    };
    assert_eq!(value.value, ScalarValue::Count("7".parse().unwrap()));
    let failure = choice
        .check_value(&[NamedValue {
            name: "array",
            value: ComponentValue::ScalarJson(b"[]"),
        }])
        .err()
        .unwrap();
    assert_eq!(failure.kind, ComponentErrorKind::UnsupportedValue);
    assert_eq!(failure.path, [0]);
}

#[test]
fn array_descriptor_limits_reject_excessive_shape() {
    let oversized = vec![b' '; validation::MAX_BYTES + 1];
    assert_eq!(
        ArrayContract::compile(validator(), &oversized, ArrayOptions::default())
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::Syntax(Failure::Size))
    );
    let mut nested = count("leaf");
    for _ in 0..validation::MAX_DEPTH {
        nested = array(json!({"value":1}), nested);
        nested["name"] = json!("dimension");
    }
    let failure = ArrayContract::compile(validator(), &bytes(&nested), ArrayOptions::default())
        .err()
        .unwrap();
    assert_eq!(
        failure.kind,
        ComponentErrorKind::Compile(AggregateError::Syntax(Failure::Depth))
    );
    assert!(failure.path.len() <= validation::MAX_DEPTH);
    let duplicate = br#"{"type":"DataArray","elementCount":{},"elementCount":{},"elementType":{}}"#;
    assert_eq!(
        ArrayContract::compile(validator(), duplicate, ArrayOptions::default())
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::Syntax(Failure::DuplicateKey))
    );
    let mut excessive = array(json!({}), count("element"));
    excessive["description"] = json!("x".repeat(validation::MAX_STRING_BYTES + 1));
    assert_eq!(
        error(&excessive),
        ComponentErrorKind::Compile(AggregateError::Syntax(Failure::String))
    );
}

#[test]
fn array_generated_dimension_counts_and_lexemes() {
    // Fixed 64 cases, one to four dimensions. Expectations use construction
    // order and exact integer lexemes, never the implementation's dimension list.
    for case in 0_u64..64 {
        let depth = (case % 4 + 1) as usize;
        let expected = (0..depth)
            .map(|axis| format!("{}.0", 9_007_199_254_740_993 + case * 4 + axis as u64))
            .collect::<Vec<_>>();
        let mut element =
            r#"{"name":"leaf","type":"Count","definition":"urn:example:n","label":"N"}"#.to_owned();
        for axis in (0..depth).rev() {
            element = format!(
                r#"{{"name":"axis{axis}","type":"DataArray","elementCount":{{"value":{}}},"elementType":{element}}}"#,
                expected[axis]
            );
        }
        let contract = compile(element.as_bytes());
        assert_eq!(
            dimension_lexemes(&contract),
            expected
                .iter()
                .map(|value| Some(value.as_str()))
                .collect::<Vec<_>>()
        );
        assert_eq!(contract.source(), element.as_bytes());
        let mut invalid: Value = serde_json::from_str(&element).unwrap();
        let mut inner = &mut invalid;
        for _ in 1..depth {
            inner = &mut inner["elementType"];
        }
        inner["elementCount"]["constraint"] = json!({"values":[1]});
        let failure =
            ArrayContract::compile(validator(), &bytes(&invalid), ArrayOptions::default())
                .err()
                .unwrap();
        assert_eq!(
            failure.kind,
            ComponentErrorKind::Compile(AggregateError::Scalar(ScalarError::ConstraintViolation))
        );
        assert_eq!(failure.path, vec![0; depth - 1]);
    }
}
