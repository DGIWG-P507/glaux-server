//! Independently authored Guide 4.3/4.10 fixtures for Glaux's quality JSON
//! interpretation, with SWE 8.2.15's four alternatives as supporting context.
//! These are description/model checks, not payload codecs or conformance claims.
use std::sync::OnceLock;

use glaux_domain::{
    numeric::NumericValue,
    scalar::{ScalarComponent, ScalarValue, TokenConstraint},
};
use serde_json::{Value, json};

use super::{InlineQuality, QualityAvailability, QualityError};
use crate::{
    aggregate::AggregateContract,
    range::{OrderCheck, RangeContract, RangeOptions},
    scalar::{ScalarContract, ScalarError},
    validation::{self, Contract, StructuralValidator},
};

const REASON: &str = "urn:example:nil:missing";

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned schemas compile offline"))
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn subject() -> Value {
    json!({"type":"Quantity","id":"PRESSURE","definition":"urn:example:pressure",
        "label":"Pressure","uom":{"code":"Pa"}})
}

fn text(value: &str) -> Value {
    json!({"type":"Text","definition":"urn:example:operator-note",
        "label":"Operator note","value":value})
}

fn quality_source(entries: Value) -> Vec<u8> {
    let mut source = subject();
    source["quality"] = entries;
    bytes(&source)
}

fn compile(input: &[u8]) -> ScalarContract {
    ScalarContract::compile(validator(), input).expect("valid independent quality fixture")
}

fn finite(token: &str) -> NumericValue {
    NumericValue::Finite(token.parse().expect("independent exact number"))
}

fn quantity_entry(entry: &InlineQuality) -> &ScalarContract {
    let InlineQuality::Quantity(contract) = entry else {
        panic!("Quantity quality assertion")
    };
    contract
}

fn assert_four_facts(contract: &ScalarContract) {
    let ScalarComponent::Quantity { metadata, uom, .. } = contract.component() else {
        panic!("pressure is the assertion subject")
    };
    assert_eq!(metadata.id.as_deref(), Some("PRESSURE"));
    assert_eq!(metadata.definition, "urn:example:pressure");
    assert_eq!(uom.code.as_deref(), Some("Pa"));
    let entries = contract.quality().entries();
    assert_eq!(entries.len(), 4);
    assert!(contract.quality().is_present());
    assert!(entries.iter().all(|entry| entry.availability() == QualityAvailability::Supplied));

    let quantity = quantity_entry(&entries[0]);
    let ScalarComponent::Quantity { metadata, uom, constraint, .. } = quantity.component() else {
        panic!("decimal uncertainty")
    };
    assert_eq!(metadata.definition, "urn:example:standard-uncertainty");
    assert_eq!(uom.code.as_deref(), Some("Pa"));
    assert_eq!(uom.label.as_deref(), Some("pascal"));
    assert_eq!(uom.symbol.as_deref(), Some("Pa"));
    assert_eq!(constraint.as_ref().unwrap().intervals, [[finite("0"), finite("1")]]);
    assert_eq!(quantity.inline_value().unwrap().value, ScalarValue::Quantity(finite("0.125")));

    let InlineQuality::QuantityRange(range) = &entries[1] else {
        panic!("interval uncertainty")
    };
    let ScalarComponent::Quantity { metadata, uom, .. } = &range.component().endpoint else {
        panic!("numeric range meaning")
    };
    assert_eq!(metadata.definition, "urn:example:tolerance");
    assert_eq!(uom.code.as_deref(), Some("Pa"));
    let pair = range.inline_value().unwrap();
    assert_eq!(pair.order, OrderCheck::Established);
    assert_eq!(pair.endpoints[0].value, ScalarValue::Quantity(finite("-0.25")));
    assert_eq!(pair.endpoints[1].value, ScalarValue::Quantity(finite("0.5")));

    let InlineQuality::Category(category) = &entries[2] else {
        panic!("categorical quality")
    };
    let ScalarComponent::Category { metadata, constraint, .. } = category.component() else {
        panic!("categorical meaning")
    };
    assert_eq!(metadata.definition, "urn:example:certification-level");
    assert_eq!(constraint, &Some(TokenConstraint::Values(vec!["GOOD".into(), "POOR".into()])));
    assert_eq!(category.inline_value().unwrap().value, ScalarValue::Category("GOOD".into()));

    let InlineQuality::Text(note) = &entries[3] else {
        panic!("textual quality")
    };
    assert_eq!(note.inline_value().unwrap().value, ScalarValue::Text("Field operator checked".into()));
}

#[test]
fn quality_all_four_types_preserve_independent_facts_after_source_round_trip() {
    let source = quality_source(json!([
        {"type":"Quantity","definition":"urn:example:standard-uncertainty",
         "label":"Standard uncertainty","uom":{"code":"Pa","label":"pascal","symbol":"Pa"},
         "constraint":{"intervals":[[0,1]]},"value":0.125},
        {"type":"QuantityRange","definition":"urn:example:tolerance","label":"Tolerance",
         "uom":{"code":"Pa"},"value":[-0.25,0.5]},
        {"type":"Category","definition":"urn:example:certification-level","label":"Certification",
         "constraint":{"values":["GOOD","POOR"]},"value":"GOOD"},
        {"type":"Text","definition":"urn:example:operator-note","label":"Operator note",
         "value":"Field operator checked"}
    ]));
    let contract = compile(&source);
    assert_eq!(contract.source(), source);
    assert_four_facts(&contract);
    let restored = compile(contract.source());
    assert_four_facts(&restored);
    for (index, expected_type) in ["Quantity", "QuantityRange", "Category", "Text"].iter().enumerate() {
        let original: Value = serde_json::from_slice(contract.quality().entries()[index].source()).unwrap();
        let restored_source = restored.quality().entries()[index].source();
        assert_eq!(original["type"], *expected_type);
        assert_eq!(restored_source, contract.quality().entries()[index].source());
    }
}

#[test]
fn quality_malformed_arrays_are_not_schema_success() {
    for malformed in [json!("GOOD"), Value::Null, json!({}), json!(true), json!(3), json!(["GOOD"]), json!([null]), json!([[]])] {
        let source = quality_source(malformed);
        assert!(validator().validate(Contract::Quantity, &source).is_ok(),
            "the original open schema does not validate its unknown quality member");
        assert_eq!(ScalarContract::compile(validator(), &source).err(), Some(ScalarError::Quality(QualityError::Shape)),
            "malformed quality must fail the separate semantic check");
    }
}

#[test]
fn quality_rejects_unsupported_types_and_reference_entries_without_claiming_resolution() {
    for kind in ["Boolean", "Count", "Time", "CountRange", "CategoryRange", "TimeRange", "DataRecord", "Geometry", "quantity"] {
        let source = quality_source(json!([{"type":kind,"definition":"urn:example:q","label":"Quality"}]));
        assert!(validator().validate(Contract::Quantity, &source).is_ok());
        assert!(ScalarContract::compile(validator(), &source).is_err(), "unsupported quality union member: {kind}");
    }
    for entry in [json!({}), json!({"type":null}), json!({"href":"#PRESS_QC"}), json!({"href":"https://example.test/quality"}), json!({"href":"file:///not-fetched"}), json!({"type":"Text","definition":"urn:example:q","label":"Q","value":"GOOD","href":"#PRESS_QC"})] {
        assert!(ScalarContract::compile(validator(), &quality_source(json!([entry]))).is_err(),
            "reference binding is deferred explicitly, never reported as checked inline quality");
    }
}

#[test]
fn quality_units_constraints_and_supplied_values_are_checked() {
    let invalid = [
        json!({"type":"Quantity","definition":"urn:example:q","label":"Q","uom":{"code":"not-a-unit"},"value":0.5}),
        json!({"type":"Quantity","definition":"urn:example:q","label":"Q","uom":{"code":"m"},"value":"0.5"}),
        json!({"type":"Quantity","definition":"urn:example:q","label":"Q","uom":{"code":"m"},"constraint":{"intervals":[[0,1]]},"value":2}),
        json!({"type":"Quantity","definition":"urn:example:q","uom":{"code":"m"},"value":0.5}),
        json!({"type":"QuantityRange","definition":"urn:example:q","label":"Q","uom":{"code":"m"},"value":[2,1]}),
        json!({"type":"QuantityRange","definition":"urn:example:q","label":"Q","uom":{"code":"m"},"value":[0,1,2]}),
        json!({"type":"QuantityRange","definition":"urn:example:q","label":"Q","uom":{"code":"m"},"value":[0,"1"]}),
        json!({"type":"QuantityRange","definition":"urn:example:q","label":"Q","uom":{"code":"bad-unit"},"value":[0,1]}),
        json!({"type":"Category","definition":"urn:example:q","label":"Q","constraint":{"values":["GOOD"]},"value":"POOR"}),
        json!({"type":"Category","definition":"urn:example:q","label":"Q","constraint":{"values":["GOOD"]},"value":1}),
        json!({"type":"Text","definition":"urn:example:q","label":"Q","constraint":{"pattern":"^[A-Z]+$"},"value":"lower"}),
        json!({"type":"Text","definition":"urn:example:q","label":"Q","value":false}),
        json!({"type":"Text","definition":"urn:example:q","label":"Q","nilValues":[{"value":4,"reason":REASON}],"value":"NA"}),
    ];
    for entry in invalid {
        let source = quality_source(json!([entry]));
        assert!(validator().validate(Contract::Quantity, &source).is_ok());
        assert!(ScalarContract::compile(validator(), &source).is_err(), "quality's own semantics cannot be skipped: {entry}");
    }
}

#[test]
fn quality_missing_nil_and_supplied_are_distinct() {
    let absent = compile(&bytes(&subject()));
    assert!(!absent.quality().is_present());
    assert!(absent.quality().entries().is_empty());
    let empty = compile(&quality_source(json!([])));
    assert!(empty.quality().is_present());
    assert!(empty.quality().entries().is_empty());

    let missing = compile(&quality_source(json!([{"type":"Quantity","definition":"urn:example:uncertainty",
        "label":"Uncertainty","uom":{"code":"Pa"}}])));
    assert_eq!(missing.quality().entries()[0].availability(), QualityAvailability::Missing,
        "description-only quality remains unavailable");
    assert!(quantity_entry(&missing.quality().entries()[0]).inline_value().is_none());
    for entry in [
        json!({"type":"QuantityRange","definition":"urn:example:q","label":"Q","uom":{"code":"m"}}),
        json!({"type":"Category","definition":"urn:example:q","label":"Q","constraint":{"values":["GOOD"]}}),
        json!({"type":"Text","definition":"urn:example:q","label":"Q"}),
    ] {
        let description = compile(&quality_source(json!([entry])));
        assert_eq!(description.quality().entries()[0].availability(), QualityAvailability::Missing);
    }

    let nil = compile(&quality_source(json!([{"type":"Quantity","definition":"urn:example:uncertainty",
        "label":"Uncertainty","uom":{"code":"Pa"},"constraint":{"intervals":[[0,1]]},
        "nilValues":[{"value":-999,"reason":REASON}],"value":-999}])));
    assert_eq!(nil.quality().entries()[0].availability(), QualityAvailability::Nil);
    let checked = quantity_entry(&nil.quality().entries()[0]).inline_value().unwrap();
    assert_eq!(checked.value, ScalarValue::Quantity(finite("-999")));
    assert_eq!(checked.nil_reason.as_deref(), Some(REASON));

    let zero = compile(&quality_source(json!([{"type":"Quantity","definition":"urn:example:uncertainty",
        "label":"Uncertainty","uom":{"code":"Pa"},"value":0}])));
    assert_eq!(zero.quality().entries()[0].availability(), QualityAvailability::Supplied);
    assert_eq!(quantity_entry(&zero.quality().entries()[0]).inline_value().unwrap().value, ScalarValue::Quantity(finite("0")));
    let empty_text = compile(&quality_source(json!([text("")])));
    assert_eq!(empty_text.quality().entries()[0].availability(), QualityAvailability::Supplied);
}

#[test]
fn quality_range_and_token_nils_do_not_become_available_quality() {
    for pair in [json!([-999,1]), json!([0,-999]), json!([-999,-999])] {
        let source = quality_source(json!([{"type":"QuantityRange","definition":"urn:example:bounds",
            "label":"Bounds","uom":{"code":"m"},"nilValues":[{"value":-999,"reason":REASON}],"value":pair}]));
        let contract = compile(&source);
        let entry = &contract.quality().entries()[0];
        assert_eq!(entry.availability(), QualityAvailability::Nil);
        let InlineQuality::QuantityRange(range) = entry else { panic!("range") };
        assert_eq!(range.inline_value().unwrap().order, OrderCheck::NilEndpoint);
        assert!(range.inline_value().unwrap().endpoints.iter().any(|endpoint| endpoint.nil_reason.as_deref() == Some(REASON)));
    }
    for kind in ["Text", "Category"] {
        let contract = compile(&quality_source(json!([{"type":kind,"definition":"urn:example:status","label":"Status",
            "constraint":{"values":["GOOD"]},"nilValues":[{"value":"NA","reason":REASON}],"value":"NA"}])));
        assert_eq!(contract.quality().entries()[0].availability(), QualityAvailability::Nil);
    }
}

#[test]
fn quality_aggregate_paths_keep_assertions_on_their_distinct_subjects() {
    let mut first = subject();
    first["name"] = json!("left");
    first["quality"] = json!([text("left source only")]);
    let mut second = subject();
    second["id"] = json!("TEMPERATURE");
    second["name"] = json!("reading");
    second["definition"] = json!("urn:example:temperature");
    second["uom"] = json!({"code":"K"});
    second["quality"] = json!([text("nested source only")]);
    let mut source = bytes(&json!({"type":"DataRecord","fields":[first,
        {"name":"right","type":"DataRecord","fields":[second]}]}));
    for _ in 0..2 {
        let contract = AggregateContract::compile(validator(), &source).unwrap();
        assert_eq!(contract.source(), source);
        assert_eq!(contract.children()[0].name(), "left");
        assert_eq!(contract.children()[1].name(), "right");
        let left = contract.children()[0].scalar().unwrap();
        let right = contract.children()[1].aggregate().unwrap();
        assert_eq!(right.children()[0].name(), "reading");
        let right = right.children()[0].scalar().unwrap();
        for (owner, id, expected) in [(left,"PRESSURE","left source only"),(right,"TEMPERATURE","nested source only")] {
            let ScalarComponent::Quantity { metadata, .. } = owner.component() else { panic!("subject") };
            assert_eq!(metadata.id.as_deref(), Some(id));
            let InlineQuality::Text(note) = &owner.quality().entries()[0] else { panic!("text") };
            assert_eq!(note.inline_value().unwrap().value, ScalarValue::Text(expected.into()));
        }
        source = contract.source().to_vec();
    }
}

#[test]
fn quality_range_owner_and_nested_assertions_are_preserved() {
    let source = bytes(&json!({"type":"QuantityRange","id":"SPAN","definition":"urn:example:span","label":"Span",
        "uom":{"code":"m"},"value":[1,2],"quality":[{"type":"Quantity","definition":"urn:example:uncertainty",
        "label":"Uncertainty","uom":{"code":"m"},"value":0.1,"quality":[text("assessed by calibration")]}]}));
    let range = RangeContract::compile(validator(), &source, RangeOptions::default()).unwrap();
    assert_eq!(range.source(), source);
    let ScalarComponent::Quantity { metadata, .. } = &range.component().endpoint else { panic!("endpoint") };
    assert_eq!(metadata.id.as_deref(), Some("SPAN"));
    assert_eq!(range.quality().entries().len(), 1);
    let outer = quantity_entry(&range.quality().entries()[0]);
    assert_eq!(outer.inline_value().unwrap().value, ScalarValue::Quantity(finite("0.1")));
    let InlineQuality::Text(inner) = &outer.quality().entries()[0] else { panic!("nested text") };
    assert_eq!(inner.inline_value().unwrap().value, ScalarValue::Text("assessed by calibration".into()));
    let restored = RangeContract::compile(validator(), range.source(), RangeOptions::default()).unwrap();
    assert_eq!(restored.quality().entries()[0].source(), range.quality().entries()[0].source());
}

#[test]
fn quality_exact_source_retains_lexemes_and_supplied_context_without_normalization() {
    let source = br##"{
      "type":"Quantity","definition":"urn:example:reading","label":"Reading","uom":{"code":"m"},
      "quality":[{"type":"Quantity","definition":"urn:example:error","label":"Error",
      "uom":{"code":"m"},"value":-0.00e+2,
      "method":{"href":"urn:example:calibration:revision:7","version":"7"},
      "evaluator":"urn:example:team:A","coverage":{"conditions":"warm\\dry","from":"2026-01-01"}},
      {"type":"Text","definition":"urn:example:comment","label":"Comment","value":"checked \u00e9"}]
    }"##;
    let contract = compile(source);
    assert_eq!(contract.source(), source);
    let entry = &contract.quality().entries()[0];
    let ScalarValue::Quantity(NumericValue::Finite(number)) = quantity_entry(entry).inline_value().unwrap().value else { panic!("finite quality") };
    assert_eq!(number.decimal_lexeme(), Some("-0.00e+2"));
    assert_eq!(number, "0".parse().unwrap());
    let original: Value = serde_json::from_slice(entry.source()).unwrap();
    assert_eq!(original["method"]["href"], "urn:example:calibration:revision:7");
    assert_eq!(original["method"]["version"], "7");
    assert_eq!(original["evaluator"], "urn:example:team:A");
    assert_eq!(original["coverage"]["conditions"], "warm\\dry");
    let InlineQuality::Text(note) = &contract.quality().entries()[1] else { panic!("text") };
    assert_eq!(note.inline_value().unwrap().value, ScalarValue::Text("checked é".into()));
    let restored = compile(contract.source());
    assert_eq!(restored.quality().entries()[0].source(), entry.source());
}

#[test]
fn quality_does_not_invent_a_universal_probability_or_rewrite_units() {
    for (code, token) in [("%", "95"), ("m", "-2"), ("1", "2.5")] {
        let entry = format!(r#"{{"type":"Quantity","definition":"urn:example:metric","label":"Metric","uom":{{"code":"{code}"}},"value":{token}}}"#);
        let source = format!(r#"{{"type":"Text","definition":"urn:example:reading","label":"Reading","quality":[{entry}]}}"#);
        let contract = compile(source.as_bytes());
        let quality = quantity_entry(&contract.quality().entries()[0]);
        let ScalarComponent::Quantity { uom, .. } = quality.component() else { panic!("quantity") };
        assert_eq!(uom.code.as_deref(), Some(code));
        assert_eq!(quality.inline_value().unwrap().value, ScalarValue::Quantity(finite(token)));
    }
}

#[test]
fn quality_static_array_element_assertion_is_not_an_inline_payload_value() {
    let mut element = subject();
    element["name"] = json!("reading");
    element["quality"] = json!([text("fixed calibration uncertainty")]);
    let source = bytes(&json!({"type":"DataArray","elementCount":{"value":2},"elementType":element}));
    let contract = AggregateContract::compile(validator(), &source).unwrap();
    let element = contract.children()[0].scalar().unwrap();
    assert!(element.inline_value().is_none(), "static quality is not an element payload value");
    let InlineQuality::Text(note) = &element.quality().entries()[0] else { panic!("static note") };
    assert_eq!(note.inline_value().unwrap().value, ScalarValue::Text("fixed calibration uncertainty".into()));
    assert_eq!(contract.source(), source);
}

#[test]
fn quality_bounded_generated_nested_values_are_checked_not_only_outer_entries() {
    for depth in 0..8 {
        let mut entry = text("leaf");
        for _ in 0..depth {
            let mut parent = text("parent");
            parent["quality"] = json!([entry]);
            entry = parent;
        }
        let contract = compile(&quality_source(json!([entry])));
        let mut current = &contract.quality().entries()[0];
        for _ in 0..depth {
            let InlineQuality::Text(text) = current else { panic!("text parent") };
            assert_eq!(text.inline_value().unwrap().value, ScalarValue::Text("parent".into()));
            current = &text.quality().entries()[0];
        }
        let InlineQuality::Text(leaf) = current else { panic!("text leaf") };
        assert_eq!(leaf.inline_value().unwrap().value, ScalarValue::Text("leaf".into()));

        let mut invalid = text("bad");
        invalid["constraint"] = json!({"values":["GOOD"]});
        for _ in 0..depth {
            let mut parent = text("parent");
            parent["quality"] = json!([invalid]);
            invalid = parent;
        }
        assert!(ScalarContract::compile(validator(), &quality_source(json!([invalid]))).is_err(),
            "invalid deepest quality cannot hide behind valid outer assertions at depth {depth}");
    }
}

#[test]
fn quality_uses_whole_document_depth_size_and_node_budgets() {
    let mut nested = text("leaf");
    for _ in 0..validation::MAX_DEPTH {
        let mut parent = text("parent");
        parent["quality"] = json!([nested]);
        nested = parent;
    }
    let too_deep = quality_source(json!([nested]));
    assert!(validation::parse(&too_deep).is_err());
    assert!(ScalarContract::compile(validator(), &too_deep).is_err());

    let mut wide_entry = text("ordinary");
    wide_entry["extension"] = json!([1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16]);
    let too_many_nodes = quality_source(json!(vec![wide_entry; 256]));
    assert!(too_many_nodes.len() < validation::MAX_BYTES);
    assert!(validation::parse(&too_many_nodes).is_err());
    assert!(ScalarContract::compile(validator(), &too_many_nodes).is_err());

    let too_large = quality_source(json!(vec![text(&"x".repeat(1024)); 256]));
    assert!(too_large.len() > validation::MAX_BYTES);
    assert!(ScalarContract::compile(validator(), &too_large).is_err());
}
