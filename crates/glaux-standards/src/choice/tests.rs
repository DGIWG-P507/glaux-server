//! Independent DataChoice description and encoding-neutral selection checks.
//! Item names, not trial decoding or value types, determine the selected arm.
use super::{
    CheckedComponentValue, ChoiceContract, ComponentErrorKind, ComponentValue, NamedValue,
};
use crate::{
    aggregate::{AggregateContract, AggregateError},
    range::{OrderCheck, RangeError},
    scalar::ScalarError,
    validation::{self, Contract, Failure, StructuralValidator},
};
use glaux_domain::{
    aggregate::AggregateComponent,
    numeric::NumericValue,
    scalar::{
        CalendarTime, ScalarComponent, ScalarValue, TimeFrame, TimePosition,
        UnsupportedTimeConversion,
    },
};
use serde_json::{Value, json};
use std::sync::OnceLock;

const CHOICE: &[u8] = include_bytes!("../../fixtures/aggregate/choice-mixed.json");

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn compile(input: &[u8]) -> ChoiceContract {
    ChoiceContract::compile(validator(), input).expect("valid independent choice fixture")
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn count(name: &str) -> Value {
    json!({"name":name,"type":"Count","definition":"urn:example:count","label":"Count"})
}

fn description(items: Vec<Value>) -> Vec<u8> {
    bytes(&json!({"type":"DataChoice","items":items}))
}

fn named<'a>(name: &'a str, value: ComponentValue<'a>) -> NamedValue<'a> {
    NamedValue { name, value }
}

#[test]
fn choice_declared_order_and_source_are_preserved() {
    let contract = compile(CHOICE);
    assert_eq!(
        contract
            .alternatives()
            .iter()
            .map(|item| item.name())
            .collect::<Vec<_>>(),
        ["zLow", "aHigh", "mBand", "rStatus", "vPosition", "cNested"]
    );
    let AggregateComponent::Choice {
        metadata,
        items,
        choice_value,
    } = contract.component()
    else {
        panic!("choice")
    };
    assert_eq!(metadata.id.as_deref(), Some("CHOICE"));
    assert_eq!(metadata.definition, None);
    assert_eq!(metadata.label.as_deref(), Some(" Mixed alternatives "));
    assert_eq!(metadata.optional, Some(true));
    assert_eq!(
        items
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["zLow", "aHigh", "mBand", "rStatus", "vPosition", "cNested"]
    );
    assert!(matches!(
        choice_value.as_deref(),
        Some(ScalarComponent::Category { .. })
    ));
    assert_eq!(contract.source(), CHOICE);
    assert_eq!(compile(contract.source()).component(), contract.component());
    let child: Value = serde_json::from_slice(contract.alternatives()[1].source()).unwrap();
    assert_eq!(child["name"], "aHigh");
    assert_eq!(child["definition"], "urn:example:exact-count");
    let mut nested: Value = serde_json::from_slice(CHOICE).unwrap();
    nested["name"] = json!("selection");
    let record = bytes(&json!({"type":"DataRecord","fields":[nested]}));
    let record = AggregateContract::compile(validator(), &record).unwrap();
    let nested = record.children()[0].aggregate().unwrap();
    assert!(matches!(
        nested.component(),
        AggregateComponent::Choice { .. }
    ));
    assert_eq!(nested.children()[1].name(), "aHigh");
    let selection = [named(
        "aHigh",
        ComponentValue::ScalarJson(b"9007199254740993"),
    )];
    let fields = [named("selection", ComponentValue::Choice(&selection))];
    let checked = record
        .check_value(&ComponentValue::Record(&fields))
        .unwrap();
    let CheckedComponentValue::Record(fields) = checked else {
        panic!("record-root value")
    };
    assert_eq!(fields[0].name, "selection");
    let Some(CheckedComponentValue::Choice(choice)) = &fields[0].value else {
        panic!("selected nested choice")
    };
    assert_eq!(choice.name, "aHigh");
    let CheckedComponentValue::Scalar(value) = &choice.value else {
        panic!("selected scalar")
    };
    let ScalarValue::Count(value) = &value.value else {
        panic!("exact Count")
    };
    assert_eq!(value.try_to_u64(), Ok(9_007_199_254_740_993));
    assert_eq!(value.number().decimal_lexeme(), Some("9007199254740993"));
    let wrong_arm = [named(
        "zLow",
        ComponentValue::ScalarJson(b"9007199254740993"),
    )];
    let fields = [named("selection", ComponentValue::Choice(&wrong_arm))];
    let error = record
        .check_value(&ComponentValue::Record(&fields))
        .err()
        .unwrap();
    assert_eq!(
        error.kind,
        ComponentErrorKind::Scalar(ScalarError::ConstraintViolation)
    );
    assert_eq!(error.path, [0, 0]);
}

#[test]
fn choice_cardinality_names_and_selector_metadata() {
    for items in [vec![], vec![count("one")]] {
        let input = description(items);
        assert_eq!(
            validator().validate(Contract::DataChoice, &input),
            Ok(()),
            "original JSON schema omits the UML [2..*] minimum"
        );
        let error = ChoiceContract::compile(validator(), &input).err().unwrap();
        assert_eq!(
            error.kind,
            ComponentErrorKind::Compile(AggregateError::ChoiceCardinality)
        );
        assert!(error.path.is_empty());
    }
    let minimal = compile(&description(vec![count("A"), count("a")]));
    assert!(
        minimal.choice_value().is_none(),
        "optional selector is not synthesized"
    );
    let AggregateComponent::Choice { metadata, .. } = minimal.component() else {
        panic!("choice")
    };
    assert_eq!(metadata.definition, None);
    assert_eq!(metadata.label, None);
    assert_eq!(
        ChoiceContract::compile(
            validator(),
            &description(vec![count("same"), count("same")])
        )
        .err()
        .unwrap()
        .kind,
        ComponentErrorKind::Compile(AggregateError::DuplicateName)
    );
    for name in ["", "1bad", "with space", "_prefix", "é"] {
        assert!(
            ChoiceContract::compile(validator(), &description(vec![count("valid"), count(name)]))
                .is_err()
        );
    }
    let mut subset: Value = serde_json::from_slice(CHOICE).unwrap();
    subset["choiceValue"]["constraint"]["values"] = json!(["zLow"]);
    let subset = compile(&bytes(&subset));
    assert!(
        subset
            .check_value(&[named("zLow", ComponentValue::ScalarJson(b"3"))])
            .is_ok()
    );
    assert_eq!(
        subset
            .choice_value()
            .unwrap()
            .check_value(br#""aHigh""#)
            .err(),
        Some(ScalarError::ConstraintViolation)
    );
    assert_eq!(
        subset
            .check_value(&[named(
                "aHigh",
                ComponentValue::ScalarJson(b"9007199254740993")
            )])
            .unwrap()
            .name,
        "aHigh",
        "model selection is not an encoded-stream selector token assignment"
    );
    let mut invalid: Value = serde_json::from_slice(CHOICE).unwrap();
    invalid["choiceValue"]["type"] = json!("Text");
    assert!(
        ChoiceContract::compile(validator(), &bytes(&invalid)).is_err(),
        "choiceValue is specifically a Category"
    );
}

#[test]
fn choice_selection_rejects_zero_multiple_and_unknown() {
    let contract = compile(CHOICE);
    assert_eq!(
        contract.check_value(&[]).err().unwrap().kind,
        ComponentErrorKind::SelectionCardinality
    );
    let both = [
        named("zLow", ComponentValue::ScalarJson(b"3")),
        named("aHigh", ComponentValue::ScalarJson(b"9007199254740993")),
    ];
    assert_eq!(
        contract.check_value(&both).err().unwrap().kind,
        ComponentErrorKind::SelectionCardinality
    );
    let repeated = [
        named("zLow", ComponentValue::ScalarJson(b"3")),
        named("zLow", ComponentValue::ScalarJson(b"4")),
    ];
    assert_eq!(
        contract.check_value(&repeated).err().unwrap().kind,
        ComponentErrorKind::SelectionCardinality
    );
    for name in ["unknown", "ZLow", "", "zLow/child"] {
        let error = contract
            .check_value(&[named(name, ComponentValue::ScalarJson(b"3"))])
            .err()
            .unwrap();
        assert_eq!(error.kind, ComponentErrorKind::UnknownSelection);
        assert!(
            error.path.is_empty(),
            "unknown caller text is not a declared path segment"
        );
    }
    assert_eq!(
        contract
            .check_value(&[named("zLow", ComponentValue::RangeJson(b"[1,2]"))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::ValueType
    );
    assert_eq!(
        contract
            .check_value(&[named("zLow", ComponentValue::ScalarJson(b"null"))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Scalar(ScalarError::ValueType)
    );
}

#[test]
fn choice_dispatches_exact_selected_arm() {
    let contract = compile(CHOICE);
    let high = contract
        .check_value(&[named(
            "aHigh",
            ComponentValue::ScalarJson(b"9007199254740993"),
        )])
        .unwrap();
    assert_eq!(high.name, "aHigh");
    let CheckedComponentValue::Scalar(high) = high.value else {
        panic!("scalar selected arm")
    };
    let ScalarValue::Count(high) = high.value else {
        panic!("exact Count")
    };
    assert_eq!(high.try_to_u64(), Ok(9_007_199_254_740_993));
    assert_eq!(high.number().decimal_lexeme(), Some("9007199254740993"));
    // Controlled wrong-arm fallback must fail this assertion, not merely setup.
    assert_eq!(
        contract
            .check_value(&[named(
                "zLow",
                ComponentValue::ScalarJson(b"9007199254740993")
            )])
            .err()
            .map(|error| error.kind),
        Some(ComponentErrorKind::Scalar(ScalarError::ConstraintViolation))
    );
    assert_eq!(
        contract
            .check_value(&[named("aHigh", ComponentValue::ScalarJson(b"3"))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Scalar(ScalarError::ConstraintViolation)
    );
    let error = contract
        .check_value(&[named("zLow", ComponentValue::ScalarJson(b"10"))])
        .err()
        .unwrap();
    assert_eq!(error.path, [0]);
    let low = contract
        .check_value(&[named("zLow", ComponentValue::ScalarJson(b"3"))])
        .unwrap();
    assert_eq!(low.name, "zLow");
    let CheckedComponentValue::Scalar(low) = low.value else {
        panic!("scalar")
    };
    assert_eq!(low.value, ScalarValue::Count("3".parse().unwrap()));
    let band = contract
        .check_value(&[named(
            "mBand",
            ComponentValue::RangeJson(b"[125.00,150.00]"),
        )])
        .unwrap();
    assert_eq!(band.name, "mBand");
    let CheckedComponentValue::Range(band) = band.value else {
        panic!("range")
    };
    assert_eq!(band.order, OrderCheck::Established);
    let ScalarValue::Quantity(NumericValue::Finite(low)) = &band.endpoints[0].value else {
        panic!("finite quantity")
    };
    assert_eq!(low.decimal_lexeme(), Some("125.00"));
    assert_eq!(
        contract
            .check_value(&[named("mBand", ComponentValue::RangeJson(b"[150,125]"))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Range(RangeError::ReversedBounds)
    );
}

#[test]
fn choice_nested_record_vector_and_choice_values() {
    let contract = compile(CHOICE);
    let fields = [
        named("aNote", ComponentValue::ScalarJson(br#""ready""#)),
        named("zFlag", ComponentValue::ScalarJson(b"false")),
    ];
    let checked = contract
        .check_value(&[named("rStatus", ComponentValue::Record(&fields))])
        .unwrap();
    let CheckedComponentValue::Record(fields) = checked.value else {
        panic!("record")
    };
    assert_eq!(
        fields
            .iter()
            .map(|field| field.name.as_str())
            .collect::<Vec<_>>(),
        ["zFlag", "aNote"]
    );
    let Some(CheckedComponentValue::Scalar(flag)) = &fields[0].value else {
        panic!("flag")
    };
    assert_eq!(flag.value, ScalarValue::Boolean(false));
    let Some(CheckedComponentValue::Scalar(note)) = &fields[1].value else {
        panic!("note")
    };
    assert_eq!(note.value, ScalarValue::Text("ready".into()));
    let coordinates = [
        named(
            "aTime",
            ComponentValue::ScalarJson(br#""2000-01-01T00:00:00.1234567890123456789Z""#),
        ),
        named("zIndex", ComponentValue::ScalarJson(b"9007199254740993")),
    ];
    let checked = contract
        .check_value(&[named("vPosition", ComponentValue::Vector(&coordinates))])
        .unwrap();
    let CheckedComponentValue::Vector {
        reference_frame,
        local_frame,
        coordinates,
    } = checked.value
    else {
        panic!("vector")
    };
    assert_eq!(reference_frame, "urn:example:unknown-frame");
    assert_eq!(local_frame.as_deref(), Some("#platform"));
    assert_eq!(
        coordinates
            .iter()
            .map(|coordinate| coordinate.name.as_str())
            .collect::<Vec<_>>(),
        ["zIndex", "aTime"]
    );
    let Some(CheckedComponentValue::Scalar(time)) = &coordinates[1].value else {
        panic!("time")
    };
    let ScalarValue::Time(time) = &time.value else {
        panic!("bound time")
    };
    assert_eq!(
        time.reference.frame,
        TimeFrame::Declared("urn:example:unknown-frame".into())
    );
    assert_eq!(time.utc_instant().err(), Some(UnsupportedTimeConversion));
    assert_eq!(
        time.position,
        TimePosition::Calendar(CalendarTime::Unresolved(
            "2000-01-01T00:00:00.1234567890123456789Z".into()
        ))
    );
    for (selection, expected) in [
        (
            named("yes", ComponentValue::ScalarJson(b"true")),
            ScalarValue::Boolean(true),
        ),
        (
            named("note", ComponentValue::ScalarJson(br#""nested""#)),
            ScalarValue::Text("nested".into()),
        ),
    ] {
        let name = selection.name;
        let inner = [selection];
        let checked = contract
            .check_value(&[named("cNested", ComponentValue::Choice(&inner))])
            .unwrap();
        assert_eq!(checked.name, "cNested");
        let CheckedComponentValue::Choice(inner) = checked.value else {
            panic!("nested choice")
        };
        assert_eq!(inner.name, name);
        let CheckedComponentValue::Scalar(value) = inner.value else {
            panic!("selected nested scalar")
        };
        assert_eq!(value.value, expected);
    }
    let error = contract
        .check_value(&[named("cNested", ComponentValue::Choice(&[]))])
        .err()
        .unwrap();
    assert_eq!(error.kind, ComponentErrorKind::SelectionCardinality);
    assert_eq!(error.path, [5]);
    let multiple = [
        named("yes", ComponentValue::ScalarJson(b"true")),
        named("note", ComponentValue::ScalarJson(br#""nested""#)),
    ];
    let error = contract
        .check_value(&[named("cNested", ComponentValue::Choice(&multiple))])
        .err()
        .unwrap();
    assert_eq!(error.kind, ComponentErrorKind::SelectionCardinality);
    assert_eq!(error.path, [5]);
    let error = contract
        .check_value(&[named("cNested", ComponentValue::Record(&[]))])
        .err()
        .unwrap();
    assert_eq!(error.kind, ComponentErrorKind::ValueType);
    assert_eq!(error.path, [5]);
    let missing = [named("aNote", ComponentValue::ScalarJson(br#""ready""#))];
    let error = contract
        .check_value(&[named("rStatus", ComponentValue::Record(&missing))])
        .err()
        .unwrap();
    assert_eq!(error.kind, ComponentErrorKind::MissingMember);
    assert_eq!(error.path, [3, 0]);
    let duplicate = [
        named("zFlag", ComponentValue::ScalarJson(b"true")),
        named("zFlag", ComponentValue::ScalarJson(b"false")),
    ];
    assert_eq!(
        contract
            .check_value(&[named("rStatus", ComponentValue::Record(&duplicate))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::DuplicateMember
    );
    let unknown = [
        named("zFlag", ComponentValue::ScalarJson(b"true")),
        named("other", ComponentValue::ScalarJson(b"false")),
    ];
    assert_eq!(
        contract
            .check_value(&[named("rStatus", ComponentValue::Record(&unknown))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::UnknownMember
    );
    let missing_coordinate = [named("zIndex", ComponentValue::ScalarJson(b"1"))];
    let error = contract
        .check_value(&[named(
            "vPosition",
            ComponentValue::Vector(&missing_coordinate),
        )])
        .err()
        .unwrap();
    assert_eq!(error.kind, ComponentErrorKind::MissingMember);
    assert_eq!(error.path, [4, 1]);
    let wrong = [named(
        "yes",
        ComponentValue::ScalarJson(br#""not boolean""#),
    )];
    let error = contract
        .check_value(&[named("cNested", ComponentValue::Choice(&wrong))])
        .err()
        .unwrap();
    assert_eq!(
        error.kind,
        ComponentErrorKind::Scalar(ScalarError::ValueType)
    );
    assert_eq!(error.path, [5, 0]);
}

#[test]
fn choice_invalid_unselected_alternative_has_bounded_path() {
    let mut invalid: Value = serde_json::from_slice(CHOICE).unwrap();
    invalid["items"][3]["fields"][0]
        .as_object_mut()
        .unwrap()
        .remove("label");
    let error = ChoiceContract::compile(validator(), &bytes(&invalid))
        .err()
        .unwrap();
    assert_eq!(
        error.kind,
        ComponentErrorKind::Compile(AggregateError::Scalar(ScalarError::Structure))
    );
    assert_eq!(
        error.path,
        [3, 0],
        "invalid alternatives are checked before any selection"
    );
    let mut invalid: Value = serde_json::from_slice(CHOICE).unwrap();
    invalid["items"][4]["coordinates"][1]
        .as_object_mut()
        .unwrap()
        .remove("axisID");
    let error = ChoiceContract::compile(validator(), &bytes(&invalid))
        .err()
        .unwrap();
    assert_eq!(
        error.kind,
        ComponentErrorKind::Compile(AggregateError::CoordinateAxis)
    );
    assert_eq!(error.path, [4, 1]);
    for kind in ["DataStream", "Unknown"] {
        let input = description(vec![count("valid"), json!({"name":"later","type":kind})]);
        let error = ChoiceContract::compile(validator(), &input).err().unwrap();
        assert_eq!(
            error.kind,
            ComponentErrorKind::Compile(AggregateError::UnsupportedComponent)
        );
        assert_eq!(error.path, [1]);
    }
    let reference = description(vec![
        count("valid"),
        json!({"name":"external","href":"https://example.invalid/component"}),
    ]);
    assert_eq!(
        validator().validate(Contract::DataChoice, &reference),
        Ok(())
    );
    assert_eq!(
        ChoiceContract::compile(validator(), &reference)
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::UnsupportedComponent)
    );
}

#[test]
fn choice_nil_absent_and_inline_are_not_selection() {
    let contract = compile(CHOICE);
    let checked = contract
        .check_value(&[named("mBand", ComponentValue::RangeJson(b"[-999,125.00]"))])
        .unwrap();
    let CheckedComponentValue::Range(band) = checked.value else {
        panic!("range")
    };
    assert_eq!(band.order, OrderCheck::NilEndpoint);
    assert_eq!(
        band.endpoints[0].nil_reason.as_deref(),
        Some("urn:example:nil:missing")
    );
    assert!(band.endpoints[1].nil_reason.is_none());
    let nils = &contract.alternatives()[2]
        .range()
        .unwrap()
        .component()
        .nil_values;
    let ScalarValue::Quantity(NumericValue::Finite(nil)) = &nils[0].value else {
        panic!("finite nil sentinel")
    };
    assert_eq!(nil.decimal_lexeme(), Some("-999.00"));
    let fields = [named("zFlag", ComponentValue::ScalarJson(b"false"))];
    let checked = contract
        .check_value(&[named("rStatus", ComponentValue::Record(&fields))])
        .unwrap();
    let CheckedComponentValue::Record(fields) = checked.value else {
        panic!("record")
    };
    assert!(
        fields[1].value.is_none(),
        "absent optional record field stays absent"
    );
    let null = [
        named("zFlag", ComponentValue::ScalarJson(b"false")),
        named("aNote", ComponentValue::ScalarJson(b"null")),
    ];
    assert_eq!(
        contract
            .check_value(&[named("rStatus", ComponentValue::Record(&null))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Scalar(ScalarError::ValueType),
        "typed presence is not the later JSON optional-null codec mapping"
    );
    let mut inline: Value = serde_json::from_slice(CHOICE).unwrap();
    inline["choiceValue"]["value"] = json!("aHigh");
    inline["items"][0]["value"] = json!(3);
    inline["items"][1]["value"] = json!(9_007_199_254_740_993_u64);
    let input = bytes(&inline);
    let inline = compile(&input);
    assert_eq!(
        inline.choice_value().unwrap().component().value(),
        Some(ScalarValue::Category("aHigh".into()))
    );
    assert_eq!(
        inline.check_value(&[]).err().unwrap().kind,
        ComponentErrorKind::SelectionCardinality
    );
    assert_eq!(
        inline
            .check_value(&[named("zLow", ComponentValue::ScalarJson(b"3"))])
            .unwrap()
            .name,
        "zLow",
        "inline selector metadata is not a constant discriminator constraint"
    );
    assert_eq!(inline.source(), input);
    assert_eq!(compile(inline.source()).component(), inline.component());
}

#[test]
fn choice_limits_cover_total_input_and_nested_traversal() {
    let contract = compile(CHOICE);
    let huge = vec![b' '; validation::MAX_BYTES + 1];
    assert_eq!(
        contract
            .check_value(&[named("zLow", ComponentValue::ScalarJson(&huge))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Limit
    );
    let text_item =
        |name| json!({"name":name,"type":"Text","definition":"urn:example:text","label":"Text"});
    let fields = (0..20)
        .map(|index| text_item(format!("f{index}")))
        .collect::<Vec<_>>();
    let input = description(vec![
        count("number"),
        json!({"name":"many","type":"DataRecord","fields":fields}),
    ]);
    let many = compile(&input);
    let names = (0..20).map(|index| format!("f{index}")).collect::<Vec<_>>();
    let text = format!("\"{}\"", "x".repeat(14_000));
    assert!(text.len() < validation::MAX_STRING_BYTES);
    let fields = names
        .iter()
        .map(|name| named(name, ComponentValue::ScalarJson(text.as_bytes())))
        .collect::<Vec<_>>();
    assert_eq!(
        many.check_value(&[named("many", ComponentValue::Record(&fields))])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Limit,
        "sum of individually small leaves exceeds the total byte budget"
    );
    let mut nested = count("leaf");
    for _ in 0..validation::MAX_DEPTH {
        nested = json!({"name":"nested","type":"DataChoice","items":[count("other"),nested]});
    }
    let error = ChoiceContract::compile(validator(), &bytes(&nested))
        .err()
        .unwrap();
    assert_eq!(
        error.kind,
        ComponentErrorKind::Compile(AggregateError::Syntax(Failure::Depth))
    );
    assert!(error.path.len() <= validation::MAX_DEPTH);
    let duplicate = br#"{"type":"DataChoice","items":[],"items":[]}"#;
    assert_eq!(
        ChoiceContract::compile(validator(), duplicate)
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::Syntax(Failure::DuplicateKey))
    );
}

#[test]
fn choice_generated_selection_identity() {
    // Fixed 64 cases: disjoint exact integer sets independently identify each
    // arm, with deliberately nonalphabetical and alternating declaration order.
    for case in 0_u64..64 {
        let values = [
            9_007_199_254_740_993 + case * 2,
            9_007_199_254_740_994 + case * 2,
        ];
        let names = [format!("z{case}"), format!("a{case}")];
        let mut items = names
            .iter()
            .zip(values)
            .map(|(name, value)| {
                let mut item = count(name);
                item["constraint"] = json!({"values":[value]});
                item
            })
            .collect::<Vec<_>>();
        if case % 2 == 1 {
            items.reverse();
        }
        let input = description(items);
        let contract = compile(&input);
        for arm in 0..2 {
            let raw = format!("{}.0", values[arm]);
            let checked = contract
                .check_value(&[named(
                    &names[arm],
                    ComponentValue::ScalarJson(raw.as_bytes()),
                )])
                .unwrap();
            assert_eq!(checked.name, names[arm]);
            let CheckedComponentValue::Scalar(value) = checked.value else {
                panic!("scalar")
            };
            let ScalarValue::Count(value) = value.value else {
                panic!("Count")
            };
            assert_eq!(value.try_to_u64(), Ok(values[arm]));
            assert_eq!(value.number().decimal_lexeme(), Some(raw.as_str()));
            let other = values[1 - arm].to_string();
            assert_eq!(
                contract
                    .check_value(&[named(
                        &names[arm],
                        ComponentValue::ScalarJson(other.as_bytes())
                    )])
                    .err()
                    .unwrap()
                    .kind,
                ComponentErrorKind::Scalar(ScalarError::ConstraintViolation)
            );
        }
        assert_eq!(contract.source(), input);
    }
}
