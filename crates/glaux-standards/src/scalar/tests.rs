//! Independent expectations from the pinned Boolean/Text/Category schemas,
//! basicTypes AllowedTokens, inherited metadata, SWE requirement 24 and Guide 4.3.
use super::{CodeSpaceCheck, ScalarContract, ScalarError};
use crate::validation::{Failure, MAX_BYTES, StructuralValidator};
use glaux_domain::scalar::{ComponentMetadata, ScalarComponent, ScalarValue, TokenConstraint};
use serde_json::json;
use std::sync::OnceLock;

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn compile(source: &[u8]) -> ScalarContract {
    ScalarContract::compile(validator(), source).expect("valid independent scalar fixture")
}

fn metadata() -> ComponentMetadata {
    ComponentMetadata {
        id: None,
        definition: "urn:example:property".into(),
        label: "Property".into(),
        description: None,
        optional: None,
        updatable: None,
        reference_frame: None,
        axis_id: None,
    }
}

#[test]
fn scalar_source_metadata_and_presence() {
    let source = br##"{
      "type":"Boolean", "id":"ACTIVE", "definition":"urn:example:active",
      "label":" Active ", "description":" supplied description ",
      "optional":false, "updatable":true, "referenceFrame":"#frame",
      "axisID":"axis", "value":false, "vendor:note":{"text":"retained"}
    }"##;
    let present = compile(source);
    assert_eq!(present.source(), source, "preserve exact received bytes");
    assert_eq!(
        present.component().value(),
        Some(ScalarValue::Boolean(false))
    );
    assert_eq!(
        present.component(),
        &ScalarComponent::Boolean {
            metadata: ComponentMetadata {
                id: Some("ACTIVE".into()),
                definition: "urn:example:active".into(),
                label: " Active ".into(),
                description: Some(" supplied description ".into()),
                optional: Some(false),
                updatable: Some(true),
                reference_frame: Some("#frame".into()),
                axis_id: Some("axis".into()),
            },
            value: Some(false),
        }
    );
    let absent =
        compile(br#"{"type":"Boolean","definition":"urn:example:property","label":"Property"}"#);
    assert_eq!(
        absent.component(),
        &ScalarComponent::Boolean {
            metadata: metadata(),
            value: None,
        }
    );
    let empty = compile(
        br#"{"type":"Text","definition":"urn:example:property","label":"Property","value":""}"#,
    );
    assert_eq!(
        empty.component().value(),
        Some(ScalarValue::Text(String::new()))
    );
    let absent =
        compile(br#"{"type":"Text","definition":"urn:example:property","label":"Property"}"#);
    assert_eq!(absent.component().value(), None);
    let empty = compile(
        br#"{"type":"Category","definition":"urn:example:property","label":"Property","codeSpace":"urn:example:terms","value":""}"#,
    );
    assert_eq!(
        empty.component().value(),
        Some(ScalarValue::Category(String::new()))
    );
    let absent = compile(
        br#"{"type":"Category","definition":"urn:example:property","label":"Property","codeSpace":"urn:example:terms"}"#,
    );
    assert_eq!(absent.component().value(), None);
}

#[test]
fn scalar_values_reject_json_type_coercion() {
    for kind in ["Boolean", "Text", "Category"] {
        let mut source = json!({
            "type": kind, "definition": "urn:example:property", "label": "Property"
        });
        if kind == "Category" {
            source["codeSpace"] = json!("urn:example:terms");
        }
        let contract = compile(&serde_json::to_vec(&source).unwrap());
        let wrong: &[&[u8]] = if kind == "Boolean" {
            &[br#""false""#, b"0", b"1", b"null", b"{}", b"[]"]
        } else {
            &[b"false", b"0", b"null", b"{}", b"[]"]
        };
        for value in wrong {
            assert_eq!(
                contract.check_value(value).err(),
                Some(ScalarError::ValueType),
                "{kind} must reject {value:?}"
            );
            let mut inline = source.clone();
            inline["value"] = serde_json::from_slice(value).unwrap();
            assert!(
                ScalarContract::compile(validator(), &serde_json::to_vec(&inline).unwrap())
                    .is_err(),
                "inline {kind} must reject {value:?}"
            );
        }
        if kind == "Boolean" {
            for value in [false, true] {
                let checked = contract
                    .check_value(if value { b"true" } else { b"false" })
                    .unwrap();
                assert_eq!(checked.value, ScalarValue::Boolean(value));
                assert_eq!(checked.code_space, CodeSpaceCheck::NotApplicable);
            }
        } else {
            let checked = contract.check_value(br#""""#).unwrap();
            assert_eq!(
                checked.value,
                if kind == "Text" {
                    ScalarValue::Text(String::new())
                } else {
                    ScalarValue::Category(String::new())
                }
            );
        }
    }
}

#[test]
fn scalar_metadata_requires_published_members() {
    for kind in ["Boolean", "Text", "Category"] {
        let mut source = json!({
            "type": kind, "definition": "urn:example:property", "label": "Property"
        });
        if kind == "Category" {
            source["codeSpace"] = json!("urn:example:terms");
        }
        for required in ["type", "definition", "label"] {
            let mut missing = source.clone();
            missing.as_object_mut().unwrap().remove(required);
            assert!(
                ScalarContract::compile(validator(), &serde_json::to_vec(&missing).unwrap())
                    .is_err(),
                "{kind} requires {required}"
            );
        }
        for (member, value) in [
            ("label", json!("")),
            ("label", json!(null)),
            ("label", json!(42)),
            ("definition", json!("relative/property")),
            ("definition", json!("")),
            ("id", json!("")),
            ("description", json!("")),
            ("optional", json!("false")),
            ("updatable", json!(0)),
            ("axisID", json!("")),
        ] {
            let mut invalid = source.clone();
            invalid[member] = value;
            assert!(
                ScalarContract::compile(validator(), &serde_json::to_vec(&invalid).unwrap())
                    .is_err(),
                "{kind} rejects invalid {member}"
            );
        }
        let mut invalid_reference = source.clone();
        invalid_reference["referenceFrame"] = json!("#bad frame");
        assert_eq!(
            ScalarContract::compile(
                validator(),
                &serde_json::to_vec(&invalid_reference).unwrap()
            )
            .err(),
            Some(ScalarError::Metadata)
        );
        let mut whitespace = source;
        whitespace["label"] = json!(" ");
        assert!(
            ScalarContract::compile(validator(), &serde_json::to_vec(&whitespace).unwrap()).is_ok(),
            "published minLength does not impose trimming"
        );
    }
}

#[test]
fn scalar_enumerations_preserve_tokens_and_enforce_membership() {
    for kind in ["Text", "Category"] {
        let source = json!({
            "type": kind, "definition": "urn:example:property", "label": "Property",
            "constraint": {"type": "AllowedTokens", "values": ["Ready", " Ready ", "Ready", " \u{00e9} "]},
            "value": " Ready "
        });
        let contract = compile(&serde_json::to_vec(&source).unwrap());
        assert_eq!(
            contract.check_value(br#""not-listed""#).err(),
            Some(ScalarError::ConstraintViolation)
        );
        let tokens = Some(TokenConstraint::Values(vec![
            "Ready".into(),
            " Ready ".into(),
            "Ready".into(),
            " \u{00e9} ".into(),
        ]));
        let expected = if kind == "Text" {
            ScalarComponent::Text {
                metadata: metadata(),
                constraint: tokens,
                value: Some(" Ready ".into()),
            }
        } else {
            ScalarComponent::Category {
                metadata: metadata(),
                code_space: None,
                constraint: tokens,
                value: Some(" Ready ".into()),
            }
        };
        assert_eq!(contract.component(), &expected);
        for accepted in ["Ready", " Ready ", " \u{00e9} "] {
            let checked = contract
                .check_value(&serde_json::to_vec(accepted).unwrap())
                .unwrap();
            assert_eq!(checked.code_space, CodeSpaceCheck::NotApplicable);
            assert_eq!(
                checked.value,
                if kind == "Text" {
                    ScalarValue::Text(accepted.into())
                } else {
                    ScalarValue::Category(accepted.into())
                }
            );
        }
        for rejected in ["ready", "READY", " Ready", "", "Other", " e\u{0301} "] {
            assert_eq!(
                contract
                    .check_value(&serde_json::to_vec(rejected).unwrap())
                    .err(),
                Some(ScalarError::ConstraintViolation),
                "{kind}: enum membership compares exact tokens"
            );
        }
        let mut invalid_inline = source;
        invalid_inline["value"] = json!("Other");
        assert_eq!(
            ScalarContract::compile(validator(), &serde_json::to_vec(&invalid_inline).unwrap())
                .err(),
            Some(ScalarError::ConstraintViolation)
        );
    }
}

#[test]
fn scalar_patterns_use_ecmascript_character_classes() {
    // JSON Schema regex searches by default; anchors belong to the supplied pattern.
    for (pattern, accepted, rejected) in [
        ("A[0-9]+", "prefix-A12-suffix", "a12"),
        ("^A[0-9]+$", "A12", "prefix-A12-suffix"),
        (r"^\d+$", "0129", "\u{0661}"),
        (r"^\w+$", "Az_09", "\u{00e9}"),
        (r"\.", "prefix.suffix", "prefix-suffix"),
        ("[.]", ".", "x"),
        ("[a-z]", "1q2", "1Q2"),
    ] {
        let source = json!({
            "type": "Text", "definition": "urn:example:property", "label": "Property",
            "constraint": {"pattern": pattern}
        });
        let contract = compile(&serde_json::to_vec(&source).unwrap());
        assert_eq!(
            contract.component(),
            &ScalarComponent::Text {
                metadata: metadata(),
                constraint: Some(TokenConstraint::Pattern(pattern.into())),
                value: None,
            }
        );
        assert_eq!(
            contract
                .check_value(&serde_json::to_vec(accepted).unwrap())
                .unwrap()
                .value,
            ScalarValue::Text(accepted.into())
        );
        assert_eq!(
            contract
                .check_value(&serde_json::to_vec(rejected).unwrap())
                .err(),
            Some(ScalarError::ConstraintViolation),
            "pattern {pattern} must reject {rejected}"
        );
    }
}

#[test]
fn scalar_category_code_space_is_explicitly_unresolved() {
    let uri = "https://example.invalid/vocabulary/status";
    for constraint in [
        None,
        Some(json!({"values": ["Ready"]})),
        Some(json!({"pattern": "^Ready$"})),
    ] {
        let mut source = json!({
            "type": "Category", "definition": "urn:example:property", "label": "Property",
            "codeSpace": uri, "value": "Ready"
        });
        if let Some(constraint) = constraint {
            source["constraint"] = constraint;
        }
        let contract = compile(&serde_json::to_vec(&source).unwrap());
        let checked = contract.check_value(br#""Ready""#).unwrap();
        assert_eq!(checked.value, ScalarValue::Category("Ready".into()));
        assert_eq!(
            checked.code_space,
            CodeSpaceCheck::Unresolved(uri.into()),
            "local success cannot prove external vocabulary membership"
        );
        if source.get("constraint").is_some() {
            assert_eq!(
                contract.check_value(br#""not-listed""#).err(),
                Some(ScalarError::ConstraintViolation)
            );
        }
        let ScalarComponent::Category { code_space, .. } = contract.component() else {
            panic!("Category meaning must retain its component type");
        };
        assert_eq!(code_space.as_deref(), Some(uri));
    }
    for constraint in [None, Some(json!({"pattern": "^[A-Z]+$"}))] {
        let mut source = json!({
            "type": "Category", "definition": "urn:example:property", "label": "Property"
        });
        if let Some(constraint) = constraint {
            source["constraint"] = constraint;
        }
        assert_eq!(
            ScalarContract::compile(validator(), &serde_json::to_vec(&source).unwrap()).err(),
            Some(ScalarError::CategoryWithoutEnumeration),
            "SWE requirement 24 needs a codeSpace or enumeration, not only a pattern"
        );
    }
    let relative = br#"{"type":"Category","definition":"urn:example:property","label":"Property","codeSpace":"relative/terms"}"#;
    assert!(ScalarContract::compile(validator(), relative).is_err());
}

#[test]
fn scalar_constraints_reject_malformed_or_ambiguous_shapes() {
    for kind in ["Text", "Category"] {
        for constraint in [
            json!({}),
            json!({"values": []}),
            json!({"values": [""]}),
            json!({"values": [false]}),
            json!({"values": "Ready"}),
            json!({"pattern": ""}),
            json!({"pattern": "["}),
            json!({"pattern": false}),
            json!({"values": ["Ready"], "pattern": "Ready"}),
            json!({"type": "AllowedValues", "values": ["Ready"]}),
        ] {
            let source = json!({
                "type": kind, "definition": "urn:example:property", "label": "Property",
                "codeSpace": "urn:example:terms", "constraint": constraint
            });
            assert!(
                ScalarContract::compile(validator(), &serde_json::to_vec(&source).unwrap())
                    .is_err(),
                "{kind} rejects malformed or ambiguous AllowedTokens: {constraint}"
            );
        }
    }
}

#[test]
fn scalar_bounded_inputs_and_unsupported_features() {
    assert_eq!(
        ScalarContract::compile(
            validator(),
            br#"{"type":"Time","definition":"urn:example:property","label":"Property","uom":{"code":"s"}}"#
        )
        .err(),
        Some(ScalarError::UnsupportedComponent)
    );
    assert_eq!(
        ScalarContract::compile(validator(), &vec![b' '; MAX_BYTES + 1]).err(),
        Some(ScalarError::Syntax(Failure::Size))
    );
    assert_eq!(
        ScalarContract::compile(
            validator(),
            br#"{"type":"Boolean","definition":"urn:example:property","label":"A","l\u0061bel":"B"}"#
        )
        .err(),
        Some(ScalarError::Syntax(Failure::DuplicateKey))
    );
    let contract =
        compile(br#"{"type":"Text","definition":"urn:example:property","label":"Property"}"#);
    assert_eq!(
        contract.check_value(b"\"unfinished").err(),
        Some(ScalarError::Syntax(Failure::Malformed))
    );
    assert_eq!(
        contract.check_value(&vec![b' '; MAX_BYTES + 1]).err(),
        Some(ScalarError::Syntax(Failure::Size))
    );
    // Implementation limits, not assertions that these are malformed SWE inputs.
    for pattern in [
        r"(?=Ready)Ready",
        r"(Ready)\1",
        r"\bReady\b",
        r"^\s$",
        r"^\S+$",
        "^.$",
        "[a&&b]",
        "[a~~b]",
        "[a-z--b]",
        "[[:alpha:]]",
    ] {
        let source = json!({
            "type": "Text", "definition": "urn:example:property", "label": "Property",
            "constraint": {"pattern": pattern}
        });
        assert_eq!(
            ScalarContract::compile(validator(), &serde_json::to_vec(&source).unwrap()).err(),
            Some(ScalarError::UnsupportedFeature),
            "unsupported pattern must not be silently reinterpreted: {pattern}"
        );
    }
    for unsupported in [
        json!({"type": "Boolean", "constraint": {"values": [true]}}),
        json!({"type": "Text", "nilValues": [{"reason": "urn:example:missing", "value": "NA"}]}),
        json!({"type": "Text", "quality": [{"href": "#quality"}]}),
    ] {
        let mut source = unsupported;
        source["definition"] = json!("urn:example:property");
        source["label"] = json!("Property");
        assert_eq!(
            ScalarContract::compile(validator(), &serde_json::to_vec(&source).unwrap()).err(),
            Some(ScalarError::UnsupportedFeature)
        );
    }
}
