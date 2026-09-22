use super::*;
use std::sync::OnceLock;

fn validator() -> &'static ProjectionValidator {
    static VALIDATOR: OnceLock<ProjectionValidator> = OnceLock::new();
    VALIDATOR.get_or_init(|| ProjectionValidator::new().unwrap())
}

fn bytes(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).unwrap()
}

fn system(uid: &str) -> Value {
    json!({"type":"PhysicalSystem", "definition":"http://www.w3.org/ns/sosa/Sensor",
           "uniqueId":uid, "label":"Sensor"})
}

fn stream() -> Value {
    json!({"name":"Temperature", "schema":{"obsFormat":"application/json",
        "resultSchema":{"type":"Quantity", "label":"Temperature",
                        "definition":"urn:example:temperature", "uom":{"code":"K"}}}})
}

fn observation() -> Value {
    json!({"resultTime":"2026-01-02T03:04:05Z", "result":{"temperature":291.25}})
}

#[test]
fn trusted_context_separates_uid_parent_and_ignored_local_id() {
    let v = validator();
    let uid: Uid = "urn:example:sensor:one".parse().unwrap();
    let mut input = system(uid.as_str());
    input["id"] = json!("a-different-client-local-id");
    for projection in [Projection::ReplaceRequest, Projection::MergedPatch] {
        assert_eq!(
            v.request(
                Resource::SystemSensorMl,
                projection,
                &bytes(&input),
                RequestContext {
                    existing_uid: Some(&uid),
                    ..Default::default()
                }
            ),
            Ok(system(uid.as_str()))
        );
        let changed = system("urn:example:sensor:two");
        assert_eq!(
            v.request(
                Resource::SystemSensorMl,
                projection,
                &bytes(&changed),
                RequestContext {
                    existing_uid: Some(&uid),
                    ..Default::default()
                }
            ),
            Err(ProjectionError::Protected("/uniqueId"))
        );
        assert_eq!(
            v.request(
                Resource::SystemSensorMl,
                projection,
                &bytes(&input),
                RequestContext::default()
            ),
            Err(ProjectionError::Context)
        );
    }
    let malformed = system("relative-not-a-uid");
    assert_eq!(
        v.request(
            Resource::SystemSensorMl,
            Projection::CreateRequest,
            &bytes(&malformed),
            RequestContext::default()
        ),
        Err(ProjectionError::Structure)
    );

    // This is the canonical parent link, not the parent's separate published UID.
    let href: Uid = "https://example.test/systems/parent".parse().unwrap();
    let mut submitted = stream();
    submitted["system@link"] = json!({"href":"https://example.test/systems/other"});
    assert_eq!(
        v.request(
            Resource::DataStream,
            Projection::CreateRequest,
            &bytes(&submitted),
            RequestContext {
                parent: Some(Parent::System(&href)),
                ..Default::default()
            }
        ),
        Err(ProjectionError::Protected("/system@link"))
    );
    submitted["system@link"] = json!({"href":href.as_str()});
    assert_eq!(
        v.request(
            Resource::DataStream,
            Projection::CreateRequest,
            &bytes(&submitted),
            RequestContext {
                parent: Some(Parent::System(&href)),
                ..Default::default()
            }
        ),
        Ok(stream())
    );
    assert_eq!(
        v.request(
            Resource::DataStream,
            Projection::CreateRequest,
            &bytes(&stream()),
            RequestContext::default()
        ),
        Err(ProjectionError::Context)
    );

    let parent: LocalId = "01890f20-7b5a-7cc3-98c4-dc0c0c07398f".parse().unwrap();
    let mut obs = observation();
    obs["datastream@id"] = json!("different-parent");
    assert_eq!(
        v.request(
            Resource::Observation,
            Projection::ReplaceRequest,
            &bytes(&obs),
            RequestContext {
                parent: Some(Parent::Datastream(&parent)),
                ..Default::default()
            }
        ),
        Err(ProjectionError::Protected("/datastream@id"))
    );
    obs["datastream@id"] = json!(parent.to_string());
    obs["id"] = json!("ignored-local-id");
    assert_eq!(
        v.request(
            Resource::Observation,
            Projection::ReplaceRequest,
            &bytes(&obs),
            RequestContext {
                parent: Some(Parent::Datastream(&parent)),
                ..Default::default()
            }
        ),
        Ok(observation())
    );
}

#[test]
fn locked_contract_rejects_change_and_patch_removal() {
    let v = validator();
    let href: Uid = "https://example.test/systems/parent".parse().unwrap();
    let baseline = stream();
    let retained = bytes(&baseline["schema"]);
    for projection in [Projection::ReplaceRequest, Projection::MergedPatch] {
        assert_eq!(
            v.request(
                Resource::DataStream,
                projection,
                &bytes(&baseline),
                RequestContext {
                    parent: Some(Parent::System(&href)),
                    locked_schema: Some(&retained),
                    ..Default::default()
                }
            ),
            Ok(baseline.clone())
        );
        let mut changed = baseline.clone();
        changed["schema"]["resultSchema"]["uom"]["code"] = json!("Cel");
        assert_eq!(
            v.request(
                Resource::DataStream,
                projection,
                &bytes(&changed),
                RequestContext {
                    parent: Some(Parent::System(&href)),
                    locked_schema: Some(&retained),
                    ..Default::default()
                }
            ),
            Err(ProjectionError::Protected("/schema"))
        );
    }
    // PATCH fragment is only a member check. Its resulting complete candidate
    // must fail if applying schema:null removed a locked contract.
    assert_eq!(
        v.patch_members(Resource::DataStream, br#"{"schema":null}"#),
        Ok(json!({"schema":null}))
    );
    let without = br#"{"name":"Temperature"}"#;
    assert_eq!(
        v.request(
            Resource::DataStream,
            Projection::MergedPatch,
            without,
            RequestContext {
                parent: Some(Parent::System(&href)),
                locked_schema: Some(&retained),
                ..Default::default()
            }
        ),
        Err(ProjectionError::Protected("/schema"))
    );
    // PUT can omit this separate write-only contract without replacing it.
    assert_eq!(
        v.request(
            Resource::DataStream,
            Projection::ReplaceRequest,
            without,
            RequestContext {
                parent: Some(Parent::System(&href)),
                locked_schema: Some(&retained),
                ..Default::default()
            }
        ),
        Ok(json!({"name":"Temperature"}))
    );
}

#[test]
fn partial_patch_intent_is_checked_before_full_candidate_validation() {
    let v = validator();
    for live in [json!(true), json!(false), Value::Null] {
        assert_eq!(
            v.patch_members(Resource::ControlStream, &bytes(&json!({"live":live}))),
            Err(ProjectionError::Protected("/live"))
        );
    }
    for patch in [
        json!({"system@link":{"href":"https://example.test/systems/parent"}}),
        json!({"system@link":null}),
    ] {
        assert_eq!(
            v.patch_members(Resource::DataStream, &bytes(&patch)),
            Err(ProjectionError::Protected("/system@link"))
        );
    }
    assert_eq!(
        v.patch_members(Resource::Observation, br#"{"datastream@id":null}"#),
        Err(ProjectionError::Protected("/datastream@id"))
    );
    assert_eq!(
        v.patch_members(
            Resource::SystemSensorMl,
            br#"{"id":null,"label":"Changed"}"#
        ),
        Ok(json!({"label":"Changed"}))
    );
    assert_eq!(
        v.patch_members(Resource::SystemGeoJson, br#"{}"#),
        Err(ProjectionError::UnsupportedProjection)
    );
    assert_eq!(
        v.patch_members(Resource::Observation, br#"null"#),
        Err(ProjectionError::Structure)
    );
    let uid: Uid = "urn:example:sensor:one".parse().unwrap();
    let same = json!({"uniqueId":uid.as_str()});
    assert_eq!(
        v.patch_members(Resource::SystemSensorMl, &bytes(&same)),
        Ok(same)
    );
    // A fragment is not a full document; caller must merge then check BEFORE
    // restoring protected data. UID deletion/change is caught on that result.
    let mut removed = system(uid.as_str());
    removed.as_object_mut().unwrap().remove("uniqueId");
    assert_eq!(
        v.request(
            Resource::SystemSensorMl,
            Projection::MergedPatch,
            &bytes(&removed),
            RequestContext {
                existing_uid: Some(&uid),
                ..Default::default()
            }
        ),
        Err(ProjectionError::Structure)
    );
}

#[test]
fn observation_envelope_ignoring_is_not_recursive_or_an_alias() {
    let v = validator();
    let parent: LocalId = "01890f20-7b5a-7cc3-98c4-dc0c0c07398f".parse().unwrap();
    let expected = json!({
        "resultTime":"2026-01-02T03:04:05Z",
        "result":{"temperature":291.25,"foreign":{"id":"nested","foi@id":"nested-value"}},
        "parameters":{"custom":"retained"},
        "procedure@link":{"href":"https://example.test/procedure","custom":"retained"}
    });
    let mut submitted = expected.clone();
    submitted["foi@id"] = json!("not-a-samplingFeature-alias");
    submitted["extra"] = json!({"secret":"discard-at-root-only"});
    for projection in [
        Projection::CreateRequest,
        Projection::ReplaceRequest,
        Projection::MergedPatch,
    ] {
        assert_eq!(
            v.request(
                Resource::Observation,
                projection,
                &bytes(&submitted),
                RequestContext {
                    parent: Some(Parent::Datastream(&parent)),
                    ..Default::default()
                }
            ),
            Ok(expected.clone())
        );
    }
    assert_eq!(
        v.patch_members(Resource::Observation, &bytes(&submitted)),
        Ok(expected.clone())
    );
    let mut response = expected;
    response["id"] = json!("observation");
    response["datastream@id"] = json!(parent.to_string());
    assert_eq!(
        v.validate(
            Resource::Observation,
            Projection::Response,
            &bytes(&response)
        ),
        Ok(())
    );
    response["extra"] = json!("not-an-advertised-extension");
    assert_eq!(
        v.validate_original(Resource::Observation, &bytes(&response)),
        Ok(())
    );
    assert_eq!(
        v.validate(
            Resource::Observation,
            Projection::Response,
            &bytes(&response)
        ),
        Err(ProjectionError::UnmappedMember)
    );
}

#[test]
fn ignored_members_do_not_bypass_bounded_lossless_parsing() {
    let v = validator();
    let parent: LocalId = "01890f20-7b5a-7cc3-98c4-dc0c0c07398f".parse().unwrap();
    let duplicate = br#"{"resultTime":"2026-01-02T03:04:05Z","result":1,"extra":0,"extra":1}"#;
    assert_eq!(
        v.request(
            Resource::Observation,
            Projection::CreateRequest,
            duplicate,
            RequestContext {
                parent: Some(Parent::Datastream(&parent)),
                ..Default::default()
            }
        ),
        Err(ProjectionError::Input(validation::Failure::DuplicateKey))
    );
    let oversized = vec![b' '; validation::MAX_BYTES + 1];
    assert_eq!(
        v.patch_members(Resource::Observation, &oversized),
        Err(ProjectionError::Input(validation::Failure::Size))
    );
    let input = br#"{"resultTime":"2026-01-02T03:04:05Z","result":9007199254740993,"extra":true}"#;
    let writable = v
        .request(
            Resource::Observation,
            Projection::CreateRequest,
            input,
            RequestContext {
                parent: Some(Parent::Datastream(&parent)),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(writable["result"].to_string(), "9007199254740993");
}

#[test]
fn adaptations_are_separate_and_fail_when_source_preconditions_change() {
    let originals = validation::catalog().unwrap();
    let snapshot = originals.clone();
    let adapted = request_catalog(&originals).unwrap();
    assert_eq!(originals, snapshot);
    let key = format!("{PIN}api/part2/openapi/schemas/json/observation.json");
    assert_eq!(
        originals[&key]["required"],
        json!(["id", "datastream@id", "resultTime"])
    );
    assert_eq!(adapted[&key]["required"], json!(["resultTime"]));
    let mut wrong = originals.clone();
    wrong.get_mut(&key).unwrap()["required"] = json!(["id", "resultTime"]);
    assert_eq!(
        request_catalog(&wrong).unwrap_err(),
        "projection source required-array changed: observation.json"
    );
    let mut absent = originals;
    absent.remove(&key);
    assert_eq!(
        request_catalog(&absent).unwrap_err(),
        "projection source absent"
    );
}
