use super::*;
use std::sync::OnceLock;

fn validator() -> &'static ProjectionValidator {
    static VALIDATOR: OnceLock<ProjectionValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| ProjectionValidator::new().expect("pinned projections compile offline"))
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

// Independently authored synthetic inputs, not output captured from a Glaux
// serializer. Sources are the pinned CSAPI api/part1/openapi/schemas/{geojson,
// sensorml}/system.json, api/part2/openapi/schemas/json/{baseStream,dataStream,
// controlStream,observation}.json and the two stream *_create.json wrappers.
// Guide 4.3/4.6/6.2/13 supplies the explicit directional adaptation; these
// fixtures do not assert endpoints, authorization, codecs or component semantics.
// Part 2 9.2.2 permits a server-generated DataStream live flag; its omission
// below follows the selected Glaux server-summary choice, not a readOnly
// annotation (the DataStream schema does not annotate live as readOnly).
fn fixtures() -> [(Resource, Value, Value); 5] {
    [
        (
            Resource::SystemGeoJson,
            json!({
                "type": "Feature",
                "geometry": null,
                "properties": {
                    "featureType": "sosa:Sensor",
                    "uid": "urn:glaux:fixture:thermometer",
                    "name": "Thermometer"
                }
            }),
            json!({
                "type": "Feature",
                "id": "system-geo",
                "geometry": null,
                "properties": {
                    "featureType": "sosa:Sensor",
                    "uid": "urn:glaux:fixture:thermometer",
                    "name": "Thermometer"
                },
                "links": [{"rel": "self", "href": "https://example.test/systems/system-geo"}]
            }),
        ),
        (
            Resource::SystemSensorMl,
            json!({
                "type": "PhysicalSystem",
                "definition": "http://www.w3.org/ns/sosa/Sensor",
                "uniqueId": "urn:glaux:fixture:thermometer",
                "label": "Thermometer"
            }),
            json!({
                "type": "PhysicalSystem",
                "id": "system-sml",
                "definition": "http://www.w3.org/ns/sosa/Sensor",
                "uniqueId": "urn:glaux:fixture:thermometer",
                "label": "Thermometer",
                "links": [{"rel": "self", "href": "https://example.test/systems/system-sml"}]
            }),
        ),
        (
            Resource::DataStream,
            json!({
                "name": "Temperature observations",
                "schema": {
                    "obsFormat": "application/json",
                    "resultSchema": {
                        "type": "Quantity",
                        "definition": "urn:glaux:fixture:temperature",
                        "label": "Temperature",
                        "uom": {"code": "K"}
                    }
                }
            }),
            json!({
                "id": "datastream-a",
                "name": "Temperature observations",
                "formats": ["application/json"],
                "system@link": {"href": "https://example.test/systems/system-sml"},
                "observedProperties": null,
                "phenomenonTime": null,
                "resultTime": null,
                "resultType": null,
                "live": null
            }),
        ),
        (
            Resource::ControlStream,
            json!({
                "name": "Temperature set point",
                "async": false,
                "schema": {
                    "commandFormat": "application/json",
                    "parametersSchema": {
                        "type": "Quantity",
                        "definition": "urn:glaux:fixture:temperature",
                        "label": "Set point",
                        "uom": {"code": "K"}
                    }
                }
            }),
            json!({
                "id": "controlstream-a",
                "name": "Temperature set point",
                "formats": ["application/json"],
                "system@link": {"href": "https://example.test/systems/system-sml"},
                "controlledProperties": null,
                "issueTime": null,
                "executionTime": null,
                "live": null,
                "async": false
            }),
        ),
        (
            Resource::Observation,
            json!({"resultTime": "2026-01-02T03:04:05Z", "result": 291.25}),
            json!({
                "id": "observation-a",
                "datastream@id": "datastream-a",
                "resultTime": "2026-01-02T03:04:05Z",
                "result": 291.25
            }),
        ),
    ]
}

fn request_projections(resource: Resource) -> &'static [Projection] {
    // Guide 4.6 fixes System PATCH to SensorML, not GeoJSON. MergedPatch is
    // validation of the whole resulting document, never a patch fragment.
    if resource == Resource::SystemGeoJson {
        &[Projection::CreateRequest, Projection::ReplaceRequest]
    } else {
        &[
            Projection::CreateRequest,
            Projection::ReplaceRequest,
            Projection::MergedPatch,
        ]
    }
}

fn remove(value: &mut Value, pointer: &str) {
    let (parent, member) = pointer.rsplit_once('/').unwrap();
    assert!(
        value
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(member)
            .is_some(),
        "test mutation must remove an existing member: {pointer}"
    );
}

#[test]
fn minimal_requests_and_complete_responses_have_independent_contracts() {
    let v = validator();
    for (resource, request, response) in fixtures() {
        for &projection in request_projections(resource) {
            assert_eq!(
                v.validate(resource, projection, &bytes(&request)),
                Ok(()),
                "{resource:?} {projection:?} must not require generated response members"
            );
        }
        assert_eq!(
            v.validate(resource, Projection::Response, &bytes(&response)),
            Ok(()),
            "complete independently authored {resource:?} response"
        );
        assert_eq!(v.validate_original(resource, &bytes(&response)), Ok(()));
    }
}

#[test]
fn generated_response_members_remain_required() {
    let v = validator();
    for (resource, _, response) in fixtures() {
        // These lists are copied from the source required arrays (baseStream's
        // requirements included), not Resource::generated() or a production
        // schema introspector. The two System response checks are the explicit
        // response contract, beyond their permissive original required arrays.
        let members: &[&str] = match resource {
            Resource::SystemGeoJson | Resource::SystemSensorMl => &["/id", "/links"],
            Resource::DataStream => &[
                "/id", "/formats", "/system@link", "/observedProperties",
                "/phenomenonTime", "/resultTime", "/resultType", "/live",
            ],
            Resource::ControlStream => &[
                "/id", "/formats", "/system@link", "/controlledProperties",
                "/issueTime", "/executionTime", "/live",
            ],
            Resource::Observation => &["/id", "/datastream@id"],
        };
        for &member in members {
            let mut incomplete = response.clone();
            remove(&mut incomplete, member);
            let expected = if matches!(resource, Resource::SystemGeoJson | Resource::SystemSensorMl) {
                ProjectionError::Missing(member)
            } else {
                ProjectionError::Structure
            };
            assert_eq!(
                v.validate(resource, Projection::Response, &bytes(&incomplete)),
                Err(expected),
                "{resource:?} response missing {member}"
            );
        }
    }
}

#[test]
fn writable_required_members_survive_request_projection() {
    let v = validator();
    for (resource, request, _) in fixtures() {
        let members: &[&str] = match resource {
            Resource::SystemGeoJson => &[
                "/type", "/geometry", "/properties/featureType", "/properties/uid", "/properties/name",
            ],
            Resource::SystemSensorMl => &["/type", "/definition", "/uniqueId", "/label"],
            Resource::DataStream => &["/name"],
            Resource::ControlStream => &["/name", "/async"],
            Resource::Observation => &["/resultTime", "/result"],
        };
        for &member in members {
            let mut incomplete = request.clone();
            remove(&mut incomplete, member);
            for &projection in request_projections(resource) {
                assert_eq!(
                    v.validate(resource, projection, &bytes(&incomplete)),
                    Err(ProjectionError::Structure),
                    "{resource:?} {projection:?} missing writable {member}"
                );
            }
        }
        if matches!(resource, Resource::DataStream | Resource::ControlStream) {
            let mut no_schema = request.clone();
            remove(&mut no_schema, "/schema");
            assert_eq!(
                v.validate(resource, Projection::CreateRequest, &bytes(&no_schema)),
                Err(ProjectionError::Missing("/schema"))
            );
            // *_create.json adds this requirement. Replacement/candidate
            // validation does not incorrectly impose creation's wrapper.
            for projection in [Projection::ReplaceRequest, Projection::MergedPatch] {
                assert_eq!(v.validate(resource, projection, &bytes(&no_schema)), Ok(()));
            }
        }
    }
}

#[test]
fn stream_schema_is_write_only_in_responses() {
    let v = validator();
    for (resource, request, response) in fixtures() {
        if !matches!(resource, Resource::DataStream | Resource::ControlStream) {
            continue;
        }
        let mut leaked = response.clone();
        leaked["schema"] = request["schema"].clone();
        // writeOnly is an annotation, so the unmodified source accepts this
        // document. It must nevertheless fail the generated-output contract.
        assert_eq!(v.validate_original(resource, &bytes(&leaked)), Ok(()));
        for supplied_schema in [request["schema"].clone(), Value::Null, json!({})] {
            leaked["schema"] = supplied_schema;
            assert_eq!(
                v.validate(resource, Projection::Response, &bytes(&leaked)),
                Err(ProjectionError::WriteOnly("/schema")),
                "{resource:?}: even a null schema member is a write-only output leak"
            );
        }
        assert_eq!(v.validate(resource, Projection::Response, &bytes(&response)), Ok(()));
    }
}

// SWE DataRecord/Quantity, AbstractSweIdentifiable and basicTypes define these
// nested IDs and Quantity's nonempty string label. DataRecord itself does not
// require a label; this fixture deliberately omits one to catch blanket rules.
fn nested_record() -> Value {
    json!({
        "type": "DataRecord",
        "id": "record-content-id",
        "name": "readings",
        "fields": [{
            "type": "Quantity",
            "id": "quantity-content-id",
            "name": "temperature",
            "definition": "urn:glaux:fixture:temperature",
            "label": "Temperature",
            "uom": {"code": "K"}
        }]
    })
}

fn with_nested_record(resource: Resource, mut value: Value) -> (Value, &'static str) {
    let path = match resource {
        Resource::SystemSensorMl => {
            value["outputs"] = json!([nested_record()]);
            "/outputs/0/fields/0"
        }
        Resource::DataStream => {
            value["schema"]["resultSchema"] = nested_record();
            "/schema/resultSchema/fields/0"
        }
        Resource::ControlStream => {
            value["schema"]["parametersSchema"] = nested_record();
            "/schema/parametersSchema/fields/0"
        }
        _ => unreachable!("test selects only component-bearing representations"),
    };
    (value, path)
}

#[test]
fn nested_quantity_label_requirements_survive_projection() {
    let v = validator();
    for (resource, request, response) in fixtures() {
        if !matches!(resource, Resource::SystemSensorMl | Resource::DataStream | Resource::ControlStream) {
            continue;
        }
        let mut cases = vec![(Projection::CreateRequest, request.clone()),
            (Projection::ReplaceRequest, request.clone()), (Projection::MergedPatch, request)];
        // Stream responses do not carry the schema; a System's SensorML
        // output description does, and must obey the same Quantity contract.
        if resource == Resource::SystemSensorMl {
            cases.push((Projection::Response, response));
        }
        for (projection, document) in cases {
            let (valid, component_path) = with_nested_record(resource, document);
            assert_eq!(v.validate(resource, projection, &bytes(&valid)), Ok(()));
            for label in [None, Some(json!("")), Some(Value::Null), Some(json!(19))] {
                let mut invalid = valid.clone();
                let component = invalid.pointer_mut(component_path).unwrap().as_object_mut().unwrap();
                if let Some(label) = label {
                    component.insert("label".into(), label);
                } else {
                    assert!(component.remove("label").is_some());
                }
                assert_eq!(
                    v.validate(resource, projection, &bytes(&invalid)),
                    Err(ProjectionError::Structure),
                    "{resource:?} {projection:?}: nested Quantity label must not be relaxed"
                );
            }
            // minLength=1 is not a whitespace prohibition or label invention.
            let mut whitespace_label = valid;
            whitespace_label.pointer_mut(component_path).unwrap()["label"] = json!(" ");
            assert_eq!(v.validate(resource, projection, &bytes(&whitespace_label)), Ok(()));
        }
    }
}

#[test]
fn malformed_generated_members_are_not_ignored() {
    let v = validator();
    for (resource, _, response) in fixtures() {
        // Present generated metadata retains its source type constraints even
        // though requests may omit it. Do not test only absent-member cases.
        let mutations: Vec<(&str, Value)> = match resource {
            Resource::SystemGeoJson | Resource::SystemSensorMl =>
                vec![("/id", json!([])), ("/links", json!({}))],
            Resource::DataStream => vec![
                ("/formats", json!([])), ("/formats", json!([7])),
                ("/system@link", Value::Null), ("/system@link", json!({})),
                ("/observedProperties", json!([])), ("/phenomenonTime", json!(false)),
                ("/resultTime", json!({})), ("/resultType", json!("unknown-kind")),
                ("/live", json!("true")),
            ],
            Resource::ControlStream => vec![
                ("/formats", json!([])), ("/formats", json!([7])),
                ("/system@link", Value::Null), ("/system@link", json!({})),
                ("/controlledProperties", json!([])), ("/issueTime", json!({})),
                ("/executionTime", json!(false)), ("/live", json!("true")),
            ],
            Resource::Observation => vec![("/id", json!([])), ("/datastream@id", json!(true))],
        };
        for (path, replacement) in mutations {
            let mut invalid = response.clone();
            *invalid.pointer_mut(path).unwrap() = replacement;
            for projection in [Projection::ReplaceRequest, Projection::Response] {
                assert_eq!(
                    v.validate(resource, projection, &bytes(&invalid)),
                    Err(ProjectionError::Structure),
                    "{resource:?} {projection:?}: invalid known member {path}"
                );
            }
        }
    }
}

#[test]
fn only_resource_local_ids_are_ignored() {
    let v = validator();
    for (resource, mut request, _) in fixtures() {
        request["id"] = json!("untrusted-body-local-id");
        for &projection in request_projections(resource) {
            assert_eq!(v.validate(resource, projection, &bytes(&request)), Ok(()));
        }
        if matches!(resource, Resource::SystemSensorMl | Resource::DataStream | Resource::ControlStream) {
            let (valid, component_path) = with_nested_record(resource, request);
            assert_eq!(v.validate(resource, Projection::CreateRequest, &bytes(&valid)), Ok(()));
            let mut invalid = valid;
            invalid.pointer_mut(component_path).unwrap()["id"] = json!(19);
            assert_eq!(
                v.validate(resource, Projection::CreateRequest, &bytes(&invalid)),
                Err(ProjectionError::Structure),
                "nested component id is content, not the ignored outer resource id"
            );
        }
    }
    let (_, system_request, _) = fixtures().into_iter()
        .find(|(resource, _, _)| *resource == Resource::SystemSensorMl).unwrap();
    let (expected, _) = with_nested_record(Resource::SystemSensorMl, system_request);
    let mut submitted = expected.clone();
    submitted["id"] = json!("ignored-outer-id");
    let actual = v.request(
        Resource::SystemSensorMl,
        Projection::CreateRequest,
        &bytes(&submitted),
        RequestContext::default(),
    ).unwrap();
    // Exact independently specified extraction: remove only the outer ID,
    // retain both distinct content IDs and every other source member.
    assert_eq!(actual, expected);
    assert!(actual.get("id").is_none());
    assert_eq!(actual.pointer("/outputs/0/id"), Some(&json!("record-content-id")));
    assert_eq!(actual.pointer("/outputs/0/fields/0/id"), Some(&json!("quantity-content-id")));
}

#[test]
fn wrong_direction_and_original_schema_results_are_distinct() {
    let v = validator();
    for (resource, request, response) in fixtures() {
        let is_system = matches!(resource, Resource::SystemGeoJson | Resource::SystemSensorMl);
        let original_expected = if is_system { Ok(()) } else { Err(ProjectionError::Structure) };
        assert_eq!(v.validate_original(resource, &bytes(&request)), original_expected);
        assert_eq!(
            v.validate(resource, Projection::Response, &bytes(&request)),
            Err(if is_system {
                ProjectionError::Missing("/id")
            } else if matches!(resource, Resource::DataStream | Resource::ControlStream) {
                // Presence of write-only registration content takes priority
                // over the missing generated response members.
                ProjectionError::WriteOnly("/schema")
            } else {
                ProjectionError::Structure
            })
        );
        assert_eq!(v.validate(resource, Projection::CreateRequest, &bytes(&request)), Ok(()));
        // Request projection must not mutate the source validator's result.
        assert_eq!(v.validate_original(resource, &bytes(&request)), original_expected);
        if matches!(resource, Resource::DataStream | Resource::ControlStream) {
            assert_eq!(
                v.validate(resource, Projection::CreateRequest, &bytes(&response)),
                Err(ProjectionError::Missing("/schema"))
            );
        }
        let fragment = bytes(&json!({"description": "A partial update"}));
        assert_eq!(
            v.validate(resource, Projection::MergedPatch, &fragment),
            Err(if resource == Resource::SystemGeoJson {
                ProjectionError::UnsupportedProjection
            } else {
                ProjectionError::Structure
            }),
            "MergedPatch must not be treated as validation of a patch fragment"
        );
    }
}
