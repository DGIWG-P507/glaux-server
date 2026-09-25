//! Independently expected CLI and raw HTTP checks in the owned database container.

use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use glaux_domain::identity::LocalId;
use glaux_server::storage::{SystemRecord, SystemRepository};
use sqlx::{Connection, PgConnection};

const BINARY: &str = "/tmp/glaux-runtime-health-server";
const ADMIN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const APP: &str = "postgres://glaux_health_app:SyntheticHealthPasswordCanary@localhost/glaux_harness_test?host=/var/run/postgresql&sslmode=disable";
const ENV: &str = "GLAUX_TEST_DATABASE_URL";
const ADDRESS: &str = "127.0.0.1:18818";
const CANARIES: [&str; 5] = [
    "SyntheticHealthPasswordCanary",
    "SyntheticHealthPayloadCanary",
    "SyntheticHealthPathCanary",
    "SyntheticHealthMissingCanary",
    "SyntheticHealthDatabaseCanary",
];

fn redacted(output: &str) {
    for canary in CANARIES {
        assert!(
            !output.contains(canary),
            "diagnostic exposed synthetic secret"
        );
    }
    assert!(
        !output.contains("postgres://"),
        "diagnostic exposed connection URL"
    );
}

struct Fixture {
    directory: PathBuf,
    serial: usize,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("glaux-health-proof-{}", std::process::id()));
        fs::create_dir(&directory).expect("owned fixture directory must be new");
        Self {
            directory,
            serial: 0,
        }
    }

    fn file(&mut self, content: &[u8]) -> PathBuf {
        self.serial += 1;
        let path = self.directory.join(format!("fixture-{}", self.serial));
        fs::write(&path, content).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.directory).expect("owned health fixture cleanup failed");
    }
}

struct Process {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
    output_pump: Option<std::thread::JoinHandle<()>>,
}

impl Process {
    fn spawn(fixture: &mut Fixture, arguments: &[&str], secret: Option<&str>) -> Self {
        let stdout = fixture.file(b"");
        let stderr = fixture.file(b"");
        let mut command = Command::new(BINARY);
        command
            .args(arguments)
            .env_remove(ENV)
            .env_remove("GLAUX_DATABASE_URL");
        if let Some(secret) = secret {
            command.env(ENV, secret).env("GLAUX_DATABASE_URL", secret);
        }
        let child = command
            .stdin(Stdio::null())
            .stdout(File::create(&stdout).unwrap())
            .stderr(File::create(&stderr).unwrap())
            .spawn()
            .expect("actual server CLI must execute");
        Self {
            child,
            stdout,
            stderr,
            output_pump: None,
        }
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                if let Some(handle) = self.output_pump.take() {
                    handle.join().expect("owned output capture failed");
                }
                redacted(&self.output());
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "bounded CLI exit deadline exceeded"
            );
            // Diagnostic polling only: process exit, not elapsed time, establishes completion.
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn output(&self) -> String {
        fs::read_to_string(&self.stdout).unwrap() + &fs::read_to_string(&self.stderr).unwrap()
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self
            .child
            .try_wait()
            .expect("owned child inspection failed")
            .is_none()
        {
            self.child.kill().expect("owned child cleanup failed");
            self.child.wait().expect("owned child reaping failed");
        }
        if let Some(handle) = self.output_pump.take() {
            handle.join().expect("owned output capture cleanup failed");
        }
    }
}

fn config(listener: &str, authentication: &str, database: &str, timeout: u32) -> String {
    let development = if authentication == "development" {
        r#","development":{"subject":"health-test-caller","groups":[],"scopes":[]}"#
    } else {
        ""
    };
    format!(
        "{{\"listener\":\"{listener}\",\"authentication\":\"{authentication}\",\"database\":{database},\"health_timeout_ms\":{timeout}{development}}}"
    )
}

fn ordinary() -> String {
    config(
        ADDRESS,
        "disabled",
        "{\"url_env\":\"GLAUX_TEST_DATABASE_URL\"}",
        500,
    )
}

fn check(fixture: &mut Fixture, input: &[u8], secret: Option<&str>, accepted: bool) {
    let path = fixture.file(input);
    let mut process = Process::spawn(fixture, &["check-config", path.to_str().unwrap()], secret);
    assert_eq!(
        process.wait().code(),
        Some(if accepted { 0 } else { 2 }),
        "configuration acceptance differs from independent matrix"
    );
    let output = process.output();
    redacted(&output);
    if accepted {
        assert_eq!(output, "Configuration valid; secrets redacted.\n");
    } else {
        assert!(!output.is_empty(), "rejection must have a safe diagnostic");
    }
}

fn configuration(fixture: &mut Fixture) {
    check(fixture, ordinary().as_bytes(), Some(APP), true);
    let base_http = ordinary();
    for (http, accepted) in [
        (r#"{"public_api_root":"https://example.test/prefix"}"#, true),
        (r#"{"public_api_root":"https://user@example.test"}"#, false),
        (
            r#"{"public_api_root":"https://example.test/../escape"}"#,
            false,
        ),
        (
            r#"{"limits":{"body_bytes":0,"header_bytes":2048,"uri_bytes":1024,"timeout_ms":500}}"#,
            false,
        ),
        (
            r#"{"limits":{"body_bytes":256,"header_bytes":2048,"uri_bytes":1024,"timeout_ms":500,"unknown":true}}"#,
            false,
        ),
    ] {
        let input = format!("{},\"http\":{http}}}", &base_http[..base_http.len() - 1]);
        check(fixture, input.as_bytes(), Some(APP), accepted);
    }
    check(
        fixture,
        config(
            "[::1]:18818",
            "development",
            "{\"url_env\":\"GLAUX_TEST_DATABASE_URL\"}",
            100,
        )
        .as_bytes(),
        Some(APP),
        true,
    );
    check(
        fixture,
        config(
            "127.0.0.1:18818",
            "development",
            "{\"url_env\":\"GLAUX_TEST_DATABASE_URL\"}",
            10_000,
        )
        .as_bytes(),
        Some(APP),
        true,
    );
    // Syntactically valid but unusable credentials establish that check-config performs no DB I/O.
    let absent_db = APP.replace("glaux_harness_test", "SyntheticHealthDatabaseCanary");
    check(fixture, ordinary().as_bytes(), Some(&absent_db), true);

    let base = ordinary();
    let development = config(
        ADDRESS,
        "development",
        "{\"url_env\":\"GLAUX_TEST_DATABASE_URL\"}",
        500,
    );
    let jwt_base = base.replace("\"disabled\"", "\"jwt\"");
    let missing_keys = format!(
        "{},\"jwt\":{{\"issuer\":\"https://issuer.example.test\",\"audience\":\"glaux\",\"keys\":[]}}}}",
        &jwt_base[..jwt_base.len() - 1],
    );
    let cases = [
        base.replace("\"listener\"", "\"SyntheticHealthPayloadCanary\""),
        base.replace("\"disabled\"", "\"SyntheticHealthPayloadCanary\""),
        base.replace("\"disabled\"", "\"jwt\""),
        base.replace("\"disabled\"", "\"development\""),
        format!("{},\"jwt\":null}}", &base[..base.len() - 1]),
        format!(
            "{},\"development\":{{\"subject\":\"fake\"}}}}",
            &base[..base.len() - 1]
        ),
        development.replace("health-test-caller", ""),
        development.replace("\"subject\"", "\"unknown\""),
        development.replace(
            "\"subject\":\"health-test-caller\"",
            "\"subject\":\"health-test-caller\",\"subject\":\"second-caller\"",
        ),
        missing_keys,
        base.replace("500", "99"),
        base.replace("500", "10001"),
        base.replace("500", "null"),
        base.replace("500", "\"500\""),
        base.replace("500", "-1"),
        base.replace("500", "500.5"),
        base.replace(",\"health_timeout_ms\":500", ""),
        base.replace(
            "\"url_env\":\"GLAUX_TEST_DATABASE_URL\"",
            "\"url\":\"SyntheticHealthPayloadCanary\"",
        ),
        base.replace(
            "\"url_env\":\"GLAUX_TEST_DATABASE_URL\"",
            "\"url_env\":\"GLAUX_TEST_DATABASE_URL\",\"extra\":\"SyntheticHealthPayloadCanary\"",
        ),
        base.replace("\"url_env\":\"GLAUX_TEST_DATABASE_URL\"", ""),
        base.replace(
            "\"url_env\":\"GLAUX_TEST_DATABASE_URL\"",
            "\"url_env\":\"GLAUX_TEST_DATABASE_URL\",\"url_file\":\"SyntheticHealthPathCanary\"",
        ),
        base.replace("127.0.0.1:18818", "localhost:18818"),
        "{\"SyntheticHealthPayloadCanary\":".to_owned(),
        "null".to_owned(),
        "[]".to_owned(),
    ];
    for input in &cases {
        check(fixture, input.as_bytes(), Some(APP), false);
    }
    for listener in [
        "0.0.0.0:18818",
        "[::]:18818",
        "192.0.2.1:18818",
        "[::ffff:192.0.2.1]:18818",
    ] {
        check(
            fixture,
            config(
                listener,
                "development",
                "{\"url_env\":\"GLAUX_TEST_DATABASE_URL\"}",
                500,
            )
            .as_bytes(),
            Some(APP),
            false,
        );
    }
    check(fixture, &[b' '; 65_537], Some(APP), false);
    check(fixture, &[0xff, 0xfe], Some(APP), false);
    println!("Runtime health group passed: typed-config-and-loopback-matrix");

    check(fixture, base.as_bytes(), None, false);
    check(fixture, base.as_bytes(), Some(""), false);
    check(
        fixture,
        base.as_bytes(),
        Some("SyntheticHealthPasswordCanary"),
        false,
    );
    check(fixture, base.as_bytes(), Some(&"x".repeat(16_385)), false);
    let secret = fixture.file(APP.as_bytes());
    let from_file = config(
        ADDRESS,
        "disabled",
        &format!("{{\"url_file\":\"{}\"}}", secret.display()),
        500,
    );
    check(fixture, from_file.as_bytes(), None, true);
    let missing_file = config(
        ADDRESS,
        "disabled",
        "{\"url_file\":\"/tmp/SyntheticHealthPathCanary\"}",
        500,
    );
    check(fixture, missing_file.as_bytes(), None, false);
    let missing_env = config(
        ADDRESS,
        "disabled",
        "{\"url_env\":\"SyntheticHealthMissingCanary\"}",
        500,
    );
    check(fixture, missing_env.as_bytes(), None, false);
    let large_file = fixture.file(&[b'x'; 16_385]);
    let large_reference = config(
        ADDRESS,
        "disabled",
        &format!("{{\"url_file\":\"{}\"}}", large_file.display()),
        500,
    );
    check(fixture, large_reference.as_bytes(), None, false);
    let mut process = Process::spawn(
        fixture,
        &["check-config", "/tmp/SyntheticHealthPathCanary"],
        Some(APP),
    );
    assert_eq!(
        process.wait().code(),
        Some(2),
        "missing configuration not safely rejected"
    );
    redacted(&process.output());
    println!("Runtime health group passed: secret-references-and-safe-diagnostics");
}

#[derive(Debug)]
struct WireResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn decode(response: &str) -> WireResponse {
    redacted(response);
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("complete HTTP header required");
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
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
    WireResponse {
        status,
        headers,
        body: body.to_owned(),
    }
}

fn matches_health(response: &WireResponse, status: u16, body: &str) -> bool {
    response.status == status
        && response.body == body
        && response
            .headers
            .iter()
            .filter(|(name, _)| name == "cache-control")
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>()
            == ["no-store"]
        && response
            .headers
            .iter()
            .filter(|(name, _)| name == "content-length")
            .map(|(_, value)| value.as_str())
            .collect::<Vec<_>>()
            == [body.len().to_string().as_str()]
}

fn oracle_controls() {
    let valid = "HTTP/1.1 200 OK\r\ncache-control: no-store\r\ncontent-length: 6\r\n\r\nready\n";
    assert!(matches_health(&decode(valid), 200, "ready\n"));
    for wrong in [
        valid.replace("200 OK", "503 Service Unavailable"),
        valid.replace("ready\n", "alive\n"),
        valid.replace("no-store", "public"),
        valid.replace("content-length: 6", "content-length: 5"),
        valid.replace("cache-control: no-store\r\n", ""),
        valid.replace("ready\n", "ready\nextra"),
    ] {
        assert!(
            !matches_health(&decode(&wrong), 200, "ready\n"),
            "independent HTTP oracle accepted known wrong bytes"
        );
    }
    println!("Runtime health group passed: independent-wire-oracle-controls");
}

fn request(path: &str) -> WireResponse {
    let address: SocketAddr = ADDRESS.parse().unwrap();
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.take(8193).read_to_string(&mut response).unwrap();
    assert!(
        response.len() <= 8192,
        "health response exceeded independent bound"
    );
    decode(&response)
}

fn health(path: &str, status: u16, body: &str, message: &str) {
    let response = request(path);
    assert!(
        matches_health(&response, status, body),
        "{message}: {response:?}"
    );
}

fn start(fixture: &mut Fixture, path: &Path) -> Process {
    // A pipe supplies an explicit post-bind signal; elapsed time never proves readiness.
    let stdout = fixture.file(b"");
    let stderr = fixture.file(b"");
    let mut child = Command::new(BINARY)
        .args(["serve", path.to_str().unwrap()])
        .env(ENV, APP)
        .env_remove("GLAUX_DATABASE_URL")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(File::create(&stderr).unwrap())
        .spawn()
        .unwrap();
    let reader = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    let capture = stdout.clone();
    let handle = std::thread::spawn(move || {
        let mut log = File::create(capture).unwrap();
        for line in BufReader::new(reader).lines() {
            let line = line.unwrap();
            writeln!(log, "{line}").unwrap();
            let _ = sender.send(line);
        }
    });
    let mut process = Process {
        child,
        stdout,
        stderr,
        output_pump: Some(handle),
    };
    assert_eq!(
        receiver
            .recv_timeout(Duration::from_secs(8))
            .expect("post-bind listener signal missing"),
        "Health listener ready."
    );
    assert!(
        process.child.try_wait().unwrap().is_none(),
        "listener exited after startup signal"
    );
    process
}

async fn execute(connection: &mut PgConnection, sql: &'static str) {
    sqlx::query(sql).execute(connection).await.unwrap();
}

async fn migrations(connection: &mut PgConnection) -> Vec<(i64, Vec<u8>, bool)> {
    sqlx::query_as("SELECT version,checksum,success FROM public._sqlx_migrations ORDER BY version")
        .fetch_all(connection)
        .await
        .unwrap()
}

async fn sentinel(connection: &mut PgConnection) {
    let found: (String, String, String) = sqlx::query_as(
        "SELECT r.id::text,r.uid,s.label FROM public.resource_identity r JOIN public.system_identity s USING(id) WHERE r.uid='urn:glaux:test:health-sentinel'",
    ).fetch_one(connection).await.unwrap();
    assert_eq!(
        found,
        (
            "01890f20-7b5a-7cc3-98c4-dc0c0c080901".to_owned(),
            "urn:glaux:test:health-sentinel".to_owned(),
            "Sentinel bytes remain unchanged".to_owned(),
        ),
        "startup/health changed retained System"
    );
}

async fn wait_for_probe_lock() {
    let mut observer = PgConnection::connect(ADMIN).await.unwrap();
    execute(&mut observer, "SET statement_timeout=5000").await;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='glaux_health_app' AND wait_event_type='Lock')")
            .fetch_one(&mut observer).await.unwrap();
        if blocked {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "database never observed health probe blocked on our schema lock"
        );
        // Diagnostic polling: the database's actual lock wait establishes the ordering.
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

async fn blocked_probe(connection: &mut PgConnection) {
    execute(connection, "BEGIN").await;
    execute(
        connection,
        "LOCK TABLE public._sqlx_migrations IN ACCESS EXCLUSIVE MODE",
    )
    .await;
    let probe = std::thread::spawn(|| request("/health/ready"));
    wait_for_probe_lock().await;
    health(
        "/health/live",
        200,
        "alive\n",
        "blocked readiness probe prevented liveness response",
    );
    let response = probe.join().expect("bounded readiness client failed");
    assert!(
        matches_health(&response, 503, "not ready\n"),
        "blocked storage probe did not fail readiness within its bound: {response:?}"
    );
    execute(connection, "ROLLBACK").await;
    health(
        "/health/ready",
        200,
        "ready\n",
        "released schema lock did not restore readiness",
    );
}

fn terminate(server: &Process) {
    // The pinned image provides /bin/sh; do not assume a separate kill program.
    // Only the PID of our owned child is passed as a positional argument.
    let signalled = Command::new("/bin/sh")
        .args([
            "-c",
            "kill -TERM \"$1\"",
            "owned-health-child",
            &server.child.id().to_string(),
        ])
        .status()
        .unwrap();
    assert!(signalled.success(), "owned server shutdown signal failed");
}

async fn forced_shutdown(fixture: &mut Fixture, connection: &mut PgConnection) {
    let document = config(
        ADDRESS,
        "disabled",
        "{\"url_env\":\"GLAUX_TEST_DATABASE_URL\"}",
        10_000,
    );
    let path = fixture.file(document.as_bytes());
    let mut server = start(fixture, &path);
    execute(connection, "BEGIN").await;
    execute(
        connection,
        "LOCK TABLE public._sqlx_migrations IN ACCESS EXCLUSIVE MODE",
    )
    .await;
    let address: SocketAddr = ADDRESS.parse().unwrap();
    let mut request = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    request
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    request
        .write_all(b"GET /health/ready HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
        .unwrap();
    wait_for_probe_lock().await;
    let before = Instant::now();
    terminate(&server);
    assert_eq!(
        server.wait().code(),
        Some(1),
        "forced bounded shutdown did not report unfinished drain"
    );
    assert!(
        before.elapsed() < Duration::from_secs(7),
        "shutdown exceeded the documented five-second bound plus runner allowance"
    );
    redacted(&server.output());
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_err(),
        "listener survived forced shutdown"
    );
    execute(connection, "ROLLBACK").await;
    drop(request);
}

async fn proof() {
    let mut fixture = Fixture::new();
    configuration(&mut fixture);
    oracle_controls();
    let mut connection = PgConnection::connect(ADMIN).await.unwrap();
    let target: (String, String, String) =
        sqlx::query_as("SELECT current_database(),current_user::text,host(inet_server_addr())")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        target,
        (
            "glaux_harness_test".to_owned(),
            "postgres".to_owned(),
            "127.0.0.1".to_owned()
        )
    );
    execute(&mut connection, "SET statement_timeout=5000").await;

    execute(&mut connection, "CREATE ROLE glaux_health_app LOGIN").await;
    let base_http = ordinary();
    let http_document = format!(
        "{},\"http\":{{\"public_api_root\":\"https://example.test/prefix\",\"limits\":{{\"body_bytes\":65536,\"header_bytes\":16384,\"uri_bytes\":128,\"timeout_ms\":15000}}}}}}",
        &base_http[..base_http.len() - 1],
    );
    let path = fixture.file(http_document.as_bytes());
    let mut missing = Process::spawn(&mut fixture, &["serve", path.to_str().unwrap()], Some(APP));
    assert_eq!(
        missing.wait().code(),
        Some(1),
        "unmigrated database not safely rejected at startup"
    );
    assert!(!missing.output().contains("Health listener ready."));
    let absent: bool = sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NULL")
        .fetch_one(&mut connection)
        .await
        .unwrap();
    assert!(
        absent,
        "startup automatically migrated uninitialized database"
    );
    // Only the explicit administrative command is permitted to install schema.
    let administrative =
        "postgres://postgres@localhost/glaux_harness_test?host=/var/run/postgresql&sslmode=disable";
    let mut migrate = Process::spawn(&mut fixture, &["migrate"], Some(administrative));
    assert!(
        migrate.wait().success(),
        "explicit migration command failed"
    );
    execute(
        &mut connection,
        "GRANT USAGE ON SCHEMA public TO glaux_health_app",
    )
    .await;
    execute(
        &mut connection,
        "GRANT SELECT ON public._sqlx_migrations TO glaux_health_app",
    )
    .await;
    let record = SystemRecord {
        id: "01890f20-7b5a-7cc3-98c4-dc0c0c080901"
            .parse::<LocalId>()
            .unwrap(),
        uid: "urn:glaux:test:health-sentinel".parse().unwrap(),
        label: "Sentinel bytes remain unchanged".to_owned(),
        sources: vec![],
        parent: None,
    };
    SystemRepository::create(&mut connection, &record)
        .await
        .unwrap();
    let expected_migrations = migrations(&mut connection).await;
    assert_eq!(expected_migrations.len(), 8);

    // A client-supplied disable flag cannot bypass verified TLS on a TCP connection.
    let network = "postgres://glaux_health_app:SyntheticHealthPasswordCanary@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
    let mut insecure = Process::spawn(
        &mut fixture,
        &["serve", path.to_str().unwrap()],
        Some(network),
    );
    assert_eq!(
        insecure.wait().code(),
        Some(1),
        "unverified network database route not safely rejected"
    );
    assert!(!insecure.output().contains("Health listener ready."));
    sentinel(&mut connection).await;
    assert_eq!(migrations(&mut connection).await, expected_migrations);
    println!("Runtime health group passed: explicit-schema-startup-and-verified-network-route");

    let mut server = start(&mut fixture, &path);
    health(
        "/health/live",
        200,
        "alive\n",
        "liveness process response differs",
    );
    health(
        "/health/ready",
        200,
        "ready\n",
        "compatible database not ready",
    );
    for route in ["/", "/systems", "/conformance", "/metrics"] {
        let response = request(route);
        assert_eq!(response.status, 404, "undeclared capability route exposed");
        let problem: serde_json::Value = serde_json::from_str(&response.body).unwrap();
        assert_eq!(
            problem.get("status").and_then(serde_json::Value::as_u64),
            Some(404)
        );
        assert_eq!(
            problem.get("type").and_then(serde_json::Value::as_str),
            Some("urn:glaux:problem:not-found")
        );
        assert!(
            response
                .headers
                .iter()
                .any(|(name, value)| name == "content-type" && value == "application/problem+json")
        );
    }
    assert_eq!(
        request(&format!("/{}", "a".repeat(128))).status,
        414,
        "configured HTTP URI limit not applied to actual binary"
    );
    sentinel(&mut connection).await;
    assert_eq!(migrations(&mut connection).await, expected_migrations);
    println!("Runtime health group passed: actual-listener-minimal-health-only");

    execute(&mut connection, "ALTER ROLE glaux_health_app NOLOGIN").await;
    let terminated: Vec<bool> = sqlx::query_scalar("SELECT pg_terminate_backend(pid,1000) FROM pg_stat_activity WHERE usename='glaux_health_app'")
        .fetch_all(&mut connection).await.unwrap();
    assert!(
        terminated.iter().all(|done| *done),
        "dedicated health connections did not terminate"
    );
    health(
        "/health/live",
        200,
        "alive\n",
        "storage outage incorrectly changed liveness",
    );
    health(
        "/health/ready",
        503,
        "not ready\n",
        "unavailable storage incorrectly reported ready",
    );
    execute(&mut connection, "ALTER ROLE glaux_health_app LOGIN").await;
    health(
        "/health/ready",
        200,
        "ready\n",
        "restored storage did not recover readiness",
    );
    blocked_probe(&mut connection).await;
    sentinel(&mut connection).await;
    assert_eq!(migrations(&mut connection).await, expected_migrations);
    println!("Runtime health group passed: isolated-storage-outage-and-recovery");

    execute(
        &mut connection,
        "UPDATE public._sqlx_migrations SET checksum=decode('00','hex') WHERE version=8",
    )
    .await;
    health(
        "/health/live",
        200,
        "alive\n",
        "schema drift incorrectly changed liveness",
    );
    health(
        "/health/ready",
        503,
        "not ready\n",
        "incompatible schema incorrectly reported ready",
    );
    let checksum: Vec<u8> =
        sqlx::query_scalar("SELECT checksum FROM public._sqlx_migrations WHERE version=8")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        checksum,
        [0],
        "health silently repaired incompatible schema"
    );
    let incompatible_config = config(
        "127.0.0.1:18819",
        "disabled",
        "{\"url_env\":\"GLAUX_TEST_DATABASE_URL\"}",
        500,
    );
    let incompatible_path = fixture.file(incompatible_config.as_bytes());
    let mut incompatible = Process::spawn(
        &mut fixture,
        &["serve", incompatible_path.to_str().unwrap()],
        Some(APP),
    );
    assert_eq!(
        incompatible.wait().code(),
        Some(1),
        "incompatible schema not safely rejected at startup"
    );
    assert!(!incompatible.output().contains("Health listener ready."));
    sqlx::query("UPDATE public._sqlx_migrations SET checksum=$1 WHERE version=8")
        .bind(&expected_migrations[7].1)
        .execute(&mut connection)
        .await
        .unwrap();
    health(
        "/health/ready",
        200,
        "ready\n",
        "restored schema did not recover readiness",
    );
    sentinel(&mut connection).await;
    assert_eq!(migrations(&mut connection).await, expected_migrations);
    println!("Runtime health group passed: schema-drift-rejection-without-repair");

    terminate(&server);
    assert!(
        server.wait().success(),
        "graceful health listener shutdown failed"
    );
    redacted(&server.output());
    let address: SocketAddr = ADDRESS.parse().unwrap();
    assert!(
        TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_err(),
        "listener survived shutdown"
    );
    forced_shutdown(&mut fixture, &mut connection).await;
    sentinel(&mut connection).await;
    assert_eq!(migrations(&mut connection).await, expected_migrations);
    println!("Runtime health group passed: bounded-shutdown-and-sentinel-preservation");
    println!("Required runtime health proof passed: 8 groups.");
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
            tokio::time::timeout(Duration::from_secs(90), proof())
                .await
                .expect("bounded runtime health proof timed out");
        });
}
