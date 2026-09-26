//! Bounded trust for one configured HTTPS issuer-key endpoint.
//!
//! Cache state is shared across authenticator clones. No lock spans an await;
//! a busy fetch is not an unbounded queue of requests waiting on the issuer.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use glaux_standards::validation;
use jsonwebtoken::DecodingKey;
use reqwest::{Certificate, Client, Url, header};
use serde_json::Value;

use super::jwt::key_set;
use super::{AuthConfigError, AuthError, Clock, JwksConfig};

const MAX_JWKS_BYTES: usize = 65_536;

pub(super) struct RemoteKeys {
    client: Client,
    url: Url,
    ttl: Duration,
    interval: Duration,
    timeout: Duration,
    clock: Arc<dyn Clock>,
    state: Mutex<Cache>,
}

#[derive(Default)]
struct Cache {
    keys: BTreeMap<String, DecodingKey>,
    expiry: Option<Duration>,
    last_attempt: Option<Duration>,
    last_clock: Option<Duration>,
    busy: bool,
}

impl Cache {
    fn observe(&mut self, now: Option<Duration>) -> Result<Duration, AuthError> {
        let now = now.ok_or(AuthError::Unavailable)?;
        if self.last_clock.is_some_and(|previous| now < previous) {
            return Err(AuthError::Unavailable);
        }
        self.last_clock = Some(now);
        Ok(now)
    }

    fn fresh(&self, now: Duration) -> bool {
        self.expiry.is_some_and(|expiry| now < expiry)
    }

    fn cooling_down(&self, now: Duration, interval: Duration) -> bool {
        self.last_attempt
            .is_some_and(|attempt| now.checked_sub(attempt).is_none_or(|age| age < interval))
    }
}

/// Dropping an authentication future cancels its request and clears busy, but
/// never rolls back the already-recorded attempt/cooldown or extends trust.
struct Attempt<'a>(&'a Mutex<Cache>);
impl Drop for Attempt<'_> {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.busy = false;
        }
        // A poisoned mutex remains an unavailable verifier, not a recovered
        // cache whose partially updated state might accidentally be trusted.
    }
}

impl RemoteKeys {
    pub(super) fn new(config: JwksConfig, clock: Arc<dyn Clock>) -> Result<Self, AuthConfigError> {
        let url = endpoint(&config)?;
        let timeout = Duration::from_millis(config.request_timeout_ms);
        let mut builder = Client::builder()
            .https_only(true)
            .http1_only()
            .http1_max_headers(32)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .no_proxy()
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .pool_max_idle_per_host(0)
            .connect_timeout(timeout)
            .timeout(timeout)
            .tls_version_min(reqwest::tls::Version::TLS_1_2);
        if let Some(pem) = &config.trusted_ca_pem {
            builder = builder.tls_certs_merge(certificates(pem)?);
        }
        let client = builder.build().map_err(|_| AuthConfigError)?;
        Ok(Self {
            client,
            url,
            ttl: Duration::from_secs(config.cache_ttl_seconds),
            interval: Duration::from_secs(config.refresh_interval_seconds),
            timeout,
            clock,
            state: Mutex::new(Cache::default()),
        })
    }

    pub(super) async fn get(&self, kid: &str) -> Result<DecodingKey, AuthError> {
        let start = {
            let mut state = self.state.lock().map_err(|_| AuthError::Unavailable)?;
            let now = state.observe(self.clock.now())?;
            let fresh = state.fresh(now);
            if fresh && let Some(key) = state.keys.get(kid) {
                // Known-key signature failure is handled by JWT verification;
                // it cannot turn this path into a refresh request.
                return Ok(key.clone());
            }
            if state.busy || state.cooling_down(now, self.interval) {
                return Err(if fresh {
                    AuthError::InvalidToken
                } else {
                    AuthError::Unavailable
                });
            }
            state.busy = true;
            state.last_attempt = Some(now);
            now
        };
        // Construction cannot yield after busy is set and before the guard.
        let _attempt = Attempt(&self.state);
        let result = tokio::time::timeout(self.timeout, self.fetch())
            .await
            .map_err(|_| AuthError::Unavailable)
            .and_then(|result| result);
        let mut state = self.state.lock().map_err(|_| AuthError::Unavailable)?;
        let now = state.observe(self.clock.now())?;
        let expiry = start.checked_add(self.ttl).ok_or(AuthError::Unavailable)?;
        if now >= expiry {
            return Err(AuthError::Unavailable);
        }
        // A failure leaves both the old contents and their original expiry
        // intact. No stale-if-error, HTTP freshness inference, merge or 304.
        let keys = result?;
        state.keys = keys;
        state.expiry = Some(expiry);
        state.keys.get(kid).cloned().ok_or(AuthError::InvalidToken)
    }

    async fn fetch(&self) -> Result<BTreeMap<String, DecodingKey>, AuthError> {
        let mut response = self
            .client
            .get(self.url.clone())
            .header(header::ACCEPT, "application/jwk-set+json, application/json")
            .send()
            .await
            .map_err(|_| AuthError::Unavailable)?;
        if response.status() != reqwest::StatusCode::OK
            || response.headers().contains_key(header::CONTENT_ENCODING)
            || response
                .content_length()
                .is_some_and(|length| length > MAX_JWKS_BYTES as u64)
        {
            return Err(AuthError::Unavailable);
        }
        let mut types = response.headers().get_all(header::CONTENT_TYPE).iter();
        let media = types
            .next()
            .ok_or(AuthError::Unavailable)?
            .to_str()
            .map_err(|_| AuthError::Unavailable)?;
        if types.next().is_some() || !json_media(media) {
            return Err(AuthError::Unavailable);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| AuthError::Unavailable)? {
            if chunk.len() > MAX_JWKS_BYTES.saturating_sub(body.len()) {
                return Err(AuthError::Unavailable);
            }
            body.extend_from_slice(&chunk);
        }
        let document = validation::parse(&body).map_err(|_| AuthError::Unavailable)?;
        let values = document
            .as_object()
            .and_then(|document| document.get("keys"))
            .and_then(Value::as_array)
            .ok_or(AuthError::Unavailable)?;
        key_set(values).map_err(|_| AuthError::Unavailable)
    }
}

fn endpoint(config: &JwksConfig) -> Result<Url, AuthConfigError> {
    if !(1..=3_600).contains(&config.cache_ttl_seconds)
        || !(1..=300).contains(&config.refresh_interval_seconds)
        || config.refresh_interval_seconds > config.cache_ttl_seconds
        || !(100..=10_000).contains(&config.request_timeout_ms)
        || config.request_timeout_ms > config.refresh_interval_seconds * 1_000
        || config.url.len() > 2_048
        || !config.url.starts_with("https://")
        || !config.url.bytes().all(|byte| byte.is_ascii_graphic())
        || config.url.contains(['\\', '@', '?', '#'])
    {
        return Err(AuthConfigError);
    }
    let url = Url::parse(&config.url).map_err(|_| AuthConfigError)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port() == Some(0)
    {
        return Err(AuthConfigError);
    }
    Ok(url)
}

fn certificates(pem: &str) -> Result<Vec<Certificate>, AuthConfigError> {
    if pem.is_empty() || pem.len() > 16_384 {
        return Err(AuthConfigError);
    }
    const BEGIN: &str = "-----BEGIN CERTIFICATE-----";
    const END: &str = "-----END CERTIFICATE-----";
    let mut remaining = pem.trim();
    let mut result = Vec::new();
    while !remaining.is_empty() {
        if !remaining.starts_with(BEGIN) || result.len() >= 8 {
            return Err(AuthConfigError);
        }
        let end = remaining.find(END).ok_or(AuthConfigError)? + END.len();
        // from_pem alone defers parsing in the selected Rustls backend. Parse
        // the bounded block now; the client builder also validates its DER.
        let mut block = Certificate::from_pem_bundle(&remaining.as_bytes()[..end])
            .map_err(|_| AuthConfigError)?;
        if block.len() != 1 {
            return Err(AuthConfigError);
        }
        result.push(block.remove(0));
        remaining = remaining[end..].trim();
    }
    if result.is_empty() {
        return Err(AuthConfigError);
    }
    Ok(result)
}

fn json_media(value: &str) -> bool {
    let mut parts = value.split(';');
    let media = parts.next().unwrap_or_default().trim();
    if !media.eq_ignore_ascii_case("application/json")
        && !media.eq_ignore_ascii_case("application/jwk-set+json")
    {
        return false;
    }
    match (parts.next(), parts.next()) {
        (None, None) => true,
        (Some(parameter), None) => {
            let Some((name, value)) = parameter.trim().split_once('=') else {
                return false;
            };
            name.eq_ignore_ascii_case("charset")
                && (value.eq_ignore_ascii_case("utf-8") || value.eq_ignore_ascii_case("\"utf-8\""))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> JwksConfig {
        JwksConfig {
            url: "https://issuer.example.test/keys".into(),
            cache_ttl_seconds: 60,
            refresh_interval_seconds: 5,
            request_timeout_ms: 1_000,
            trusted_ca_pem: None,
        }
    }

    #[test]
    fn issuer_key_configuration_rejects_unsafe_endpoints_and_timing() {
        assert!(endpoint(&config()).is_ok());
        for url in [
            "http://issuer.example.test/keys",
            "https://user:secret@issuer.example.test/keys",
            "https://issuer.example.test/keys?secret=x",
            "https://issuer.example.test/keys#x",
            "https://issuer.example.test:0/keys",
            "https://issuer.example.test\\keys",
            "https://issuer.example.test/\nkeys",
        ] {
            let mut changed = config();
            changed.url = url.into();
            assert!(endpoint(&changed).is_err(), "{url}");
        }
        for (ttl, interval, timeout) in [
            (0, 1, 100),
            (3_601, 1, 100),
            (60, 0, 100),
            (600, 301, 100),
            (5, 6, 100),
            (60, 5, 99),
            (60, 5, 10_001),
            (60, 1, 1_001),
        ] {
            let mut changed = config();
            changed.cache_ttl_seconds = ttl;
            changed.refresh_interval_seconds = interval;
            changed.request_timeout_ms = timeout;
            assert!(endpoint(&changed).is_err());
        }
        for pem in [
            "",
            " ",
            "-----BEGIN PRIVATE KEY-----\nx\n-----END PRIVATE KEY-----",
            "-----BEGIN CERTIFICATE-----\ninvalid\n-----END CERTIFICATE-----",
        ] {
            assert!(certificates(pem).is_err());
        }
        let mut invalid_der = config();
        invalid_der.trusted_ca_pem =
            Some("-----BEGIN CERTIFICATE-----\nAA==\n-----END CERTIFICATE-----".into());
        assert!(RemoteKeys::new(invalid_der, Arc::new(super::super::SystemClock)).is_err());
    }

    #[test]
    fn issuer_key_cache_time_and_cancellation_preserve_bounds() {
        let mut state = Cache::default();
        assert_eq!(
            state.observe(Some(Duration::from_secs(10))).unwrap(),
            Duration::from_secs(10)
        );
        assert!(state.observe(None).is_err());
        assert!(state.observe(Some(Duration::from_secs(9))).is_err());
        assert_eq!(state.last_clock, Some(Duration::from_secs(10)));
        state.expiry = Some(Duration::from_secs(20));
        state.last_attempt = Some(Duration::from_secs(10));
        assert!(state.fresh(Duration::from_secs(19)));
        assert!(!state.fresh(Duration::from_secs(20)));
        assert!(state.cooling_down(Duration::from_secs(14), Duration::from_secs(5)));
        assert!(!state.cooling_down(Duration::from_secs(15), Duration::from_secs(5)));
        state.busy = true;
        let state = Mutex::new(state);
        drop(Attempt(&state));
        let state = state.lock().unwrap();
        assert!(!state.busy);
        assert_eq!(state.expiry, Some(Duration::from_secs(20)));
        assert_eq!(state.last_attempt, Some(Duration::from_secs(10)));
    }

    #[test]
    fn issuer_key_response_media_is_explicit() {
        for value in [
            "application/json",
            "Application/JWK-Set+JSON",
            "application/json; charset=utf-8",
            "application/json;charset=\"UTF-8\"",
        ] {
            assert!(json_media(value));
        }
        for value in [
            "text/json",
            "application/jwk+json",
            "application/json,application/json",
            "application/json; charset=latin1",
            "application/json; charset=utf-8; charset=utf-8",
        ] {
            assert!(!json_media(value));
        }
    }
}
