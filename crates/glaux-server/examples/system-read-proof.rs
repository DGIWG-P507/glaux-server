//! Independent raw HTTP and SQL proof of canonical System retrieval (#25).
//!
//! Every expected answer here is written from the documented minimal creation
//! and retrieval contracts, never copied from a server response.
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BINARY: &str = "/tmp/glaux-system-read-server";
const ADMIN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const APP: &str = "postgres://glaux_read_app:SyntheticReadSecret@localhost/glaux_harness_test?host=/var/run/postgresql&sslmode=disable";
const ENV: &str = "GLAUX_TEST_DATABASE_URL";
const ADDRESS: &str = "127.0.0.1:18826";
const PUBLIC: &str = "https://api.example.invalid/edge";
const SOURCE_A: &str = "urn:glaux:test:source-a";
const SOURCE_B: &str = "urn:glaux:test:source-b";
const GEOJSON: &str = "application/geo+json";
const SUPPLIED_ID: &str = "01890f20-7b5a-7cc3-98c4-dc0c0c220999";
const MISSING_ID: &str = "0190f5c2-7b5a-7cc3-98c4-dc0c0c22ffff";

fn passed(name: &str) {
    println!("System read group passed: {name}");
}
fn safe(text: &str) {
    for secret in ["SyntheticReadSecret", "postgres://"] {
        assert!(
            !text.contains(secret),
            "private fixture detail reached response/output"
        );
    }
}
struct Fixture {
    directory: PathBuf,
    serial: usize,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("glaux-system-read-proof-{}", std::process::id()));
        fs::create_dir(&directory).expect("owned fixture directory must be new");
        Self {
            directory,
            serial: 0,
        }
    }

    fn file(&mut self, bytes: &[u8]) -> PathBuf {
        self.serial += 1;
        let path = self.directory.join(format!("fixture-{}", self.serial));
        fs::write(&path, bytes).unwrap();
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
        fs::remove_dir_all(&self.directory).expect("owned system-read fixture cleanup failed");
    }
}

struct Process {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
    pump: Option<std::thread::JoinHandle<()>>,
}

impl Process {
    fn spawn(fixture: &mut Fixture, arguments: &[&str], database: &str, serving: bool) -> Self {
        let stdout = fixture.file(b"");
        let stderr = fixture.file(b"");
        let mut command = Command::new(BINARY);
        command
            .args(arguments)
            .env(ENV, database)
            .env("GLAUX_DATABASE_URL", database)
            .stdin(Stdio::null())
            .stderr(File::create(&stderr).unwrap());
        if serving {
            command.stdout(Stdio::piped());
        } else {
            command.stdout(File::create(&stdout).unwrap());
        }
        let child = command.spawn().expect("actual server binary must execute");
        let mut process = Self {
            child,
            stdout,
            stderr,
            pump: None,
        };
        if serving {
            let reader = process.child.stdout.take().unwrap();
            let capture = process.stdout.clone();
            let (sender, receiver) = mpsc::channel();
            process.pump = Some(std::thread::spawn(move || {
                let mut output = File::create(capture).unwrap();
                for line in BufReader::new(reader).lines() {
                    let line = line.unwrap();
                    writeln!(output, "{line}").unwrap();
                    let _ = sender.send(line);
                }
            }));
            // Explicit post-bind readiness signal with a bounded wait.
            assert_eq!(
                receiver
                    .recv_timeout(Duration::from_secs(8))
                    .expect("post-bind listener signal absent"),
                "Health listener ready."
            );
            assert!(process.child.try_wait().unwrap().is_none());
        }
        process
    }

    fn wait(&mut self) -> ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(8);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                if let Some(pump) = self.pump.take() {
                    pump.join().expect("output capture failed");
                }
                safe(&self.output());
                return status;
            }
            assert!(Instant::now() < deadline, "bounded child exit exceeded");
            // Only observed process exit establishes completion.
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn output(&self) -> String {
        fs::read_to_string(&self.stdout).unwrap() + &fs::read_to_string(&self.stderr).unwrap()
    }

    fn stop(&mut self) {
        assert!(
            Command::new("/bin/sh")
                .args([
                    "-c",
                    "kill -TERM \"$1\"",
                    "owned-system-read-child",
                    &self.child.id().to_string()
                ])
                .status()
                .unwrap()
                .success(),
            "owned shutdown signal failed"
        );
        assert!(
            self.wait().success(),
            "graceful system-read shutdown failed"
        );
        assert!(
            TcpStream::connect_timeout(&ADDRESS.parse().unwrap(), Duration::from_millis(200))
                .is_err(),
            "owned system-read listener survived shutdown"
        );
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
        if let Some(pump) = self.pump.take() {
            pump.join().expect("output capture cleanup failed");
        }
    }
}

fn configuration(subject: &str, group: &str, source: &str) -> Value {
    json!({
        "listener": ADDRESS, "database":{"url_env":ENV}, "health_timeout_ms":500,
        "authentication":"development",
        "development":{"subject":subject,"groups":[group]},
        "http":{"public_api_root":PUBLIC},"discovery":true,
        "policy":{"grants":[
            {"issuer":"urn:glaux:development","group":"group-a","source":SOURCE_A,"actions":["create","read"]},
            {"issuer":"urn:glaux:development","group":"group-b","source":SOURCE_B,"actions":["create","read"]}
        ],"denial_audit":{"max_records":100,"max_per_window":100,"window_seconds":10}},
        "system_creation":{"source":source,"retry_retention_seconds":3600}
    })
}
fn start(fixture: &mut Fixture, config: &Value) -> Process {
    let path = fixture.file(&serde_json::to_vec(config).unwrap());
    Process::spawn(fixture, &["serve", path.to_str().unwrap()], APP, true)
}
#[derive(Debug, Clone)]
struct Wire {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}
impl Wire {
    fn header(&self, name: &str) -> Option<&str> {
        let found: Vec<_> = self.headers.iter().filter(|(key, _)| key == name).collect();
        assert!(found.len() <= 1, "duplicate response metadata");
        found.first().map(|(_, value)| value.as_str())
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("general JSON parser rejected response")
    }
    fn text(&self) -> String {
        String::from_utf8(self.body.clone()).expect("response body is not UTF-8")
    }
}
/// A separate raw HTTP/1.1 client: no server code, framework or JSON schema.
fn request(method: &str, path: &str, headers: &str, body: &str) -> Wire {
    let mut socket =
        TcpStream::connect_timeout(&ADDRESS.parse().unwrap(), Duration::from_secs(2)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(socket,"{method} {path} HTTP/1.1\r\nHost: attacker.invalid\r\nConnection: close\r\n{headers}Content-Length: {}\r\n\r\n{body}",body.len()).unwrap();
    let mut bytes = Vec::new();
    socket.take(262_145).read_to_end(&mut bytes).unwrap();
    assert!(bytes.len() <= 262_144, "response exceeds independent bound");
    let split = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
    let mut lines = std::str::from_utf8(&bytes[..split]).unwrap().split("\r\n");
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
            let (key, value) = line.split_once(':').unwrap();
            (key.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let wire = Wire {
        status,
        headers,
        body: bytes[split + 4..].to_vec(),
    };
    assert!(
        wire.header("transfer-encoding").is_none(),
        "fixed response unexpectedly chunked"
    );
    // HEAD advertises the GET length without sending the entity.
    if let Some(length) = wire.header("content-length").filter(|_| method != "HEAD") {
        assert_eq!(wire.body.len(), length.parse::<usize>().unwrap());
    }
    safe(&wire.text());
    wire
}
fn get(path: &str, headers: &str) -> Wire {
    request("GET", path, headers, "")
}
/// Links are data; only targets under the configured public root are followed.
fn local(href: &str) -> String {
    href.strip_prefix(PUBLIC)
        .expect("link escaped the configured public root")
        .to_owned()
}
fn uuid_v7(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
        && value.as_bytes()[14] == b'7'
        && b"89ab".contains(&value.as_bytes()[19])
}
fn body(uid: &str, name: &str, tag: &str) -> String {
    serde_json::to_string(&json!({"type":"Feature","geometry":null,"properties":{"uid":uid,"name":name,"featureType":tag}})).unwrap()
}
fn creation_id(wire: &Wire) -> Option<String> {
    if wire.status != 201
        || !wire.body.is_empty()
        || wire.header("cache-control") != Some("private, no-store")
    {
        return None;
    }
    let id = wire
        .header("location")?
        .strip_prefix(&format!("{PUBLIC}/systems/"))?;
    uuid_v7(id).then(|| id.to_owned())
}
fn create(collection: &str, submitted: &str) -> (String, String) {
    let wire = request(
        "POST",
        collection,
        "Content-Type: application/geo+json\r\n",
        submitted,
    );
    let id = creation_id(&wire).expect("public write path did not create the fixture System");
    (id, local(wire.header("location").unwrap()))
}
/// The documented minimal representation, from the submitted fields and the
/// generated identity only: no client id or link, and one canonical self link.
fn expected(id: &str, uid: &str, name: &str, tag: &str) -> Value {
    json!({
        "type":"Feature", "id":id, "geometry":null,
        "properties":{"uid":uid, "name":name, "featureType":tag},
        "links":[{"href":format!("{PUBLIC}/systems/{id}"), "rel":"self", "type":GEOJSON, "title":"This System"}]
    })
}
/// Independent wire oracle for one retrieved representation.
fn representation_error(wire: &Wire, expected: &Value) -> Option<&'static str> {
    if wire.status != 200 {
        return Some("status");
    }
    if wire.header("content-type") != Some(GEOJSON) {
        return Some("content-type");
    }
    if wire.header("cache-control") != Some("private, no-store") {
        return Some("cache-control");
    }
    if wire.header("vary") != Some("Accept") {
        return Some("vary");
    }
    // No validator is promised in this increment; an unbound one is a defect.
    if wire.header("etag").is_some() || wire.header("last-modified").is_some() {
        return Some("validator");
    }
    if wire.header("location").is_some() || !wire.header("x-request-id").is_some_and(uuid_v7) {
        return Some("metadata");
    }
    match serde_json::from_slice::<Value>(&wire.body) {
        Ok(value) if &value == expected => None,
        _ => Some("body"),
    }
}
fn retrieved(wire: &Wire, expected: &Value, when: &str) {
    assert_eq!(
        representation_error(wire, expected),
        None,
        "{when}: retrieved System differs from its independent expectation: {wire:?}"
    );
}
/// Safe problem facts with the fresh correlation removed. Two outcomes are
/// indistinguishable when these facts are equal and each correlation is new.
fn safe_problem(wire: &Wire, status: u16, cache: &str) -> Option<Value> {
    if wire.status != status
        || wire.header("content-type") != Some("application/problem+json")
        || wire.header("cache-control") != Some(cache)
    {
        return None;
    }
    for name in [
        "location",
        "etag",
        "last-modified",
        "link",
        "content-location",
    ] {
        if wire.header(name).is_some() {
            return None;
        }
    }
    let mut value: Value = serde_json::from_slice(&wire.body).ok()?;
    let correlation = value.as_object_mut()?.remove("correlation")?;
    let correlation = correlation.as_str()?;
    if !uuid_v7(correlation) || wire.header("x-request-id") != Some(correlation) {
        return None;
    }
    let members: BTreeSet<&str> = value.as_object()?.keys().map(String::as_str).collect();
    let catalog = BTreeSet::from(["detail", "status", "title", "type"]);
    if members != catalog || value["status"] != json!(status) {
        return None;
    }
    let names: BTreeSet<&str> = wire
        .headers
        .iter()
        .map(|(name, _)| name.as_str())
        .filter(|name| *name != "date")
        .collect();
    Some(json!({"headers":names, "length":wire.header("content-length"), "body":value}))
}
fn concealed(wire: &Wire, reference: &Value, label: &str, secrets: &[&str]) {
    assert_eq!(
        safe_problem(wire, 404, "private, no-store").as_ref(),
        Some(reference),
        "{label} System response is distinguishable from a missing System: {wire:?}"
    );
    let text = wire.text();
    for secret in secrets {
        assert!(
            !text.contains(secret),
            "{label} response disclosed System data"
        );
    }
}
fn oracle_controls() {
    let target = expected(
        SUPPLIED_ID,
        "urn:glaux:test:oracle",
        "Oracle",
        "sosa:Sensor",
    );
    let good = Wire {
        status: 200,
        headers: vec![
            ("content-type".into(), GEOJSON.into()),
            ("cache-control".into(), "private, no-store".into()),
            ("vary".into(), "Accept".into()),
            ("x-request-id".into(), SUPPLIED_ID.into()),
        ],
        body: serde_json::to_vec(&target).unwrap(),
    };
    assert_eq!(representation_error(&good, &target), None);
    let mut bodies = Vec::new();
    for (pointer, value) in [
        ("/id", json!("01890f20-7b5a-7cc3-98c4-dc0c0c220998")),
        ("/properties/name", json!("Oracle renamed")),
        ("/properties/uid", json!("urn:glaux:test:ORACLE")),
        (
            "/properties/featureType",
            json!("http://www.w3.org/ns/sosa/Sensor"),
        ),
        ("/geometry", json!({"type":"Point","coordinates":[0,0]})),
        ("/links/0/href", json!("https://attacker.invalid/systems/x")),
        ("/links/0/rel", json!("canonical")),
    ] {
        let mut changed = target.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        bodies.push(changed);
    }
    let mut changed = target.clone();
    changed["properties"]["description"] = json!("Unpromised field");
    bodies.push(changed);
    let mut changed = target.clone();
    changed["links"]
        .as_array_mut()
        .unwrap()
        .push(json!({"href":format!("{PUBLIC}/systems"), "rel":"collection", "type":GEOJSON, "title":"Systems"}));
    bodies.push(changed);
    let mut changed = target.clone();
    changed.as_object_mut().unwrap().remove("links");
    bodies.push(changed);
    for changed in bodies {
        let mut wrong = good.clone();
        wrong.body = serde_json::to_vec(&changed).unwrap();
        assert_eq!(representation_error(&wrong, &target), Some("body"));
    }
    let mut wrong = good.clone();
    wrong.status = 201;
    assert_eq!(representation_error(&wrong, &target), Some("status"));
    for (index, value, reason) in [
        (0, "application/json", "content-type"),
        (1, "no-store", "cache-control"),
        (2, "Origin", "vary"),
        (3, "caller-supplied-correlation", "metadata"),
    ] {
        let mut wrong = good.clone();
        wrong.headers[index].1 = value.into();
        assert_eq!(representation_error(&wrong, &target), Some(reason));
    }
    let mut wrong = good;
    wrong.headers.push(("etag".into(), "\"unbound\"".into()));
    assert_eq!(representation_error(&wrong, &target), Some("validator"));

    let problem = json!({"type":"urn:glaux:problem:not-found", "title":"Not Found", "status":404, "detail":"The requested resource is not available.", "correlation":SUPPLIED_ID});
    let base = Wire {
        status: 404,
        headers: vec![
            ("content-type".into(), "application/problem+json".into()),
            ("cache-control".into(), "private, no-store".into()),
            ("x-request-id".into(), SUPPLIED_ID.into()),
        ],
        body: serde_json::to_vec(&problem).unwrap(),
    };
    let reference = safe_problem(&base, 404, "private, no-store").unwrap();
    let mut other = base.clone();
    other.headers[2].1 = "01890f20-7b5a-7cc3-98c4-dc0c0c220998".into();
    let mut value = problem.clone();
    value["correlation"] = json!("01890f20-7b5a-7cc3-98c4-dc0c0c220998");
    other.body = serde_json::to_vec(&value).unwrap();
    assert_eq!(
        safe_problem(&other, 404, "private, no-store"),
        Some(reference.clone())
    );
    let mut variants = Vec::new();
    for (member, value) in [
        ("detail", json!("System urn:glaux:test:oracle is hidden")),
        ("title", json!("Forbidden")),
        ("id", json!(SUPPLIED_ID)),
    ] {
        let mut changed = problem.clone();
        changed[member] = value;
        let mut wrong = base.clone();
        wrong.body = serde_json::to_vec(&changed).unwrap();
        variants.push(wrong);
    }
    let mut wrong = base.clone();
    wrong
        .headers
        .push(("location".into(), format!("{PUBLIC}/systems/x")));
    variants.push(wrong);
    let mut wrong = base.clone();
    wrong
        .headers
        .push(("x-glaux-reason".into(), "denied".into()));
    variants.push(wrong);
    let mut wrong = base.clone();
    wrong.headers[2].1 = "01890f20-7b5a-7cc3-98c4-dc0c0c220998".into();
    variants.push(wrong);
    let mut wrong = base.clone();
    wrong.headers[1].1 = "no-store".into();
    variants.push(wrong);
    for wrong in variants {
        assert_ne!(
            safe_problem(&wrong, 404, "private, no-store").as_ref(),
            Some(&reference)
        );
    }
    let mut wrong = base;
    wrong.status = 403;
    assert!(safe_problem(&wrong, 404, "private, no-store").is_none());
    passed("independent-wire-oracle-controls");
}
async fn execute(connection: &mut PgConnection, sql: &'static str) {
    sqlx::query(sql).execute(connection).await.unwrap();
}
async fn snapshot(connection: &mut PgConnection) -> Value {
    let text:String=sqlx::query_scalar("SELECT json_build_object(
      'migrations',(SELECT coalesce(json_agg(t ORDER BY version),'[]') FROM public._sqlx_migrations t),
      'identity',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.resource_identity t),
      'system',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.system_identity t),
      'source',(SELECT coalesce(json_agg(t ORDER BY resource_id,authority,identifier),'[]') FROM public.source_identity t),
      'parent',(SELECT coalesce(json_agg(t ORDER BY child_id),'[]') FROM public.system_parent t),
      'artifact',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.source_artifact t),
      'revision',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.system_revision t),
      'audit',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.server_audit t),
      'work',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.outgoing_work t),
      'head',(SELECT coalesce(json_agg(t ORDER BY system_id),'[]') FROM public.system_write_head t),
      'retry',(SELECT coalesce(json_agg(t ORDER BY actor,source,operation,target,key),'[]') FROM public.system_create_retry t))::text")
        .fetch_one(connection).await.unwrap();
    serde_json::from_str(&text).unwrap()
}
const METHODS: [&str; 8] = [
    "get", "head", "post", "put", "patch", "delete", "options", "trace",
];
fn methods(item: &Value) -> BTreeSet<&str> {
    item.as_object()
        .expect("API path item missing")
        .keys()
        .map(String::as_str)
        .filter(|key| METHODS.contains(key))
        .collect()
}
fn allowed(wire: &Wire) -> BTreeSet<String> {
    wire.header("allow")
        .unwrap_or_default()
        .split(',')
        .map(|method| method.trim().to_owned())
        .collect()
}
async fn proof() {
    oracle_controls();
    let mut fixture = Fixture::new();
    let mut connection = PgConnection::connect(ADMIN).await.unwrap();
    execute(&mut connection, "SET statement_timeout=5000").await;
    let mut migration = Process::spawn(
        &mut fixture,
        &["migrate"],
        "postgres://postgres@localhost/glaux_harness_test?host=/var/run/postgresql&sslmode=disable",
        false,
    );
    assert!(migration.wait().success(), "explicit migration failed");
    for sql in [
        "CREATE ROLE glaux_read_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT",
        "GRANT USAGE ON SCHEMA public TO glaux_read_app",
        "GRANT SELECT ON public._sqlx_migrations TO glaux_read_app",
        "GRANT SELECT,INSERT ON public.resource_identity,public.system_identity,public.source_identity,public.system_parent,public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work,public.system_write_head,public.system_create_retry TO glaux_read_app",
        "GRANT UPDATE(digest,system_id,revision_id,artifact_id,audit_id,event_id,retained_at,expires_at) ON public.system_create_retry TO glaux_read_app",
        "GRANT SELECT,UPDATE ON public.system_parent_write_guard TO glaux_read_app",
    ] {
        execute(&mut connection, sql).await;
    }
    let writer_a = configuration("writer-a", "group-a", SOURCE_A);
    let mut server = start(&mut fixture, &writer_a);

    // Start at the root; follow only advertised links and the API description.
    let landing = get("/", "");
    assert_eq!(landing.status, 200, "root is not reachable");
    let landing = landing.json();
    let links = landing["links"].as_array().unwrap();
    let descriptions: Vec<_> = links
        .iter()
        .filter(|link| link["rel"] == "service-desc")
        .collect();
    assert_eq!(descriptions.len(), 1, "root must link one API description");
    let api = get(&local(descriptions[0]["href"].as_str().unwrap()), "").json();
    let root = api["servers"][0]["url"].as_str().unwrap().to_owned();
    assert_eq!(root, PUBLIC, "API description names another server root");
    assert!(
        api["paths"]["/systems"]["post"].is_object(),
        "API description omits the public write path"
    );
    let collection = local(&format!("{root}/systems"));
    let before = snapshot(&mut connection).await;
    let (a_id, a_path) = create(
        &collection,
        &body(
            "urn:glaux:test:read-a",
            "First retrieved System",
            "sosa:Sensor",
        ),
    );
    let a_expected = expected(
        &a_id,
        "urn:glaux:test:read-a",
        "First retrieved System",
        "sosa:Sensor",
    );
    let a_first = get(&a_path, "");
    retrieved(&a_first, &a_expected, "right after creation");
    let after = snapshot(&mut connection).await;
    let mut accepted = after["head"].as_array().unwrap().clone();
    accepted.retain(|row| !before["head"].as_array().unwrap().contains(row));
    assert_eq!(accepted.len(), 1, "one accepted write head expected");
    assert_eq!(accepted[0]["system_id"], json!(a_id));
    for accept in ["Accept: application/geo+json\r\n", "Accept: */*\r\n"] {
        let wire = get(&a_path, accept);
        retrieved(&wire, &a_expected, "negotiated retrieval");
        assert_eq!(wire.body, a_first.body);
    }
    let head = request("HEAD", &a_path, "", "");
    assert_eq!(head.status, 200, "canonical HEAD failed");
    assert!(head.body.is_empty(), "HEAD returned entity bytes");
    for name in ["content-type", "cache-control", "vary"] {
        assert_eq!(
            head.header(name),
            a_first.header(name),
            "HEAD {name} differs"
        );
    }
    assert_eq!(
        head.header("content-length"),
        Some(a_first.body.len().to_string().as_str())
    );
    // A second System keeps its exact full-URI spelling; supplied id and links
    // are neither identity nor representation.
    let supplied = json!({
        "type":"Feature", "id":SUPPLIED_ID, "geometry":null,
        "links":[{"href":"https://attacker.invalid/systems/forged", "rel":"alternate"}],
        "properties":{"uid":"urn:glaux:test:read-supplied", "name":"Supplied identifiers ignored", "featureType":"http://www.w3.org/ns/sosa/Platform"}
    });
    let (s_id, s_path) = create(&collection, &supplied.to_string());
    assert_ne!(s_id, SUPPLIED_ID, "client-supplied id became the identity");
    let s_expected = expected(
        &s_id,
        "urn:glaux:test:read-supplied",
        "Supplied identifiers ignored",
        "http://www.w3.org/ns/sosa/Platform",
    );
    let s_first = get(&s_path, "");
    retrieved(&s_first, &s_expected, "right after creation");
    let secrets = [
        a_id.as_str(),
        "urn:glaux:test:read-a",
        "First retrieved System",
    ];
    for accept in ["application/json", "application/sml+json"] {
        let wire = get(&a_path, &format!("Accept: {accept}\r\n"));
        assert!(
            safe_problem(&wire, 406, "private, no-store").is_some(),
            "unsupported Accept was not a safe 406: {wire:?}"
        );
        assert!(!secrets.iter().any(|secret| wire.text().contains(secret)));
    }
    // The API description and its example match what the listener does.
    let member = &api["paths"]["/systems/{id}"];
    assert_eq!(
        methods(member),
        BTreeSet::from(["get", "head"]),
        "API description differs from the implemented System methods"
    );
    assert_eq!(methods(&api["paths"]["/systems"]), BTreeSet::from(["post"]));
    let get_doc = &member["get"];
    let parameters = get_doc["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 1, "documented parameters differ");
    assert_eq!(parameters[0]["name"], "id");
    assert_eq!(parameters[0]["in"], "path");
    assert_eq!(parameters[0]["required"], true);
    assert_eq!(
        format!("{root}/systems/{a_id}"),
        format!("{PUBLIC}{a_path}"),
        "documented template does not produce the Location"
    );
    let success = &get_doc["responses"]["200"];
    let content = success["content"].as_object().unwrap();
    assert_eq!(
        content.keys().map(String::as_str).collect::<Vec<_>>(),
        [GEOJSON],
        "documented media differs from the response"
    );
    for (name, header) in [("Cache-Control", "cache-control"), ("Vary", "vary")] {
        assert_eq!(
            success["headers"][name]["schema"]["const"].as_str(),
            a_first.header(header),
            "documented {name} differs from the response"
        );
    }
    let head_doc = &member["head"]["responses"]["200"];
    assert!(
        head_doc.get("content").is_none(),
        "HEAD advertises an entity body"
    );
    let example = &content[GEOJSON]["example"];
    let example_id = example["id"].as_str().unwrap();
    assert!(uuid_v7(example_id));
    assert_eq!(
        example,
        &expected(
            example_id,
            "urn:glaux:example:thermometer",
            "Example thermometer",
            "sosa:Sensor",
        ),
        "documented example differs from the implemented representation"
    );
    for method in ["get", "head"] {
        assert_eq!(member[method]["security"], json!([]));
        assert_eq!(
            member[method]["x-glaux-conformance-dependencies"],
            json!([])
        );
    }
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let wire = request(method, &a_path, "", "");
        assert_eq!(wire.status, 405, "undocumented {method} was served");
        assert_eq!(
            allowed(&wire),
            BTreeSet::from(["GET".to_owned(), "HEAD".to_owned()])
        );
    }
    let list = get(&collection, "");
    assert_eq!(
        list.status, 405,
        "undocumented System collection GET was served"
    );
    assert_eq!(allowed(&list), BTreeSet::from(["POST".to_owned()]));
    assert!(
        links
            .iter()
            .all(|link| !link["href"].as_str().unwrap().contains("/systems")),
        "root advertised an unimplemented System collection"
    );
    assert_eq!(get("/conformance", "").json()["conformsTo"], json!([]));
    passed("root-navigation-create-and-exact-retrieval");

    // Restart only the server. A fresh process can answer only from the database.
    let before = snapshot(&mut connection).await;
    server.stop();
    let mut server = start(&mut fixture, &writer_a);
    let a_after = get(&a_path, "");
    retrieved(&a_after, &a_expected, "after server restart");
    assert_eq!(
        a_after.body, a_first.body,
        "restart changed representation bytes"
    );
    let s_after = get(&s_path, "");
    retrieved(&s_after, &s_expected, "after server restart");
    assert_eq!(
        s_after.body, s_first.body,
        "restart changed representation bytes"
    );
    assert_eq!(
        snapshot(&mut connection).await,
        before,
        "retrieval changed retained state"
    );
    passed("restart-retains-identity-and-meaning");

    server.stop();
    let mut server = start(
        &mut fixture,
        &configuration("reader-b", "group-b", SOURCE_B),
    );
    let before = snapshot(&mut connection).await;
    let missing = get(&format!("/systems/{MISSING_ID}"), "");
    let reference = safe_problem(&missing, 404, "private, no-store")
        .expect("missing System was not a safe 404");
    let denied = get(&a_path, "");
    concealed(&denied, &reference, "cross-source", &secrets);
    let upper = get(&format!("/systems/{}", a_id.to_uppercase()), "");
    concealed(&upper, &reference, "non-canonical", &secrets);
    let malformed = get("/systems/not-a-system-id", "");
    concealed(&malformed, &reference, "malformed", &secrets);
    let correlations: BTreeSet<_> = [&missing, &denied, &upper, &malformed]
        .iter()
        .map(|wire| wire.header("x-request-id").unwrap().to_owned())
        .collect();
    assert_eq!(correlations.len(), 4, "problem correlation was reused");
    // Negotiation fails identically before any lookup can reveal existence.
    let hidden = get(&a_path, "Accept: application/json\r\n");
    let absent = get(
        &format!("/systems/{MISSING_ID}"),
        "Accept: application/json\r\n",
    );
    assert_eq!(
        safe_problem(&hidden, 406, "private, no-store"),
        safe_problem(&absent, 406, "private, no-store")
    );
    assert!(safe_problem(&hidden, 406, "private, no-store").is_some());
    assert_eq!(
        snapshot(&mut connection).await,
        before,
        "denied or missing retrieval changed retained state"
    );
    // The same caller reads its own source, so the 404 is authorization.
    let (b_id, b_path) = create(
        &collection,
        &body(
            "urn:glaux:test:read-b",
            "Second source System",
            "sosa:Actuator",
        ),
    );
    let b_expected = expected(
        &b_id,
        "urn:glaux:test:read-b",
        "Second source System",
        "sosa:Actuator",
    );
    retrieved(&get(&b_path, ""), &b_expected, "own-source reader");
    server.stop();
    // Resource scope: group-a may read only the supplied-id System.
    let mut scoped = configuration("writer-a", "group-a", SOURCE_A);
    scoped["policy"]["grants"][0]["resources"] = json!([s_id]);
    let mut server = start(&mut fixture, &scoped);
    concealed(&get(&a_path, ""), &reference, "resource-scoped", &secrets);
    retrieved(&get(&s_path, ""), &s_expected, "resource-scoped reader");
    server.stop();
    // Action scope: creation permission alone does not grant retrieval.
    let mut create_only = configuration("writer-a", "group-a", SOURCE_A);
    create_only["policy"]["grants"][0]["actions"] = json!(["create"]);
    let mut server = start(&mut fixture, &create_only);
    concealed(&get(&a_path, ""), &reference, "create-only", &secrets);
    server.stop();
    passed("missing-and-concealed-are-indistinguishable");

    let fixture_bytes = fs::read("/tmp/glaux-system-read-fixtures.json").unwrap();
    assert!(fixture_bytes.len() < 16384);
    let signed: Value = serde_json::from_slice(&fixture_bytes).unwrap();
    let mut jwt = configuration("writer-a", "group-a", SOURCE_A);
    jwt.as_object_mut().unwrap().remove("development");
    jwt["authentication"] = json!("jwt");
    jwt["jwt"] = json!({"issuer":"https://issuer.example.test","audience":"https://api.example.test","keys":[signed["jwk"]],"required_scopes":["write"]});
    jwt["policy"]["grants"] = json!([{"issuer":"https://issuer.example.test","group":"group-a","source":SOURCE_A,"actions":["create","read"]}]);
    let mut server = start(&mut fixture, &jwt);
    let bad = format!(
        "Authorization: Bearer {}\r\n",
        signed["bad_token"].as_str().unwrap()
    );
    for headers in ["", bad.as_str()] {
        let hidden = get(&a_path, headers);
        let absent = get(&format!("/systems/{MISSING_ID}"), headers);
        let facts = safe_problem(&hidden, 401, "no-store");
        assert!(facts.is_some(), "unverified retrieval was not a safe 401");
        assert_eq!(facts, safe_problem(&absent, 401, "no-store"));
        assert!(hidden.header("www-authenticate").is_some());
        assert!(!secrets.iter().any(|secret| hidden.text().contains(secret)));
    }
    let token = format!(
        "Authorization: Bearer {}\r\nX-Glaux-Subject: attacker\r\n",
        signed["token"].as_str().unwrap()
    );
    let verified = get(&a_path, &token);
    retrieved(&verified, &a_expected, "verified token caller");
    assert_eq!(verified.body, a_first.body);
    concealed(
        &get(&b_path, &token),
        &reference,
        "token cross-source",
        &[b_id.as_str(), "urn:glaux:test:read-b"],
    );
    server.stop();
    passed("verified-token-callers-and-unauthenticated-requests");

    let before = snapshot(&mut connection).await;
    let mut disabled = writer_a.clone();
    disabled.as_object_mut().unwrap().remove("system_creation");
    let mut server = start(&mut fixture, &disabled);
    let absent = get(&a_path, "");
    assert!(
        safe_problem(&absent, 404, "no-store").is_some(),
        "disabled retrieval route answered: {absent:?}"
    );
    assert!(!secrets.iter().any(|secret| absent.text().contains(secret)));
    let api = get("/api", "").json();
    assert!(api["paths"].get("/systems/{id}").is_none());
    assert!(api["paths"].get("/systems").is_none());
    server.stop();
    assert_eq!(snapshot(&mut connection).await, before);
    passed("disabled-creation-removes-retrieval");
    println!("Required System read proof passed: 6 groups.");
}
fn main() {
    assert_eq!(
        std::env::args().count(),
        1,
        "No target or selection override accepted"
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(100), proof())
                .await
                .expect("bounded System read proof expired");
        });
}
