//! One configured RFC 9068 signed-access-token adapter, not a general JWT API.
//!
//! The cryptographic library checks the signature and fixed algorithm. This
//! module owns claim typing, exact injected-clock checks and issuer/audience
//! binding. Decoding by itself never produces a caller context.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use glaux_domain::numeric::ExactNumber;
use glaux_standards::validation;
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use serde_json::{Map, Value};

use super::{AuthConfigError, AuthError, CallerContext, CallerKind, JwtConfig};

const MAX_TOKEN_BYTES: usize = 16_384;
const MAX_HEADER_BYTES: usize = 2_048;
const MAX_CLAIMS_BYTES: usize = 12_288;
const MAX_KEYS: usize = 16;
const MAX_ID_BYTES: usize = 1_024;
const MAX_KID_BYTES: usize = 128;
const MAX_ITEMS: usize = 64;
const MAX_SCOPE_BYTES: usize = 128;
const MAX_GROUP_BYTES: usize = 256;
const MAX_NUMERIC_DATE_BYTES: usize = 128;

#[derive(Clone)]
pub(super) struct JwtVerifier {
    issuer: String,
    audience: String,
    keys: BTreeMap<String, DecodingKey>,
    required_scopes: Vec<String>,
}

impl JwtVerifier {
    pub(super) fn new(config: JwtConfig) -> Result<Self, AuthConfigError> {
        if !bounded_text(&config.issuer, MAX_ID_BYTES)
            || !bounded_text(&config.audience, MAX_ID_BYTES)
            || config.keys.is_empty()
            || config.keys.len() > MAX_KEYS
            || config.required_scopes.len() > MAX_ITEMS
        {
            return Err(AuthConfigError);
        }
        let mut required = BTreeSet::new();
        for scope in &config.required_scopes {
            if !scope_token(scope) || !required.insert(scope) {
                return Err(AuthConfigError);
            }
        }
        let mut keys = BTreeMap::new();
        for key in config.keys {
            let (kid, decoding) = configured_key(&key)?;
            if keys.insert(kid, decoding).is_some() {
                return Err(AuthConfigError);
            }
        }
        Ok(Self {
            issuer: config.issuer,
            audience: config.audience,
            keys,
            required_scopes: config.required_scopes,
        })
    }

    pub(super) fn verify(&self, token: &str, now: Duration) -> Result<CallerContext, AuthError> {
        if token.is_empty() || token.len() > MAX_TOKEN_BYTES {
            return Err(AuthError::InvalidToken);
        }
        let mut segments = token.split('.');
        let encoded_header = segments.next().ok_or(AuthError::InvalidToken)?;
        let encoded_claims = segments.next().ok_or(AuthError::InvalidToken)?;
        let encoded_signature = segments.next().ok_or(AuthError::InvalidToken)?;
        if segments.next().is_some() {
            return Err(AuthError::InvalidToken);
        }
        let header = object(&segment(encoded_header, MAX_HEADER_BYTES)?)?;
        let claims = object(&segment(encoded_claims, MAX_CLAIMS_BYTES)?)?;
        // Canonical base64url and bounded signature size are checked before
        // invoking crypto. Only configured 2048..4096-bit RSA keys are allowed.
        let signature = segment(encoded_signature, 512)?;
        if !(256..=512).contains(&signature.len()) {
            return Err(AuthError::InvalidToken);
        }
        let kid = access_header(&header)?;
        let key = self.keys.get(kid).ok_or(AuthError::InvalidToken)?;

        let mut checks = Validation::new(Algorithm::RS256);
        checks.required_spec_claims.clear();
        checks.validate_exp = false;
        checks.validate_nbf = false;
        checks.validate_aud = false;
        checks.leeway = 0;
        // These claim checks are intentionally performed below, not omitted:
        // the library's wall clock and integer-rounded NumericDates cannot
        // express this adapter's injected-clock, exact fractional boundaries.
        // IgnoredAny prevents library-decoded/normalized values becoming the
        // authority; the duplicate-safe, wire-kind-preserving parse above is
        // used only AFTER cryptographic verification succeeds.
        decode::<serde::de::IgnoredAny>(token, key, &checks)
            .map_err(|_| AuthError::InvalidToken)?;

        let issuer = claim_text(&claims, "iss", MAX_ID_BYTES)?;
        if issuer != self.issuer {
            return Err(AuthError::InvalidToken);
        }
        let subject = claim_text(&claims, "sub", MAX_ID_BYTES)?;
        let client_id = claim_text(&claims, "client_id", MAX_ID_BYTES)?;
        claim_text(&claims, "jti", MAX_ID_BYTES)?;
        let audiences = audiences(claims.get("aud").ok_or(AuthError::InvalidToken)?)?;
        if !audiences.iter().any(|value| *value == self.audience) {
            return Err(AuthError::InvalidToken);
        }
        check_times(&claims, now)?;
        let scopes = scopes(claims.get("scope"))?;
        let groups = groups(claims.get("groups"))?;
        if self
            .required_scopes
            .iter()
            .any(|required| !scopes.contains(required))
        {
            return Err(AuthError::InsufficientScope);
        }
        // Neither a group nor a scope is a producer, reporting or delegation
        // permission. Later resource policy consumes this verified identity.
        Ok(CallerContext {
            issuer: issuer.to_owned(),
            subject: subject.to_owned(),
            client_id: Some(client_id.to_owned()),
            scopes,
            groups,
            kind: CallerKind::Jwt,
        })
    }
}

fn configured_key(value: &Value) -> Result<(String, DecodingKey), AuthConfigError> {
    let key = value.as_object().ok_or(AuthConfigError)?;
    if key.len() > 32
        || key.get("kty").and_then(Value::as_str) != Some("RSA")
        || ["d", "p", "q", "dp", "dq", "qi", "oth", "k"]
            .iter()
            .any(|name| key.contains_key(*name))
        || key
            .get("alg")
            .is_some_and(|value| value.as_str() != Some("RS256"))
        || key
            .get("use")
            .is_some_and(|value| value.as_str() != Some("sig"))
    {
        return Err(AuthConfigError);
    }
    if let Some(operations) = key.get("key_ops") {
        let operations = operations.as_array().ok_or(AuthConfigError)?;
        if operations.len() != 1 || operations[0].as_str() != Some("verify") {
            return Err(AuthConfigError);
        }
    }
    let kid = claim_text(key, "kid", MAX_KID_BYTES).map_err(|_| AuthConfigError)?;
    let n = claim_text(key, "n", 684).map_err(|_| AuthConfigError)?;
    let e = claim_text(key, "e", 6).map_err(|_| AuthConfigError)?;
    let modulus = segment(n, 512).map_err(|_| AuthConfigError)?;
    let exponent = segment(e, 4).map_err(|_| AuthConfigError)?;
    // RFC 7518 Base64urlUInt is minimal unsigned big-endian, not a signed DER
    // integer with a zero sign octet. Bounds are local profile choices.
    let first = *modulus.first().ok_or(AuthConfigError)?;
    let bits = modulus.len() * 8 - first.leading_zeros() as usize;
    if first == 0
        || !(2_048..=4_096).contains(&bits)
        || modulus.last().is_none_or(|value| value & 1 == 0)
        || exponent.first().is_none_or(|value| *value == 0)
    {
        return Err(AuthConfigError);
    }
    let exponent_value = exponent
        .iter()
        .fold(0_u32, |result, byte| (result << 8) | u32::from(*byte));
    if exponent_value < 3 || exponent_value & 1 == 0 {
        return Err(AuthConfigError);
    }
    Ok((
        kid.to_owned(),
        DecodingKey::from_rsa_raw_components(&modulus, &exponent),
    ))
}

fn segment(encoded: &str, max_decoded: usize) -> Result<Vec<u8>, AuthError> {
    if encoded.is_empty()
        || encoded.len() > max_decoded.div_ceil(3) * 4
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(AuthError::InvalidToken);
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| AuthError::InvalidToken)?;
    if bytes.len() > max_decoded || URL_SAFE_NO_PAD.encode(&bytes) != encoded {
        return Err(AuthError::InvalidToken);
    }
    Ok(bytes)
}

fn object(bytes: &[u8]) -> Result<Map<String, Value>, AuthError> {
    match validation::parse(bytes).map_err(|_| AuthError::InvalidToken)? {
        Value::Object(value) => Ok(value),
        _ => Err(AuthError::InvalidToken),
    }
}

fn access_header(header: &Map<String, Value>) -> Result<&str, AuthError> {
    let kind = claim_text(header, "typ", 64)?;
    if !(kind.eq_ignore_ascii_case("at+jwt") || kind.eq_ignore_ascii_case("application/at+jwt"))
        || header.get("alg").and_then(Value::as_str) != Some("RS256")
        || header.contains_key("crit")
        || header
            .get("b64")
            .is_some_and(|value| value != &Value::Bool(true))
        || ["jku", "jwk", "x5u", "x5c", "enc", "zip"]
            .iter()
            .any(|name| header.contains_key(*name))
    {
        // This compact signed-only adapter has no critical extension, remote
        // key, certificate-chain, encryption or compression interpretation.
        return Err(AuthError::InvalidToken);
    }
    claim_text(header, "kid", MAX_KID_BYTES)
}

fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn claim_text<'a>(
    claims: &'a Map<String, Value>,
    name: &str,
    max: usize,
) -> Result<&'a str, AuthError> {
    claims
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| bounded_text(value, max))
        .ok_or(AuthError::InvalidToken)
}

fn audiences(value: &Value) -> Result<Vec<&str>, AuthError> {
    let values = match value {
        Value::String(value) => vec![value.as_str()],
        Value::Array(values) if !values.is_empty() && values.len() <= MAX_ITEMS => values
            .iter()
            .map(|value| value.as_str().ok_or(AuthError::InvalidToken))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(AuthError::InvalidToken),
    };
    if values
        .iter()
        .any(|value| !bounded_text(value, MAX_ID_BYTES))
    {
        return Err(AuthError::InvalidToken);
    }
    Ok(values)
}

fn numeric_date(claims: &Map<String, Value>, name: &str) -> Result<ExactNumber, AuthError> {
    match claims.get(name) {
        Some(Value::Number(value)) => {
            let text = value.to_string();
            if text.len() > MAX_NUMERIC_DATE_BYTES {
                return Err(AuthError::InvalidToken);
            }
            ExactNumber::parse_json_number(&text).map_err(|_| AuthError::InvalidToken)
        }
        _ => Err(AuthError::InvalidToken),
    }
}

fn check_times(claims: &Map<String, Value>, now: Duration) -> Result<(), AuthError> {
    let now =
        ExactNumber::parse_json_number(&format!("{}.{:09}", now.as_secs(), now.subsec_nanos()))
            .map_err(|_| AuthError::Unavailable)?;
    let expiry = numeric_date(claims, "exp")?;
    let issued = numeric_date(claims, "iat")?;
    if expiry <= now || issued > now || expiry <= issued {
        return Err(AuthError::InvalidToken);
    }
    if claims.contains_key("nbf") && numeric_date(claims, "nbf")? > now {
        return Err(AuthError::InvalidToken);
    }
    Ok(())
}

fn scope_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_SCOPE_BYTES
        && value.bytes().all(|byte| {
            byte == 0x21 || (0x23..=0x5b).contains(&byte) || (0x5d..=0x7e).contains(&byte)
        })
}

fn scopes(value: Option<&Value>) -> Result<Vec<String>, AuthError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let text = value.as_str().ok_or(AuthError::InvalidToken)?;
    // RFC 6749's scope-token *( SP scope-token ), not arbitrary whitespace.
    let mut result = Vec::new();
    let mut count = 0;
    for token in text.split(' ') {
        count += 1;
        if count > MAX_ITEMS || !scope_token(token) {
            return Err(AuthError::InvalidToken);
        }
        if !result.iter().any(|value| value == token) {
            result.push(token.to_owned());
        }
    }
    Ok(result)
}

fn groups(value: Option<&Value>) -> Result<Vec<String>, AuthError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value.as_array().ok_or(AuthError::InvalidToken)?;
    if values.len() > MAX_ITEMS {
        return Err(AuthError::InvalidToken);
    }
    let mut result = Vec::new();
    for value in values {
        let value = value
            .as_str()
            .filter(|value| bounded_text(value, MAX_GROUP_BYTES))
            .ok_or(AuthError::InvalidToken)?;
        if !result.iter().any(|item| item == value) {
            result.push(value.to_owned());
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn jwt_compact_parser_rejects_ambiguous_inputs_and_key_hints() {
        for encoded in ["", "e30=", "e30+", "e30/", "e3_", "é"] {
            assert!(segment(encoded, 128).is_err(), "{encoded}");
        }
        assert_eq!(segment("e30", 2).unwrap(), b"{}");
        assert!(segment("e30", 1).is_err());
        assert!(object(br#"{"sub":"a","\u0073ub":"b"}"#).is_err());
        assert!(object(br#"[]"#).is_err());
        let valid = json!({"typ":"AT+JWT","alg":"RS256","kid":"key-1"});
        assert_eq!(access_header(valid.as_object().unwrap()).unwrap(), "key-1");
        for (name, value) in [
            ("typ", json!("JWT")),
            ("alg", json!("HS256")),
            ("crit", json!([])),
            ("b64", json!(false)),
            ("jku", json!("https://attacker.invalid/keys")),
            ("jwk", json!({"kty":"RSA"})),
            ("x5c", json!([])),
        ] {
            let mut header = valid.as_object().unwrap().clone();
            header.insert(name.into(), value);
            assert!(access_header(&header).is_err(), "{name}");
        }
        let mut header = valid.as_object().unwrap().clone();
        header.insert("extension".into(), json!({"no":"authority"}));
        assert!(access_header(&header).is_ok());
    }

    #[test]
    fn jwt_numeric_dates_keep_exact_fractional_boundaries() {
        let claims = object(br#"{"exp":100.000000001,"iat":99.5,"nbf":100}"#).unwrap();
        assert!(check_times(&claims, Duration::from_secs(100)).is_ok());
        assert!(check_times(&claims, Duration::new(100, 1)).is_err());
        assert!(check_times(&claims, Duration::new(99, 999_999_999)).is_err());
        let tiny = object(br#"{"exp":100.00000000000000000001,"iat":1e2}"#).unwrap();
        assert!(check_times(&tiny, Duration::from_secs(100)).is_ok());
        assert!(check_times(&tiny, Duration::new(100, 1)).is_err());
        for raw in [
            br#"{"exp":"101","iat":99}"#.as_slice(),
            br#"{"exp":101,"iat":100.000000001}"#,
            br#"{"exp":100,"iat":99}"#,
            br#"{"exp":101,"iat":99,"nbf":null}"#,
            br#"{"exp":101,"iat":99,"nbf":100.000000001}"#,
            br#"{"exp":{"$serde_json::private::Number":"101"},"iat":99}"#,
        ] {
            assert!(check_times(&object(raw).unwrap(), Duration::from_secs(100)).is_err());
        }
    }

    #[test]
    fn jwt_claim_lists_are_bounded_typed_and_not_policy() {
        assert_eq!(
            scopes(Some(&json!("read write read"))).unwrap(),
            ["read", "write"]
        );
        assert_eq!(scopes(None).unwrap(), Vec::<String>::new());
        for value in [
            json!(""),
            json!(" read"),
            json!("read  write"),
            json!("read\twrite"),
            json!(["read"]),
            json!("r\\w"),
        ] {
            assert!(scopes(Some(&value)).is_err());
        }
        assert!(
            scopes(Some(&json!(
                (0..65)
                    .map(|i| format!("s{i}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            )))
            .is_err()
        );
        assert_eq!(
            groups(Some(&json!(["operators", "operators"]))).unwrap(),
            ["operators"]
        );
        assert!(groups(Some(&json!([{"value":"operators"}]))).is_err());
        assert!(groups(Some(&json!([""]))).is_err());
        assert!(audiences(&json!([])).is_err());
        assert!(audiences(&json!(["service", 1])).is_err());
        assert_eq!(
            audiences(&json!(["service", "other"])).unwrap(),
            ["service", "other"]
        );
    }

    #[test]
    fn jwt_config_limits_rsa_keys_and_rejects_private_material() {
        let mut n = vec![0x80; 256];
        n[255] = 1;
        let public = json!({"kty":"RSA","kid":"k1","n":URL_SAFE_NO_PAD.encode(&n),"e":"AQAB","alg":"RS256","use":"sig","key_ops":["verify"]});
        assert!(configured_key(&public).is_ok());
        for (name, value) in [
            ("kty", json!("oct")),
            ("alg", json!("HS256")),
            ("use", json!("enc")),
            ("key_ops", json!(["sign"])),
            ("d", json!("private")),
            ("e", json!("Ag")),
            ("e", json!("AAEAAQ")),
            ("n", json!(URL_SAFE_NO_PAD.encode(&n[..255]))),
            ("n", json!(URL_SAFE_NO_PAD.encode([&[0][..], &n].concat()))),
        ] {
            let mut key = public.clone();
            key[name] = value;
            assert!(configured_key(&key).is_err(), "{name}");
        }
        let config = JwtConfig {
            issuer: "https://issuer.example.test".into(),
            audience: "glaux".into(),
            keys: vec![public.clone(), public],
            required_scopes: vec![],
        };
        assert!(JwtVerifier::new(config).is_err());
    }
}
