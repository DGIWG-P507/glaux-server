//! Explicitly configured minimal System POST and canonical GET. No full CRUD claim.
use crate::application::{AuditContext, AuditId, CreateSystem, EventId, RetryKey};
use crate::authentication::CallerContext;
use crate::authorization::{AccessError, Admission, CurrentSystem, OperationContext};
use crate::configuration::Configuration;
use crate::discovery;
use crate::http_boundary::{HttpBoundary, Problem, negotiate};
use crate::revisions::{ArtifactId, NewSourceArtifact, RevisionId, SystemRevision};
use crate::storage::SystemRecord;
use axum::Router;
use axum::extract::{Path, Request, State, rejection::PathRejection};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use glaux_domain::identity::{LocalId, Uid};
use glaux_domain::temporal::ExactInstant;
use glaux_standards::projection::{Projection, ProjectionValidator, RequestContext, Resource};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use std::sync::Arc;

const INPUT_MEDIA: &str = "application/geo+json";

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SystemCreationConfig {
    pub source: String,
    pub retry_retention_seconds: u32,
}

impl SystemCreationConfig {
    pub(crate) fn valid(&self) -> bool {
        !self.source.is_empty()
            && self.source.len() <= 256
            && !self.source.chars().any(char::is_control)
            && self.retry_retention_seconds > 0
    }
}

#[derive(Clone)]
struct SystemState {
    pool: PgPool,
    boundary: HttpBoundary,
    admission: Admission,
    config: SystemCreationConfig,
    validator: Arc<ProjectionValidator>,
}

enum Error {
    Http(Problem),
    Access(AccessError),
}

impl From<Problem> for Error {
    fn from(value: Problem) -> Self {
        Self::Http(value)
    }
}

impl From<AccessError> for Error {
    fn from(value: AccessError) -> Self {
        Self::Access(value)
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match self {
            Self::Http(error) => error.into_response(),
            Self::Access(error) => error.into_response(),
        }
    }
}

/// Compile pinned schemas offline before exposing the route. No fetch, migration
/// or write occurs here. Call only for an explicitly enabled configuration.
pub fn router(config: &Configuration, pool: PgPool) -> Result<Router, Problem> {
    let state = SystemState {
        pool,
        boundary: config.http_boundary(),
        admission: config.admission(),
        config: config.system_creation().ok_or_else(Problem::internal)?,
        validator: Arc::new(ProjectionValidator::new().map_err(|_| Problem::unavailable())?),
    };
    let routes = Router::new()
        .route(
            discovery::SYSTEM_CREATE.path(),
            discovery::SYSTEM_CREATE.method(create),
        )
        .route(
            discovery::SYSTEM_READ.path(),
            discovery::SYSTEM_READ.method(read),
        )
        .with_state(state);
    Ok(config.authenticator().protect(routes))
}

fn retry(headers: &HeaderMap, retention_seconds: u32) -> Result<Option<RetryKey>, Problem> {
    let mut values = headers.get_all("idempotency-key").iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    let key = value.to_str().map_err(|_| Problem::bad_request())?;
    if values.next().is_some()
        || key.is_empty()
        || key.len() > 256
        || !key.bytes().all(|byte| byte.is_ascii_graphic())
    {
        return Err(Problem::bad_request());
    }
    Ok(Some(RetryKey {
        key: key.to_owned(),
        retention_seconds,
    }))
}

/// RFC 9110 entity-tag list syntax. No current /systems collection representation
/// is exposed at this stage, so every If-Match fails and every If-None-Match
/// passes. Conditions never describe the newly generated member identifier.
fn entity_condition(headers: &HeaderMap, name: HeaderName) -> Result<bool, Problem> {
    let mut bytes = Vec::new();
    let mut present = false;
    for value in headers.get_all(name) {
        if value.as_bytes().len() + usize::from(present) > 4096usize.saturating_sub(bytes.len()) {
            return Err(Problem::bad_request());
        }
        if present {
            bytes.push(b',');
        }
        present = true;
        bytes.extend_from_slice(value.as_bytes());
    }
    if !present {
        return Ok(false);
    }
    let trim = |slice: &[u8]| {
        let start = slice
            .iter()
            .position(|byte| !matches!(byte, b' ' | b'\t'))
            .unwrap_or(slice.len());
        let end = slice
            .iter()
            .rposition(|byte| !matches!(byte, b' ' | b'\t'))
            .map_or(start, |index| index + 1);
        (start, end)
    };
    let (start, end) = trim(&bytes);
    let bytes = &bytes[start..end];
    if bytes == b"*" {
        return Ok(true);
    }
    let mut index = 0;
    let mut tags = 0;
    let mut empty = 0;
    while index < bytes.len() {
        while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
            index += 1;
        }
        if index == bytes.len() {
            break;
        }
        if bytes[index] == b',' {
            empty += 1;
            if empty > 64 {
                return Err(Problem::bad_request());
            }
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"W/") {
            index += 2;
        }
        if bytes.get(index) != Some(&b'"') {
            return Err(Problem::bad_request());
        }
        index += 1;
        while let Some(&byte) = bytes.get(index) {
            if byte == b'"' {
                break;
            }
            if !(byte == 0x21 || (0x23..=0x7e).contains(&byte) || byte >= 0x80) {
                return Err(Problem::bad_request());
            }
            index += 1;
        }
        if bytes.get(index) != Some(&b'"') {
            return Err(Problem::bad_request());
        }
        index += 1;
        tags += 1;
        if tags > 64 {
            return Err(Problem::bad_request());
        }
        while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') {
            index += 1;
        }
        if index < bytes.len() {
            if bytes[index] != b',' {
                return Err(Problem::bad_request());
            }
            index += 1;
        }
    }
    // RFC 9110 uses #entity-tag, not 1#entity-tag: a present empty list is valid
    // and distinct from an absent field. Bounded empty elements are ignored.
    Ok(true)
}

fn precondition(headers: &HeaderMap) -> Result<bool, Problem> {
    let has_match = entity_condition(headers, header::IF_MATCH)?;
    entity_condition(headers, header::IF_NONE_MATCH)?;
    // If-Modified-Since applies only to GET/HEAD; If-Unmodified-Since is ignored
    // when this target supplies no last-modification date. Neither is mandatory.
    Ok(!has_match)
}

fn minimal(validator: &ProjectionValidator, bytes: &[u8]) -> Result<(Uid, String), Problem> {
    let value = validator
        .request(
            Resource::SystemGeoJson,
            Projection::CreateRequest,
            bytes,
            RequestContext::default(),
        )
        .map_err(|_| Problem::bad_request())?;
    let root = value.as_object().ok_or_else(Problem::bad_request)?;
    let properties = value
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(Problem::bad_request)?;
    if root
        .keys()
        .any(|name| !["type", "geometry", "properties"].contains(&name.as_str()))
        || properties
            .keys()
            .any(|name| !["uid", "name", "featureType"].contains(&name.as_str()))
        || value.get("geometry") != Some(&Value::Null)
    {
        return Err(Problem::unprocessable());
    }
    let uid = properties
        .get("uid")
        .and_then(Value::as_str)
        .ok_or_else(Problem::bad_request)?
        .parse::<Uid>()
        .map_err(|_| Problem::bad_request())?;
    let label = properties
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(Problem::bad_request)?;
    if label.len() > 4096 {
        return Err(Problem::unprocessable());
    }
    // featureType's ten exact schema spellings survive in the retained artifact;
    // this initial normalized identity table does not reinterpret or erase them.
    Ok((uid, label.to_owned()))
}

async fn create(State(state): State<SystemState>, request: Request) -> Result<Response, Error> {
    let caller = request
        .extensions()
        .get::<CallerContext>()
        .cloned()
        .ok_or_else(Problem::unauthorized)?;
    let mut connection = state
        .pool
        .acquire()
        .await
        .map_err(|_| Problem::unavailable())?;
    // One explicit trusted operation-receipt sample, before the write transaction.
    let time = receipt_time(&mut connection).await?;
    let context = OperationContext::new(caller, time.clone())?;
    state
        .admission
        .preflight_system_create(&mut connection, &context, &state.config.source)
        .await?;
    let retry = retry(request.headers(), state.config.retry_retention_seconds)?;
    let precondition = precondition(request.headers())?;
    let bytes = state
        .boundary
        .read_json_bytes(request, &[INPUT_MEDIA])
        .await?;
    let (uid, label) = minimal(&state.validator, &bytes)?;
    let system_id = LocalId::generate().map_err(|_| Problem::unavailable())?;
    let artifact_id = ArtifactId::generate().map_err(|_| Problem::unavailable())?;
    let revision_id = RevisionId::generate().map_err(|_| Problem::unavailable())?;
    let audit_id = AuditId::generate().map_err(|_| Problem::unavailable())?;
    let event_id = EventId::generate().map_err(|_| Problem::unavailable())?;
    let input = CreateSystem {
        system: SystemRecord {
            id: system_id,
            uid,
            label,
            sources: vec![],
            parent: None,
        },
        artifact: NewSourceArtifact {
            id: artifact_id,
            media_type: INPUT_MEDIA.to_owned(),
            bytes: bytes.to_vec(),
        },
        revision: SystemRevision {
            id: revision_id,
            system_id,
            artifact_id,
            semantic_time: None,
            receipt_time: time.clone(),
        },
        audit_id,
        event_id,
        // Admission replaces all attribution with verified operation context.
        audit: AuditContext {
            actor: None,
            source: None,
            correlation: context.correlation().to_owned(),
            time,
        },
    };
    let receipt = state
        .admission
        .create_system_if(
            &mut connection,
            &context,
            &state.config.source,
            input,
            retry.as_ref(),
            precondition,
        )
        .await?;
    let location = canonical(&state.boundary, receipt.system_id)?;
    let location = HeaderValue::from_str(&location).map_err(|_| Problem::internal())?;
    let correlation =
        HeaderValue::from_str(context.correlation()).map_err(|_| Problem::internal())?;
    // The empty creation receipt negotiates no resource representation. No ETag,
    // body or internal revision is disclosed; GET on Location returns the System.
    Ok((
        StatusCode::CREATED,
        [
            (header::LOCATION, location),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            ),
            (HeaderName::from_static("x-request-id"), correlation),
        ],
    )
        .into_response())
}

/// No request timestamp, UUID timestamp, implicit column default or exact
/// network-arrival/commit-time claim is substituted for this observation.
async fn receipt_time(connection: &mut PgConnection) -> Result<ExactInstant, Problem> {
    let lexeme: String = sqlx::query_scalar(
        "SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')",
    )
    .fetch_one(connection)
    .await
    .map_err(|_| Problem::unavailable())?;
    ExactInstant::parse_rfc3339(&lexeme).map_err(|_| Problem::unavailable())
}

/// The one canonical member URL: the creation Location and the GET self link.
fn canonical(boundary: &HttpBoundary, id: LocalId) -> Result<String, Problem> {
    boundary.link(&["systems", &id.to_string()], &[])
}

/// Canonical GET/HEAD. The authentication layer marks every authenticated
/// outcome, including problems, `private, no-store`. Missing, concealed and
/// non-canonical IDs share one 404.
async fn read(
    State(state): State<SystemState>,
    id: Result<Path<String>, PathRejection>,
    request: Request,
) -> Result<Response, Error> {
    let caller = request
        .extensions()
        .get::<CallerContext>()
        .cloned()
        .ok_or_else(Problem::unauthorized)?;
    // Absent Accept selects the only offered representation.
    negotiate(request.headers(), &[INPUT_MEDIA])?;
    // LocalId accepts only the canonical lowercase UUIDv7 spelling. Any other
    // path value names no stored System and gets the same safe 404.
    let id = id
        .ok()
        .and_then(|Path(text)| text.parse::<LocalId>().ok())
        .ok_or_else(Problem::not_found)?;
    let mut connection = state
        .pool
        .acquire()
        .await
        .map_err(|_| Problem::unavailable())?;
    let time = receipt_time(&mut connection).await?;
    let context = OperationContext::new(caller, time)?;
    // SYSTEM_READ_STORAGE: the persisted, authorized source of the response.
    let system = state
        .admission
        .current_system(&mut connection, &context, id)
        .await?;
    let value = representation(&state.boundary, &system)?;
    let bytes = serde_json::to_vec(&value).map_err(|_| Problem::internal())?;
    // Never emit a System that the pinned response projection rejects.
    state
        .validator
        .validate(Resource::SystemGeoJson, Projection::Response, &bytes)
        .map_err(|_| Problem::internal())?;
    let correlation =
        HeaderValue::from_str(context.correlation()).map_err(|_| Problem::internal())?;
    // No ETag or conditional GET in this increment (Roadmap 2.4.9 owns them).
    Ok((
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(INPUT_MEDIA)),
            (header::VARY, HeaderValue::from_static("Accept")),
            (HeaderName::from_static("x-request-id"), correlation),
        ],
        bytes,
    )
        .into_response())
}

/// Stored identity and label, the exact featureType spelling retained in the
/// current accepted source, and generated canonical links. Nothing else from
/// the retained source (such as a supplied id or links) is echoed.
fn representation(boundary: &HttpBoundary, system: &CurrentSystem) -> Result<Value, Problem> {
    if system.media_type != INPUT_MEDIA {
        return Err(Problem::internal());
    }
    let source: Value = serde_json::from_slice(&system.bytes).map_err(|_| Problem::internal())?;
    let feature_type = source
        .pointer("/properties/featureType")
        .and_then(Value::as_str)
        .ok_or_else(Problem::internal)?;
    // SYSTEM_READ_IDENTITY: the body id is the stored canonical local ID.
    let id = system.id.to_string();
    let self_link = json!({
        "href": canonical(boundary, system.id)?,
        "rel": "self",
        "type": INPUT_MEDIA,
        "title": "This System"
    });
    let mut links = vec![self_link];
    // The authorized statement returns a child only when its parent is visible.
    if let Some(parent) = system.parent {
        links.push(json!({
            "href": canonical(boundary, parent)?,
            "rel": "ogc-rel:parentSystem",
            "type": INPUT_MEDIA,
            "title": "Parent System"
        }));
    }
    Ok(json!({
        "type": "Feature",
        "id": id,
        "geometry": null,
        "properties": {
            "uid": system.uid.as_str(),
            "name": system.label,
            "featureType": feature_type
        },
        "links": links
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_boundary::Limits;

    #[test]
    fn system_creation_conditions_target_absent_collection_representation() {
        let mut headers = HeaderMap::new();
        assert!(precondition(&headers).unwrap());
        for value in [
            "",
            " \t ",
            ",,,",
            "*",
            "\"old\"",
            "W/\"old\"",
            "\"comma,inside\", W/\"second\"",
            ",\"tag\",,",
        ] {
            headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
            assert!(!precondition(&headers).unwrap(), "{value}");
            headers.remove(header::IF_MATCH);
            headers.insert(header::IF_NONE_MATCH, HeaderValue::from_str(value).unwrap());
            assert!(precondition(&headers).unwrap(), "{value}");
            headers.remove(header::IF_NONE_MATCH);
        }
        for value in [
            "w/\"lowercase\"",
            "*,\"tag\"",
            "\"unfinished",
            "\"bad space\"",
            "tag",
            "\"one\" \"two\"",
        ] {
            headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
            assert!(precondition(&headers).is_err(), "{value}");
        }
        headers.clear();
        headers.insert(
            header::IF_UNMODIFIED_SINCE,
            HeaderValue::from_static("Wed, 01 Jan 2020 00:00:00 GMT"),
        );
        headers.insert(
            header::IF_MODIFIED_SINCE,
            HeaderValue::from_static("Wed, 01 Jan 2020 00:00:00 GMT"),
        );
        assert!(precondition(&headers).unwrap());
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"one\""));
        headers.append(header::IF_MATCH, HeaderValue::from_static("W/\"two\""));
        assert!(!precondition(&headers).unwrap());
        headers.insert(
            header::IF_MATCH,
            HeaderValue::from_str(&format!("\"{}\"", "x".repeat(4096))).unwrap(),
        );
        assert!(precondition(&headers).is_err());
        headers.insert(
            header::IF_MATCH,
            HeaderValue::from_str(&",".repeat(64)).unwrap(),
        );
        assert!(!precondition(&headers).unwrap());
        headers.insert(
            header::IF_MATCH,
            HeaderValue::from_str(&",".repeat(65)).unwrap(),
        );
        assert!(precondition(&headers).is_err());
    }

    #[test]
    fn system_creation_projection_retains_supported_meaning_and_bounds() {
        let validator = ProjectionValidator::new().unwrap();
        let mut value = json!({"type":"Feature", "geometry":null, "properties":{"uid":"urn:glaux:test:http-system", "name":"First System", "featureType":"sosa:Sensor"}});
        let (uid, label) = minimal(&validator, value.to_string().as_bytes()).unwrap();
        assert_eq!(uid.as_str(), "urn:glaux:test:http-system");
        assert_eq!(label, "First System");
        for tag in ["Sensor", "Actuator", "Sampler", "Platform", "System"] {
            for prefix in ["sosa:", "http://www.w3.org/ns/sosa/"] {
                value["properties"]["featureType"] = json!(format!("{prefix}{tag}"));
                assert!(minimal(&validator, value.to_string().as_bytes()).is_ok());
            }
        }
        value["id"] = json!("client-selected-id");
        value["links"] = json!([{"href":"https://example.test/untrusted-link", "rel":"related"}]);
        assert!(minimal(&validator, value.to_string().as_bytes()).is_ok());
        let mut invalid = value.clone();
        invalid["links"] = json!([]);
        assert_eq!(
            minimal(&validator, invalid.to_string().as_bytes())
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        invalid = value.clone();
        invalid["properties"]["name"] = json!("");
        assert_eq!(
            minimal(&validator, invalid.to_string().as_bytes())
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        invalid = value.clone();
        invalid["properties"]["uid"] = json!(format!("urn:test:{}", "x".repeat(4087)));
        assert!(minimal(&validator, invalid.to_string().as_bytes()).is_ok());
        invalid["properties"]["uid"] = json!(format!("urn:test:{}", "x".repeat(4088)));
        assert_eq!(
            minimal(&validator, invalid.to_string().as_bytes())
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        invalid = value.clone();
        invalid["properties"]["name"] = json!("x".repeat(4096));
        assert!(minimal(&validator, invalid.to_string().as_bytes()).is_ok());
        invalid["properties"]["name"] = json!("x".repeat(4097));
        assert_eq!(
            minimal(&validator, invalid.to_string().as_bytes())
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        value["id"] = json!([]);
        assert_eq!(
            minimal(&validator, value.to_string().as_bytes())
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::BAD_REQUEST
        );
        value.as_object_mut().unwrap().remove("id");
        value["geometry"] = json!({"type":"Point", "coordinates":[1,2]});
        assert_eq!(
            minimal(&validator, value.to_string().as_bytes())
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        value["geometry"] = Value::Null;
        value["properties"]["description"] = json!("Unimplemented optional content");
        assert_eq!(
            minimal(&validator, value.to_string().as_bytes())
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }

    #[test]
    fn system_creation_retry_header_is_optional_single_and_bounded() {
        let mut headers = HeaderMap::new();
        assert!(retry(&headers, 3600).unwrap().is_none());
        headers.insert("idempotency-key", HeaderValue::from_static("original-key"));
        assert_eq!(retry(&headers, 3600).unwrap().unwrap().key, "original-key");
        headers.append("idempotency-key", HeaderValue::from_static("second-key"));
        assert!(retry(&headers, 3600).is_err());
        for value in ["".to_owned(), "has space".to_owned(), "x".repeat(257)] {
            headers.insert("idempotency-key", HeaderValue::from_str(&value).unwrap());
            assert!(retry(&headers, 3600).is_err());
        }
        headers.insert(
            "idempotency-key",
            HeaderValue::from_str(&"x".repeat(256)).unwrap(),
        );
        assert!(retry(&headers, 3600).is_ok());
    }

    #[test]
    fn system_read_representation_uses_stored_identity_and_retained_type() {
        let boundary =
            HttpBoundary::new(Some("https://example.test/prefix"), Limits::default()).unwrap();
        let validator = ProjectionValidator::new().unwrap();
        // The retained source carries a client id/link and a different name; only
        // featureType's exact spelling may come from it.
        let source = json!({"type":"Feature", "id":"client-selected-id", "geometry":null, "links":[{"href":"https://example.test/untrusted-link", "rel":"related"}], "properties":{"uid":"urn:glaux:test:source-uid", "name":"Source name", "featureType":"http://www.w3.org/ns/sosa/Platform"}});
        let mut system = CurrentSystem {
            id: "0190f5c2-7b5a-7cc3-98c4-dc0c0c220001".parse().unwrap(),
            uid: "urn:glaux:test:stored-uid".parse().unwrap(),
            label: "Stored label".to_owned(),
            parent: None,
            media_type: INPUT_MEDIA.to_owned(),
            bytes: source.to_string().into_bytes(),
        };
        let own = json!({"href":"https://example.test/prefix/systems/0190f5c2-7b5a-7cc3-98c4-dc0c0c220001", "rel":"self", "type":"application/geo+json", "title":"This System"});
        let value = representation(&boundary, &system).unwrap();
        assert_eq!(
            value,
            json!({"type":"Feature", "id":"0190f5c2-7b5a-7cc3-98c4-dc0c0c220001", "geometry":null,
                "properties":{"uid":"urn:glaux:test:stored-uid", "name":"Stored label", "featureType":"http://www.w3.org/ns/sosa/Platform"},
                "links":[own]})
        );
        let (geo, response) = (Resource::SystemGeoJson, Projection::Response);
        let bytes = value.to_string().into_bytes();
        assert!(validator.validate(geo, response, &bytes).is_ok());
        system.parent = Some("0190f5c2-7b5a-7cc3-98c4-dc0c0c220002".parse().unwrap());
        let value = representation(&boundary, &system).unwrap();
        assert_eq!(
            value["links"],
            json!([own, {"href":"https://example.test/prefix/systems/0190f5c2-7b5a-7cc3-98c4-dc0c0c220002", "rel":"ogc-rel:parentSystem", "type":"application/geo+json", "title":"Parent System"}])
        );
        let bytes = value.to_string().into_bytes();
        assert!(validator.validate(geo, response, &bytes).is_ok());
        let mut wrong = value;
        wrong.as_object_mut().unwrap().remove("links");
        let bytes = wrong.to_string().into_bytes();
        assert!(validator.validate(geo, response, &bytes).is_err());
        system.media_type = "application/json".to_owned();
        assert_eq!(
            representation(&boundary, &system)
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
        system.media_type = INPUT_MEDIA.to_owned();
        system.bytes = br#"{"type":"Feature","geometry":null,"properties":{}}"#.to_vec();
        assert_eq!(
            representation(&boundary, &system)
                .unwrap_err()
                .into_response()
                .status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }
}
