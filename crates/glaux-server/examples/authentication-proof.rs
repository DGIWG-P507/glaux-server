//! Independent signatures, raw HTTP observations and exact verified identities.
use axum::{
    Router,
    extract::{Extension, State},
    http::HeaderMap,
    response::Response,
    routing::get,
};
use glaux_server::{
    authentication::{
        Authenticator, CallerContext, CallerKind, Clock, DevelopmentConfig, JwtConfig,
    },
    http_boundary::{HttpBoundary, Limits, Problem, json_response},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::oneshot;

const FINAL: &str = "Required authentication proof passed: 7 groups.";
const NOW: Duration = Duration::new(1_700_000_000, 500_000_000);
const CANARY: &str = "SyntheticCredentialSecretCanary";

struct ControlledClock(Mutex<Option<Duration>>);

impl Clock for ControlledClock {
    fn now(&self) -> Option<Duration> {
        *self.0.lock().unwrap()
    }
}

async fn who(
    Extension(caller): Extension<CallerContext>,
    State(count): State<Arc<AtomicUsize>>,
) -> Result<Response, Problem> {
    count.fetch_add(1, Ordering::SeqCst);
    let kind = match caller.kind() {
        CallerKind::Jwt => "jwt",
        CallerKind::Development => "development",
    };
    json_response(
        &json!({
            "issuer": caller.issuer(), "subject": caller.subject(),
            "client_id": caller.client_id(), "scopes": caller.scopes(),
            "groups": caller.groups(), "kind": kind,
        }),
        "application/json",
    )
}

fn fixture(auth: Authenticator, count: Arc<AtomicUsize>) -> Router {
    let routes = Router::new().route("/who", get(who)).with_state(count);
    HttpBoundary::new(None, Limits::default())
        .unwrap()
        .router(auth.protect(routes))
}

fn jwt_config(fixtures: &Value) -> JwtConfig {
    JwtConfig {
        issuer: "https://issuer.example.test".to_owned(),
        audience: "https://api.example.test".to_owned(),
        keys: vec![fixtures["jwk"].clone()],
        required_scopes: vec!["read".to_owned()],
    }
}

fn development_config() -> DevelopmentConfig {
    DevelopmentConfig {
        subject: "development-alice".to_owned(),
        scopes: vec!["read".to_owned()],
        groups: vec!["development-group".to_owned()],
    }
}

#[derive(Debug, Clone)]
struct Wire {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Wire {
    fn header(&self, name: &str) -> Option<&str> {
        let values: Vec<_> = self.headers.iter().filter(|(key, _)| key == name).collect();
        assert!(values.len() <= 1, "unexpected repeated response header");
        values.first().map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("complete general-purpose JSON required")
    }
}

fn decode(bytes: &[u8]) -> Wire {
    let offset = bytes
        .windows(4)
        .position(|value| value == b"\r\n\r\n")
        .unwrap();
    let text = std::str::from_utf8(&bytes[..offset]).unwrap();
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap();
    assert!(status_line.starts_with("HTTP/1.1 "));
    let status = status_line
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let mut wire = Wire {
        status,
        headers,
        body: bytes[offset + 4..].to_vec(),
    };
    if wire.header("transfer-encoding") == Some("chunked") {
        let mut encoded = wire.body.as_slice();
        let mut body = Vec::new();
        loop {
            let end = encoded.windows(2).position(|v| v == b"\r\n").unwrap();
            let count =
                usize::from_str_radix(std::str::from_utf8(&encoded[..end]).unwrap(), 16).unwrap();
            encoded = &encoded[end + 2..];
            if count == 0 {
                assert_eq!(encoded, b"\r\n");
                break;
            }
            assert!(encoded.len() >= count + 2);
            body.extend_from_slice(&encoded[..count]);
            assert_eq!(&encoded[count..count + 2], b"\r\n");
            encoded = &encoded[count + 2..];
        }
        wire.body = body;
    } else if let Some(length) = wire.header("content-length") {
        assert_eq!(wire.body.len(), length.parse::<usize>().unwrap());
    }
    wire
}

fn request(address: SocketAddr, path: &str, headers: &str) -> Wire {
    let mut connection = TcpStream::connect_timeout(&address, Duration::from_secs(3)).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    connection
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let message =
        format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{headers}\r\n");
    connection.write_all(message.as_bytes()).unwrap();
    let mut response = Vec::new();
    connection.take(16_385).read_to_end(&mut response).unwrap();
    assert!(response.len() <= 16_384, "response escaped bound");
    decode(&response)
}

fn bearer(fixtures: &Value, name: &str) -> String {
    format!(
        "Authorization: Bearer {}\r\n",
        fixtures["tokens"][name]
            .as_str()
            .expect("named independent token required")
    )
}

fn expected_identity() -> Value {
    json!({"issuer":"https://issuer.example.test", "subject":"fixture-alice",
        "client_id":"fixture-client", "scopes":["read","write"],
        "groups":["group-a","group-b"], "kind":"jwt"})
}

fn matches_identity(wire: &Wire) -> bool {
    wire.status == 200
        && wire.header("content-type") == Some("application/json")
        && wire.json() == expected_identity()
}

fn oracle_controls() {
    let correct = Wire { status: 200,
        headers: vec![("content-type".to_owned(), "application/json".to_owned())],
        body: br#"{"issuer":"https://issuer.example.test","subject":"fixture-alice","client_id":"fixture-client","scopes":["read","write"],"groups":["group-a","group-b"],"kind":"jwt"}"#.to_vec(),
    };
    assert!(matches_identity(&correct));
    for bad in [json!(null), json!(17), json!("attacker-subject")] {
        let mut wrong = correct.clone();
        let mut value = correct.json();
        value["subject"] = bad;
        wrong.body = serde_json::to_vec(&value).unwrap();
        assert!(
            !matches_identity(&wrong),
            "bad identity field escaped independent oracle"
        );
    }
    let mut missing = correct.json();
    missing.as_object_mut().unwrap().remove("subject");
    let mut wrong = correct;
    wrong.body = serde_json::to_vec(&missing).unwrap();
    assert!(
        !matches_identity(&wrong),
        "missing identity escaped independent oracle"
    );
    println!("Authentication group passed: independent-wire-oracle-controls");
}

fn accepted(address: SocketAddr, headers: &str, count: &AtomicUsize) {
    let before = count.load(Ordering::SeqCst);
    let wire = request(address, "/who", headers);
    assert!(
        matches_identity(&wire),
        "valid signed access token did not produce exact verified caller: {wire:?}"
    );
    assert_eq!(count.load(Ordering::SeqCst), before + 1);
}

fn denied(
    address: SocketAddr,
    path: &str,
    headers: &str,
    status: u16,
    challenge: Option<&str>,
    count: &AtomicUsize,
) {
    let before = count.load(Ordering::SeqCst);
    let wire = request(address, path, headers);
    assert_eq!(
        wire.status, status,
        "credential rejection status differs: {wire:?}"
    );
    assert_eq!(wire.header("www-authenticate"), challenge);
    assert_eq!(
        wire.header("content-type"),
        Some("application/problem+json")
    );
    assert_eq!(wire.header("cache-control"), Some("no-store"));
    let (slug, title, detail) = match status {
        400 => ("bad-request", "Bad Request", "The request is malformed."),
        401 => (
            "unauthorized",
            "Unauthorized",
            "Authentication is required.",
        ),
        403 => (
            "forbidden",
            "Forbidden",
            "The credential lacks required scope.",
        ),
        503 => (
            "unavailable",
            "Service Unavailable",
            "The operation is temporarily unavailable.",
        ),
        _ => panic!("unknown expected fixture status"),
    };
    let value = wire.json();
    assert_eq!(value["status"], json!(status));
    assert_eq!(value["type"], json!(format!("urn:glaux:problem:{slug}")));
    assert_eq!(value["title"], json!(title));
    assert_eq!(value["detail"], json!(detail));
    assert_eq!(value["correlation"].as_str(), wire.header("x-request-id"));
    assert!(
        wire.header("x-request-id")
            .is_some_and(|value| !value.is_empty())
    );
    let body = String::from_utf8_lossy(&wire.body);
    for secret in [
        CANARY,
        "fixture-alice",
        "fixture-client",
        "group-a",
        "BEGIN PRIVATE KEY",
        "stack trace",
    ] {
        assert!(
            !body.contains(secret),
            "credential problem disclosed private context"
        );
    }
    assert_eq!(
        count.load(Ordering::SeqCst),
        before,
        "rejected credential reached protected handler"
    );
}

fn verified(address: SocketAddr, fixtures: &Value, count: &AtomicUsize) {
    for name in [
        "valid",
        "valid-media-type",
        "valid-case-type",
        "valid-aud-array",
        "valid-extension",
    ] {
        accepted(address, &bearer(fixtures, name), count);
    }
    accepted(
        address,
        &bearer(fixtures, "valid").replace("Bearer", "bEaReR"),
        count,
    );
    accepted(
        address,
        &(bearer(fixtures, "valid")
            + "X-User: attacker\r\nX-Groups: administrators\r\nForwarded: for=192.0.2.1\r\n"),
        count,
    );
    println!("Authentication group passed: verified-caller-context");
}

fn rejections(address: SocketAddr, fixtures: &Value, count: &AtomicUsize) {
    for name in [
        "wrong-audience",
        "wrong-issuer",
        "bad-signature",
        "id-token",
        "missing-type",
        "unknown-algorithm",
        "unknown-key",
        "unsupported-critical",
        "unencoded-payload",
        "unsigned",
        "symmetric-confusion",
        "duplicate-claim",
        "duplicate-header",
        "audience-empty",
        "audience-mixed",
        "subject-type",
        "expiry-type",
        "scope-type",
        "groups-type",
        "groups-mixed",
        "missing-iss",
        "missing-sub",
        "missing-aud",
        "missing-exp",
        "missing-iat",
        "missing-client_id",
        "missing-jti",
    ] {
        // The first case is the controlled audience-bypass fault's exact target.
        let before = count.load(Ordering::SeqCst);
        let wire = request(address, "/who", &bearer(fixtures, name));
        assert_eq!(
            wire.status, 401,
            "invalid access-token case accepted: {name}"
        );
        assert_eq!(
            count.load(Ordering::SeqCst),
            before,
            "invalid token reached handler: {name}"
        );
        denied(
            address,
            "/who",
            &bearer(fixtures, name),
            401,
            Some("Bearer error=\"invalid_token\""),
            count,
        );
    }
    println!("Authentication group passed: signature-profile-and-claim-rejections");
}

fn validity(address: SocketAddr, fixtures: &Value, count: &AtomicUsize, clock: &ControlledClock) {
    for name in [
        "expired",
        "future-not-before",
        "future-issued",
        "expiry-before-issued",
        "fraction-exp-equal",
        "fraction-exp-before",
        "fraction-nbf-after",
        "fraction-iat-after",
    ] {
        denied(
            address,
            "/who",
            &bearer(fixtures, name),
            401,
            Some("Bearer error=\"invalid_token\""),
            count,
        );
    }
    for name in [
        "fraction-exp-after",
        "fraction-nbf-equal",
        "fraction-iat-equal",
    ] {
        accepted(address, &bearer(fixtures, name), count);
    }
    denied(
        address,
        "/who",
        &bearer(fixtures, "missing-scope"),
        403,
        Some("Bearer error=\"insufficient_scope\""),
        count,
    );
    *clock.0.lock().unwrap() = None;
    denied(
        address,
        "/who",
        &bearer(fixtures, "valid"),
        503,
        None,
        count,
    );
    *clock.0.lock().unwrap() = Some(NOW);
    accepted(address, &bearer(fixtures, "valid"), count);
    println!("Authentication group passed: exact-validity-clock-and-scope");
}

fn framing(address: SocketAddr, fixtures: &Value, count: &AtomicUsize) {
    denied(address, "/who", "", 401, Some("Bearer"), count);
    denied(
        address,
        "/who?access_token=ignored",
        "X-User: administrator\r\n",
        401,
        Some("Bearer"),
        count,
    );
    denied(
        address,
        "/who",
        "Authorization: Basic SyntheticCredentialSecretCanary\r\n",
        401,
        Some("Bearer"),
        count,
    );
    for headers in [
        "Authorization: Bearer\r\n".to_owned(),
        "Authorization: Bearer a b\r\n".to_owned(),
        bearer(fixtures, "valid") + &bearer(fixtures, "valid"),
    ] {
        denied(
            address,
            "/who",
            &headers,
            400,
            Some("Bearer error=\"invalid_request\""),
            count,
        );
    }
    for token in ["one", "a.b.c", "a.b.c.d", "%%%.e30.invalid", CANARY] {
        denied(
            address,
            "/who",
            &format!("Authorization: Bearer {token}\r\n"),
            401,
            Some("Bearer error=\"invalid_token\""),
            count,
        );
    }
    println!("Authentication group passed: header-framing-and-no-disclosure");
}

fn config_controls(fixtures: &Value, clock: Arc<ControlledClock>) {
    let valid = jwt_config(fixtures);
    assert!(Authenticator::jwt(valid.clone(), clock.clone()).is_ok());
    let mut missing_keys = valid.clone();
    missing_keys.keys.clear();
    assert!(Authenticator::jwt(missing_keys, clock.clone()).is_err());
    let mut repeated = valid.clone();
    repeated.keys.push(repeated.keys[0].clone());
    assert!(Authenticator::jwt(repeated, clock.clone()).is_err());
    let mut secret = valid;
    secret.keys[0]["d"] = json!(CANARY);
    assert!(Authenticator::jwt(secret, clock).is_err());
    for bind in ["0.0.0.0:8080", "[::]:8080", "192.0.2.1:8080"] {
        assert!(Authenticator::development(development_config(), bind.parse().unwrap()).is_err());
    }
    let auth = Authenticator::development(development_config(), "127.0.0.1:8080".parse().unwrap())
        .unwrap();
    assert!(auth.authenticate(&HeaderMap::new(), None).is_err());
    assert!(
        auth.authenticate(&HeaderMap::new(), Some("192.0.2.1:1234".parse().unwrap()))
            .is_err()
    );
    assert!(
        auth.authenticate(&HeaderMap::new(), Some("127.0.0.1:1234".parse().unwrap()))
            .is_ok()
    );
    assert!(
        auth.authenticate(&HeaderMap::new(), Some("[::1]:1234".parse().unwrap()))
            .is_ok()
    );
}

async fn listener(router: Router, checks: impl FnOnce(SocketAddr) + Send + 'static) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    assert!(address.ip().is_loopback());
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stop_rx.await;
        })
        .await
        .unwrap();
    });
    let (done_tx, done_rx) = oneshot::channel();
    let client = std::thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| checks(address)));
        let _ = done_tx.send(outcome);
    });
    let outcome = done_rx
        .await
        .expect("independent client completion signal lost");
    client.join().expect("client failed outside assertions");
    stop_tx
        .send(())
        .expect("owned listener stopped before cleanup");
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .expect("listener cleanup timed out")
        .unwrap();
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err(),
        "owned listener survived cleanup"
    );
    if let Err(error) = outcome {
        resume_unwind(error);
    }
}

async fn proof(fixtures: Value) {
    oracle_controls();
    let count = Arc::new(AtomicUsize::new(0));
    let clock = Arc::new(ControlledClock(Mutex::new(Some(NOW))));
    let auth = Authenticator::jwt(jwt_config(&fixtures), clock.clone())
        .expect("valid public-key configuration rejected");
    let jwt_fixtures = fixtures.clone();
    let jwt_count = count.clone();
    let jwt_clock = clock.clone();
    listener(fixture(auth, count.clone()), move |address| {
        verified(address, &jwt_fixtures, &jwt_count);
        rejections(address, &jwt_fixtures, &jwt_count);
        validity(address, &jwt_fixtures, &jwt_count, &jwt_clock);
        framing(address, &jwt_fixtures, &jwt_count);
    })
    .await;
    config_controls(&fixtures, clock.clone());
    let dev = Authenticator::development(development_config(), "127.0.0.1:8080".parse().unwrap())
        .unwrap();
    let dev_count = count.clone();
    let valid_bearer = bearer(&fixtures, "valid");
    listener(fixture(dev, count.clone()), move |address| {
        for headers in [
            "",
            "X-User: attacker\r\nX-Groups: administrators\r\nForwarded: for=192.0.2.1\r\n",
        ] {
            let before = dev_count.load(Ordering::SeqCst);
            let wire = request(address, "/who", headers);
            assert_eq!(wire.status, 200);
            let value = wire.json();
            assert_eq!(value["subject"], json!("development-alice"));
            assert_eq!(value["kind"], json!("development"));
            assert_eq!(value["groups"], json!(["development-group"]));
            assert_eq!(value["scopes"], json!(["read"]));
            assert_eq!(value["client_id"], Value::Null);
            assert_eq!(dev_count.load(Ordering::SeqCst), before + 1);
        }
        for headers in [
            valid_bearer.as_str(),
            "Authorization: Bearer SyntheticCredentialSecretCanary\r\n",
        ] {
            denied(
                address,
                "/who",
                headers,
                401,
                Some("Bearer error=\"invalid_token\""),
                &dev_count,
            );
        }
    })
    .await;
    let disabled_count = count.clone();
    listener(
        fixture(Authenticator::disabled(), count.clone()),
        move |address| {
            denied(address, "/who", "", 503, None, &disabled_count);
        },
    )
    .await;
    println!("Authentication group passed: explicit-loopback-development-boundary");
    let auth = Authenticator::jwt(jwt_config(&fixtures), clock).unwrap();
    listener(fixture(auth, count.clone()), move |address| {
        let mut seed = 0x2001_u32;
        for index in 0..96 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let token = match seed % 3 {
                0 => format!("e30.e30.invalid{index}"),
                1 => format!("a{index}.b.c.d"),
                _ => format!("invalid{index}"),
            };
            denied(
                address,
                "/who",
                &format!("Authorization: Bearer {token}\r\n"),
                401,
                Some("Bearer error=\"invalid_token\""),
                &count,
            );
        }
    })
    .await;
    println!("Authentication group passed: bounded-generated-input-and-clean-shutdown");
    println!("{FINAL}");
}

fn main() {
    let arguments: Vec<_> = std::env::args_os().collect();
    assert_eq!(arguments.len(), 2, "one owned public-fixture file required");
    let bytes = std::fs::read(&arguments[1]).unwrap();
    assert!(bytes.len() <= 131_072, "public fixture bound exceeded");
    let fixtures = serde_json::from_slice(&bytes).unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(30), proof(fixtures))
                .await
                .expect("bounded authentication proof timed out");
        });
}
