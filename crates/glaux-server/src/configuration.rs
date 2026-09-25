//! Bounded, explicit startup configuration. Never format input or resolved secrets.
use serde::Deserialize;
use sqlx::ConnectOptions;
use sqlx::postgres::{PgConnectOptions, PgSslMode};
use std::fmt;
use std::io::Read;
use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

const CONFIG_BYTES: u64 = 65_536;
const SECRET_BYTES: u64 = 16_384;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Authentication {
    Disabled,
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
    database: SecretReference,
    health_timeout_ms: u64,
}

/// Validated configuration, intentionally not Debug/Serialize and not constructible
/// with unchecked fields. No resource authentication or permission is implemented.
pub struct Configuration {
    listener: SocketAddr,
    authentication: Authentication,
    database: PgConnectOptions,
    timeout: Duration,
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
    let file = std::fs::File::open(path).map_err(|_| ())?;
    // Do not open devices/pipes as configuration or secret providers.
    if !file.metadata().map_err(|_| ())?.is_file() {
        return Err(());
    }
    let mut bytes = Vec::new();
    file.take(maximum + 1).read_to_end(&mut bytes).map_err(|_| ())?;
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

    fn parse(bytes: &[u8], environment: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        if bytes.len() as u64 > CONFIG_BYTES {
            return Err(ConfigError::Invalid);
        }
        let document: Document = serde_json::from_slice(bytes).map_err(|_| ConfigError::Invalid)?;
        if document.listener.port() == 0 || !(100..=10_000).contains(&document.health_timeout_ms) {
            return Err(ConfigError::Invalid);
        }
        if document.authentication == Authentication::Development && !document.listener.ip().is_loopback() {
            return Err(ConfigError::UnsafeDevelopment);
        }
        let secret = match (&document.database.url_env, &document.database.url_file) {
            (Some(name), None) if !name.is_empty() && name.len() <= 256 && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') => {
                environment(name).ok_or(ConfigError::SecretUnavailable)?
            }
            (None, Some(path)) if !path.is_empty() => {
                String::from_utf8(bounded_file(Path::new(path), SECRET_BYTES).map_err(|_| ConfigError::SecretUnavailable)?)
                    .map_err(|_| ConfigError::SecretUnavailable)?
            }
            _ => return Err(ConfigError::Invalid),
        };
        if secret.len() as u64 > SECRET_BYTES || secret.trim().is_empty() {
            return Err(ConfigError::SecretUnavailable);
        }
        let options = secret.trim().parse::<PgConnectOptions>().map_err(|_| ConfigError::InvalidDatabase)?;
        // Never relax network certificate verification based on a URL parameter.
        let mode = if options.get_socket().is_some() || options.get_host().starts_with('/') {
            PgSslMode::Disable
        } else {
            PgSslMode::VerifyFull
        };
        let timeout = document.health_timeout_ms.to_string();
        let options = options.ssl_mode(mode)
            .options([("statement_timeout", timeout.as_str()), ("lock_timeout", timeout.as_str())])
            .disable_statement_logging();
        Ok(Self {
            listener: document.listener,
            authentication: document.authentication,
            database: options,
            timeout: Duration::from_millis(document.health_timeout_ms),
        })
    }

    pub fn listener(&self) -> SocketAddr { self.listener }
    pub fn authentication(&self) -> Authentication { self.authentication }
    pub(crate) fn database(&self) -> PgConnectOptions { self.database.clone() }
    pub(crate) fn timeout(&self) -> Duration { self.timeout }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(listener: &str, authentication: &str) -> String {
        format!(r#"{{"listener":"{listener}","authentication":"{authentication}","database":{{"url_env":"TEST_SECRET"}},"health_timeout_ms":500}}"#)
    }
    fn parse(text: &str) -> Result<Configuration, ConfigError> {
        Configuration::parse(text.as_bytes(), |_| Some("postgres://health:canary@localhost/test?sslmode=disable".into()))
    }

    #[test]
    fn runtime_config_rejects_unsafe_development_and_unknown_fields() {
        for address in ["127.0.0.1:8080", "127.3.2.1:8080", "[::1]:8080"] {
            assert!(parse(&document(address, "development")).is_ok());
        }
        for address in ["0.0.0.0:8080", "192.0.2.1:8080", "[::]:8080", "[::ffff:127.0.0.1]:8080"] {
            assert!(matches!(parse(&document(address, "development")), Err(ConfigError::UnsafeDevelopment)));
            assert!(parse(&document(address, "disabled")).is_ok());
        }
        let valid = document("127.0.0.1:8080", "disabled");
        for changed in [valid.replace("500}", "500,\"secret-canary\":true}"), valid.replace("\"url_env\"", "\"unknown\""), valid.replace("500", "0"), valid.replace("500", "10001"), valid.replace("500", "\"500\""), valid.replace("8080", "0"), valid.replace("127.0.0.1", "localhost"), valid.replace("disabled", "jwt")] {
            assert!(parse(&changed).is_err(), "invalid config accepted");
        }
        assert!(parse(&valid.replace("500}", "500,\"health_timeout_ms\":500}")).is_err());
        assert!(parse(&valid.replace("\"url_env\":\"TEST_SECRET\"", "\"url_env\":\"TEST_SECRET\",\"url_file\":\"canary\"")).is_err());
    }

    #[test]
    fn runtime_config_secrets_are_required_bounded_and_not_in_errors() {
        let valid = document("127.0.0.1:8080", "disabled");
        for value in [None, Some(String::new()), Some(" ".into()), Some("SYNTHETIC_SECRET_CANARY".into()), Some("x".repeat(16_385))] {
            let error = Configuration::parse(valid.as_bytes(), |_| value.clone()).err().expect("reject invalid secret");
            assert!(!format!("{error:?} {error}").contains("SYNTHETIC_SECRET_CANARY"));
        }
        assert!(Configuration::parse(&vec![b' '; 65_537], |_| None).is_err());
        let config = parse(&valid).unwrap();
        assert_eq!(config.database.get_ssl_mode(), PgSslMode::VerifyFull);
        assert_eq!(config.listener(), "127.0.0.1:8080".parse().unwrap());
        assert_eq!(config.timeout(), Duration::from_millis(500));
    }
}
