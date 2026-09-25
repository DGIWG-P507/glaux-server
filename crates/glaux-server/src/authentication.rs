//! Explicit caller authentication. A caller is not resource or producer authority.
mod jwt;

use crate::http_boundary::Problem;
use axum::Router;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::{HeaderMap, HeaderValue, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::Value;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JwtConfig {
    pub issuer: String,
    pub audience: String,
    pub keys: Vec<Value>,
    #[serde(default)]
    pub required_scopes: Vec<String>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DevelopmentConfig {
    pub subject: String,
    #[serde(default)]
    pub groups: Vec<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CallerKind {
    Jwt,
    Development,
}

/// No public constructor or deserializer can promote unchecked request claims.
#[derive(Clone)]
pub struct CallerContext {
    issuer: String,
    subject: String,
    client_id: Option<String>,
    scopes: Vec<String>,
    groups: Vec<String>,
    kind: CallerKind,
}

impl CallerContext {
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub fn client_id(&self) -> Option<&str> {
        self.client_id.as_deref()
    }
    pub fn scopes(&self) -> &[String] {
        &self.scopes
    }
    pub fn groups(&self) -> &[String] {
        &self.groups
    }
    pub fn kind(&self) -> CallerKind {
        self.kind
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Option<Duration>;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Option<Duration> {
        SystemTime::now().duration_since(UNIX_EPOCH).ok()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AuthConfigError;
impl fmt::Display for AuthConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid authentication configuration")
    }
}
impl std::error::Error for AuthConfigError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthError {
    Missing,
    InvalidRequest,
    InvalidToken,
    InsufficientScope,
    Unavailable,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (problem, challenge) = match self {
            Self::Missing => (Problem::unauthorized(), Some("Bearer")),
            Self::InvalidRequest => (
                Problem::bad_request(),
                Some("Bearer error=\"invalid_request\""),
            ),
            Self::InvalidToken => (
                Problem::unauthorized(),
                Some("Bearer error=\"invalid_token\""),
            ),
            Self::InsufficientScope => (
                Problem::insufficient_scope(),
                Some("Bearer error=\"insufficient_scope\""),
            ),
            Self::Unavailable => (Problem::unavailable(), None),
        };
        let mut response = problem.into_response();
        if let Some(challenge) = challenge {
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static(challenge),
            );
        }
        response
    }
}

#[derive(Clone)]
enum Mode {
    Disabled,
    Jwt(Arc<jwt::JwtVerifier>),
    Development(DevelopmentConfig),
}

#[derive(Clone)]
pub struct Authenticator {
    mode: Mode,
    clock: Arc<dyn Clock>,
}

impl Authenticator {
    pub fn disabled() -> Self {
        Self {
            mode: Mode::Disabled,
            clock: Arc::new(SystemClock),
        }
    }
    pub fn jwt(config: JwtConfig, clock: Arc<dyn Clock>) -> Result<Self, AuthConfigError> {
        Ok(Self {
            mode: Mode::Jwt(Arc::new(jwt::JwtVerifier::new(config)?)),
            clock,
        })
    }
    pub fn development(
        config: DevelopmentConfig,
        listener: SocketAddr,
    ) -> Result<Self, AuthConfigError> {
        fn text(value: &str, maximum: usize) -> bool {
            !value.is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
        }
        if !listener.ip().is_loopback()
            || !text(&config.subject, 1024)
            || config.groups.len() > 64
            || config.scopes.len() > 64
            || config.groups.iter().any(|v| !text(v, 256))
            || config.scopes.iter().any(|v| {
                v.is_empty()
                    || v.len() > 128
                    || !v.bytes().all(|b| {
                        b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b)
                    })
            })
        {
            return Err(AuthConfigError);
        }
        Ok(Self {
            mode: Mode::Development(config),
            clock: Arc::new(SystemClock),
        })
    }
    pub fn protect(self, routes: Router) -> Router {
        routes.layer(middleware::from_fn_with_state(self, authenticate))
    }
    pub fn authenticate(
        &self,
        headers: &HeaderMap,
        peer: Option<SocketAddr>,
    ) -> Result<CallerContext, AuthError> {
        match &self.mode {
            Mode::Disabled => Err(AuthError::Unavailable),
            Mode::Development(config) => {
                if headers.contains_key(header::AUTHORIZATION) {
                    return Err(AuthError::InvalidToken);
                }
                if !peer.is_some_and(|peer| peer.ip().is_loopback()) {
                    return Err(AuthError::InvalidToken);
                }
                Ok(CallerContext {
                    issuer: "urn:glaux:development".into(),
                    subject: config.subject.clone(),
                    client_id: None,
                    scopes: config.scopes.clone(),
                    groups: config.groups.clone(),
                    kind: CallerKind::Development,
                })
            }
            Mode::Jwt(verifier) => {
                let token = bearer(headers)?;
                let now = self.clock.now().ok_or(AuthError::Unavailable)?;
                verifier.verify(token, now)
            }
        }
    }
}

fn bearer(headers: &HeaderMap) -> Result<&str, AuthError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let Some(value) = values.next() else {
        return Err(AuthError::Missing);
    };
    if values.next().is_some() {
        return Err(AuthError::InvalidRequest);
    }
    let value = value.to_str().map_err(|_| AuthError::InvalidRequest)?;
    let Some((scheme, token)) = value.split_once(' ') else {
        return Err(AuthError::InvalidRequest);
    };
    if !scheme.eq_ignore_ascii_case("Bearer") {
        return Err(AuthError::Missing);
    }
    let token = token.trim_start_matches(' ');
    if token.is_empty() || token.bytes().any(|b| b.is_ascii_whitespace() || b == b',') {
        return Err(AuthError::InvalidRequest);
    }
    if token.len() > 16_384 {
        return Err(AuthError::InvalidToken);
    }
    Ok(token)
}

async fn authenticate(
    State(auth): State<Authenticator>,
    mut request: Request,
    next: Next,
) -> Response {
    // Discard any previously attached context before evaluating this boundary.
    request.extensions_mut().remove::<CallerContext>();
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0);
    match auth.authenticate(request.headers(), peer) {
        Ok(caller) => {
            request.extensions_mut().insert(caller);
            // Raw bearer material is no longer needed by the protected handler.
            request.headers_mut().remove(header::AUTHORIZATION);
            let mut response = next.run(request).await;
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, no-store"),
            );
            response
        }
        Err(error) => error.into_response(),
    }
}
