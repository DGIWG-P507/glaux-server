//! Task 1.4.2 test-first interface; implementation follows the behavioral red.
use axum::extract::Request;
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::Router;
use serde_json::Value;

#[derive(Clone, Copy, Debug)]
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
pub struct HttpBoundary;

#[derive(Debug)]
pub struct BoundaryConfigError;

#[derive(Debug)]
pub struct Problem;

impl Problem {
    pub fn internal() -> Self {
        Self
    }
}

impl IntoResponse for Problem {
    fn into_response(self) -> Response {
        axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()
    }
}

impl HttpBoundary {
    pub fn new(_public_api_root: Option<&str>, _limits: Limits) -> Result<Self, BoundaryConfigError> {
        Ok(Self)
    }

    pub fn router(&self, routes: Router) -> Router {
        routes
    }

    pub async fn read_json(&self, _request: Request) -> Result<Value, Problem> {
        Err(Problem::internal())
    }

    pub fn link(&self, _segments: &[&str], _query: &[(&str, &str)]) -> Result<String, Problem> {
        Err(Problem::internal())
    }
}

pub fn negotiate(_headers: &HeaderMap, _offered: &[&str]) -> Result<usize, Problem> {
    Ok(0)
}

pub fn json_response(_value: &Value, _media: &str) -> Result<Response, Problem> {
    Err(Problem::internal())
}
