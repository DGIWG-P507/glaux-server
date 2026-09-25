//! Bounded shared HTTP handling. No resource, identity or policy implementation.
mod media;

use axum::Router;
use axum::body::{Body, Bytes, HttpBody};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use glaux_domain::identity::LocalId;
use glaux_standards::validation::{self, Failure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::fmt;
use std::future::poll_fn;
use std::pin::Pin;
use std::time::Duration;
use tokio::time::{Instant, timeout_at};

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub body_bytes: usize,
    pub header_bytes: usize,
    pub uri_bytes: usize,
    pub timeout_ms: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            body_bytes: 65_536,
            header_bytes: 16_384,
            uri_bytes: 4_096,
            timeout_ms: 15_000,
        }
    }
}

#[derive(Clone)]
pub struct HttpBoundary {
    public_api_root: Option<String>,
    limits: Limits,
}

#[derive(Clone, Copy, Debug)]
pub struct BoundaryConfigError;

impl fmt::Display for BoundaryConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid public API root or HTTP limits")
    }
}

impl std::error::Error for BoundaryConfigError {}

#[derive(Clone, Copy, Debug)]
enum Kind {
    BadRequest,
    NotFound,
    Method,
    NotAcceptable,
    RequestTimeout,
    BodySize,
    UriSize,
    Media,
    HeadersSize,
    Internal,
    Unavailable,
}

/// Fixed safe catalog: arbitrary diagnostic or request text cannot become detail.
#[derive(Clone, Copy, Debug)]
pub struct Problem {
    kind: Kind,
    unsupported_coding: bool,
}

impl Problem {
    fn new(kind: Kind) -> Self {
        Self { kind, unsupported_coding: false }
    }
    pub fn internal() -> Self {
        Self::new(Kind::Internal)
    }
    pub(crate) fn bad_request() -> Self {
        Self::new(Kind::BadRequest)
    }
    pub(crate) fn not_acceptable() -> Self {
        Self::new(Kind::NotAcceptable)
    }
    pub(crate) fn unsupported_media_type(coding: bool) -> Self {
        Self { kind: Kind::Media, unsupported_coding: coding }
    }
    fn catalog(self) -> (StatusCode, &'static str, &'static str, &'static str) {
        match self.kind {
            Kind::BadRequest => (StatusCode::BAD_REQUEST, "bad-request", "Bad Request", "The request is malformed."),
            Kind::NotFound => (StatusCode::NOT_FOUND, "not-found", "Not Found", "The requested resource is unavailable."),
            Kind::Method => (StatusCode::METHOD_NOT_ALLOWED, "method-not-allowed", "Method Not Allowed", "The method is unavailable on this route."),
            Kind::NotAcceptable => (StatusCode::NOT_ACCEPTABLE, "not-acceptable", "Not Acceptable", "No offered representation is acceptable."),
            Kind::RequestTimeout => (StatusCode::REQUEST_TIMEOUT, "request-timeout", "Request Timeout", "The request body did not complete within its limit."),
            Kind::BodySize => (StatusCode::PAYLOAD_TOO_LARGE, "payload-too-large", "Content Too Large", "The request body exceeds its limit."),
            Kind::UriSize => (StatusCode::URI_TOO_LONG, "uri-too-long", "URI Too Long", "The request target exceeds its limit."),
            Kind::Media => (StatusCode::UNSUPPORTED_MEDIA_TYPE, "unsupported-media-type", "Unsupported Media Type", "The request media type or coding is unsupported."),
            Kind::HeadersSize => (StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE, "headers-too-large", "Request Header Fields Too Large", "The request headers exceed their limit."),
            Kind::Internal => (StatusCode::INTERNAL_SERVER_ERROR, "internal", "Internal Server Error", "The operation could not be completed."),
            Kind::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable", "Service Unavailable", "The operation is temporarily unavailable."),
        }
    }
}

fn correlation() -> String {
    // Not an authorization token or an ordering assertion. Entropy failure is
    // explicit in the diagnostic identifier, never replaced with client data.
    LocalId::generate().map(|id| id.to_string()).unwrap_or_else(|_| "unavailable".into())
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        let (status, slug, title, detail) = self.catalog();
        let correlation = correlation();
        let value = json!({
            "type": format!("urn:glaux:problem:{slug}"),
            "title": title,
            "status": status.as_u16(),
            "detail": detail,
            "correlation": correlation,
        });
        let mut response = (
            status,
            [(header::CONTENT_TYPE, "application/problem+json"), (header::CACHE_CONTROL, "no-store")],
            value.to_string(),
        ).into_response();
        if let Ok(value) = HeaderValue::from_str(&correlation) {
            response.headers_mut().insert("x-request-id", value);
        }
        if self.unsupported_coding {
            response.headers_mut().insert(header::ACCEPT_ENCODING, HeaderValue::from_static("identity"));
        }
        response
    }
}

impl HttpBoundary {
    pub fn new(public_api_root: Option<&str>, limits: Limits) -> Result<Self, BoundaryConfigError> {
        if !(1..=8_388_608).contains(&limits.body_bytes)
            || !(256..=65_536).contains(&limits.header_bytes)
            || !(128..=16_384).contains(&limits.uri_bytes)
            || !(10..=60_000).contains(&limits.timeout_ms)
        {
            return Err(BoundaryConfigError);
        }
        Ok(Self {
            public_api_root: public_api_root.map(public_root).transpose()?,
            limits,
        })
    }

    /// Apply after the caller assembles routes; no synthetic/discovery route here.
    pub fn router(&self, routes: Router) -> Router {
        routes
            .fallback(|| async { Problem::new(Kind::NotFound) })
            .layer(middleware::from_fn_with_state(self.clone(), boundary))
    }

    /// Syntax/budgets only, not schema validity, authority or resource semantics.
    pub async fn read_json(&self, request: Request) -> Result<Value, Problem> {
        media::check_coding(request.headers())?;
        media::check_json_media(request.headers())?;
        let deadline = Instant::now() + Duration::from_millis(self.limits.timeout_ms);
        let bytes = timeout_at(deadline, collect(request.into_body(), self.limits))
            .await.map_err(|_| Problem::new(Kind::RequestTimeout))??;
        validation::parse(&bytes).map_err(|failure| match failure {
            Failure::Size => Problem::new(Kind::BodySize),
            _ => Problem::bad_request(),
        })
    }

    /// Path components are data, never a relative-reference resolution request.
    pub fn link(&self, segments: &[&str], query: &[(&str, &str)]) -> Result<String, Problem> {
        let mut result = self.public_api_root.clone().ok_or_else(Problem::internal)?;
        if segments.len() > 64 || query.len() > 128 {
            return Err(Problem::internal());
        }
        for segment in segments {
            if segment.is_empty() || *segment == "." || *segment == ".."
                || segment.len() > 2_048 || segment.chars().any(char::is_control)
            {
                return Err(Problem::internal());
            }
            result.push('/');
            encode(segment, &mut result);
        }
        for (index, (name, value)) in query.iter().enumerate() {
            if name.len() > 2_048 || value.len() > 4_096 {
                return Err(Problem::internal());
            }
            result.push(if index == 0 { '?' } else { '&' });
            encode(name, &mut result);
            result.push('=');
            encode(value, &mut result);
        }
        if result.len() > 16_384 {
            return Err(Problem::internal());
        }
        Ok(result)
    }
}

fn unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
}

fn encode(text: &str, target: &mut String) {
    const HEX: &[u8] = b"0123456789ABCDEF";
    for byte in text.bytes() {
        if unreserved(byte) {
            target.push(char::from(byte));
        } else {
            target.push('%');
            target.push(char::from(HEX[usize::from(byte >> 4)]));
            target.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
}

fn public_root(text: &str) -> Result<String, BoundaryConfigError> {
    if text.len() > 2_048 || text.contains(['?', '#', '\\'])
        || text.bytes().any(|byte| !byte.is_ascii_graphic())
    {
        return Err(BoundaryConfigError);
    }
    let uri: Uri = text.parse().map_err(|_| BoundaryConfigError)?;
    let scheme = uri.scheme_str().ok_or(BoundaryConfigError)?;
    if scheme != "http" && scheme != "https" {
        return Err(BoundaryConfigError);
    }
    let authority = uri.authority().ok_or(BoundaryConfigError)?;
    if authority.as_str().contains(['@', '%']) {
        return Err(BoundaryConfigError);
    }
    let host = authority.host();
    if host.starts_with('[') {
        host.strip_prefix('[').and_then(|s| s.strip_suffix(']'))
            .ok_or(BoundaryConfigError)?
            .parse::<std::net::Ipv6Addr>().map_err(|_| BoundaryConfigError)?;
    } else if host.is_empty() || host.bytes().any(|byte| !byte.is_ascii_alphanumeric() && !matches!(byte, b'.' | b'-' | b'_')) {
        return Err(BoundaryConfigError);
    }
    let port = &authority.as_str()[host.len()..];
    if !port.is_empty() {
        let port = port.strip_prefix(':').ok_or(BoundaryConfigError)?
            .parse::<u16>().map_err(|_| BoundaryConfigError)?;
        if port == 0 {
            return Err(BoundaryConfigError);
        }
    }
    let path = uri.path().strip_suffix('/').unwrap_or(uri.path());
    if !path.is_empty() {
        for segment in path.strip_prefix('/').ok_or(BoundaryConfigError)?.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." || !segment.bytes().all(unreserved) {
                return Err(BoundaryConfigError);
            }
        }
    }
    Ok(format!("{scheme}://{authority}{path}"))
}

fn header_size(headers: &HeaderMap) -> usize {
    headers.iter().fold(0usize, |total, (name, value)| {
        total.saturating_add(name.as_str().len()).saturating_add(value.as_bytes().len()).saturating_add(4)
    })
}

fn validate_target(uri: &Uri) -> Result<(), Problem> {
    let target = uri.path_and_query().map_or("", |value| value.as_str()).as_bytes();
    let mut index = 0;
    while index < target.len() {
        if target[index] == b'%' {
            let digit = |byte: u8| char::from(byte).to_digit(16);
            let a = target.get(index + 1).copied().and_then(digit).ok_or_else(Problem::bad_request)?;
            let b = target.get(index + 2).copied().and_then(digit).ok_or_else(Problem::bad_request)?;
            let value = a * 16 + b;
            if value < 32 || value == 127 || value == 92 {
                return Err(Problem::bad_request());
            }
            index += 3;
        } else {
            if target[index] < 32 || target[index] == 127 || target[index] == b'\\' {
                return Err(Problem::bad_request());
            }
            index += 1;
        }
    }
    Ok(())
}

async fn collect(mut body: Body, limits: Limits) -> Result<Bytes, Problem> {
    let mut data = Vec::new();
    let mut trailer_bytes = 0usize;
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut body).poll_frame(cx)).await {
        let frame = frame.map_err(|_| Problem::bad_request())?;
        match frame.into_data() {
            Ok(bytes) => {
                if bytes.len() > limits.body_bytes.saturating_sub(data.len()) {
                    return Err(Problem::new(Kind::BodySize));
                }
                data.extend_from_slice(&bytes);
            }
            Err(frame) => {
                if let Ok(trailers) = frame.into_trailers() {
                    trailer_bytes = trailer_bytes.saturating_add(header_size(&trailers));
                    if trailer_bytes > limits.header_bytes {
                        return Err(Problem::new(Kind::HeadersSize));
                    }
                    // Trailers never override routing, coding, type or authority.
                }
            }
        }
    }
    Ok(Bytes::from(data))
}

async fn dispatch(state: &HttpBoundary, request: Request, next: Next) -> Result<Response, Problem> {
    if request.uri().to_string().len() > state.limits.uri_bytes {
        return Err(Problem::new(Kind::UriSize));
    }
    if header_size(request.headers()) > state.limits.header_bytes {
        return Err(Problem::new(Kind::HeadersSize));
    }
    validate_target(request.uri())?;
    media::check_coding(request.headers())?;
    if let Some(value) = request.headers().get(header::CONTENT_LENGTH) {
        let value: u64 = value.to_str().map_err(|_| Problem::bad_request())?
            .parse().map_err(|_| Problem::bad_request())?;
        if value > state.limits.body_bytes as u64 {
            return Err(Problem::new(Kind::BodySize));
        }
    }
    let deadline = Instant::now() + Duration::from_millis(state.limits.timeout_ms);
    let (parts, body) = request.into_parts();
    let bytes = timeout_at(deadline, collect(body, state.limits))
        .await.map_err(|_| Problem::new(Kind::RequestTimeout))??;
    let request = Request::from_parts(parts, Body::from(bytes));
    timeout_at(deadline, next.run(request)).await.map_err(|_| Problem::new(Kind::Unavailable))
}

async fn boundary(State(state): State<HttpBoundary>, request: Request, next: Next) -> Response {
    let head = request.method() == Method::HEAD;
    let mut response = match dispatch(&state, request, next).await {
        Ok(response) => response,
        Err(problem) => {
            let mut response = problem.into_response();
            // The framed request may not have been consumed; do not reuse it.
            response.headers_mut().insert(header::CONNECTION, HeaderValue::from_static("close"));
            response
        }
    };
    if response.status() == StatusCode::METHOD_NOT_ALLOWED {
        let allow = response.headers().get(header::ALLOW).cloned();
        response = Problem::new(Kind::Method).into_response();
        if let Some(value) = allow {
            response.headers_mut().insert(header::ALLOW, value);
        }
    }
    if !response.headers().contains_key("x-request-id")
        && let Ok(value) = HeaderValue::from_str(&correlation())
    {
        response.headers_mut().insert("x-request-id", value);
    }
    if head {
        *response.body_mut() = Body::empty();
    }
    response
}

pub fn negotiate(headers: &HeaderMap, offered: &[&str]) -> Result<usize, Problem> {
    media::negotiate(headers, offered)
}

pub fn json_response(value: &Value, media: &str) -> Result<Response, Problem> {
    let media = HeaderValue::from_str(media).map_err(|_| Problem::internal())?;
    let bytes = serde_json::to_vec(value).map_err(|_| Problem::internal())?;
    Ok((
        [(header::CONTENT_TYPE, media), (header::VARY, HeaderValue::from_static("Accept")), (header::CACHE_CONTROL, HeaderValue::from_static("no-store"))],
        bytes,
    ).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_roots_reject_ambiguity_and_keep_prefixes() {
        for root in [
            "", "//example.test/api", "ftp://example.test/api", "https://user@example.test/api",
            "https://example.test/api?token=x", "https://example.test/api#x",
            "https://example.test/a/../b", "https://example.test/a/./b", "https://example.test/a//b",
            "https://example.test/%2e%2e", "https://example.test/%2f", "https://example.test/%",
            "https://example.test/a\\b", "https://example.test:65536/api", "https://example.test:0/api",
            "https://example.test:/api", "https://exa mple.test/api",
        ] {
            assert!(HttpBoundary::new(Some(root), Limits::default()).is_err(), "unsafe root accepted");
        }
        for root in ["https://example.test/api", "http://127.0.0.1:8080/api", "https://[::1]:8080/api"] {
            let boundary = HttpBoundary::new(Some(&format!("{root}/")), Limits::default()).unwrap();
            assert_eq!(boundary.link(&["systems", "Case-ID"], &[]).unwrap(), format!("{root}/systems/Case-ID"));
        }
        assert!(HttpBoundary::new(None, Limits::default()).unwrap().link(&["systems"], &[]).is_err());
    }

    #[test]
    fn link_components_are_encoded_without_reference_resolution() {
        let boundary = HttpBoundary::new(Some("https://example.test/prefix"), Limits::default()).unwrap();
        assert_eq!(boundary.link(&["%2e%2e", "//evil.test/é"], &[("name", "a+b&c")]).unwrap(),
            "https://example.test/prefix/%252e%252e/%2F%2Fevil.test%2F%C3%A9?name=a%2Bb%26c");
        for segment in ["", ".", "..", "\r\nHeader: x", "\0"] {
            assert!(boundary.link(&[segment], &[]).is_err());
        }
        for limits in [
            Limits { body_bytes: 0, ..Limits::default() },
            Limits { header_bytes: 255, ..Limits::default() },
            Limits { uri_bytes: 16_385, ..Limits::default() },
            Limits { timeout_ms: 60_001, ..Limits::default() },
        ] {
            assert!(HttpBoundary::new(None, limits).is_err());
        }
    }
}
