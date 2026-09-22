use super::*;
use std::{fs, path::PathBuf, sync::OnceLock};

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned corpus compiles offline"))
}

fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("corpus/fixtures")
            .join(name),
    )
    .unwrap()
}

#[test]
fn published_corpus_expectations() {
    let catalog = catalog().unwrap();
    crate::schema_guard::check_catalog(&catalog).unwrap();
    let cases: Value = serde_json::from_slice(&fixture("cases.json")).unwrap();
    let cases = cases["cases"].as_array().unwrap();
    assert_eq!(
        cases.len(),
        23,
        "required source-conflict corpus cannot disappear"
    );
    let mut compiled = BTreeMap::new();
    for case in cases {
        let uri = case["schema_uri"].as_str().unwrap();
        if !compiled.contains_key(uri) {
            compiled.insert(uri, compile(&catalog, uri).unwrap());
        }
        let bytes = fixture(case["input"].as_str().unwrap());
        let actual = compiled[uri].is_valid(&parse(&bytes).unwrap());
        assert_eq!(
            actual,
            case["expected_valid"].as_bool().unwrap(),
            "source expectation failed: {}",
            case["id"]
        );
    }
    println!("Published structural expectations: 23 executed; 7 accepted; 16 rejected.");
}

#[test]
fn fixed_encoding_selection() {
    let v = validator();
    let binary = fixture("binary-valid.json");
    assert_eq!(v.validate_encoding(Encoding::Binary, &binary), Ok(()));
    assert_eq!(
        v.validate_encoding(Encoding::Json, &binary),
        Err(Failure::EncodingMismatch)
    );
    assert_eq!(
        v.validate_encoding(Encoding::Text, &binary),
        Err(Failure::EncodingMismatch)
    );
    assert_eq!(
        v.validate_encoding(Encoding::Json, br#"{"type":"JSONEncoding"}"#),
        Ok(())
    );
    assert_eq!(
        v.validate_encoding(
            Encoding::Text,
            br#"{"type":"TextEncoding","tokenSeparator":",","blockSeparator":"\n"}"#
        ),
        Ok(())
    );
    assert_eq!(
        v.validate_encoding(Encoding::Binary, br#"{"type":"XMLEncoding"}"#),
        Err(Failure::EncodingMismatch)
    );
    for name in [
        "binary-missing-byte-order.json",
        "binary-invalid-byte-order.json",
        "binary-empty-members.json",
    ] {
        assert_eq!(
            v.validate_encoding(Encoding::Binary, &fixture(name)),
            Err(Failure::Structure),
            "{name}"
        );
    }
    assert_eq!(
        v.validate(
            Contract::ObservationSwe,
            &fixture("binary-wrapper-valid.json")
        ),
        Ok(())
    );
    assert_eq!(
        v.validate(
            Contract::ObservationSwe,
            &fixture("binary-wrapper-text-encoding.json")
        ),
        Err(Failure::Structure)
    );
    assert_eq!(
        v.validate(
            Contract::ObservationSwe,
            &fixture("binary-wrapper-unresolved-component.json")
        ),
        Ok(()),
        "structural success is not semantic acceptance"
    );
}

#[test]
fn limits_and_safe_parse() {
    let v = validator();
    assert_eq!(
        v.validate(Contract::Quantity, &vec![b' '; MAX_BYTES + 1]),
        Err(Failure::Size)
    );
    assert_eq!(parse(&vec![b'['; MAX_DEPTH + 1]), Err(Failure::Depth));
    let at_depth = format!("{}0{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
    assert!(parse(at_depth.as_bytes()).is_ok());
    let string = serde_json::to_vec(&"x".repeat(MAX_STRING_BYTES)).unwrap();
    assert!(parse(&string).is_ok());
    assert_eq!(
        parse(&serde_json::to_vec(&"x".repeat(MAX_STRING_BYTES + 1)).unwrap()),
        Err(Failure::String)
    );
    assert_eq!(
        parse(br#"{"label":"a","l\u0061bel":"b"}"#),
        Err(Failure::DuplicateKey)
    );
    for bad in [
        b"null null".as_slice(),
        b"{",
        b"[}",
        b"\xff",
        b"NaN",
        b"01",
        b"\"\\",
    ] {
        assert_eq!(parse(bad), Err(Failure::Malformed), "{bad:?}");
    }
    let array = serde_json::to_vec(&vec![0; MAX_MEMBERS]).unwrap();
    assert!(parse(&array).is_ok());
    assert_eq!(
        parse(&serde_json::to_vec(&vec![0; MAX_MEMBERS + 1]).unwrap()),
        Err(Failure::Members)
    );
    let many = serde_json::to_vec(&vec![vec![0; MAX_MEMBERS]; 9]).unwrap();
    assert_eq!(parse(&many), Err(Failure::Nodes));
    // Numbers are retained, not silently rounded by a floating-point parser.
    let number = b"18446744073709551616001";
    assert_eq!(parse(number).unwrap().to_string().as_bytes(), number);
}

#[test]
fn external_links_are_data_not_retrieval_instructions() {
    let corpus = catalog().unwrap();
    let observer = DenyRetrieval::default();
    let compiled =
        compile_with_denial(&corpus, &Contract::Quantity.uri(), observer.clone()).unwrap();
    let mut instance: Value = serde_json::from_slice(&fixture("quantity-labelled.json")).unwrap();
    for uri in [
        "http://127.0.0.1:9/canary",
        "file:///glaux-no-such-file",
        "data:application/json,false",
    ] {
        instance["extension"] = serde_json::json!({"$ref":uri,"href":uri});
        assert_eq!(
            validator().validate(Contract::Quantity, &serde_json::to_vec(&instance).unwrap()),
            Ok(())
        );
        assert!(compiled.is_valid(&instance));
        assert_eq!(
            observer.0.load(Ordering::Relaxed),
            0,
            "instance metadata requested retrieval"
        );
    }
    // Prove the observer is connected: missing schema URIs reach the installed
    // denial hook, which records the attempt and returns without any I/O.
    for uri in [
        "http://127.0.0.1:9/canary",
        "file:///glaux-no-such-file",
        "data:application/json,false",
    ] {
        let observer = DenyRetrieval::default();
        assert!(
            compile_with_denial(&corpus, uri, observer.clone()).is_err(),
            "unknown URI escaped: {uri}"
        );
        assert!(
            observer.0.load(Ordering::Relaxed) > 0,
            "denial observer was not connected"
        );
    }
    let observer = DenyRetrieval::default();
    let broken = BTreeMap::from([(
        "https://schemas.example/root".to_owned(),
        serde_json::json!({"$ref":"file:///unavailable-schema"}),
    )]);
    assert!(
        compile_with_denial(&broken, "https://schemas.example/root", observer.clone()).is_err()
    );
    assert!(
        observer.0.load(Ordering::Relaxed) > 0,
        "registry-preparation denial was not observed"
    );
}

#[test]
fn parser_fuzz_regressions() {
    let v = validator();
    assert_eq!(
        v.validate(
            Contract::Quantity,
            include_bytes!("../../../../fuzz/corpus/schema-parser/duplicate-label.json")
        ),
        Err(Failure::DuplicateKey)
    );
    assert_eq!(
        v.validate(
            Contract::Quantity,
            include_bytes!("../../../../fuzz/corpus/schema-parser/missing-label.json")
        ),
        Err(Failure::Structure)
    );
    for name in [
        "quantity-missing-label.json",
        "quantity-empty-label.json",
        "quantity-null-label.json",
        "quantity-numeric-label.json",
    ] {
        assert_eq!(
            v.validate(Contract::Quantity, &fixture(name)),
            Err(Failure::Structure),
            "{name}"
        );
    }
}

#[test]
fn preserves_wire_object_kind_with_private_number_member() {
    let mut instance: Value = serde_json::from_slice(&fixture("quantity-labelled.json")).unwrap();
    instance["value"] = serde_json::json!({"$serde_json::private::Number":"1"});
    assert_eq!(validator().validate(Contract::Quantity, &serde_json::to_vec(&instance).unwrap()),
               Err(Failure::Structure), "wire object must not become a number");
    let raw = br#"{"$serde_json::private::Number":"1"}"#;
    assert!(parse(raw).unwrap().is_object(), "ordinary extension member is not parser control syntax");
    instance.as_object_mut().unwrap().remove("value");
    instance["extension"] = serde_json::json!({"$serde_json::private::Number":"1"});
    let bytes = serde_json::to_vec(&instance).unwrap();
    assert_eq!(validator().validate(Contract::Quantity, &bytes), Ok(()));
    assert!(parse(&bytes).unwrap()["extension"].is_object());
}
