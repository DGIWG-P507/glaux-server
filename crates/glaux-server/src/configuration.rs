//! Bounded, explicit startup configuration. Never format input or resolved secrets.
use crate::authentication::{Authenticator, DevelopmentConfig, JwtConfig, SystemClock};
use crate::authorization::{Admission, ConfiguredPolicy, PolicyConfig, SystemRateClock};
use crate::http_boundary::{HttpBoundary, Limits};
use serde::Deserialize;
use sqlx::ConnectOptions;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::fmt;
use std::io::Read;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const CONFIG_BYTES: u64 = 65_536;
const SECRET_BYTES: u64 = 16_384;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Authentication {
    Disabled,
    Jwt,
    Development,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SecretReference {
    url_env: Option<String>,
    url_file: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    listener: SocketAddr,
    authentication: Authentication,
    jwt: Option<JwtConfig>,
    development: Option<DevelopmentConfig>,
    policy: Option<PolicyConfig>,
    #[serde(default)]
    discovery: bool,
    database: SecretReference,
    health_timeout_ms: u64,
    http: Option<HttpDocument>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HttpDocument {
    public_api_root: Option<String>,
    #[serde(default)]
    limits: Limits,
}

/// Validated configuration, intentionally not Debug/Serialize and not constructible
/// with unchecked fields. Authentication does not grant resource permissions.
pub struct Configuration {
    listener: SocketAddr,
    authentication: Authentication,
    authenticator: Authenticator,
    admission: Admission,
    discovery: bool,
    database: PgConnectOptions,
    timeout: Duration,
    http: HttpBoundary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    Unreadable,
    Invalid,
    UnsafeDevelopment,
    SecretUnavailable,
    InvalidDatabase,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unreadable => "configuration file unavailable or exceeds its bound",
            Self::Invalid => "invalid configuration structure or bounds",
            Self::UnsafeDevelopment => "development authentication requires a loopback listener",
            Self::SecretUnavailable => "required database secret unavailable or invalid",
            Self::InvalidDatabase => "invalid database connection configuration",
        })
    }
}

impl std::error::Error for ConfigError {}

fn bounded_file(path: &Path, maximum: u64) -> Result<Vec<u8>, ()> {
    if !std::fs::metadata(path).map_err(|_| ())?.is_file() {
        return Err(());
    }
    let file = std::fs::File::open(path).map_err(|_| ())?;
    // Do not open devices/pipes as configuration or secret providers.
    if !file.metadata().map_err(|_| ())?.is_file() {
        return Err(());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > maximum {
        return Err(());
    }
    Ok(bytes)
}

impl Configuration {
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let bytes = bounded_file(path, CONFIG_BYTES).map_err(|_| ConfigError::Unreadable)?;
        Self::parse(&bytes, |name| std::env::var(name).ok())
    }

    fn parse(
        bytes: &[u8],
        environment: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        if bytes.len() as u64 > CONFIG_BYTES {
            return Err(ConfigError::Invalid);
        }
        // Typed deserialization alone would not reject duplicate members inside
        // JWK Values. Check every object before constructing trusted key state.
        let syntax = glaux_standards::validation::parse(bytes).map_err(|_| ConfigError::Invalid)?;
        let has_jwt = syntax.get("jwt").is_some();
        let has_development = syntax.get("development").is_some();
        let has_policy = syntax.get("policy").is_some();
        let document: Document = serde_json::from_slice(bytes).map_err(|_| ConfigError::Invalid)?;
        let policy = match (has_policy, document.policy) {
            (false, None) => ConfiguredPolicy::deny_all(),
            (true, Some(policy)) => {
                ConfiguredPolicy::new(policy).map_err(|_| ConfigError::Invalid)?
            }
            _ => return Err(ConfigError::Invalid),
        };
        let admission = Admission::new(
            Arc::new(policy.clone()),
            policy.limits(),
            Arc::new(SystemRateClock::default()),
        )
        .map_err(|_| ConfigError::Invalid)?;
        let http = document.http.unwrap_or_default();
        if document.discovery && http.public_api_root.is_none() {
            return Err(ConfigError::Invalid);
        }
        let http = HttpBoundary::new(http.public_api_root.as_deref(), http.limits)
            .map_err(|_| ConfigError::Invalid)?;
        if document.listener.port() == 0 || !(100..=10_000).contains(&document.health_timeout_ms) {
            return Err(ConfigError::Invalid);
        }
        if document.authentication == Authentication::Development
            && !document.listener.ip().is_loopback()
        {
            return Err(ConfigError::UnsafeDevelopment);
        }
        let authenticator = match document.authentication {
            Authentication::Disabled if !has_jwt && !has_development => Authenticator::disabled(),
            Authentication::Jwt if has_jwt && !has_development => Authenticator::jwt(
                document.jwt.ok_or(ConfigError::Invalid)?,
                Arc::new(SystemClock),
            )
            .map_err(|_| ConfigError::Invalid)?,
            Authentication::Development if has_development && !has_jwt => {
                Authenticator::development(
                    document.development.ok_or(ConfigError::Invalid)?,
                    document.listener,
                )
                .map_err(|_| ConfigError::Invalid)?
            }
            _ => return Err(ConfigError::Invalid),
        };
        let secret = match (&document.database.url_env, &document.database.url_file) {
            (Some(name), None)
                if !name.is_empty()
                    && name.len() <= 256
                    && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') =>
            {
                environment(name).ok_or(ConfigError::SecretUnavailable)?
            }
            (None, Some(path)) if !path.is_empty() => String::from_utf8(
                bounded_file(Path::new(path), SECRET_BYTES)
                    .map_err(|_| ConfigError::SecretUnavailable)?,
            )
            .map_err(|_| ConfigError::SecretUnavailable)?,
            _ => return Err(ConfigError::Invalid),
        };
        if secret.len() as u64 > SECRET_BYTES || secret.trim().is_empty() {
            return Err(ConfigError::SecretUnavailable);
        }
        let options = secret
            .trim()
            .parse::<PgConnectOptions>()
            .map_err(|_| ConfigError::InvalidDatabase)?;
        // Never relax network certificate verification based on a URL parameter.
        let mode = if options.get_socket().is_some() || options.get_host().starts_with('/') {
            PgSslMode::Disable
        } else {
            PgSslMode::VerifyFull
        };
        let timeout = document.health_timeout_ms.to_string();
        let options = options
            .ssl_mode(mode)
            .options([
                ("statement_timeout", timeout.as_str()),
                ("lock_timeout", timeout.as_str()),
            ])
            .disable_statement_logging();
        Ok(Self {
            listener: document.listener,
            authentication: document.authentication,
            authenticator,
            admission,
            discovery: document.discovery,
            database: options,
            timeout: Duration::from_millis(document.health_timeout_ms),
            http,
        })
    }

    pub fn listener(&self) -> SocketAddr {
        self.listener
    }
    pub fn authentication(&self) -> Authentication {
        self.authentication
    }
    pub fn authenticator(&self) -> Authenticator {
        self.authenticator.clone()
    }
    pub fn http_boundary(&self) -> HttpBoundary {
        self.http.clone()
    }
    /// Discovery is an explicit deployment choice, never inferred from Host.
    pub fn discovery_enabled(&self) -> bool {
        self.discovery
    }
    /// Share the same validated policy and denial-rate state across route handlers.
    /// Authentication must supply a verified caller before using this boundary.
    pub fn admission(&self) -> Admission {
        self.admission.clone()
    }
    pub(crate) fn database(&self) -> PgConnectOptions {
        self.database.clone()
    }
    pub(crate) fn timeout(&self) -> Duration {
        self.timeout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(listener: &str, authentication: &str) -> String {
        let development = if authentication == "development" {
            r#","development":{"subject":"health-test-caller","groups":[],"scopes":[]}"#
        } else {
            ""
        };
        format!(
            r#"{{"listener":"{listener}","authentication":"{authentication}","database":{{"url_env":"TEST_SECRET"}},"health_timeout_ms":500{development}}}"#
        )
    }
    fn parse(text: &str) -> Result<Configuration, ConfigError> {
        Configuration::parse(text.as_bytes(), |_| {
            Some("postgres://health:canary@localhost/test?sslmode=disable".into())
        })
    }

    #[test]
    fn runtime_config_rejects_unsafe_development_and_unknown_fields() {
        for address in ["127.0.0.1:8080", "127.3.2.1:8080", "[::1]:8080"] {
            assert!(parse(&document(address, "development")).is_ok());
        }
        for address in [
            "0.0.0.0:8080",
            "192.0.2.1:8080",
            "[::]:8080",
            "[::ffff:127.0.0.1]:8080",
        ] {
            assert!(matches!(
                parse(&document(address, "development")),
                Err(ConfigError::UnsafeDevelopment)
            ));
            assert!(parse(&document(address, "disabled")).is_ok());
        }
        let valid = document("127.0.0.1:8080", "disabled");
        for changed in [
            valid.replace("500}", "500,\"secret-canary\":true}"),
            valid.replace("\"url_env\"", "\"unknown\""),
            valid.replace("500", "0"),
            valid.replace("500", "10001"),
            valid.replace("500", "\"500\""),
            valid.replace("8080", "0"),
            valid.replace("127.0.0.1", "localhost"),
            valid.replace("disabled", "jwt"),
        ] {
            assert!(parse(&changed).is_err(), "invalid config accepted");
        }
        assert!(parse(&valid.replace("500}", "500,\"health_timeout_ms\":500}")).is_err());
        assert!(
            parse(&valid.replace(
                "\"url_env\":\"TEST_SECRET\"",
                "\"url_env\":\"TEST_SECRET\",\"url_file\":\"canary\""
            ))
            .is_err()
        );
    }

    #[test]
    fn runtime_config_secrets_are_required_bounded_and_not_in_errors() {
        let valid = document("127.0.0.1:8080", "disabled");
        for value in [
            None,
            Some(String::new()),
            Some(" ".into()),
            Some("SYNTHETIC_SECRET_CANARY".into()),
            Some("x".repeat(16_385)),
        ] {
            let error = Configuration::parse(valid.as_bytes(), |_| value.clone())
                .err()
                .expect("reject invalid secret");
            assert!(!format!("{error:?} {error}").contains("SYNTHETIC_SECRET_CANARY"));
        }
        assert!(Configuration::parse(&vec![b' '; 65_537], |_| None).is_err());
        let config = parse(&valid).unwrap();
        assert!(matches!(
            config.database.get_ssl_mode(),
            PgSslMode::VerifyFull
        ));
        assert_eq!(config.listener(), "127.0.0.1:8080".parse().unwrap());
        assert_eq!(config.timeout(), Duration::from_millis(500));
    }

    #[test]
    fn runtime_http_config_is_explicit_strict_and_bounded() {
        let valid = document("127.0.0.1:8080", "disabled");
        let with_http = |http: &str| format!("{},\"http\":{http}}}", &valid[..valid.len() - 1]);
        let config = parse(&with_http(
            r#"{"public_api_root":"https://example.test/prefix"}"#,
        ))
        .unwrap();
        assert_eq!(
            config
                .http_boundary()
                .link(&["systems", "id"], &[])
                .unwrap(),
            "https://example.test/prefix/systems/id"
        );
        assert!(
            parse(&valid)
                .unwrap()
                .http_boundary()
                .link(&["systems"], &[])
                .is_err()
        );
        for http in [
            r#"{"public_api_root":"//attacker.test"}"#,
            r#"{"public_api_root":"https://example.test/prefix?secret=canary"}"#,
            r#"{"public_api_root":"https://example.test","unknown":true}"#,
            r#"{"limits":{"body_bytes":256}}"#,
            r#"{"limits":{"body_bytes":0,"header_bytes":2048,"uri_bytes":1024,"timeout_ms":500}}"#,
            r#"{"limits":{"body_bytes":256,"header_bytes":2048,"uri_bytes":1024,"timeout_ms":500,"unknown":true}}"#,
        ] {
            assert!(
                parse(&with_http(http)).is_err(),
                "invalid HTTP configuration accepted"
            );
        }
        assert!(parse(&with_http(r#"{"limits":{"body_bytes":256,"header_bytes":2048,"uri_bytes":1024,"timeout_ms":500}}"#)).is_ok());
    }

    #[test]
    fn runtime_policy_configuration_is_explicit_and_bounded() {
        use serde_json::{Value, json};
        let mut input: Value =
            serde_json::from_str(&document("127.0.0.1:8080", "development")).unwrap();
        input["policy"] = json!({
            "grants": [{"issuer":"urn:glaux:development", "group":"example-group",
                "source":"urn:glaux:test:source-a", "actions":["read"], "resources":null}],
            "denial_audit": {"max_records":100, "max_per_window":10, "window_seconds":60}
        });
        assert!(parse(&input.to_string()).is_ok());
        for (path, value) in [
            ("/policy", Value::Null),
            ("/policy/denial_audit/max_records", json!(0)),
            ("/policy/denial_audit/max_records", json!(100001)),
            ("/policy/denial_audit/max_per_window", json!(0)),
            ("/policy/denial_audit/window_seconds", json!(0)),
            ("/policy/grants/0/source", json!("")),
            ("/policy/grants/0/issuer", json!("")),
            ("/policy/grants/0/group", json!("")),
            ("/policy/grants/0/resources", json!(["not-a-local-id"])),
            ("/policy/grants/0/actions", json!(["invented_action"])),
        ] {
            let mut invalid = input.clone();
            *invalid.pointer_mut(path).unwrap() = value;
            assert!(parse(&invalid.to_string()).is_err(), "{path}");
        }
        for (path, key, value) in [
            ("/policy", "unknown", json!(true)),
            ("/policy/denial_audit", "unlimited", json!(true)),
            ("/policy/grants/0", "subject", json!("also-subject")),
            ("/policy/grants/0", "unknown", json!(true)),
        ] {
            let mut invalid = input.clone();
            invalid
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert(key.into(), value);
            assert!(parse(&invalid.to_string()).is_err(), "{path}/{key}");
        }
        let mut missing = input.clone();
        missing["policy"]
            .as_object_mut()
            .unwrap()
            .remove("denial_audit");
        assert!(parse(&missing.to_string()).is_err());
        let mut no_subject = input.clone();
        no_subject["policy"]["grants"][0]
            .as_object_mut()
            .unwrap()
            .remove("group");
        assert!(parse(&no_subject.to_string()).is_err());
        let duplicated = input.to_string().replace(
            "\"max_records\":100",
            "\"max_records\":100,\"max_records\":100",
        );
        assert!(parse(&duplicated).is_err());
    }

    #[test]
    fn runtime_discovery_requires_explicit_public_root() {
        use serde_json::{Value, json};
        let mut input: Value = serde_json::from_str(&document("127.0.0.1:8080", "disabled")).unwrap();
        assert!(!parse(&input.to_string()).unwrap().discovery_enabled());
        input["discovery"] = json!(true);
        assert!(parse(&input.to_string()).is_err());
        input["http"] = json!({"public_api_root":"https://example.test/prefix"});
        assert!(parse(&input.to_string()).unwrap().discovery_enabled());
        for invalid in [Value::Null, json!("true"), json!(1), json!({"enabled":true})] {
            input["discovery"] = invalid;
            assert!(parse(&input.to_string()).is_err());
        }
        input["discovery"] = json!(false);
        assert!(!parse(&input.to_string()).unwrap().discovery_enabled());
        input.as_object_mut().unwrap().remove("http");
        assert!(!parse(&input.to_string()).unwrap().discovery_enabled());
    }

    #[test]
    fn runtime_key_refresh_configuration_is_explicit_and_offline() {
        use crate::authentication::AuthError;
        use axum::http::HeaderMap;
        use serde_json::{Value, json};

        let mut input: Value =
            serde_json::from_str(&document("127.0.0.1:8080", "disabled")).unwrap();
        input["authentication"] = json!("jwt");
        input["jwt"] = json!({
            "issuer":"https://issuer.example.test", "audience":"glaux",
            "jwks": {"url":"https://127.0.0.1:9/keys", "cache_ttl_seconds":60,
                     "refresh_interval_seconds":5, "request_timeout_ms":500}
        });
        // No issuer is started: syntactic validation must not retrieve keys.
        let configured = parse(&input.to_string()).unwrap();
        assert_eq!(configured.authentication(), Authentication::Jwt);
        assert!(matches!(
            configured
                .authenticator()
                .authenticate(&HeaderMap::new(), None),
            Err(AuthError::Missing)
        ));
        let mut supplied = HeaderMap::new();
        supplied.insert(
            axum::http::header::AUTHORIZATION,
            "Bearer synthetic".parse().unwrap(),
        );
        assert!(matches!(
            configured.authenticator().authenticate(&supplied, None),
            Err(AuthError::Unavailable)
        ));
        for (path, value) in [
            ("/jwt/jwks/url", json!("http://127.0.0.1:9/keys")),
            (
                "/jwt/jwks/url",
                json!("https://user:password@example.test/keys"),
            ),
            (
                "/jwt/jwks/url",
                json!("https://example.test/keys?credential=secret"),
            ),
            ("/jwt/jwks/url", json!("https://example.test/keys#fragment")),
            ("/jwt/jwks/cache_ttl_seconds", json!(0)),
            ("/jwt/jwks/cache_ttl_seconds", json!(3601)),
            ("/jwt/jwks/refresh_interval_seconds", json!(61)),
            ("/jwt/jwks/request_timeout_ms", json!(5001)),
            ("/jwt/jwks/request_timeout_ms", json!(99)),
            ("/jwt/jwks", Value::Null),
        ] {
            let mut invalid = input.clone();
            *invalid.pointer_mut(path).unwrap() = value;
            assert!(parse(&invalid.to_string()).is_err(), "{path}");
        }
        for field in [
            "url",
            "cache_ttl_seconds",
            "refresh_interval_seconds",
            "request_timeout_ms",
        ] {
            let mut invalid = input.clone();
            invalid["jwt"]["jwks"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(parse(&invalid.to_string()).is_err(), "missing {field}");
        }
        let mut invalid = input.clone();
        invalid["jwt"]["jwks"]["follow_redirects"] = json!(true);
        assert!(parse(&invalid.to_string()).is_err());
        let mut both = input.clone();
        both["jwt"]["keys"] = json!([{"kty":"RSA","kid":"also-static"}]);
        assert!(parse(&both.to_string()).is_err());
        let duplicate = input.to_string().replace(
            "\"cache_ttl_seconds\":60",
            "\"cache_ttl_seconds\":60,\"cache_ttl_seconds\":60",
        );
        assert!(parse(&duplicate).is_err());
    }

    #[test]
    fn runtime_authentication_config_is_mode_bound_and_validated() {
        use crate::authentication::{AuthError, CallerKind};
        use axum::http::HeaderMap;
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        use serde_json::{Value, json};

        let disabled = document("127.0.0.1:8080", "disabled");
        let headers = HeaderMap::new();
        let peer = Some("127.0.0.1:32100".parse().unwrap());
        assert!(matches!(
            parse(&disabled)
                .unwrap()
                .authenticator()
                .authenticate(&headers, peer),
            Err(AuthError::Unavailable)
        ));
        let development = document("127.0.0.1:8080", "development");
        let caller = parse(&development)
            .unwrap()
            .authenticator()
            .authenticate(&headers, peer)
            .unwrap();
        assert_eq!(caller.kind(), CallerKind::Development);
        assert_eq!(caller.issuer(), "urn:glaux:development");
        assert_eq!(caller.subject(), "health-test-caller");

        // Synthetic public key shape, not signing material or proof of a token.
        let mut modulus = vec![0x80; 256];
        modulus[255] = 1;
        let mut jwt: serde_json::Value = serde_json::from_str(&disabled).unwrap();
        jwt["authentication"] = json!("jwt");
        jwt["jwt"] = json!({
            "issuer":"https://issuer.example.test",
            "audience":"glaux",
            "keys":[{"kty":"RSA","kid":"key-1","n":URL_SAFE_NO_PAD.encode(modulus),"e":"AQAB"}],
            "required_scopes":["read"]
        });
        let configured = parse(&jwt.to_string()).unwrap();
        assert_eq!(configured.authentication(), Authentication::Jwt);
        assert!(matches!(
            configured.authenticator().authenticate(&headers, peer),
            Err(AuthError::Missing)
        ));
        for (path, value) in [
            ("/jwt/keys", json!([])),
            ("/jwt/keys/0/kty", json!("oct")),
            ("/jwt/keys/0/n", json!("AQAB")),
            ("/jwt/issuer", json!("")),
            ("/jwt/audience", json!("")),
            ("/jwt/required_scopes", json!(["has space"])),
            ("/jwt", Value::Null),
        ] {
            let mut invalid = jwt.clone();
            *invalid.pointer_mut(path).unwrap() = value;
            assert!(parse(&invalid.to_string()).is_err(), "{path}");
        }
        let mut extra = jwt.clone();
        extra["jwt"]["keys_url"] = json!("https://attacker.invalid/keys");
        assert!(parse(&extra.to_string()).is_err());
        let duplicate = jwt
            .to_string()
            .replace("\"kid\":\"key-1\"", "\"kid\":\"key-1\",\"kid\":\"key-1\"");
        assert!(parse(&duplicate).is_err());
        let jwt_text = jwt.to_string();
        for input in [
            disabled.replace("\"disabled\"", "\"development\""),
            development.replace("\"development\",", "\"disabled\","),
            development.replace("health-test-caller", ""),
            development.replace("\"subject\"", "\"unknown\""),
            format!("{},\"jwt\":null}}", &disabled[..disabled.len() - 1]),
            format!("{},\"development\":null}}", &disabled[..disabled.len() - 1]),
            format!(
                "{},\"development\":{{\"subject\":\"fake\"}}}}",
                &jwt_text[..jwt_text.len() - 1]
            ),
        ] {
            assert!(
                parse(&input).is_err(),
                "invalid authentication mode combination accepted"
            );
        }
    }
}
