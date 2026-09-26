//! Real isolated HTTPS retrieval, independent signatures and explicit barriers.
use axum::{
    Router,
    extract::{Extension, State},
    http::{HeaderMap, HeaderValue, header},
    response::Response,
    routing::get,
};
use glaux_server::{
    authentication::{
        AuthError, Authenticator, CallerContext, CallerKind, Clock, JwksConfig, JwtConfig,
    },
    http_boundary::{HttpBoundary, Limits, Problem, json_response},
};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{sync::oneshot, task::JoinHandle};

const FINAL: &str = "Required key-refresh proof passed: 7 groups.";
const WALL: u64 = 1_700_000_000;

struct ControlledClock(Mutex<Option<Duration>>);

impl ControlledClock {
    fn new(seconds: u64) -> Arc<Self> {
        Arc::new(Self(Mutex::new(Some(Duration::from_secs(seconds)))))
    }

    fn set(&self, seconds: Option<u64>) {
        *self.0.lock().unwrap() = seconds.map(Duration::from_secs);
    }
}

impl Clock for ControlledClock {
    fn now(&self) -> Option<Duration> {
        *self.0.lock().unwrap()
    }
}

struct Adapter {
    auth: Authenticator,
    age: Arc<ControlledClock>,
    wall: Arc<ControlledClock>,
}

fn config(fixtures: &Value, target: &str, timeout: u64) -> JwtConfig {
    JwtConfig {
        issuer: "https://issuer.example.test".into(),
        audience: "https://api.example.test".into(),
        keys: vec![],
        jwks: Some(JwksConfig {
            url: format!("{}/{target}", fixtures["base"].as_str().unwrap()),
            cache_ttl_seconds: 10,
            refresh_interval_seconds: 2,
            request_timeout_ms: timeout,
            trusted_ca_pem: Some(fixtures["ca"].as_str().unwrap().into()),
        }),
        required_scopes: vec!["read".into()],
    }
}

fn adapter_with(config: JwtConfig) -> Adapter {
    let age = ControlledClock::new(100);
    let wall = ControlledClock::new(WALL);
    let auth = Authenticator::jwt_with_key_clock(config, wall.clone(), age.clone()).unwrap();
    Adapter { auth, age, wall }
}

fn adapter(fixtures: &Value, target: &str) -> Adapter {
    adapter_with(config(fixtures, target, 300))
}

fn token(fixtures: &Value, name: &str) -> String {
    fixtures["tokens"][name].as_str().unwrap().to_owned()
}

fn headers(token: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {token}")).unwrap(),
    );
    headers
}

async fn authenticate(auth: &Authenticator, fixtures: &Value, name: &str) -> Result<CallerContext, AuthError> {
    auth.authenticate_async(&headers(&token(fixtures, name)), None).await
}

fn identity(caller: CallerContext, name: &str) {
    assert_eq!(caller.issuer(), "https://issuer.example.test");
    assert_eq!(caller.subject(), if name == "a" { "fixture-alice" } else { "fixture-bob" });
    assert_eq!(caller.client_id(), Some("fixture-client"));
    assert_eq!(caller.scopes(), ["read"]);
    assert!(caller.groups().is_empty());
    assert_eq!(caller.kind(), CallerKind::Jwt);
}

async fn accepted(auth: &Authenticator, fixtures: &Value, name: &str) {
    identity(authenticate(auth, fixtures, name).await.expect("independently signed caller rejected"), name);
}

async fn denied(auth: &Authenticator, fixtures: &Value, name: &str, expected: AuthError, reason: &str) {
    assert_eq!(authenticate(auth, fixtures, name).await.err(), Some(expected), "{reason}");
}

fn begin(auth: &Authenticator, fixtures: &Value, name: &str) -> JoinHandle<Result<CallerContext, AuthError>> {
    let auth = auth.clone();
    let headers = headers(&token(fixtures, name));
    tokio::spawn(async move { auth.authenticate_async(&headers, None).await })
}

#[derive(Clone, Debug)]
struct Wire {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Wire {
    fn header(&self, name: &str) -> Option<&str> {
        let values: Vec<_> = self.headers.iter().filter(|(key, _)| key == name).collect();
        assert!(values.len() <= 1, "ambiguous repeated response header");
        values.first().map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("complete general-purpose JSON required")
    }
}

fn decode(bytes: &[u8]) -> Wire {
    let offset = bytes.windows(4).position(|part| part == b"\r\n\r\n").unwrap();
    let text = std::str::from_utf8(&bytes[..offset]).unwrap();
    let mut lines = text.split("\r\n");
    let status = lines.next().unwrap();
    assert!(status.starts_with("HTTP/1.1 "));
    let status = status.split_whitespace().nth(1).unwrap().parse().unwrap();
    let headers = lines.map(|line| {
        let (name, value) = line.split_once(':').unwrap();
        (name.to_ascii_lowercase(), value.trim().to_owned())
    }).collect();
    let mut wire = Wire { status, headers, body: bytes[offset + 4..].to_vec() };
    if wire.header("transfer-encoding") == Some("chunked") {
        let mut encoded = wire.body.as_slice();
        let mut body = Vec::new();
        loop {
            let end = encoded.windows(2).position(|part| part == b"\r\n").unwrap();
            let count = usize::from_str_radix(std::str::from_utf8(&encoded[..end]).unwrap(), 16).unwrap();
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
    } else {
        assert_eq!(wire.body.len(), wire.header("content-length").unwrap().parse::<usize>().unwrap());
    }
    wire
}

fn request(address: SocketAddr, path: &str, token: Option<&str>) -> Wire {
    let mut connection = TcpStream::connect_timeout(&address, Duration::from_secs(4)).unwrap();
    connection.set_read_timeout(Some(Duration::from_secs(4))).unwrap();
    connection.set_write_timeout(Some(Duration::from_secs(4))).unwrap();
    let authorization = token.map(|token| format!("Authorization: Bearer {token}\r\n")).unwrap_or_default();
    let message = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{authorization}\r\n");
    connection.write_all(message.as_bytes()).unwrap();
    let mut response = Vec::new();
    connection.take(32_769).read_to_end(&mut response).unwrap();
    assert!(response.len() <= 32_768, "response escaped bound");
    decode(&response)
}

async fn wire_request(address: SocketAddr, path: String, token: Option<String>) -> Wire {
    tokio::task::spawn_blocking(move || request(address, &path, token.as_deref())).await.unwrap()
}

async fn control(fixtures: &Value, command: &str) -> Value {
    let address = fixtures["control"].as_str().unwrap().parse().unwrap();
    let wire = wire_request(address, format!("/{command}"), None).await;
    assert_eq!(wire.status, 200, "issuer control/barrier failed");
    let value = wire.json();
    assert_eq!(value["bad_headers"], json!(false), "bearer credential reached issuer");
    value
}

async fn mode(fixtures: &Value, target: &str, mode: &str) {
    control(fixtures, &format!("set/{target}/{mode}")).await;
}

async fn count(fixtures: &Value, target: &str, expected: u64) {
    let counts = control(fixtures, "counts").await;
    let actual = counts["counts"][target].as_u64().unwrap_or(0);
    assert_eq!(actual, expected, "unexpected configured issuer fetch count for {target}");
}

async fn barrier(fixtures: &Value, target: &str, expected: u64) {
    control(fixtures, &format!("wait/{target}/{expected}")).await;
    count(fixtures, target, expected).await;
}

async fn release(fixtures: &Value, target: &str) {
    control(fixtures, &format!("release/{target}")).await;
}

async fn initial(fixtures: &Value) {
    let ctx = adapter(fixtures, "initial");
    assert_eq!(ctx.auth.authenticate(&headers(&token(fixtures, "a")), None).err(), Some(AuthError::Unavailable));
    count(fixtures, "initial", 0).await;
    accepted(&ctx.auth, fixtures, "a").await;
    accepted(&ctx.auth.clone(), fixtures, "a").await;
    ctx.age.set(Some(109));
    accepted(&ctx.auth, fixtures, "a").await;
    denied(&ctx.auth, fixtures, "bad-signature", AuthError::InvalidToken, "known-key invalid signature accepted").await;
    count(fixtures, "initial", 1).await;
    println!("Key refresh group passed: initial-cache-reuse");
}

async fn rotation(fixtures: &Value) {
    let ctx = adapter(fixtures, "rotation");
    accepted(&ctx.auth, fixtures, "a").await;
    for index in 0..32 {
        denied(&ctx.auth, fixtures, &format!("unknown-{index}"), AuthError::InvalidToken, "unknown key escaped global cooldown").await;
    }
    count(fixtures, "rotation", 1).await;
    ctx.age.set(Some(102));
    mode(fixtures, "rotation", "ab").await;
    accepted(&ctx.auth, fixtures, "b").await;
    accepted(&ctx.auth, fixtures, "a").await;
    count(fixtures, "rotation", 2).await;
    ctx.age.set(Some(104));
    mode(fixtures, "rotation", "b").await;
    denied(&ctx.auth, fixtures, "unknown-0", AuthError::InvalidToken, "absent key accepted after complete replacement").await;
    accepted(&ctx.auth, fixtures, "b").await;
    denied(&ctx.auth, fixtures, "a", AuthError::InvalidToken, "retired A survived A-to-AB-to-B replacement").await;
    count(fixtures, "rotation", 3).await;
    ctx.age.set(Some(106));
    denied(&ctx.auth, fixtures, "a", AuthError::InvalidToken, "retired A accepted after eligible refresh").await;
    for index in 0..32 {
        denied(&ctx.auth, fixtures, &format!("unknown-{index}"), AuthError::InvalidToken, "per-kid cooldown allowed fetch amplification").await;
    }
    count(fixtures, "rotation", 4).await;
    println!("Key refresh group passed: rotation-and-bounded-unknown-keys");
}

async fn expiry(fixtures: &Value) {
    let ctx = adapter(fixtures, "expiry");
    accepted(&ctx.auth, fixtures, "a").await;
    ctx.age.set(Some(108));
    mode(fixtures, "expiry", "outage").await;
    denied(&ctx.auth, fixtures, "b", AuthError::Unavailable, "failed refresh did not fail initiating request").await;
    accepted(&ctx.auth, fixtures, "a").await;
    count(fixtures, "expiry", 2).await;
    ctx.age.set(Some(110));
    // Named target of the compiled stale-key fault: equality is expired, and
    // the failed attempt at 108 did not extend the original deadline of 110.
    denied(&ctx.auth, fixtures, "a", AuthError::Unavailable, "expired trust accepted during issuer outage").await;
    count(fixtures, "expiry", 3).await;
    ctx.age.set(Some(111));
    for name in ["a", "b", "unknown-0"] {
        denied(&ctx.auth, fixtures, name, AuthError::Unavailable, "expired cooldown trusted stale key or refetched").await;
    }
    count(fixtures, "expiry", 3).await;
    ctx.age.set(Some(112));
    mode(fixtures, "expiry", "b").await;
    accepted(&ctx.auth, fixtures, "b").await;
    denied(&ctx.auth, fixtures, "a", AuthError::InvalidToken, "recovery retained retired key").await;
    count(fixtures, "expiry", 4).await;
    println!("Key refresh group passed: expired-trust-outage-and-recovery");
}

async fn hostile(fixtures: &Value) {
    let ctx = adapter(fixtures, "hints");
    let before = control(fixtures, "counts").await;
    for name in ["jku", "x5u", "jwk", "x5c", "bad-profile", "malformed"] {
        denied(&ctx.auth, fixtures, name, AuthError::InvalidToken, "hostile token profile accepted").await;
    }
    assert_eq!(control(fixtures, "counts").await, before, "hostile token hints initiated retrieval");
    count(fixtures, "attacker", 0).await;
    // A bad issuer claim may cause a bounded configured-key retrieval, but
    // cannot choose another destination or become an authenticated identity.
    denied(&ctx.auth, fixtures, "wrong-issuer", AuthError::InvalidToken, "unknown issuer accepted").await;
    count(fixtures, "hints", 1).await;
    count(fixtures, "attacker", 0).await;
    for bad in ["empty", "malformed", "duplicate", "duplicate-member", "private", "mixed-invalid", "oversized", "oversized-chunked"] {
        let target = format!("body-{bad}");
        let ctx = adapter(fixtures, &target);
        accepted(&ctx.auth, fixtures, "a").await;
        ctx.age.set(Some(102));
        mode(fixtures, &target, bad).await;
        denied(&ctx.auth, fixtures, "b", AuthError::Unavailable, "untrusted response replaced cache").await;
        accepted(&ctx.auth, fixtures, "a").await;
        denied(&ctx.auth, fixtures, "b", AuthError::InvalidToken, "partially valid response leaked a key into cache").await;
        count(fixtures, &target, 2).await;
        ctx.age.set(Some(110));
        denied(&ctx.auth, fixtures, "a", AuthError::Unavailable, "rejected response extended trust").await;
        count(fixtures, &target, 3).await;
        ctx.age.set(Some(112));
        mode(fixtures, &target, "b").await;
        accepted(&ctx.auth, fixtures, "b").await;
        count(fixtures, &target, 4).await;
    }
    println!("Key refresh group passed: hostile-hints-and-bounded-responses");
}

async fn transport(fixtures: &Value) {
    let tls_failures = control(fixtures, "counts").await["tls_failures"].as_u64().unwrap();
    let mut untrusted = config(fixtures, "untrusted", 300);
    untrusted.jwks.as_mut().unwrap().trusted_ca_pem = None;
    denied(&adapter_with(untrusted).auth, fixtures, "a", AuthError::Unavailable, "untrusted TLS issuer accepted").await;
    count(fixtures, "untrusted", 0).await;
    let observed = control(fixtures, &format!("wait-tls/{}", tls_failures + 1)).await;
    assert_eq!(observed["tls_failures"], json!(tls_failures + 1), "untrusted certificate did not reach isolated TLS listener");
    let mut wrong_name = config(fixtures, "wrong-name", 300);
    wrong_name.jwks.as_mut().unwrap().url = format!("{}/wrong-name", fixtures["wrong_host"].as_str().unwrap());
    denied(&adapter_with(wrong_name).auth, fixtures, "a", AuthError::Unavailable, "wrong TLS certificate name accepted").await;
    count(fixtures, "wrong-name", 0).await;
    let observed = control(fixtures, &format!("wait-tls/{}", tls_failures + 2)).await;
    assert_eq!(observed["tls_failures"], json!(tls_failures + 2), "wrong-name certificate did not reach isolated TLS listener");
    let mut cleartext = config(fixtures, "cleartext", 300);
    let url = cleartext.jwks.as_ref().unwrap().url.replacen("https:", "http:", 1);
    cleartext.jwks.as_mut().unwrap().url = url;
    assert!(Authenticator::jwt_with_key_clock(cleartext, ControlledClock::new(WALL), ControlledClock::new(100)).is_err(), "cleartext issuer configuration accepted");
    for bad in ["redirect", "wrong-media", "outage", "not-modified"] {
        let target = format!("transport-{bad}");
        mode(fixtures, &target, bad).await;
        denied(&adapter(fixtures, &target).auth, fixtures, "a", AuthError::Unavailable, "failed transport trusted keys").await;
        count(fixtures, &target, 1).await;
    }
    count(fixtures, "attacker", 0).await;
    mode(fixtures, "body-deadline", "hold-body").await;
    let ctx = adapter(fixtures, "body-deadline");
    let pending = begin(&ctx.auth, fixtures, "a");
    barrier(fixtures, "body-deadline", 1).await;
    let result = tokio::time::timeout(Duration::from_secs(1), pending).await.expect("streamed body escaped whole-fetch deadline").unwrap();
    assert_eq!(result.err(), Some(AuthError::Unavailable), "partial streamed body accepted");
    release(fixtures, "body-deadline").await;
    count(fixtures, "body-deadline", 1).await;
    println!("Key refresh group passed: tls-and-http-transport-failures");
}

async fn concurrency(fixtures: &Value) {
    // Barrier cases allow one second for control round trips. Issuer gates
    // remain explicit; wall-clock delays never establish test ordering.
    let ctx = adapter_with(config(fixtures, "busy", 1_000));
    accepted(&ctx.auth, fixtures, "a").await;
    ctx.age.set(Some(102));
    mode(fixtures, "busy", "hold-b").await;
    let pending = begin(&ctx.auth, fixtures, "b");
    barrier(fixtures, "busy", 2).await;
    for _ in 0..8 {
        accepted(&ctx.auth, fixtures, "a").await;
        denied(&ctx.auth, fixtures, "unknown-0", AuthError::InvalidToken, "fresh unknown queued behind refresh").await;
    }
    ctx.age.set(Some(110));
    denied(&ctx.auth, fixtures, "a", AuthError::Unavailable, "expired key usable during in-flight refresh").await;
    count(fixtures, "busy", 2).await;
    release(fixtures, "busy").await;
    identity(pending.await.unwrap().expect("held rotation failed"), "b");
    count(fixtures, "busy", 2).await;

    let ctx = adapter_with(config(fixtures, "cancel", 1_000));
    mode(fixtures, "cancel", "hold-a").await;
    let pending = begin(&ctx.auth, fixtures, "a");
    barrier(fixtures, "cancel", 1).await;
    for _ in 0..8 {
        denied(&ctx.auth, fixtures, "a", AuthError::Unavailable, "empty cache waited for in-flight issuer").await;
    }
    pending.abort();
    assert!(matches!(pending.await, Err(error) if error.is_cancelled()));
    release(fixtures, "cancel").await;
    denied(&ctx.auth, fixtures, "a", AuthError::Unavailable, "aborted refresh lost global cooldown").await;
    count(fixtures, "cancel", 1).await;
    ctx.age.set(Some(102));
    mode(fixtures, "cancel", "a").await;
    accepted(&ctx.auth, fixtures, "a").await;
    count(fixtures, "cancel", 2).await;
    ctx.age.set(None);
    denied(&ctx.auth, fixtures, "a", AuthError::Unavailable, "missing key clock accepted trust").await;
    ctx.age.set(Some(101));
    denied(&ctx.auth, fixtures, "a", AuthError::Unavailable, "backward key clock made trust fresh").await;
    ctx.age.set(Some(102));
    accepted(&ctx.auth, fixtures, "a").await;
    count(fixtures, "cancel", 2).await;

    for scenario in ["token-expiry", "key-expiry", "key-absent", "key-backward", "token-clock-absent"] {
        let ctx = adapter_with(config(fixtures, scenario, 1_000));
        mode(fixtures, scenario, "hold-a").await;
        let pending = begin(&ctx.auth, fixtures, "a");
        barrier(fixtures, scenario, 1).await;
        match scenario {
            "token-expiry" => ctx.wall.set(Some(WALL + 600)),
            "key-expiry" => ctx.age.set(Some(110)),
            "key-absent" => ctx.age.set(None),
            "key-backward" => ctx.age.set(Some(99)),
            "token-clock-absent" => ctx.wall.set(None),
            _ => unreachable!(),
        }
        release(fixtures, scenario).await;
        let expected = if scenario == "token-expiry" { AuthError::InvalidToken } else { AuthError::Unavailable };
        assert_eq!(pending.await.unwrap().err(), Some(expected), "post-await clock not rechecked: {scenario}");
        count(fixtures, scenario, 1).await;
        ctx.wall.set(Some(WALL));
        ctx.age.set(Some(112));
        mode(fixtures, scenario, "a").await;
        accepted(&ctx.auth, fixtures, "a").await;
        count(fixtures, scenario, 2).await;
    }
    println!("Key refresh group passed: concurrency-cancellation-and-clocks");
}

async fn who(Extension(caller): Extension<CallerContext>, State(count): State<Arc<AtomicUsize>>, headers: HeaderMap) -> Result<Response, Problem> {
    assert!(!headers.contains_key(header::AUTHORIZATION), "raw bearer reached protected handler");
    count.fetch_add(1, Ordering::SeqCst);
    json_response(&json!({"issuer":caller.issuer(), "subject":caller.subject(), "client_id":caller.client_id(),
        "scopes":caller.scopes(), "groups":caller.groups(), "kind":if caller.kind() == CallerKind::Jwt {"jwt"} else {"development"}}), "application/json")
}

fn expected_identity(name: &str) -> Value {
    json!({"issuer":"https://issuer.example.test", "subject":if name == "a" {"fixture-alice"} else {"fixture-bob"},
        "client_id":"fixture-client", "scopes":["read"], "groups":[], "kind":"jwt"})
}

fn matches_identity(wire: &Wire, name: &str) -> bool {
    wire.status == 200 && wire.header("content-type") == Some("application/json") && wire.json() == expected_identity(name)
}

fn matches_problem(wire: &Wire, status: u16) -> bool {
    let (slug, title, detail) = match status {
        401 => ("unauthorized", "Unauthorized", "Authentication is required."),
        503 => ("unavailable", "Service Unavailable", "The operation is temporarily unavailable."),
        _ => panic!("unplanned fixture error status"),
    };
    let body = wire.json();
    wire.status == status
        && wire.header("content-type") == Some("application/problem+json")
        && body["status"] == json!(status)
        && body["type"] == json!(format!("urn:glaux:problem:{slug}"))
        && body["title"] == json!(title)
        && body["detail"] == json!(detail)
        && wire.header("x-request-id").is_some_and(|id| !id.is_empty() && body["correlation"].as_str() == Some(id))
}

fn oracle_controls() {
    let correct = Wire { status: 200, headers: vec![("content-type".into(), "application/json".into())], body: serde_json::to_vec(&expected_identity("a")).unwrap() };
    assert!(matches_identity(&correct, "a"));
    for wrong in [json!("attacker"), json!(17), Value::Null] {
        let mut body = correct.json();
        body["subject"] = wrong;
        let mut wire = correct.clone();
        wire.body = serde_json::to_vec(&body).unwrap();
        assert!(!matches_identity(&wire, "a"), "wrong caller escaped independent observation");
    }
    let problem = Wire { status: 503, headers: vec![("content-type".into(), "application/problem+json".into()), ("x-request-id".into(), "fixture-correlation".into())],
        body: br#"{"type":"urn:glaux:problem:unavailable","title":"Service Unavailable","status":503,"detail":"The operation is temporarily unavailable.","correlation":"fixture-correlation"}"#.to_vec() };
    assert!(matches_problem(&problem, 503));
    for wrong in [Some(json!("503")), Some(json!(401)), None] {
        let mut body = problem.json();
        if let Some(value) = wrong { body["status"] = value; } else { body.as_object_mut().unwrap().remove("status"); }
        let mut wire = problem.clone();
        wire.body = serde_json::to_vec(&body).unwrap();
        assert!(!matches_problem(&wire, 503), "mistyped/missing problem status escaped observation");
    }
    let mut wrong = problem;
    wrong.status = 200;
    assert!(!matches_problem(&wrong, 503), "wrong wire status escaped observation");
}

async fn wire_accepted(address: SocketAddr, fixtures: &Value, name: &str, count: &AtomicUsize) {
    let before = count.load(Ordering::SeqCst);
    let wire = wire_request(address, "/who".into(), Some(token(fixtures, name))).await;
    assert!(matches_identity(&wire, name), "wire caller differed from independent expected identity: {wire:?}");
    assert_eq!(wire.header("cache-control"), Some("private, no-store"));
    assert_eq!(count.load(Ordering::SeqCst), before + 1);
}

async fn wire_denied(address: SocketAddr, fixtures: &Value, name: Option<&str>, status: u16, count: &AtomicUsize) {
    let before = count.load(Ordering::SeqCst);
    let credential = name.map(|name| token(fixtures, name));
    let wire = wire_request(address, "/who".into(), credential.clone()).await;
    assert!(matches_problem(&wire, status), "wrong independent denial response: {wire:?}");
    assert_eq!(wire.header("cache-control"), Some("no-store"));
    let challenge = match (status, name) { (503, _) => None, (401, None) => Some("Bearer"), _ => Some("Bearer error=\"invalid_token\"") };
    assert_eq!(wire.header("www-authenticate"), challenge);
    let disclosure = format!("{:?}{}", wire.headers, String::from_utf8_lossy(&wire.body));
    for secret in ["SyntheticIssuerPrivateCanary", "SyntheticPrivateKeyCanary", "fixture-alice", "fixture-bob", "fixture-client", "BEGIN PRIVATE KEY", fixtures["base"].as_str().unwrap()] {
        assert!(!disclosure.contains(secret), "denial disclosed issuer/credential context");
    }
    if let Some(credential) = credential { assert!(!disclosure.contains(&credential), "denial disclosed bearer"); }
    assert_eq!(count.load(Ordering::SeqCst), before, "rejected request reached protected handler");
}

async fn middleware(fixtures: Value) {
    oracle_controls();
    let ctx = adapter_with(config(&fixtures, "middleware", 1_000));
    let executions = Arc::new(AtomicUsize::new(0));
    let routes = Router::new().route("/who", get(who)).with_state(executions.clone());
    let app = HttpBoundary::new(None, Limits::default()).unwrap().router(ctx.auth.clone().protect(routes));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>()).with_graceful_shutdown(async { let _ = stopped.await; }).await.unwrap();
    });
    // A separate task returns assertion panics so listener cleanup still runs.
    let checks = tokio::spawn(async move {
        wire_accepted(address, &fixtures, "a", &executions).await;
        wire_denied(address, &fixtures, None, 401, &executions).await;
        wire_denied(address, &fixtures, Some("unknown-0"), 401, &executions).await;
        ctx.age.set(Some(102));
        mode(&fixtures, "middleware", "hold-b").await;
        let pending = begin(&ctx.auth, &fixtures, "b");
        barrier(&fixtures, "middleware", 2).await;
        wire_accepted(address, &fixtures, "a", &executions).await;
        wire_denied(address, &fixtures, Some("unknown-1"), 401, &executions).await;
        ctx.age.set(Some(110));
        wire_denied(address, &fixtures, Some("a"), 503, &executions).await;
        release(&fixtures, "middleware").await;
        identity(pending.await.unwrap().unwrap(), "b");
        wire_accepted(address, &fixtures, "b", &executions).await;
        ctx.age.set(Some(112));
        mode(&fixtures, "middleware", "outage").await;
        wire_denied(address, &fixtures, Some("b"), 503, &executions).await;
        count(&fixtures, "middleware", 3).await;
        ctx.age.set(Some(114));
        mode(&fixtures, "middleware", "a").await;
        wire_accepted(address, &fixtures, "a", &executions).await;
        wire_denied(address, &fixtures, Some("jku"), 401, &executions).await;
        count(&fixtures, "attacker", 0).await;
        assert_eq!(executions.load(Ordering::SeqCst), 4);
    }).await;
    shutdown.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(3), server).await.expect("owned proof HTTP listener did not stop").unwrap();
    assert!(tokio::net::TcpStream::connect(address).await.is_err(), "owned proof listener survived shutdown");
    checks.expect("middleware assertions failed after listener cleanup");
    println!("Key refresh group passed: middleware-and-clean-shutdown");
}

async fn proof(fixtures: Value) {
    initial(&fixtures).await;
    rotation(&fixtures).await;
    expiry(&fixtures).await;
    hostile(&fixtures).await;
    transport(&fixtures).await;
    concurrency(&fixtures).await;
    middleware(fixtures).await;
    println!("{FINAL}");
}

fn main() {
    let arguments: Vec<_> = std::env::args_os().collect();
    assert_eq!(arguments.len(), 2, "one owned public-fixture file required");
    let bytes = std::fs::read(&arguments[1]).unwrap();
    assert!(bytes.len() <= 131_072, "public fixture bound exceeded");
    let fixtures = serde_json::from_slice(&bytes).unwrap();
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
        tokio::time::timeout(Duration::from_secs(45), proof(fixtures)).await.expect("bounded key-refresh proof timed out");
    });
}
