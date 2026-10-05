//! Source-derived geometry descriptions and exact values, not spatial operations.
use super::{GeometryContract, GeometryError, SrsCheck};
use crate::{
    aggregate::{AggregateContract, AggregateError},
    array::{ArrayContract, ArrayOptions},
    choice::{CheckedComponentValue, ChoiceContract, ComponentErrorKind, ComponentValue, NamedValue},
    scalar::{MAX_NIL_DECLARATIONS, ScalarContract},
    validation::{self, Contract, Failure, StructuralValidator},
};
use glaux_domain::{
    aggregate::{AggregateComponent, Component},
    geometry::{GeometryKind, GeometryValue, Position},
    numeric::ExactNumber,
};
use serde_json::{Value, json};
use std::sync::OnceLock;

const POINT: &[u8] = include_bytes!("../../fixtures/geometry/point-height.json");
const POLYGON: &[u8] = include_bytes!("../../fixtures/geometry/polygon-holes.json");
const NIL: &[u8] = include_bytes!("../../fixtures/geometry/nil-descriptor.json");
const CRS84: &str = "http://www.opengis.net/def/crs/OGC/1.3/CRS84";
const CRS84H: &str = "http://www.opengis.net/def/crs/OGC/0/CRS84h";

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn compile(input: &[u8]) -> GeometryContract {
    GeometryContract::compile(validator(), input).expect("valid independent geometry fixture")
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn descriptor(srs: &str) -> Value {
    json!({"type":"Geometry","definition":"urn:example:geometry","label":"Geometry","srs":srs})
}

fn position(ordinates: &[&str]) -> Position {
    Position {
        ordinates: ordinates.iter().map(|value| value.parse().unwrap()).collect(),
    }
}

fn count(name: &str) -> Value {
    json!({"name":name,"type":"Count","definition":"urn:example:count","label":"Count"})
}

#[test]
fn geometry_height_and_exact_source_are_preserved() {
    let contract = compile(POINT);
    let checked = contract.inline_value().unwrap();
    let GeometryValue::Point(point) = &checked.value else {
        panic!("Point value")
    };
    assert_eq!(
        point.height().and_then(ExactNumber::decimal_lexeme),
        Some("123.4500")
    );
    assert_eq!(
        point
            .ordinates
            .iter()
            .map(ExactNumber::decimal_lexeme)
            .collect::<Vec<_>>(),
        [
            Some("-75.1234567890123456789"),
            Some("40.000000000000000001"),
            Some("123.4500")
        ]
    );
    assert_eq!(checked.srs, CRS84H);
    assert_eq!(checked.srs_check, SrsCheck::KnownDimensions(3));
    assert_eq!(
        checked
            .bbox
            .as_ref()
            .unwrap()
            .iter()
            .map(ExactNumber::decimal_lexeme)
            .collect::<Vec<_>>(),
        [
            Some("-76"),
            Some("39"),
            Some("100"),
            Some("-74"),
            Some("41"),
            Some("200")
        ]
    );
    let metadata = &contract.component().metadata;
    assert_eq!(metadata.id.as_deref(), Some("target"));
    assert_eq!(
        metadata.definition.as_deref(),
        Some("urn:example:target-location")
    );
    assert_eq!(metadata.label.as_deref(), Some(" Target location "));
    assert_eq!(metadata.optional, Some(true));
    assert_eq!(metadata.updatable, Some(false));
    assert_eq!(contract.component().srs, CRS84H);
    assert_eq!(contract.source(), POINT);
    assert_eq!(compile(contract.source()).component(), contract.component());
    assert_eq!(validator().validate(Contract::Geometry, POINT), Ok(()));
    let source: Value = serde_json::from_slice(&checked.source).unwrap();
    assert_eq!(source["sourceTag"], json!("synthetic"));
    assert!(ScalarContract::compile(validator(), POINT).is_err());
}

#[test]
fn geometry_all_six_shapes_keep_coordinate_order() {
    let contract = compile(&bytes(&descriptor(CRS84)));
    let p = || position(&["12.50", "34.25"]);
    let q = || position(&["13.75", "35.50"]);
    let r = || position(&["12.50", "35.50"]);
    let cases = [
        (
            r#"{"type":"Point","coordinates":[12.50,34.25]}"#,
            GeometryValue::Point(p()),
        ),
        (
            r#"{"type":"MultiPoint","coordinates":[[13.75,35.50],[12.50,34.25]]}"#,
            GeometryValue::MultiPoint(vec![q(), p()]),
        ),
        (
            r#"{"type":"LineString","coordinates":[[12.50,34.25],[13.75,35.50]]}"#,
            GeometryValue::LineString(vec![p(), q()]),
        ),
        (
            r#"{"type":"MultiLineString","coordinates":[[[12.50,34.25],[13.75,35.50]],[[13.75,35.50],[12.50,35.50]]]}"#,
            GeometryValue::MultiLineString(vec![vec![p(), q()], vec![q(), r()]]),
        ),
        (
            r#"{"type":"Polygon","coordinates":[[[12.50,34.25],[13.75,35.50],[12.50,35.50],[12.50,34.25]]]}"#,
            GeometryValue::Polygon(vec![vec![p(), q(), r(), p()]]),
        ),
        (
            r#"{"type":"MultiPolygon","coordinates":[[[[12.50,34.25],[13.75,35.50],[12.50,35.50],[12.50,34.25]]],[[[13.75,35.50],[12.50,34.25],[12.50,35.50],[13.75,35.50]]]]}"#,
            GeometryValue::MultiPolygon(vec![
                vec![vec![p(), q(), r(), p()]],
                vec![vec![q(), p(), r(), q()]],
            ]),
        ),
    ];
    for (input, expected) in cases {
        let checked = contract.check_value(input.as_bytes()).unwrap();
        assert_eq!(checked.value, expected);
        assert_eq!(checked.source, input.as_bytes());
        assert_eq!(checked.srs, CRS84);
    }
    let polygon = compile(POLYGON);
    let checked = polygon.inline_value().unwrap();
    assert_eq!(
        checked.value,
        GeometryValue::Polygon(vec![
            vec![
                position(&["0", "0"]),
                position(&["6", "0"]),
                position(&["6", "6"]),
                position(&["0", "6"]),
                position(&["0.0", "0.0"]),
            ],
            vec![
                position(&["1", "1"]),
                position(&["1", "2"]),
                position(&["2", "2"]),
                position(&["2", "1"]),
                position(&["1", "1"]),
            ],
        ])
    );
}

#[test]
fn geometry_srs_dimensions_and_unknown_references() {
    for (srs, dimensions) in [
        (CRS84, 2),
        ("http://www.opengis.net/def/crs/EPSG/0/4326", 2),
        (CRS84H, 3),
        ("http://www.opengis.net/def/crs/EPSG/0/4979", 3),
    ] {
        let contract = compile(&bytes(&descriptor(srs)));
        assert_eq!(
            contract.srs_check().clone(),
            SrsCheck::KnownDimensions(dimensions)
        );
        let (valid, wrong) = if dimensions == 2 {
            (
                r#"{"type":"Point","coordinates":[12,34]}"#,
                r#"{"type":"Point","coordinates":[12,34,56]}"#,
            )
        } else {
            (
                r#"{"type":"Point","coordinates":[12,34,56]}"#,
                r#"{"type":"Point","coordinates":[12,34]}"#,
            )
        };
        let checked = contract.check_value(valid.as_bytes()).unwrap();
        assert_eq!(checked.srs, srs);
        let GeometryValue::Point(point) = checked.value else {
            panic!("Point")
        };
        assert_eq!(point.ordinates[0].decimal_lexeme(), Some("12"));
        assert_eq!(point.ordinates[1].decimal_lexeme(), Some("34"));
        assert_eq!(
            contract.check_value(wrong.as_bytes()).err(),
            Some(GeometryError::Dimension)
        );
    }
    let unknown = "urn:example:spatial-reference";
    let contract = compile(&bytes(&descriptor(unknown)));
    assert_eq!(
        contract.srs_check().clone(),
        SrsCheck::Unresolved(unknown.into())
    );
    for value in [
        r#"{"type":"Point","coordinates":[12,34]}"#,
        r#"{"type":"Point","coordinates":[12,34,56]}"#,
    ] {
        assert_eq!(
            contract.check_value(value.as_bytes()).unwrap().srs_check,
            SrsCheck::Unresolved(unknown.into())
        );
    }
    for srs in ["", "#local", "relative/crs", "not a URI"] {
        assert!(GeometryContract::compile(validator(), &bytes(&descriptor(srs))).is_err());
    }
    for value in [
        r#"{"type":"Point","coordinates":[12]}"#,
        r#"{"type":"Point","coordinates":[12,34,56,78]}"#,
        r#"{"type":"LineString","coordinates":[[12,34],[12,34,56]]}"#,
    ] {
        assert!(contract.check_value(value.as_bytes()).is_err());
    }
}

#[test]
fn geometry_allowed_types_preserve_absent_empty_and_order() {
    let unconstrained = compile(&bytes(&descriptor(CRS84)));
    assert!(unconstrained.component().constraint.is_none());
    let mut input = descriptor(CRS84);
    input["constraint"] = json!({});
    let empty_object = compile(&bytes(&input));
    assert_eq!(
        empty_object.component().constraint.as_ref().unwrap().geom_types,
        None
    );
    let point = br#"{"type":"Point","coordinates":[1,2]}"#;
    assert!(empty_object.check_value(point).is_ok());
    input["constraint"] = json!({"geomTypes":[]});
    let raw = bytes(&input);
    assert_eq!(validator().validate(Contract::Geometry, &raw), Ok(()));
    let no_types = compile(&raw);
    assert_eq!(
        no_types.component().constraint.as_ref().unwrap().geom_types,
        Some(vec![])
    );
    assert_eq!(
        no_types.check_value(point).err(),
        Some(GeometryError::ConstraintViolation)
    );
    input["constraint"] = json!({"geomTypes":["MultiPolygon","Point","Point"]});
    let ordered = compile(&bytes(&input));
    assert_eq!(
        ordered.component().constraint.as_ref().unwrap().geom_types,
        Some(vec![
            GeometryKind::MultiPolygon,
            GeometryKind::Point,
            GeometryKind::Point
        ])
    );
    assert!(ordered.check_value(point).is_ok());
    assert_eq!(
        ordered
            .check_value(br#"{"type":"LineString","coordinates":[[1,2],[3,4]]}"#)
            .err(),
        Some(GeometryError::ConstraintViolation)
    );
    input["value"] = json!({"type":"LineString","coordinates":[[1,2],[3,4]]});
    assert_eq!(
        GeometryContract::compile(validator(), &bytes(&input)).err(),
        Some(GeometryError::ConstraintViolation)
    );
    for constraint in [
        json!({"geomTypes":["GeometryCollection"]}),
        json!({"geomTypes":null}),
        json!({"geomTypes":[7]}),
        json!({"extra":true}),
    ] {
        let mut input = descriptor(CRS84);
        input["constraint"] = constraint;
        assert_eq!(
            GeometryContract::compile(validator(), &bytes(&input)).err(),
            Some(GeometryError::Structure)
        );
    }
}

#[test]
fn geometry_shape_and_ring_semantics_exceed_schema() {
    let contract = compile(&bytes(&descriptor(CRS84)));
    let unclosed = br#"{"type":"Polygon","coordinates":[[[0,0],[2,0],[2,2],[0,1]]]}"#;
    assert_eq!(
        validator().validate(Contract::GeometryValue, unclosed),
        Ok(())
    );
    assert_eq!(
        contract.check_value(unclosed).err(),
        Some(GeometryError::Ring)
    );
    let closed = br#"{"type":"Polygon","coordinates":[[[0,0],[2,0],[2,2],[0.00,-0]]]}"#;
    assert!(contract.check_value(closed).is_ok());
    let reversed = br#"{"type":"Polygon","coordinates":[[[0,0],[2,2],[2,0],[0,0]]]}"#;
    assert!(
        contract.check_value(reversed).is_ok(),
        "RFC7946 says parsers should not reject winding direction alone"
    );
    for value in [
        r#"{"type":"Point","coordinates":[]}"#,
        r#"{"type":"LineString","coordinates":[[1,2]]}"#,
        r#"{"type":"Polygon","coordinates":[[[0,0],[1,1],[0,0]]]}"#,
        r#"{"type":"MultiLineString","coordinates":[[[1,2]]]}"#,
        r#"{"type":"Point","coordinates":["1",2]}"#,
        r#"{"type":"Point","coordinates":[1,null]}"#,
        r#"{"type":"Point","coordinates":[1,"NaN"]}"#,
        r#"{"type":"Point","coordinates":[{"$ref":"file:///forbidden"},2]}"#,
        r#"{"type":"GeometryCollection","geometries":[]}"#,
        r#"{"type":"Feature","geometry":{"type":"Point","coordinates":[1,2]},"properties":{}}"#,
        r#""POINT(1 2)""#,
        "null",
    ] {
        assert!(
            contract.check_value(value.as_bytes()).is_err(),
            "invalid geometry: {value}"
        );
    }
    for (name, expected) in [
        ("MultiPoint", GeometryValue::MultiPoint(vec![])),
        ("MultiLineString", GeometryValue::MultiLineString(vec![])),
        ("Polygon", GeometryValue::Polygon(vec![])),
        ("MultiPolygon", GeometryValue::MultiPolygon(vec![])),
    ] {
        let input = format!(r#"{{"type":"{name}","coordinates":[]}}"#);
        assert_eq!(
            contract.check_value(input.as_bytes()).unwrap().value,
            expected
        );
    }
    let bbox = br#"{"type":"Point","coordinates":[1,2],"bbox":[0,0,2,3,4]}"#;
    assert_eq!(validator().validate(Contract::GeometryValue, bbox), Ok(()));
    assert_eq!(contract.check_value(bbox).err(), Some(GeometryError::Bbox));
    let valid_bbox = br#"{"type":"Point","coordinates":[1,2],"bbox":[0,0,3,4]}"#;
    assert_eq!(
        contract.check_value(valid_bbox).unwrap().bbox,
        Some(vec![
            "0".parse().unwrap(),
            "0".parse().unwrap(),
            "3".parse().unwrap(),
            "4".parse().unwrap()
        ])
    );
    let three_d = compile(&bytes(&descriptor(CRS84H)));
    assert_eq!(
        three_d
            .check_value(br#"{"type":"Point","coordinates":[1,2,3],"bbox":[0,0,3,4]}"#)
            .err(),
        Some(GeometryError::Bbox)
    );
    let across_dateline = br#"{"type":"MultiPoint","coordinates":[[177,-20],[-178,-16]],"bbox":[177,-20,-178,-16]}"#;
    assert!(
        contract.check_value(across_dateline).is_ok(),
        "west greater than east is valid for an antimeridian bbox"
    );
    for (member, value) in [
        ("geometry", json!(null)),
        ("properties", json!({})),
        ("features", json!([])),
    ] {
        let mut input = json!({"type":"Point","coordinates":[1,2]});
        input[member] = value;
        let input = bytes(&input);
        assert_eq!(validator().validate(Contract::GeometryValue, &input), Ok(()));
        assert_eq!(contract.check_value(&input).err(), Some(GeometryError::Structure));
    }
    let foreign = br#"{"type":"Point","coordinates":[1,2],"extension":{"geometry":null,"properties":{},"features":[]}}"#;
    assert_eq!(contract.check_value(foreign).unwrap().source, foreign);
}

#[test]
fn geometry_nil_metadata_absence_and_extensions() {
    let contract = compile(NIL);
    assert!(contract.component().value.is_none());
    assert!(contract.inline_value().is_none());
    assert_eq!(contract.nil_declarations().len(), 2);
    assert_eq!(contract.nil_declarations()[0].value, "MISSING");
    assert_eq!(contract.nil_declarations()[0].reason, "urn:example:nil:missing");
    assert_eq!(contract.nil_declarations()[1].value, "");
    assert_eq!(contract.nil_declarations()[1].reason, "urn:example:nil:empty");
    assert_eq!(contract.source(), NIL);
    assert_eq!(compile(contract.source()).component(), contract.component());
    for input in [br#""MISSING""#.as_slice(), br#""""#.as_slice(), b"null"] {
        assert!(
            contract.check_value(input).is_err(),
            "nil declarations do not invent a geometry wire mapping"
        );
    }
    for inline in [json!(null), json!("MISSING")] {
        let mut input: Value = serde_json::from_slice(NIL).unwrap();
        input["value"] = inline;
        assert_eq!(
            validator().validate(Contract::Geometry, &bytes(&input)),
            Err(Failure::Structure)
        );
        assert_eq!(
            GeometryContract::compile(validator(), &bytes(&input)).err(),
            Some(GeometryError::Structure)
        );
    }
    let mut duplicate: Value = serde_json::from_slice(NIL).unwrap();
    duplicate["nilValues"][1]["value"] = json!("MISSING");
    assert_eq!(
        GeometryContract::compile(validator(), &bytes(&duplicate)).err(),
        Some(GeometryError::DuplicateNilValue)
    );
    let mut excessive = descriptor(CRS84);
    excessive["nilValues"] = json!(
        (0..=MAX_NIL_DECLARATIONS)
            .map(|index| json!({"reason":"urn:example:nil","value":format!("nil{index}")}))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        GeometryContract::compile(validator(), &bytes(&excessive)).err(),
        Some(GeometryError::NilLimit)
    );
    for nils in [
        json!([]),
        json!([{"reason":"relative","value":"nil"}]),
        json!([{"reason":"urn:example:nil","value":null}]),
    ] {
        let mut input = descriptor(CRS84);
        input["nilValues"] = nils;
        assert!(GeometryContract::compile(validator(), &bytes(&input)).is_err());
    }
    let mut quality = descriptor(CRS84);
    quality["quality"] = json!([]);
    assert_eq!(
        GeometryContract::compile(validator(), &bytes(&quality)).err(),
        Some(GeometryError::UnsupportedFeature)
    );
}

#[test]
fn geometry_nested_record_choice_and_array_boundaries() {
    let mut field: Value = serde_json::from_slice(POINT).unwrap();
    field["name"] = json!("location");
    let input = bytes(&json!({"type":"DataRecord","fields":[field,count("n")]}));
    let record = AggregateContract::compile(validator(), &input).unwrap();
    let component = record.children()[0].geometry().unwrap();
    assert_eq!(component.component().srs, CRS84H);
    let AggregateComponent::Record { fields, .. } = record.component() else {
        panic!("record")
    };
    assert!(matches!(fields[0].component, Component::Geometry(_)));
    let supplied = [
        NamedValue {
            name: "n",
            value: ComponentValue::ScalarJson(b"7"),
        },
        NamedValue {
            name: "location",
            value: ComponentValue::GeometryJson(
                br#"{"type":"Point","coordinates":[12,34,56.00]}"#,
            ),
        },
    ];
    let checked = record
        .check_value(&ComponentValue::Record(&supplied))
        .unwrap();
    let CheckedComponentValue::Record(fields) = checked else {
        panic!("checked record")
    };
    assert_eq!(fields[0].name, "location");
    let Some(CheckedComponentValue::Geometry(value)) = &fields[0].value else {
        panic!("Geometry remains a distinct component family")
    };
    assert_eq!(
        value.value,
        GeometryValue::Point(position(&["12", "34", "56.00"]))
    );
    let absent = [NamedValue {
        name: "n",
        value: ComponentValue::ScalarJson(b"7"),
    }];
    let CheckedComponentValue::Record(fields) = record
        .check_value(&ComponentValue::Record(&absent))
        .unwrap()
    else {
        panic!("record with absent optional geometry")
    };
    assert!(
        fields[0].value.is_none(),
        "inline descriptor value does not fill an omitted optional member"
    );
    let mut arm = descriptor(CRS84);
    arm["name"] = json!("shape");
    arm["constraint"] = json!({"geomTypes":["Point"]});
    let input = bytes(&json!({"type":"DataChoice","items":[count("n"),arm]}));
    let choice = ChoiceContract::compile(validator(), &input).unwrap();
    let checked = choice
        .check_value(&[NamedValue {
            name: "shape",
            value: ComponentValue::GeometryJson(br#"{"type":"Point","coordinates":[12,34]}"#),
        }])
        .unwrap();
    assert_eq!(checked.name, "shape");
    let CheckedComponentValue::Geometry(value) = checked.value else {
        panic!("selected Geometry")
    };
    assert_eq!(value.value, GeometryValue::Point(position(&["12", "34"])));
    let failure = choice
        .check_value(&[NamedValue {
            name: "shape",
            value: ComponentValue::GeometryJson(
                br#"{"type":"LineString","coordinates":[[1,2],[3,4]]}"#,
            ),
        }])
        .err()
        .unwrap();
    assert_eq!(
        failure.kind,
        ComponentErrorKind::Geometry(GeometryError::ConstraintViolation)
    );
    assert_eq!(failure.path, [1]);
    assert_eq!(
        choice
            .check_value(&[NamedValue {
                name: "shape",
                value: ComponentValue::ScalarJson(br#"{"type":"Point","coordinates":[12,34]}"#),
            }])
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::ValueType
    );
    let mut element = descriptor(CRS84);
    element["name"] = json!("shape");
    let mut input = json!({"type":"DataArray","elementCount":{"value":2},"elementType":element});
    let array =
        ArrayContract::compile(validator(), &bytes(&input), ArrayOptions::default()).unwrap();
    assert_eq!(array.element().geometry().unwrap().component().srs, CRS84);
    input["elementType"]["value"] = json!({"type":"Point","coordinates":[1,2]});
    assert_eq!(
        ArrayContract::compile(validator(), &bytes(&input), ArrayOptions::default())
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::InlineElementValue)
    );
    input["elementType"].as_object_mut().unwrap().remove("value");
    input["type"] = json!("Matrix");
    assert_eq!(
        ArrayContract::compile(validator(), &bytes(&input), ArrayOptions::default())
            .err()
            .unwrap()
            .kind,
        ComponentErrorKind::Compile(AggregateError::MatrixElement)
    );
    let vector = bytes(&json!({
        "type":"Vector",
        "definition":"urn:example:vector",
        "label":"Vector",
        "referenceFrame":"urn:example:frame",
        "coordinates":[input["elementType"]]
    }));
    assert_eq!(
        AggregateContract::compile(validator(), &vector).err(),
        Some(AggregateError::CoordinateType)
    );
}

#[test]
fn geometry_original_schema_required_metadata() {
    for member in ["type", "definition", "label", "srs"] {
        let mut input = descriptor(CRS84);
        input.as_object_mut().unwrap().remove(member);
        assert_eq!(
            validator().validate(Contract::Geometry, &bytes(&input)),
            Err(Failure::Structure)
        );
        assert!(GeometryContract::compile(validator(), &bytes(&input)).is_err());
    }
    for label in [json!(""), json!(null), json!(5)] {
        let mut input = descriptor(CRS84);
        input["label"] = label;
        assert_eq!(
            GeometryContract::compile(validator(), &bytes(&input)).err(),
            Some(GeometryError::Structure)
        );
    }
    for (member, value) in [
        ("definition", json!("relative")),
        ("srs", json!(null)),
        ("optional", json!("true")),
    ] {
        let mut input = descriptor(CRS84);
        input[member] = value;
        assert_eq!(
            GeometryContract::compile(validator(), &bytes(&input)).err(),
            Some(GeometryError::Structure)
        );
    }
    for kind in ["Feature", "Point", "Quantity", "Unknown"] {
        let mut input = descriptor(CRS84);
        input["type"] = json!(kind);
        assert!(GeometryContract::compile(validator(), &bytes(&input)).is_err());
    }
    let mut whitespace = descriptor(CRS84);
    whitespace["label"] = json!(" ");
    assert_eq!(
        compile(&bytes(&whitespace)).component().metadata.label.as_deref(),
        Some(" ")
    );
}

#[test]
fn geometry_bounded_generated_positions_and_limits() {
    let contract = compile(&bytes(&descriptor(CRS84H)));
    // Fixed 64 cases: exact height spellings and every generated case's
    // dimension/type-corrupted counterpart, without geographic calculations.
    for case in 0..64 {
        let height = format!("{case}.2500");
        let input = format!(r#"{{"type":"Point","coordinates":[12.50,34.25,{height}]}}"#);
        let checked = contract.check_value(input.as_bytes()).unwrap();
        let GeometryValue::Point(point) = checked.value else {
            panic!("generated point")
        };
        assert_eq!(
            point.height().and_then(ExactNumber::decimal_lexeme),
            Some(height.as_str())
        );
        assert_eq!(point.ordinates[0].decimal_lexeme(), Some("12.50"));
        assert_eq!(point.ordinates[1].decimal_lexeme(), Some("34.25"));
        assert_eq!(checked.source, input.as_bytes());
        let invalid = format!(r#"{{"type":"Point","coordinates":[12.50,34.25,{height},0]}}"#);
        assert_eq!(
            contract.check_value(invalid.as_bytes()).err(),
            Some(GeometryError::Dimension)
        );
    }
    let huge = vec![b' '; validation::MAX_BYTES + 1];
    assert_eq!(
        contract.check_value(&huge).err(),
        Some(GeometryError::Syntax(Failure::Size))
    );
    let deep = format!(
        "{}0{}",
        "[".repeat(validation::MAX_DEPTH + 1),
        "]".repeat(validation::MAX_DEPTH + 1)
    );
    assert_eq!(
        contract.check_value(deep.as_bytes()).err(),
        Some(GeometryError::Syntax(Failure::Depth))
    );
    let duplicate = br#"{"type":"Point","coordinates":[1,2,3],"coordinates":[4,5,6]}"#;
    assert_eq!(
        contract.check_value(duplicate).err(),
        Some(GeometryError::Syntax(Failure::DuplicateKey))
    );
    let mut oversized = descriptor(CRS84);
    oversized["description"] = json!("x".repeat(validation::MAX_STRING_BYTES + 1));
    assert_eq!(
        GeometryContract::compile(validator(), &bytes(&oversized)).err(),
        Some(GeometryError::Syntax(Failure::String))
    );
}
