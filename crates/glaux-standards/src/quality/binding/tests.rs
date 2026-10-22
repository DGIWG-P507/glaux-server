//! Guide 4.3's local quality interpretation, using independently assigned
//! record values and occurrence paths. These are model tests, not wire codecs.
use std::sync::OnceLock;

use glaux_domain::{numeric::NumericValue, scalar::ScalarValue};
use serde_json::{Value, json};

use super::{QualityBindingError, QualityReferencePlan, QualityReferenceValue};
use crate::{
    array::ArrayOptions,
    choice::{ComponentValue, NamedValue},
    quality::{InlineQuality, QualityAvailability},
    references::{ReferenceError, ReferenceTarget},
    scalar::ScalarContract,
    validation::{self, StructuralValidator},
};

const NIL_REASON: &str = "urn:example:nil:unavailable";

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("offline pinned schemas"))
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn compile(source: &Value) -> QualityReferencePlan {
    QualityReferencePlan::compile(validator(), &bytes(source), ArrayOptions::default())
        .expect("independently authored quality contract")
}

fn field(name: &str, id: &str, kind: &str) -> Value {
    let mut value = json!({"name":name,"id":id,"type":kind,
        "definition":format!("urn:example:{name}"),"label":id});
    if matches!(kind, "Quantity" | "QuantityRange") {
        value["uom"] = json!({"code":"m"});
    }
    if kind == "Category" {
        // A closed, fixture-specific vocabulary; no remote vocabulary lookup.
        value["constraint"] = json!({"values":["GOOD","POOR","LEFT","RIGHT","FIRST","SECOND","ONE","TWO"]});
    }
    value
}

fn subject(hrefs: &[&str]) -> Value {
    let mut value = field("reading", "READING", "Quantity");
    value["quality"] = json!(hrefs.iter().map(|href| json!({"href":href})).collect::<Vec<_>>());
    value
}

fn record(fields: Vec<Value>) -> Value {
    json!({"type":"DataRecord","fields":fields})
}

fn named<'a>(name: &'a str, value: ComponentValue<'a>) -> NamedValue<'a> {
    NamedValue { name, value }
}

fn finite(value: &str) -> NumericValue {
    NumericValue::Finite(value.parse().expect("independent exact decimal"))
}

fn component(plan: &QualityReferencePlan, id: &str) -> usize {
    plan.graph()
        .components()
        .iter()
        .position(|node| node.id.as_deref() == Some(id))
        .expect("fixture's exact component ID")
}

fn scalar(value: &QualityReferenceValue) -> &crate::scalar::CheckedScalarValue {
    let QualityReferenceValue::Scalar(value) = value else {
        panic!("expected a checked scalar, not an unavailable or range result")
    };
    value
}

#[test]
fn quality_references_nested_records_select_exact_neighbor_values() {
    let element = record(vec![
        subject(&["#ERROR", "#BOUNDS", "#QC", "#NOTE"]),
        field("error", "ERROR", "Quantity"),
        field("bounds", "BOUNDS", "QuantityRange"),
        field("qc", "QC", "Category"),
        field("note", "NOTE", "Text"),
    ]);
    let mut element = element;
    element["name"] = json!("sample");
    let source = record(vec![json!({"name":"samples","type":"DataArray",
        "elementCount":{"value":2},"elementType":element})]);
    let source_bytes = bytes(&source);
    let plan = compile(&source);
    let first = [
        named("reading", ComponentValue::ScalarJson(b"11")),
        named("error", ComponentValue::ScalarJson(b"0.125")),
        named("bounds", ComponentValue::RangeJson(b"[-0.25,0.5]")),
        named("qc", ComponentValue::ScalarJson(br#""GOOD""#)),
        named("note", ComponentValue::ScalarJson(br#""first only""#)),
    ];
    let second = [
        named("note", ComponentValue::ScalarJson(br#""second only""#)),
        named("qc", ComponentValue::ScalarJson(br#""POOR""#)),
        named("bounds", ComponentValue::RangeJson(b"[-2,3]")),
        named("error", ComponentValue::ScalarJson(b"0.875")),
        named("reading", ComponentValue::ScalarJson(b"22")),
    ];
    let rows = [ComponentValue::Record(&first), ComponentValue::Record(&second)];
    let root = [named("samples", ComponentValue::Array(&rows))];
    let bound = plan.bind(&ComponentValue::Record(&root)).unwrap();
    assert_eq!(bound.len(), 8);
    for (row, expected_error, bounds, category, note) in [
        (0, "0.125", ["-0.25", "0.5"], "GOOD", "first only"),
        (1, "0.875", ["-2", "3"], "POOR", "second only"),
    ] {
        let entries = bound
            .iter()
            .filter(|entry| entry.subject.path == [0, row, 0])
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 4);
        for (index, id) in ["ERROR", "BOUNDS", "QC", "NOTE"].iter().enumerate() {
            assert_eq!(entries[index].subject.component, component(&plan, "READING"));
            let target = entries[index].target.as_ref().unwrap();
            assert_eq!(target.component, component(&plan, id));
            assert_eq!(target.path, [0, row, index + 1]);
            assert_eq!(entries[index].value.availability(), QualityAvailability::Supplied);
        }
        let ScalarValue::Quantity(NumericValue::Finite(number)) = &scalar(&entries[0].value).value else {
            panic!("exact finite uncertainty")
        };
        assert_eq!(number.decimal_lexeme(), Some(expected_error));
        assert_eq!(
            scalar(&entries[0].value).value,
            ScalarValue::Quantity(finite(expected_error)),
            "a neighboring record's valid uncertainty is not this record's quality"
        );
        let QualityReferenceValue::Range(range) = &entries[1].value else {
            panic!("checked QuantityRange")
        };
        assert_eq!(range.endpoints[0].value, ScalarValue::Quantity(finite(bounds[0])));
        assert_eq!(range.endpoints[1].value, ScalarValue::Quantity(finite(bounds[1])));
        assert_eq!(scalar(&entries[2].value).value, ScalarValue::Category(category.into()));
        assert_eq!(scalar(&entries[3].value).value, ScalarValue::Text(note.into()));
    }
    assert_eq!(plan.graph().source(), source_bytes);
    let preserved: Value = serde_json::from_slice(plan.graph().source()).unwrap();
    let names = preserved["fields"][0]["elementType"]["fields"]
        .as_array().unwrap().iter().map(|value| value["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(names, ["reading", "error", "bounds", "qc", "note"]);
    assert_eq!(first.iter().map(|value| value.name).collect::<Vec<_>>(), names);
    assert_eq!(second.iter().map(|value| value.name).collect::<Vec<_>>(),
        ["note", "qc", "bounds", "error", "reading"]);
}

#[test]
fn quality_references_shared_record_definitions_keep_occurrences_separate() {
    let mut template = record(vec![subject(&["#QC"]), field("qc", "QC", "Category")]);
    template["name"] = json!("first");
    template["id"] = json!("TEMPLATE");
    let plan = compile(&record(vec![template, json!({"name":"second","href":"#TEMPLATE"})]));
    let left = [
        named("reading", ComponentValue::ScalarJson(b"10")),
        named("qc", ComponentValue::ScalarJson(br#""LEFT""#)),
    ];
    let right = [
        named("reading", ComponentValue::ScalarJson(b"20")),
        named("qc", ComponentValue::ScalarJson(br#""RIGHT""#)),
    ];
    let values = [named("first", ComponentValue::Record(&left)), named("second", ComponentValue::Record(&right))];
    let bound = plan.bind(&ComponentValue::Record(&values)).unwrap();
    assert_eq!(bound.len(), 2);
    for (index, expected) in ["LEFT", "RIGHT"].iter().enumerate() {
        assert_eq!(bound[index].subject.path, [index, 0]);
        assert_eq!(bound[index].target.as_ref().unwrap().path, [index, 1]);
        assert_eq!(scalar(&bound[index].value).value, ScalarValue::Category((*expected).into()));
    }
}

#[test]
fn quality_references_missing_nil_and_neighbor_values_remain_distinct() {
    let mut quality = field("qc", "QC", "Category");
    quality["optional"] = json!(true);
    quality["constraint"] = json!({"values":["GOOD","POOR"]});
    quality["nilValues"] = json!([{"value":"NA","reason":NIL_REASON}]);
    let mut element = record(vec![subject(&["#QC"]), quality]);
    element["name"] = json!("sample");
    let plan = compile(&json!({"type":"DataArray","elementCount":{"value":4},"elementType":element}));
    let good = [named("reading", ComponentValue::ScalarJson(b"1")), named("qc", ComponentValue::ScalarJson(br#""GOOD""#))];
    let nil = [named("reading", ComponentValue::ScalarJson(b"2")), named("qc", ComponentValue::ScalarJson(br#""NA""#))];
    let missing = [named("reading", ComponentValue::ScalarJson(b"3"))];
    let poor = [named("reading", ComponentValue::ScalarJson(b"4")), named("qc", ComponentValue::ScalarJson(br#""POOR""#))];
    let records = [ComponentValue::Record(&good), ComponentValue::Record(&nil), ComponentValue::Record(&missing), ComponentValue::Record(&poor)];
    let bound = plan.bind(&ComponentValue::Array(&records)).unwrap();
    assert_eq!(bound.iter().map(|entry| entry.value.availability()).collect::<Vec<_>>(),
        [QualityAvailability::Supplied, QualityAvailability::Nil, QualityAvailability::Missing, QualityAvailability::Supplied]);
    assert_eq!(scalar(&bound[0].value).value, ScalarValue::Category("GOOD".into()));
    assert_eq!(scalar(&bound[1].value).value, ScalarValue::Category("NA".into()));
    assert_eq!(scalar(&bound[1].value).nil_reason.as_deref(), Some(NIL_REASON));
    assert!(matches!(bound[2].value, QualityReferenceValue::Missing));
    assert_eq!(scalar(&bound[3].value).value, ScalarValue::Category("POOR".into()));
    assert_eq!(bound[2].subject.path, [2, 0]);
}

#[test]
fn quality_references_range_nil_does_not_become_available() {
    let mut quality = field("bounds", "BOUNDS", "QuantityRange");
    quality["nilValues"] = json!([{"value":-999,"reason":NIL_REASON}]);
    let plan = compile(&record(vec![subject(&["#BOUNDS"]), quality]));
    for raw in [b"[-999,1]".as_slice(), b"[0,-999]", b"[-999,-999]"] {
        let values = [named("reading", ComponentValue::ScalarJson(b"9")), named("bounds", ComponentValue::RangeJson(raw))];
        let bound = plan.bind(&ComponentValue::Record(&values)).unwrap();
        assert_eq!(bound[0].value.availability(), QualityAvailability::Nil);
        let QualityReferenceValue::Range(range) = &bound[0].value else { panic!("range nil retained") };
        assert!(range.endpoints.iter().any(|value| value.nil_reason.as_deref() == Some(NIL_REASON)));
    }
}

#[test]
fn quality_references_repeated_target_without_unique_context_is_ambiguous() {
    let plan = compile(&record(vec![subject(&["#QC"]), json!({"name":"checks","type":"DataArray",
        "elementCount":{"value":2},"elementType":field("qc", "QC", "Category")})]));
    let samples = [ComponentValue::ScalarJson(br#""FIRST""#), ComponentValue::ScalarJson(br#""SECOND""#)];
    let values = [named("reading", ComponentValue::ScalarJson(b"10")), named("checks", ComponentValue::Array(&samples))];
    assert_eq!(plan.bind(&ComponentValue::Record(&values)).err(), Some(QualityBindingError::AmbiguousOccurrence));
}

#[test]
fn quality_references_source_ids_targets_and_cycles_are_checked() {
    let cases = [
        (record(vec![subject(&["#UNKNOWN"]), field("qc", "QC", "Category")]), ReferenceError::UnresolvedLocal),
        (record(vec![subject(&["#QC"]), field("one", "QC", "Category"), field("two", "QC", "Text")]), ReferenceError::DuplicateId),
        (record(vec![subject(&["#READING"])]), ReferenceError::Cycle),
    ];
    for (source, expected) in cases {
        assert_eq!(QualityReferencePlan::compile(validator(), &bytes(&source), ArrayOptions::default()).err(),
            Some(QualityBindingError::Reference(expected)));
    }
    let mut a = subject(&["#QC"]);
    let mut b = field("qc", "QC", "Category");
    b["quality"] = json!([{"href":"#READING"}]);
    assert_eq!(QualityReferencePlan::compile(validator(), &bytes(&record(vec![a.clone(), b])), ArrayOptions::default()).err(),
        Some(QualityBindingError::Reference(ReferenceError::Cycle)));
    a["quality"] = json!([{"type":"Text","id":"QC","definition":"urn:example:note","label":"Note","value":"inline metadata"}]);
    assert_eq!(QualityReferencePlan::compile(validator(), &bytes(&record(vec![a, field("qc", "QC", "Category")])), ArrayOptions::default()).err(),
        Some(QualityBindingError::Reference(ReferenceError::DuplicateId)));
    for kind in ["Boolean", "Count", "CountRange", "CategoryRange", "DataRecord", "Geometry"] {
        let target = match kind {
            "DataRecord" => json!({"name":"qc","id":"QC","type":"DataRecord","fields":[]}),
            "Geometry" => json!({"name":"qc","id":"QC","type":"Geometry","definition":"urn:example:q","label":"Q","srs":"urn:example:frame"}),
            _ => field("qc", "QC", kind),
        };
        assert!(QualityReferencePlan::compile(validator(), &bytes(&record(vec![subject(&["#QC"]), target])), ArrayOptions::default()).is_err(),
            "{kind} is outside the eligible quality union");
    }
}

#[test]
fn quality_references_inline_metadata_is_not_an_encoded_target() {
    let mut reading = subject(&["#STATIC"]);
    reading["quality"] = json!([{"href":"#STATIC"},{"type":"Text","id":"STATIC",
        "definition":"urn:example:static","label":"Static","value":"not a payload field"}]);
    assert!(QualityReferencePlan::compile(validator(), &bytes(&record(vec![reading])), ArrayOptions::default()).is_err());
}

#[test]
fn quality_references_nonlocal_metadata_stays_unresolved_and_exact() {
    for href in ["https://example.invalid/quality", "file:///not-opened", "data:text/plain,not-read", "other-contract#QC"] {
        let entry = json!({"href":href,"role":"urn:example:quality-role","arcrole":"urn:example:quality-arc",
            "title":" Supplied title ","vendor":{"retained":true}});
        let mut reading = subject(&[]);
        reading["quality"] = json!([entry]);
        let source = record(vec![reading.clone()]);
        let plan = compile(&source);
        assert_eq!(plan.graph().source(), bytes(&source));
        let reference = &plan.graph().references()[0];
        assert_eq!(reference.href, href);
        assert_eq!(reference.role.as_deref(), Some("urn:example:quality-role"));
        assert_eq!(reference.arcrole.as_deref(), Some("urn:example:quality-arc"));
        assert_eq!(reference.title.as_deref(), Some(" Supplied title "));
        assert_eq!(reference.target, ReferenceTarget::Nonlocal);
        let scalar = ScalarContract::compile(validator(), &bytes(&reading)).unwrap();
        let InlineQuality::Reference(metadata) = &scalar.quality().entries()[0] else { panic!("reference metadata") };
        let preserved: Value = serde_json::from_slice(metadata.source()).unwrap();
        assert_eq!(preserved, entry);
        let values = [named("reading", ComponentValue::ScalarJson(b"1"))];
        let bound = plan.bind(&ComponentValue::Record(&values)).unwrap();
        assert!(matches!(bound[0].value, QualityReferenceValue::Nonlocal));
        assert_eq!(bound[0].target, None);
        assert_eq!(bound[0].value.availability(), QualityAvailability::Missing);
    }
}

#[test]
fn quality_references_targets_keep_constraints_and_missing_does_not_use_static_default() {
    let mut qc = field("qc", "QC", "Category");
    qc["constraint"] = json!({"values":["GOOD"]});
    qc["optional"] = json!(true);
    qc["value"] = json!("GOOD");
    let plan = compile(&record(vec![subject(&["#QC"]), qc]));
    let invalid = [named("reading", ComponentValue::ScalarJson(b"1")), named("qc", ComponentValue::ScalarJson(br#""BAD""#))];
    assert!(plan.bind(&ComponentValue::Record(&invalid)).is_err());
    let wrong_type = [named("reading", ComponentValue::ScalarJson(b"1")), named("qc", ComponentValue::ScalarJson(b"7"))];
    assert!(plan.bind(&ComponentValue::Record(&wrong_type)).is_err());
    let missing = [named("reading", ComponentValue::ScalarJson(b"1"))];
    let bound = plan.bind(&ComponentValue::Record(&missing)).unwrap();
    assert!(matches!(bound[0].value, QualityReferenceValue::Missing));
    assert_eq!(bound[0].value.availability(), QualityAvailability::Missing);
}

#[test]
fn quality_references_fragment_matching_is_id_based_exact_and_decoded_once() {
    for (id, href) in [("QC space", "#QC%20space"), ("QC%20literal", "#QC%2520literal")] {
        let plan = compile(&record(vec![subject(&[href]), field("differentName", id, "Text")]));
        let values = [named("reading", ComponentValue::ScalarJson(b"1")), named("differentName", ComponentValue::ScalarJson(br#""exact target""#))];
        let bound = plan.bind(&ComponentValue::Record(&values)).unwrap();
        assert_eq!(bound[0].target.as_ref().unwrap().component, component(&plan, id));
        assert_eq!(scalar(&bound[0].value).value, ScalarValue::Text("exact target".into()));
    }
    for href in ["#qc", "#/fields/1", "#differentName"] {
        let source = record(vec![subject(&[href]), field("differentName", "QC", "Text")]);
        assert_eq!(QualityReferencePlan::compile(validator(), &bytes(&source), ArrayOptions::default()).err(),
            Some(QualityBindingError::Reference(ReferenceError::UnresolvedLocal)));
    }
}

#[test]
fn quality_references_source_depth_reference_and_byte_budgets_fail_closed() {
    for depth in [31, 32] {
        let fields = (0..depth).map(|index| {
            let mut value = field(&format!("q{index}"), &format!("Q{index}"), "Text");
            if index + 1 < depth {
                value["quality"] = json!([{"href":format!("#Q{}", index + 1)}]);
            }
            value
        }).collect();
        let result = QualityReferencePlan::compile(validator(), &bytes(&record(fields)), ArrayOptions::default());
        if depth == 31 { assert!(result.is_ok()); }
        else { assert_eq!(result.err(), Some(QualityBindingError::Reference(ReferenceError::DepthLimit))); }
    }
    for count in [512, 513] {
        let mut first = subject(&[]);
        first["quality"] = json!((0..256).map(|_| json!({"href":"#QC"})).collect::<Vec<_>>());
        let mut second = field("second", "SECOND", "Text");
        second["quality"] = json!((256..count).map(|_| json!({"href":"#QC"})).collect::<Vec<_>>());
        let result = QualityReferencePlan::compile(validator(), &bytes(&record(vec![first, second, field("qc", "QC", "Category")])), ArrayOptions::default());
        if count == 512 { assert!(result.is_ok()); }
        else { assert_eq!(result.err(), Some(QualityBindingError::Reference(ReferenceError::ReferenceLimit))); }
    }
    let oversized = vec![b' '; validation::MAX_BYTES + 1];
    assert!(QualityReferencePlan::compile(validator(), &oversized, ArrayOptions::default()).is_err());
}

#[test]
fn quality_references_model_budget_and_duplicate_fields_are_not_bypassed() {
    fn nested_budget(depth: usize, value: ComponentValue<'_>) -> bool {
        if depth == 1 {
            return crate::choice::value::budget(&value).is_ok();
        }
        let child = [value];
        nested_budget(depth - 1, ComponentValue::Array(&child))
    }
    for depth in [31, 32, 33] {
        assert_eq!(nested_budget(depth, ComponentValue::ScalarJson(b"1")), depth <= 32);
    }
    let plan = compile(&record(vec![subject(&["#QC"]), field("qc", "QC", "Category")]));
    let duplicate = [named("reading", ComponentValue::ScalarJson(b"1")), named("qc", ComponentValue::ScalarJson(br#""ONE""#)), named("qc", ComponentValue::ScalarJson(br#""TWO""#))];
    assert!(plan.bind(&ComponentValue::Record(&duplicate)).is_err());
    let huge = vec![b'x'; validation::MAX_BYTES + 1];
    let values = [named("reading", ComponentValue::ScalarJson(b"1")), named("qc", ComponentValue::ScalarJson(&huge))];
    assert_eq!(plan.bind(&ComponentValue::Record(&values)).err(), Some(QualityBindingError::Limit));

    // The same small source cannot amplify bounded model input into unbounded
    // quality output. The 4096-result boundary uses the existing traversal cap.
    let mut reading = subject(&[]);
    reading["quality"] = json!((0..128).map(|_| json!({"href":"#QC"})).collect::<Vec<_>>());
    let mut element = record(vec![reading, field("qc", "QC", "Category")]);
    element["name"] = json!("sample");
    let plan = compile(&json!({"type":"DataArray","elementCount":{"value":32},"elementType":element}));
    let row = [named("reading", ComponentValue::ScalarJson(b"1")), named("qc", ComponentValue::ScalarJson(br#""GOOD""#))];
    let rows = (0..32).map(|_| ComponentValue::Record(&row)).collect::<Vec<_>>();
    let bounded = plan.bind(&ComponentValue::Array(&rows)).unwrap();
    assert_eq!(bounded.len(), 4096);
    assert_eq!(bounded[0].target.as_ref().unwrap().path, [0, 1]);
    assert_eq!(bounded[4095].target.as_ref().unwrap().path, [31, 1]);
    let over = (0..33).map(|_| ComponentValue::Record(&row)).collect::<Vec<_>>();
    assert_eq!(plan.bind(&ComponentValue::Array(&over)).err(), Some(QualityBindingError::Limit));
}

#[test]
fn quality_references_nested_inline_assertions_use_subject_record_without_hidden_fields() {
    let mut reading = subject(&[]);
    reading["quality"] = json!([{"type":"Text","id":"METHOD","definition":"urn:example:method",
        "label":"Method","value":"fixed calibration","quality":[{"href":"#QC"}]}]);
    let source = record(vec![reading, field("qc", "QC", "Category")]);
    let plan = compile(&source);
    let values = [named("reading", ComponentValue::ScalarJson(b"1")), named("qc", ComponentValue::ScalarJson(br#""GOOD""#))];
    let bound = plan.bind(&ComponentValue::Record(&values)).unwrap();
    assert_eq!(bound.len(), 1);
    assert_eq!(bound[0].subject.component, component(&plan, "METHOD"));
    assert_eq!(bound[0].subject.path, [0]);
    assert_eq!(bound[0].target.as_ref().unwrap().path, [1]);
    assert_eq!(scalar(&bound[0].value).value, ScalarValue::Category("GOOD".into()));
    assert_eq!(plan.graph().source(), bytes(&source));
    assert_eq!(values.iter().map(|value| value.name).collect::<Vec<_>>(), ["reading", "qc"]);
}

#[test]
fn quality_references_inactive_choice_is_missing_not_an_alternative_value() {
    let source = record(vec![subject(&["#QC"]), json!({"type":"DataChoice","name":"selection",
        "items":[field("qc", "QC", "Category"),field("other", "OTHER", "Text")]})]);
    let plan = compile(&source);
    let selected = [named("qc", ComponentValue::ScalarJson(br#""GOOD""#))];
    let values = [named("reading", ComponentValue::ScalarJson(b"1")), named("selection", ComponentValue::Choice(&selected))];
    let bound = plan.bind(&ComponentValue::Record(&values)).unwrap();
    assert_eq!(scalar(&bound[0].value).value, ScalarValue::Category("GOOD".into()));
    assert_eq!(bound[0].target.as_ref().unwrap().path, [1, 0]);
    let alternate = [named("other", ComponentValue::ScalarJson(br#""GOOD""#))];
    let values = [named("reading", ComponentValue::ScalarJson(b"1")), named("selection", ComponentValue::Choice(&alternate))];
    let bound = plan.bind(&ComponentValue::Record(&values)).unwrap();
    assert!(matches!(bound[0].value, QualityReferenceValue::Missing));
    assert_eq!(bound[0].target, None);
    assert_eq!(bound[0].value.availability(), QualityAvailability::Missing);
}
