//! Independent raw-HTTP evidence for the shared boundary, not CSAPI handlers.
use axum::{
    Router,
    extract::{Request, State},
    http::HeaderMap,
    response::Response,
    routing::{get, post},
};
use glaux_server::http_boundary::{HttpBoundary, Limits, Problem, json_response, negotiate};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;

const ROOT: &str = "https://example.test/prefix/api";
const FINAL: &str = "Required HTTP boundary proof passed: 7 groups.";
const CANARY: &str = "SyntheticPrivateSchemaCanary";

fn limits() -> Limits {
    Limits {
        body_bytes: 256,
        header_bytes: 2048,
        uri_bytes: 1024,
        timeout_ms: 500,
    }
}

async fn representation(headers: HeaderMap) -> Result<Response, Problem> {
    let offered = ["application/json", "application/geo+json"];
    let selected = negotiate(&headers, &offered)?;
    json_response(&json!({"fixture":"wire", "value":17}), offered[selected])
}

async fn input(
    State(boundary): State<HttpBoundary>,
    request: Request,
) -> Result<Response, Problem> {
    let value = boundary.read_json(request).await?;
    json_response(&value, "application/json")
}

async fn link(State(boundary): State<HttpBoundary>) -> Result<Response, Problem> {
    let href = boundary.link(
        &["systems", "a/b ?#é"],
        &[("obsFormat", "application/swe+json"), ("name", "x y&z=é")],
    )?;
    json_response(&json!({"href":href}), "application/json")
}

async fn invalid_link(State(boundary): State<HttpBoundary>) -> Result<Response, Problem> {
    let href = boundary.link(&[".."], &[])?;
    json_response(&json!({"href":href}), "application/json")
}

async fn private_failure() -> Result<Response, Problem> {
    Err(Problem::internal())
}

async fn never_finishes() -> &'static str {
    std::future::pending::<()>().await;
    "unreachable"
}

fn fixture(root: Option<&str>) -> Router {
    let boundary = HttpBoundary::new(root, limits()).unwrap();
    let routes = Router::new()
        .route("/representation", get(representation))
        .route("/json", post(input))
        .route("/link", get(link))
        .route("/invalid-link", get(invalid_link))
        .route("/private", get(private_failure))
        .route("/slow", get(never_finishes))
        .with_state(boundary.clone());
    boundary.router(routes)
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
        assert!(values.len() <= 1, "unexpected repeated {name}: {self:?}");
        values.first().map(|(_, value)| value.as_str())
    }

    fn json(&self) -> Value {
        // This is an ordinary JSON value, deliberately never a production wire type.
        serde_json::from_slice(&self.body).expect("response must contain complete JSON")
    }
}

fn decode(bytes: &[u8], head: bool) -> Wire {
    let offset = bytes
        .windows(4)
        .position(|value| value == b"\r\n\r\n")
        .expect("complete HTTP response headers required");
    let text = std::str::from_utf8(&bytes[..offset]).unwrap();
    let mut lines = text.split("\r\n");
    let status_line = lines.next().unwrap();
    assert!(
        status_line.starts_with("HTTP/1.1 "),
        "expected actual HTTP/1.1 response"
    );
    let status = status_line
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<_> = lines
        .map(|line| {
            let (name, value) = line.split_once(':').expect("header colon required");
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let mut wire = Wire {
        status,
        headers,
        body: bytes[offset + 4..].to_vec(),
    };
    if head {
        assert!(
            wire.body.is_empty(),
            "HEAD emitted a response body: {wire:?}"
        );
    } else if wire.header("transfer-encoding") == Some("chunked") {
        let mut encoded = wire.body.as_slice();
        let mut body = Vec::new();
        loop {
            let end = encoded.windows(2).position(|v| v == b"\r\n").unwrap();
            let count =
                usize::from_str_radix(std::str::from_utf8(&encoded[..end]).unwrap(), 16).unwrap();
            encoded = &encoded[end + 2..];
            if count == 0 {
                assert_eq!(
                    encoded, b"\r\n",
                    "unexpected chunk trailers or extra response"
                );
                break;
            }
            assert!(encoded.len() >= count + 2, "truncated chunk");
            body.extend_from_slice(&encoded[..count]);
            assert_eq!(&encoded[count..count + 2], b"\r\n");
            encoded = &encoded[count + 2..];
        }
        wire.body = body;
    } else if let Some(length) = wire.header("content-length") {
        assert_eq!(
            wire.body.len(),
            length.parse::<usize>().unwrap(),
            "wire body length differs"
        );
    }
    wire
}

fn exchange(address: SocketAddr, request: &str, head: bool) -> Wire {
    let mut connection = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    connection
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    connection
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    connection.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    connection.take(16_385).read_to_end(&mut response).unwrap();
    assert!(
        response.len() <= 16_384,
        "independent response bound exceeded"
    );
    decode(&response, head)
}

fn request(address: SocketAddr, method: &str, path: &str, headers: &str, body: &str) -> Wire {
    exchange(
        address,
        &format!(
            "{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{headers}Content-Length: {}\r\n\r\n{body}",
            body.len()
        ),
        method == "HEAD",
    )
}

fn contract(status: u16) -> (&'static str, &'static str, &'static str) {
    match status {
        400 => ("Bad Request", "bad-request", "The request is malformed."),
        404 => (
            "Not Found",
            "not-found",
            "The requested resource is unavailable.",
        ),
        405 => (
            "Method Not Allowed",
            "method-not-allowed",
            "The method is unavailable on this route.",
        ),
        406 => (
            "Not Acceptable",
            "not-acceptable",
            "No offered representation is acceptable.",
        ),
        408 => (
            "Request Timeout",
            "request-timeout",
            "The request body did not complete within its limit.",
        ),
        413 => (
            "Content Too Large",
            "payload-too-large",
            "The request body exceeds its limit.",
        ),
        414 => (
            "URI Too Long",
            "uri-too-long",
            "The request target exceeds its limit.",
        ),
        415 => (
            "Unsupported Media Type",
            "unsupported-media-type",
            "The request media type or coding is unsupported.",
        ),
        431 => (
            "Request Header Fields Too Large",
            "headers-too-large",
            "The request headers exceed their limit.",
        ),
        500 => (
            "Internal Server Error",
            "internal",
            "The operation could not be completed.",
        ),
        503 => (
            "Service Unavailable",
            "unavailable",
            "The operation is temporarily unavailable.",
        ),
        _ => panic!("fixture contract missing"),
    }
}

fn matches_problem(wire: &Wire, status: u16) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(&wire.body) else {
        return false;
    };
    let (title, slug, detail) = contract(status);
    wire.status == status
        && wire.header("content-type") == Some("application/problem+json")
        && wire.header("cache-control") == Some("no-store")
        && value.get("status").and_then(Value::as_u64) == Some(u64::from(status))
        && value.get("type").and_then(Value::as_str)
            == Some(format!("urn:glaux:problem:{slug}").as_str())
        && value.get("title").and_then(Value::as_str) == Some(title)
        && value.get("detail").and_then(Value::as_str) == Some(detail)
        && value
            .get("correlation")
            .and_then(Value::as_str)
            .is_some_and(|correlation| {
                !correlation.is_empty()
                    && correlation != CANARY
                    && wire.header("x-request-id") == Some(correlation)
            })
}

fn problem(wire: &Wire, status: u16) {
    assert!(
        matches_problem(wire, status),
        "safe problem wire contract differs: expected {status}, got {wire:?}"
    );
    let text = String::from_utf8_lossy(&wire.body);
    for forbidden in [
        CANARY,
        "SELECT password",
        "postgres://",
        "schemaPath",
        "stack trace",
    ] {
        assert!(
            !text.contains(forbidden),
            "problem leaked protected details: {wire:?}"
        );
    }
}

fn oracle_controls() {
    let mut correct = Wire {
        status: 400,
        headers: vec![
            ("content-type".to_owned(), "application/problem+json".to_owned()),
            ("cache-control".to_owned(), "no-store".to_owned()),
            ("x-request-id".to_owned(), "independent-fixture-correlation".to_owned()),
        ],
        body: br#"{"type":"urn:glaux:problem:bad-request","title":"Bad Request","status":400,"detail":"The request is malformed.","correlation":"independent-fixture-correlation"}"#.to_vec(),
    };
    assert!(matches_problem(&correct, 400));
    let value = correct.json();
    let mut missing = value.clone();
    missing.as_object_mut().unwrap().remove("status");
    correct.body = serde_json::to_vec(&missing).unwrap();
    assert!(
        !matches_problem(&correct, 400),
        "missing required wire field passed"
    );
    let mut wrong_type = value.clone();
    wrong_type["status"] = json!("400");
    correct.body = serde_json::to_vec(&wrong_type).unwrap();
    assert!(
        !matches_problem(&correct, 400),
        "mistyped required wire field passed"
    );
    let mut wrong_value = value.clone();
    wrong_value["status"] = json!(500);
    correct.body = serde_json::to_vec(&wrong_value).unwrap();
    assert!(
        !matches_problem(&correct, 400),
        "wrong required wire value passed"
    );
    let mut extension = value;
    extension["permittedExtension"] = json!({"unfamiliar":true});
    correct.body = serde_json::to_vec(&extension).unwrap();
    assert!(
        matches_problem(&correct, 400),
        "permitted extension incorrectly rejected"
    );
    println!("HTTP boundary group passed: independent-wire-oracle-controls");
}

fn selected(wire: &Wire, media: &str) {
    assert_eq!(
        wire.status, 200,
        "negotiation did not select acceptable representation: {wire:?}"
    );
    assert_eq!(wire.header("content-type"), Some(media));
    assert_eq!(wire.header("vary"), Some("Accept"));
    assert_eq!(wire.header("cache-control"), Some("no-store"));
    assert_eq!(wire.json(), json!({"fixture":"wire", "value":17}));
}

fn media(address: SocketAddr) {
    for (headers, expected) in [
        ("", "application/json"),
        ("Accept: */*\r\n", "application/json"),
        ("Accept: application/*\r\n", "application/json"),
        ("Accept: APPLICATION/JSON\r\n", "application/json"),
        (
            "Accept: application/json;q=0.2, application/geo+json;q=0.9\r\n",
            "application/geo+json",
        ),
        (
            "Accept: application/json;q=0, */*;q=1\r\n",
            "application/geo+json",
        ),
        (
            "Accept: application/json;q=0.1, application/*;q=0.9\r\n",
            "application/geo+json",
        ),
        (
            "Accept: application/json;q=0\r\nAccept: application/geo+json;q=1\r\n",
            "application/geo+json",
        ),
        (
            "Accept: application/json;profile=\"x,y\";q=1, application/geo+json;q=0.5\r\n",
            "application/geo+json",
        ),
        (
            "Accept: application/vnd.ogc.sml+json, application/json;q=0.1\r\n",
            "application/json",
        ),
    ] {
        selected(
            &request(address, "GET", "/representation", headers, ""),
            expected,
        );
    }
    for accept in [
        "text/xml",
        "application/json;q=0, application/geo+json;q=0, */*;q=1",
        "application/vnd.ogc.sml+json",
    ] {
        problem(
            &request(
                address,
                "GET",
                "/representation",
                &format!("Accept: {accept}\r\n"),
                "",
            ),
            406,
        );
    }
    for accept in [
        "application/json;q=1.001",
        "application/json;q=-1",
        "application/json;q=0.0000",
        "application/json;q=wat",
        "application/json;q=1;q=0",
        "application/json;profile=\"unterminated",
    ] {
        problem(
            &request(
                address,
                "GET",
                "/representation",
                &format!("Accept: {accept}\r\n"),
                "",
            ),
            400,
        );
    }
    let value = r#"{"value":17,"permittedExtension":{"unfamiliar":true}}"#;
    for media in [
        "application/json",
        "APPLICATION/JSON",
        "application/json;charset=utf-8",
        "application/json;profile=\"not-a-schema-selector\"",
    ] {
        let wire = request(
            address,
            "POST",
            "/json",
            &format!("Content-Type: {media}\r\n"),
            value,
        );
        assert_eq!(
            wire.status, 200,
            "valid JSON media unexpectedly rejected: {wire:?}"
        );
        assert_eq!(
            wire.json(),
            json!({"value":17,"permittedExtension":{"unfamiliar":true}})
        );
    }
    for headers in [
        "",
        "Content-Type: text/plain\r\n",
        "Content-Type: application/sml+json\r\n",
        "Content-Type: application/json\r\nContent-Encoding: gzip\r\n",
    ] {
        let wire = request(address, "POST", "/json", headers, value);
        problem(&wire, 415);
        if headers.contains("gzip") {
            assert_eq!(wire.header("accept-encoding"), Some("identity"));
        }
    }
    for malformed in ["", "{", "{\"x\":}", "{} trailing", "[1,]"] {
        problem(
            &request(
                address,
                "POST",
                "/json",
                "Content-Type: application/json\r\n",
                malformed,
            ),
            400,
        );
    }
    println!("HTTP boundary group passed: media-and-json-contracts");
}

fn problems(address: SocketAddr) {
    let missing = request(
        address,
        "GET",
        "/SyntheticPrivateSchemaCanary",
        "Accept: text/xml\r\nX-Request-Id: SyntheticPrivateSchemaCanary\r\n",
        "",
    );
    problem(&missing, 404);
    let failed = request(
        address,
        "GET",
        "/private",
        "X-Request-Id: SyntheticPrivateSchemaCanary\r\n",
        "",
    );
    problem(&failed, 500);
    assert_ne!(
        missing.header("x-request-id"),
        failed.header("x-request-id"),
        "correlation reused across requests"
    );
    let method = request(address, "POST", "/representation", "", "");
    problem(&method, 405);
    let mut allowed: Vec<_> = method
        .header("allow")
        .expect("405 Allow required")
        .split(',')
        .map(str::trim)
        .collect();
    allowed.sort_unstable();
    assert_eq!(allowed, ["GET", "HEAD"]);
    let head = request(address, "HEAD", "/representation", "", "");
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-type"), Some("application/json"));
    let missing_head = request(address, "HEAD", "/absent", "", "");
    assert_eq!(missing_head.status, 404);
    assert_eq!(
        missing_head.header("content-type"),
        Some("application/problem+json")
    );
    assert!(missing_head.header("x-request-id").is_some());
    println!("HTTP boundary group passed: safe-problems-methods-and-head");
}

fn bounds(address: SocketAddr) {
    let exact = format!("{{}}{}", " ".repeat(254));
    let success = request(
        address,
        "POST",
        "/json",
        "Content-Type: application/json\r\n",
        &exact,
    );
    assert_eq!(success.status, 200, "exact body limit rejected");
    assert_eq!(success.json(), json!({}));
    problem(
        &request(
            address,
            "POST",
            "/json",
            "Content-Type: application/json\r\n",
            &format!("{exact} "),
        ),
        413,
    );
    let chunked = format!(
        "POST /json HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\n80\r\n{}\r\n81\r\n{}\r\n0\r\n\r\n",
        " ".repeat(128),
        " ".repeat(129)
    );
    problem(&exchange(address, &chunked, false), 413);
    problem(
        &request(
            address,
            "GET",
            "/representation",
            &format!("X-Pad: {}\r\n", "x".repeat(2500)),
            "",
        ),
        431,
    );
    problem(
        &request(address, "GET", &format!("/{}", "a".repeat(1024)), "", ""),
        414,
    );
    for path in ["/%", "/%G0", "/%00", "/%0a", "/%0D", "/%7f", "/?x=%00"] {
        problem(&request(address, "GET", path, "", ""), 400);
    }
    let started = Instant::now();
    // Intentionally incomplete framed body: no sleep is used to establish ordering.
    let stalled = exchange(
        address,
        "POST /json HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: 16\r\n\r\n{",
        false,
    );
    problem(&stalled, 408);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "body collection escaped its limit"
    );
    let started = Instant::now();
    problem(&request(address, "GET", "/slow", "", ""), 503);
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "handler escaped its deadline"
    );
    println!("HTTP boundary group passed: bounded-bodies-headers-paths-and-timeouts");
}

fn links(address: SocketAddr) {
    let expected = "https://example.test/prefix/api/systems/a%2Fb%20%3F%23%C3%A9?obsFormat=application%2Fswe%2Bjson&name=x%20y%26z%3D%C3%A9";
    for headers in [
        "",
        "Forwarded: for=192.0.2.1;host=attacker.invalid;proto=http\r\n",
        "X-Forwarded-Host: attacker.invalid\r\nX-Forwarded-Proto: http\r\nX-Forwarded-Prefix: /escape\r\n",
        "Forwarded: host=attacker.invalid\r\nForwarded: host=other.invalid\r\nX-Original-URL: //attacker.invalid/\r\n",
    ] {
        let wire = request(address, "GET", "/link", headers, "");
        assert_eq!(wire.status, 200);
        assert_eq!(
            wire.json().get("href").and_then(Value::as_str),
            Some(expected),
            "configured origin changed or escaping lost"
        );
    }
    let host = exchange(
        address,
        "GET /link HTTP/1.1\r\nHost: attacker.invalid\r\nConnection: close\r\n\r\n",
        false,
    );
    assert_eq!(
        host.json().get("href").and_then(Value::as_str),
        Some(expected),
        "configured origin changed or escaping lost"
    );
    problem(&request(address, "GET", "/invalid-link", "", ""), 500);
    println!("HTTP boundary group passed: configured-origin-link-isolation");
}

fn generated(address: SocketAddr) {
    // Reproducible bounded parser target, seed 0x19_01; semantic outcomes, not just no panic.
    let mut state = 0x1901_u32;
    let mut partitions = [0; 4];
    for index in 0..128 {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let partition = (state >> 16) as usize % 4;
        partitions[partition] += 1;
        let accept = match partition {
            0 => format!(
                "application/json;q=0.{:03}, application/geo+json;q=1",
                index % 1000
            ),
            1 => format!(
                "application/json;profile=\"seed-{index},x\";q=1, application/geo+json;q=0.5"
            ),
            2 => format!("application/json;q=0, application/geo+json;q=0, text/x-case-{index}"),
            _ => format!("application/json;q=invalid{index}"),
        };
        let wire = request(
            address,
            "GET",
            "/representation",
            &format!("Accept: {accept}\r\n"),
            "",
        );
        match partition {
            0 | 1 => selected(&wire, "application/geo+json"),
            2 => problem(&wire, 406),
            _ => problem(&wire, 400),
        }
    }
    assert!(
        partitions.iter().all(|count| *count > 0),
        "generator missed a semantic partition"
    );
    for index in 0..32 {
        let path = format!("/absent-{index}/safe%20segment?probe=escape%2F{index}%3F");
        problem(
            &request(
                address,
                "GET",
                &path,
                "Forwarded: host=attacker.invalid\r\n",
                "",
            ),
            404,
        );
        let bad = format!("/absent-{index}/%{:02X}", index % 32);
        problem(&request(address, "GET", &bad, "", ""), 400);
    }
    println!("HTTP boundary group passed: generated-accept-and-path-cases");
}

async fn listener(router: Router, checks: impl FnOnce(SocketAddr) + Send + 'static) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    assert!(address.ip().is_loopback());
    let (stop_tx, stop_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
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
    client
        .join()
        .expect("independent client thread failed outside assertions");
    stop_tx
        .send(())
        .expect("owned listener stopped before cleanup");
    tokio::time::timeout(Duration::from_secs(3), server)
        .await
        .expect("owned listener cleanup timed out")
        .unwrap();
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_millis(100)).is_err(),
        "owned listener survived cleanup"
    );
    if let Err(error) = outcome {
        resume_unwind(error);
    }
}

async fn proof() {
    oracle_controls();
    listener(fixture(Some(ROOT)), |address| {
        media(address);
        problems(address);
        bounds(address);
        links(address);
        generated(address);
        for path in ["/", "/systems", "/conformance", "/collections", "/metrics"] {
            problem(&request(address, "GET", path, "", ""), 404);
        }
    })
    .await;
    listener(fixture(None), |address| {
        problem(
            &request(
                address,
                "GET",
                "/link",
                "Host-Ignored: attacker.invalid\r\n",
                "",
            ),
            500,
        );
    })
    .await;
    println!("HTTP boundary group passed: no-extra-routes-and-clean-shutdown");
    println!("{FINAL}");
}

fn main() {
    assert_eq!(
        std::env::args().count(),
        1,
        "No target or selection override is accepted"
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(30), proof())
                .await
                .expect("bounded HTTP boundary proof timed out");
        });
}
