//! First, explicitly configured System POST. No retrieval or full CRUD claim.
use crate::application::{AuditContext, AuditId, CreateSystem, EventId, RetryKey};
use crate::authentication::CallerContext;
use crate::authorization::{AccessError, Admission, OperationContext};
use crate::configuration::Configuration;
use crate::discovery;
use crate::http_boundary::{HttpBoundary, Problem};
use crate::revisions::{ArtifactId, NewSourceArtifact, RevisionId, SystemRevision};
use crate::storage::SystemRecord;
use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use glaux_domain::identity::{LocalId, Uid};
use glaux_domain::temporal::ExactInstant;
use glaux_standards::projection::{Projection, ProjectionValidator, RequestContext, Resource};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;

// Temporary, explicit first hosted behavioral-red checkpoint. The route and
// positive implementation are drafted together; activation awaits that result.
const SYSTEM_CREATION_ACTIVE: bool = false; // SYSTEM_CREATION_RED
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
struct CreateState {
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
    fn from(value: Problem) -> Self { Self::Http(value) }
}

impl From<AccessError> for Error {
    fn from(value: AccessError) -> Self { Self::Access(value) }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        match self { Self::Http(error) => error.into_response(), Self::Access(error) => error.into_response() }
    }
}

/// Compile pinned schemas offline before exposing the route. No fetch, migration
/// or write occurs here. Call only for an explicitly enabled configuration.
pub fn router(config: &Configuration, pool: PgPool) -> Result<Router, Problem> {
    let state = CreateState {
        pool,
        boundary: config.http_boundary(),
        admission: config.admission(),
        config: config.system_creation().ok_or_else(Problem::internal)?,
        validator: Arc::new(ProjectionValidator::new().map_err(|_| Problem::unavailable())?),
    };
    let routes = Router::new()
        .route(discovery::SYSTEM_CREATE.path(), discovery::SYSTEM_CREATE.method(create))
        .with_state(state);
    Ok(config.authenticator().protect(routes))
}

fn retry(headers: &HeaderMap, retention_seconds: u32) -> Result<Option<RetryKey>, Problem> {
    let mut values = headers.get_all("idempotency-key").iter();
    let Some(value) = values.next() else { return Ok(None); };
    let key = value.to_str().map_err(|_| Problem::bad_request())?;
    if values.next().is_some() || key.is_empty() || key.len() > 256
        || !key.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err(Problem::bad_request());
    }
    Ok(Some(RetryKey { key: key.to_owned(), retention_seconds }))
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
        if present { bytes.push(b','); }
        present = true;
        bytes.extend_from_slice(value.as_bytes());
    }
    if !present { return Ok(false); }
    let trim = |slice: &[u8]| {
        let start = slice.iter().position(|byte| !matches!(byte, b' ' | b'\t')).unwrap_or(slice.len());
        let end = slice.iter().rposition(|byte| !matches!(byte, b' ' | b'\t')).map_or(start, |index| index + 1);
        (start, end)
    };
    let (start, end) = trim(&bytes);
    let bytes = &bytes[start..end];
    if bytes == b"*" { return Ok(true); }
    let mut index = 0;
    let mut tags = 0;
    let mut empty = 0;
    while index < bytes.len() {
        while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') { index += 1; }
        if index == bytes.len() { break; }
        if bytes[index] == b',' {
            empty += 1;
            if empty > 64 { return Err(Problem::bad_request()); }
            index += 1;
            continue;
        }
        if bytes[index..].starts_with(b"W/") { index += 2; }
        if bytes.get(index) != Some(&b'"') { return Err(Problem::bad_request()); }
        index += 1;
        while let Some(&byte) = bytes.get(index) {
            if byte == b'"' { break; }
            if !(byte == 0x21 || (0x23..=0x7e).contains(&byte) || byte >= 0x80) {
                return Err(Problem::bad_request());
            }
            index += 1;
        }
        if bytes.get(index) != Some(&b'"') { return Err(Problem::bad_request()); }
        index += 1;
        tags += 1;
        if tags > 64 { return Err(Problem::bad_request()); }
        while index < bytes.len() && matches!(bytes[index], b' ' | b'\t') { index += 1; }
        if index < bytes.len() {
            if bytes[index] != b',' { return Err(Problem::bad_request()); }
            index += 1;
        }
    }
    if tags == 0 { return Err(Problem::bad_request()); }
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
    let value = validator.request(Resource::SystemGeoJson, Projection::CreateRequest, bytes, RequestContext::default())
        .map_err(|_| Problem::bad_request())?;
    let root = value.as_object().ok_or_else(Problem::bad_request)?;
    let properties = value.get("properties").and_then(Value::as_object).ok_or_else(Problem::bad_request)?;
    if root.keys().any(|name| !["type", "geometry", "properties"].contains(&name.as_str()))
        || properties.keys().any(|name| !["uid", "name", "featureType"].contains(&name.as_str()))
        || value.get("geometry") != Some(&Value::Null)
    {
        return Err(Problem::unprocessable());
    }
    let uid = properties.get("uid").and_then(Value::as_str).ok_or_else(Problem::bad_request)?
        .parse::<Uid>().map_err(|_| Problem::bad_request())?;
    let label = properties.get("name").and_then(Value::as_str).ok_or_else(Problem::bad_request)?;
    if label.len() > 4096 { return Err(Problem::unprocessable()); }
    // featureType's ten exact schema spellings survive in the retained artifact;
    // this initial normalized identity table does not reinterpret or erase them.
    Ok((uid, label.to_owned()))
}

async fn create(State(state): State<CreateState>, request: Request) -> Result<Response, Error> {
    if !SYSTEM_CREATION_ACTIVE { return Err(Problem::unavailable().into()); }
    let caller = request.extensions().get::<CallerContext>().cloned().ok_or_else(Problem::unauthorized)?;
    let retry = retry(request.headers(), state.config.retry_retention_seconds)?;
    let precondition = precondition(request.headers())?;
    let bytes = state.boundary.read_json_bytes(request, &[INPUT_MEDIA]).await?;
    let (uid, label) = minimal(&state.validator, &bytes)?;
    let system_id = LocalId::generate().map_err(|_| Problem::unavailable())?;
    let artifact_id = ArtifactId::generate().map_err(|_| Problem::unavailable())?;
    let revision_id = RevisionId::generate().map_err(|_| Problem::unavailable())?;
    let audit_id = AuditId::generate().map_err(|_| Problem::unavailable())?;
    let event_id = EventId::generate().map_err(|_| Problem::unavailable())?;
    let mut connection = state.pool.acquire().await.map_err(|_| Problem::unavailable())?;
    // One explicit trusted operation-receipt sample, before the write transaction.
    // No request timestamp, UUID timestamp, implicit column default or exact
    // network-arrival/commit-time claim is substituted for this observation.
    let source: String = sqlx::query_scalar("SELECT to_char(clock_timestamp() AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"')")
        .fetch_one(&mut *connection).await.map_err(|_| Problem::unavailable())?;
    let time = ExactInstant::parse_rfc3339(&source).map_err(|_| Problem::unavailable())?;
    let context = OperationContext::new(caller, time.clone())?;
    let input = CreateSystem {
        system: SystemRecord { id: system_id, uid, label, sources: vec![], parent: None },
        artifact: NewSourceArtifact { id: artifact_id, media_type: INPUT_MEDIA.to_owned(), bytes: bytes.to_vec() },
        revision: SystemRevision { id: revision_id, system_id, artifact_id, semantic_time: None, receipt_time: time.clone() },
        audit_id,
        event_id,
        // Admission replaces all attribution with verified operation context.
        audit: AuditContext { actor: None, source: None, correlation: context.correlation().to_owned(), time },
    };
    let receipt = state.admission.create_system_if(&mut connection, &context, &state.config.source, input, retry.as_ref(), precondition).await?;
    let location = state.boundary.link(&["systems", &receipt.system_id.to_string()], &[])?;
    let location = HeaderValue::from_str(&location).map_err(|_| Problem::internal())?;
    let correlation = HeaderValue::from_str(context.correlation()).map_err(|_| Problem::internal())?;
    // The empty creation receipt negotiates no resource representation. No ETag,
    // body or internal revision is disclosed; canonical GET belongs to #25.
    Ok((StatusCode::CREATED, [
        (header::LOCATION, location),
        (header::CACHE_CONTROL, HeaderValue::from_static("private, no-store")),
        (HeaderName::from_static("x-request-id"), correlation),
    ]).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn system_creation_conditions_target_absent_collection_representation() {
        let mut headers = HeaderMap::new();
        assert!(precondition(&headers).unwrap());
        for value in ["*", "\"old\"", "W/\"old\"", "\"comma,inside\", W/\"second\"", ",\"tag\",,"] {
            headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
            assert!(!precondition(&headers).unwrap(), "{value}");
            headers.remove(header::IF_MATCH);
            headers.insert(header::IF_NONE_MATCH, HeaderValue::from_str(value).unwrap());
            assert!(precondition(&headers).unwrap(), "{value}");
            headers.remove(header::IF_NONE_MATCH);
        }
        for value in ["", "w/\"lowercase\"", "*,\"tag\"", "\"unfinished", "\"bad space\"", "tag", "\"one\" \"two\"", ",,,"] {
            headers.insert(header::IF_MATCH, HeaderValue::from_str(value).unwrap());
            assert!(precondition(&headers).is_err(), "{value}");
        }
        headers.clear();
        headers.insert(header::IF_UNMODIFIED_SINCE, HeaderValue::from_static("Wed, 01 Jan 2020 00:00:00 GMT"));
        headers.insert(header::IF_MODIFIED_SINCE, HeaderValue::from_static("Wed, 01 Jan 2020 00:00:00 GMT"));
        assert!(precondition(&headers).unwrap());
        headers.insert(header::IF_MATCH, HeaderValue::from_static("\"one\""));
        headers.append(header::IF_MATCH, HeaderValue::from_static("W/\"two\""));
        assert!(!precondition(&headers).unwrap());
        headers.insert(header::IF_MATCH, HeaderValue::from_str(&format!("\"{}\"", "x".repeat(4096))).unwrap());
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
        assert_eq!(minimal(&validator, invalid.to_string().as_bytes()).unwrap_err().into_response().status(), StatusCode::BAD_REQUEST);
        invalid = value.clone();
        invalid["properties"]["name"] = json!("");
        assert_eq!(minimal(&validator, invalid.to_string().as_bytes()).unwrap_err().into_response().status(), StatusCode::BAD_REQUEST);
        invalid = value.clone();
        invalid["properties"]["uid"] = json!(format!("urn:test:{}", "x".repeat(4087)));
        assert!(minimal(&validator, invalid.to_string().as_bytes()).is_ok());
        invalid["properties"]["uid"] = json!(format!("urn:test:{}", "x".repeat(4088)));
        assert_eq!(minimal(&validator, invalid.to_string().as_bytes()).unwrap_err().into_response().status(), StatusCode::BAD_REQUEST);
        invalid = value.clone();
        invalid["properties"]["name"] = json!("x".repeat(4096));
        assert!(minimal(&validator, invalid.to_string().as_bytes()).is_ok());
        invalid["properties"]["name"] = json!("x".repeat(4097));
        assert_eq!(minimal(&validator, invalid.to_string().as_bytes()).unwrap_err().into_response().status(), StatusCode::UNPROCESSABLE_ENTITY);
        value["id"] = json!([]);
        assert_eq!(minimal(&validator, value.to_string().as_bytes()).unwrap_err().into_response().status(), StatusCode::BAD_REQUEST);
        value.as_object_mut().unwrap().remove("id");
        value["geometry"] = json!({"type":"Point", "coordinates":[1,2]});
        assert_eq!(minimal(&validator, value.to_string().as_bytes()).unwrap_err().into_response().status(), StatusCode::UNPROCESSABLE_ENTITY);
        value["geometry"] = Value::Null;
        value["properties"]["description"] = json!("Unimplemented optional content");
        assert_eq!(minimal(&validator, value.to_string().as_bytes()).unwrap_err().into_response().status(), StatusCode::UNPROCESSABLE_ENTITY);
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
        headers.insert("idempotency-key", HeaderValue::from_str(&"x".repeat(256)).unwrap());
        assert!(retry(&headers, 3600).is_ok());
    }
}
