//! Original synthetic fixtures and independent expected fields: see
//! fixtures/sensorml-common/README.md and its pinned primary schema references.
//! Generated JSON is inspected before any production-type interpretation.
use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use serde_json::{Map, Value, json};

use super::{CommonDescription, Contact, DescriptionError, ValidityBound};
use crate::validation::{Failure, StructuralValidator};

const MINIMAL: &[u8] = include_bytes!("../../fixtures/sensorml-common/minimal.json");
const RICH: &[u8] = include_bytes!("../../fixtures/sensorml-common/rich.json");
const MEDIA: &str = "application/sml+json";

fn validator() -> &'static StructuralValidator {
    static VALIDATOR: OnceLock<StructuralValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| StructuralValidator::new().expect("pinned offline schema closure"))
}

fn compile(input: &[u8]) -> CommonDescription {
    CommonDescription::compile(validator(), input, MEDIA)
        .expect("source-authored common descriptive content")
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn minimal_value() -> Value {
    serde_json::from_slice(MINIMAL).unwrap()
}

fn rich_value() -> Value {
    serde_json::from_slice(RICH).unwrap()
}

/// This table is authored directly from the fixture contract, not extracted
/// from the source object or produced by the production mapper/serializer.
fn rich_expected_fields() -> Vec<(&'static str, Value)> {
    vec![
        ("/type", json!("SimpleProcess")),
        ("/id", json!("local-A")),
        ("/uniqueId", json!("urn:example:sensor:alpha")),
        ("/label", json!(" Alpha sensor ")),
        (
            "/description",
            json!("Synthetic common metadata, not a complete family mapping"),
        ),
        ("/lang", json!("en")),
        ("/keywords", json!(["temperature", "calibration"])),
        (
            "/identifiers",
            json!([
                {"definition":"urn:example:term:serial-number","label":"Serial","codeSpace":"urn:example:authority:manufacturer-a","value":"000042"},
                {"definition":"urn:example:term:serial-number","label":"Serial","codeSpace":"urn:example:authority:operator-b","value":"000042"}
            ]),
        ),
        (
            "/classifiers",
            json!([
                {"definition":"urn:example:term:sensor-kind","label":"Kind","codeSpace":"urn:example:classification:local","value":"thermometer"}
            ]),
        ),
        (
            "/validTime",
            json!([
                "2026-10-01T00:00:00.000000001Z",
                "2026-10-31T00:00:00.000000002Z"
            ]),
        ),
        (
            "/contacts",
            json!([
                {
                    "individualName":"Synthetic Operator",
                    "positionName":"Test custodian",
                    "role":"urn:example:role:custodian",
                    "contactInfo":{
                        "phone":{"voice":"000-555-0100","facsimile":"000-555-0101"},
                        "address":{
                            "deliveryPoint":"1 Example Road","city":"Test City",
                            "administrativeArea":"Example Region","postalCode":"00000",
                            "country":"Test Country","electronicMailAddress":"operator@example.invalid"
                        },
                        "website":"https://example.invalid/operator",
                        "hoursOfService":"Synthetic fixture only",
                        "contactInstructions":"Do not contact"
                    }
                },
                {
                    "role":"urn:example:role:manufacturer","name":"Synthetic Manufacturer",
                    "link":{
                        "href":"https://example.invalid/contact/A","rel":"related",
                        "type":"application/json","hreflang":"en","title":"Manufacturer record",
                        "uid":"urn:example:contact:manufacturer-a","rt":"urn:example:class:organization",
                        "if":"urn:example:interface:contact","ex:assertion":{"verified":false}
                    }
                }
            ]),
        ),
        (
            "/documents",
            json!([
                {
                    "role":"urn:example:role:calibration-report","name":"Calibration report A",
                    "description":"Synthetic and not fetched",
                    "link":{
                        "href":"https://example.invalid/document/A.pdf","rel":"describedby",
                        "type":"application/pdf","title":"Report A","uid":"urn:example:document:calibration-a"
                    }
                }
            ]),
        ),
        (
            "/ex:calibration",
            json!({
                "version":"v1","id":"asserted-not-resource-id",
                "contacts":["asserted-not-verified-actor"],
                "coefficient":9007199254740993_u64,"enabled":false,"unknown":null
            }),
        ),
        (
            "/additionalInfo",
            json!(["permitted open descriptive member", {"revision":"r1"}]),
        ),
    ]
}

fn field_mismatches(actual: &Value) -> Vec<&'static str> {
    rich_expected_fields()
        .into_iter()
        .filter_map(|(pointer, expected)| {
            (actual.pointer(pointer) != Some(&expected)).then_some(pointer)
        })
        .collect()
}

#[test]
fn sensorml_common_minimal_description_has_no_invented_metadata() {
    let contract = compile(MINIMAL);
    assert_eq!(contract.type_name(), "SimpleProcess");
    assert_eq!(contract.label(), " Minimal description ");
    assert_eq!(contract.unique_id(), "urn:example:sensor:minimal");
    assert_eq!(contract.id(), None);
    assert_eq!(contract.identifiers(), None);
    assert_eq!(contract.classifiers(), None);
    assert_eq!(contract.contacts(), None);
    assert_eq!(contract.documents(), None);
    assert_eq!(contract.valid_time(), None);
    assert!(contract.remaining_members().is_empty());
    assert_eq!(
        contract.generated_json(),
        json!({
            "type":"SimpleProcess","label":" Minimal description ","uniqueId":"urn:example:sensor:minimal"
        })
    );
}

#[test]
fn sensorml_common_generated_fields_match_independent_oracle() {
    let generated = compile(RICH).generated_json();
    assert_eq!(
        field_mismatches(&generated),
        Vec::<&str>::new(),
        "all common descriptive fields must retain independently expected meaning"
    );
    assert_eq!(
        generated.as_object().unwrap().len(),
        14,
        "the mapper must not add identity, authority or policy fields"
    );
}

#[test]
fn sensorml_common_source_bytes_digest_and_owned_state_are_separate() {
    let mut caller_bytes = RICH.to_vec();
    let contract = compile(&caller_bytes);
    caller_bytes.fill(b' ');
    drop(caller_bytes);
    assert_eq!(contract.source_bytes(), RICH);
    assert_eq!(contract.source_media_type(), MEDIA);
    // Independent SHA-256 calculated with PowerShell/.NET Get-FileHash over
    // the checked-in LF fixture bytes, not the implementation's digest helper.
    let actual = contract
        .source_sha256()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(
        actual,
        "a8a45ed90fd2728e1aa968e1283cac37cb4996810babab99e016095965b9bc2b"
    );
    let generated_bytes = bytes(&contract.generated_json());
    assert_ne!(
        generated_bytes, RICH,
        "generated JSON is not retained source bytes"
    );
    assert!(field_mismatches(&contract.generated_json()).is_empty());
}

#[test]
fn sensorml_common_reordered_members_preserve_meaning_not_source_digest() {
    let original = compile(RICH);
    let source = rich_value();
    let fields = source
        .as_object()
        .unwrap()
        .iter()
        .rev()
        .map(|(key, value)| {
            format!(
                "{}:{}",
                serde_json::to_string(key).unwrap(),
                serde_json::to_string(value).unwrap()
            )
        })
        .collect::<Vec<_>>();
    let reordered = format!("{{{}}}", fields.join(","));
    assert_ne!(reordered.as_bytes(), RICH);
    let contract = compile(reordered.as_bytes());
    assert_eq!(contract.generated_json(), original.generated_json());
    assert!(field_mismatches(&contract.generated_json()).is_empty());
    assert_eq!(contract.source_bytes(), reordered.as_bytes());
    assert_ne!(contract.source_sha256(), original.source_sha256());
}

#[test]
fn sensorml_common_identifier_authorities_and_classifier_definitions_are_distinct() {
    let contract = compile(RICH);
    let identifiers = contract.identifiers().unwrap();
    assert_eq!(identifiers.len(), 2);
    assert_eq!(identifiers[0].value, "000042");
    assert_eq!(identifiers[1].value, "000042");
    assert_eq!(
        identifiers[0].code_space.as_deref(),
        Some("urn:example:authority:manufacturer-a")
    );
    assert_eq!(
        identifiers[1].code_space.as_deref(),
        Some("urn:example:authority:operator-b")
    );
    assert_eq!(
        identifiers[0].definition.as_deref(),
        Some("urn:example:term:serial-number")
    );
    let classifiers = contract.classifiers().unwrap();
    assert_eq!(
        classifiers[0].definition.as_deref(),
        Some("urn:example:term:sensor-kind")
    );
    assert_eq!(
        classifiers[0].code_space.as_deref(),
        Some("urn:example:classification:local")
    );
    assert_eq!(classifiers[0].value, "thermometer");
    assert_eq!(contract.id(), Some("local-A"));
    assert_eq!(contract.unique_id(), "urn:example:sensor:alpha");
}

#[test]
fn sensorml_common_contacts_keep_inline_linked_and_organization_forms() {
    let contract = compile(RICH);
    assert!(matches!(
        &contract.contacts().unwrap()[0],
        Contact::Party(_)
    ));
    assert!(matches!(&contract.contacts().unwrap()[1], Contact::Link(_)));
    let mut source = minimal_value();
    source["contacts"] = json!([{
        "organisationName":"Synthetic Organization","role":"urn:example:role:operator"
    }]);
    let organization = compile(&bytes(&source));
    assert!(matches!(
        &organization.contacts().unwrap()[0],
        Contact::Party(_)
    ));
    assert_eq!(
        organization.generated_json()["contacts"],
        json!([{
            "organisationName":"Synthetic Organization","role":"urn:example:role:operator"
        }])
    );
}

#[test]
fn sensorml_common_validity_keeps_precision_offsets_and_now_symbolic() {
    let rich = compile(RICH);
    let validity = rich.valid_time().unwrap();
    assert!(matches!(&validity.start, ValidityBound::Instant(_)));
    assert!(matches!(&validity.end, ValidityBound::Instant(_)));
    let mut source = minimal_value();
    source["validTime"] = json!(["2026-10-01T01:00:00.000000001+01:00", "now"]);
    let contract = compile(&bytes(&source));
    assert!(matches!(
        &contract.valid_time().unwrap().end,
        ValidityBound::Now
    ));
    assert_eq!(
        contract.generated_json()["validTime"],
        json!(["2026-10-01T01:00:00.000000001+01:00", "now"])
    );
    source["validTime"] = json!(["now", "now"]);
    let symbolic = compile(&bytes(&source));
    assert!(matches!(
        &symbolic.valid_time().unwrap().start,
        ValidityBound::Now
    ));
    assert_eq!(
        symbolic.generated_json()["validTime"],
        json!(["now", "now"])
    );
}

#[test]
fn sensorml_common_reversed_or_malformed_validity_is_rejected() {
    let mut source = minimal_value();
    source["validTime"] = json!([
        "2026-10-01T00:00:00.000000002Z",
        "2026-10-01T00:00:00.000000001Z"
    ]);
    assert_eq!(
        CommonDescription::compile(validator(), &bytes(&source), MEDIA).err(),
        Some(DescriptionError::ReversedValidity)
    );
    // Equal instants with different source offsets are not reversed.
    source["validTime"] = json!(["2026-10-01T01:00:00+01:00", "2026-10-01T00:00:00Z"]);
    compile(&bytes(&source));
    for invalid in [
        json!(["2026-02-30T00:00:00Z", "now"]),
        json!(["now"]),
        json!([null, "now"]),
        json!(["yesterday", "now"]),
    ] {
        source["validTime"] = invalid;
        assert!(CommonDescription::compile(validator(), &bytes(&source), MEDIA).is_err());
    }
}

#[test]
fn sensorml_common_malformed_typed_fields_fail_not_become_extensions() {
    let mutations = [
        ("/id", json!(17)),
        ("/uniqueId", json!("not a URI")),
        ("/label", json!("")),
        ("/identifiers/0/value", json!(42)),
        ("/identifiers/0/codeSpace", json!("not a URI")),
        ("/classifiers/0/definition", json!(true)),
        ("/contacts/0/role", json!(12)),
        (
            "/contacts/0/contactInfo/address/electronicMailAddress",
            json!("not an email"),
        ),
        ("/contacts/1/link/href", json!({"nested":"wrong shape"})),
        ("/documents/0/name", json!(false)),
        ("/documents/0/link/href", json!("not a URI")),
    ];
    for (pointer, invalid) in mutations {
        let mut source = rich_value();
        *source.pointer_mut(pointer).unwrap() = invalid;
        assert!(
            CommonDescription::compile(validator(), &bytes(&source), MEDIA).is_err(),
            "malformed typed field must fail: {pointer}"
        );
    }
    let mut term_extra = rich_value();
    term_extra["identifiers"][0]["unrecognized"] = json!("not allowed by closed Term schema");
    assert!(CommonDescription::compile(validator(), &bytes(&term_extra), MEDIA).is_err());
    let mut ambiguous_party = rich_value();
    ambiguous_party["contacts"][0]["organisationName"] = json!("Both branches would match");
    assert!(CommonDescription::compile(validator(), &bytes(&ambiguous_party), MEDIA).is_err());
}

#[test]
fn sensorml_common_extension_content_is_retained_without_identity_promotion() {
    let contract = compile(RICH);
    assert_eq!(contract.id(), Some("local-A"));
    assert_eq!(contract.contacts().unwrap().len(), 2);
    assert_eq!(
        contract.remaining_members()["ex:calibration"]["id"],
        json!("asserted-not-resource-id")
    );
    assert_eq!(
        contract.remaining_members()["ex:calibration"]["coefficient"],
        json!(9007199254740993_u64)
    );
    let mut added = Map::new();
    added.insert(
        "org:provenance".into(),
        json!({"id":"unverified assertion","role":"unverified creator"}),
    );
    let generated = contract.with_extensions(&added).unwrap();
    assert_eq!(
        generated["org:provenance"],
        json!({"id":"unverified assertion","role":"unverified creator"})
    );
    assert!(field_mismatches(&generated).is_empty());
    assert!(contract.generated_json().get("org:provenance").is_none());
    assert_eq!(contract.source_bytes(), RICH);
}

#[test]
fn sensorml_common_extensions_cannot_replace_reserved_or_existing_members() {
    let contract = compile(RICH);
    for reserved in [
        "id",
        "uniqueId",
        "type",
        "label",
        "identifiers",
        "contacts",
        "typeOf",
        "components",
        "systemKind@link",
        "system@link",
        "links",
    ] {
        let additions = Map::from_iter([(reserved.to_owned(), json!("replacement"))]);
        assert!(
            contract.with_extensions(&additions).is_err(),
            "reserved name: {reserved}"
        );
        assert!(field_mismatches(&contract.generated_json()).is_empty());
    }
    let collision = Map::from_iter([("ex:calibration".into(), json!({"version":"replace"}))]);
    assert_eq!(
        contract.with_extensions(&collision).err(),
        Some(DescriptionError::ExtensionCollision)
    );
    let unqualified = Map::from_iter([("unqualifiedNew".into(), json!(true))]);
    assert_eq!(
        contract.with_extensions(&unqualified).err(),
        Some(DescriptionError::ExtensionNamespace)
    );
}

#[test]
fn sensorml_common_extension_preflight_and_combined_output_are_bounded() {
    let contract = compile(MINIMAL);
    let mut deep = Value::Null;
    for _ in 0..crate::validation::MAX_DEPTH {
        deep = Value::Array(vec![deep]);
    }
    for (value, failure) in [
        (
            json!("a".repeat(crate::validation::MAX_STRING_BYTES + 1)),
            Failure::String,
        ),
        (
            Value::Array(vec![Value::Null; crate::validation::MAX_MEMBERS + 1]),
            Failure::Members,
        ),
        (deep, Failure::Depth),
        (
            Value::Array(vec![Value::Array(vec![Value::Null; 500]); 9]),
            Failure::Nodes,
        ),
    ] {
        let additions = Map::from_iter([("ex:bounded".into(), value)]);
        assert_eq!(
            contract.with_extensions(&additions).err(),
            Some(DescriptionError::Syntax(failure))
        );
    }
    let mut source = minimal_value();
    source["ex:existing"] = Value::Array(vec![Value::Array(vec![Value::Null; 500]); 6]);
    let larger = compile(&bytes(&source));
    let additions = Map::from_iter([(
        "ex:added".into(),
        Value::Array(vec![Value::Array(vec![Value::Null; 500]); 3]),
    )]);
    assert!(
        contract.with_extensions(&additions).is_ok(),
        "the added value alone fits"
    );
    assert_eq!(
        larger.with_extensions(&additions).err(),
        Some(DescriptionError::Syntax(Failure::Nodes)),
        "the complete generated document, not each part separately, must fit"
    );
    assert_eq!(larger.source_bytes(), bytes(&source));
}

#[test]
fn sensorml_common_duplicate_identity_and_input_limits_fail_safely() {
    let duplicate = br#"{"type":"SimpleProcess","label":"test","uniqueId":"urn:example:A","uniqueId":"urn:example:B"}"#;
    assert_eq!(
        CommonDescription::compile(validator(), duplicate, MEDIA).err(),
        Some(DescriptionError::Syntax(Failure::DuplicateKey))
    );
    let oversized = vec![b' '; crate::validation::MAX_BYTES + 1];
    assert_eq!(
        CommonDescription::compile(validator(), &oversized, MEDIA).err(),
        Some(DescriptionError::Syntax(Failure::Size))
    );
    assert_eq!(
        CommonDescription::compile(validator(), MINIMAL, "application/json\n").err(),
        Some(DescriptionError::MediaType)
    );
}

#[test]
fn sensorml_common_empty_optional_lists_remain_distinct_from_absence() {
    let mut source = minimal_value();
    for key in ["identifiers", "classifiers", "contacts", "documents"] {
        source[key] = json!([]);
    }
    let contract = compile(&bytes(&source));
    assert_eq!(contract.identifiers().unwrap().len(), 0);
    assert_eq!(contract.classifiers().unwrap().len(), 0);
    assert_eq!(contract.contacts().unwrap().len(), 0);
    assert_eq!(contract.documents().unwrap().len(), 0);
    for key in ["identifiers", "classifiers", "contacts", "documents"] {
        assert_eq!(contract.generated_json()[key], json!([]));
    }
}

/// A live controlled endpoint with an explicit sensitivity control. Completion
/// is synchronized by joining, not inferred from a sleep window. Responses let
/// an accidental synchronous metadata fetch finish and be counted rather than
/// hanging the test. Drop joins even if an assertion unwinds.
struct Canary {
    address: String,
    count: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl Canary {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("owned loopback canary");
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap().to_string();
        let count = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_count = Arc::clone(&count);
        let worker_stop = Arc::clone(&stop);
        let worker = thread::spawn(move || {
            while !worker_stop.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut connection, _)) => {
                        worker_count.fetch_add(1, Ordering::SeqCst);
                        connection
                            .set_write_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        connection.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::yield_now()
                    }
                    Err(error) => panic!("canary accept failed: {error}"),
                }
            }
        });
        Self {
            address,
            count,
            stop,
            worker: Some(worker),
        }
    }

    fn verify_sensitive(&self) {
        let mut connection = TcpStream::connect(&self.address).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut response = String::new();
        connection.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert_eq!(self.count.load(Ordering::SeqCst), 1);
    }

    fn finish(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.worker
            .take()
            .unwrap()
            .join()
            .expect("canary worker succeeded");
    }
}

impl Drop for Canary {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            if !thread::panicking() {
                result.expect("canary worker cleanup succeeded");
            }
        }
    }
}

#[test]
fn sensorml_common_links_and_reference_shaped_extensions_never_fetch() {
    let mut canary = Canary::new();
    canary.verify_sensitive();
    let url = format!("http://{}/must-not-fetch", canary.address);
    let mut source = rich_value();
    source["identifiers"][0]["definition"] = json!(url);
    source["identifiers"][0]["codeSpace"] = json!(url);
    source["contacts"][1]["link"]["href"] = json!(url);
    source["documents"][0]["link"]["href"] = json!(url);
    source["ex:reference"] = json!({"$ref":url});
    let generated = compile(&bytes(&source)).generated_json();
    assert_eq!(generated["contacts"][1]["link"]["href"], json!(url));
    assert_eq!(generated["documents"][0]["link"]["href"], json!(url));
    assert_eq!(generated["ex:reference"], json!({"$ref":url}));
    source["identifiers"][0]["value"] = json!(17);
    assert!(CommonDescription::compile(validator(), &bytes(&source), MEDIA).is_err());
    canary.finish();
    assert_eq!(
        canary.count.load(Ordering::SeqCst),
        1,
        "only the explicit sensitivity-control connection is permitted"
    );
}

#[test]
fn sensorml_common_oracle_detects_contact_extension_and_identifier_corruption() {
    let correct = compile(RICH).generated_json();
    assert!(field_mismatches(&correct).is_empty());
    let mut no_contact = correct.clone();
    no_contact["contacts"].as_array_mut().unwrap().remove(0);
    assert_eq!(field_mismatches(&no_contact), vec!["/contacts"]);
    let mut no_extension = correct.clone();
    no_extension
        .as_object_mut()
        .unwrap()
        .remove("ex:calibration");
    assert_eq!(field_mismatches(&no_extension), vec!["/ex:calibration"]);
    let mut wrong_identifier = correct;
    wrong_identifier["identifiers"][0]["value"] = json!("wrong-but-valid-identifier");
    assert_eq!(field_mismatches(&wrong_identifier), vec!["/identifiers"]);
}
