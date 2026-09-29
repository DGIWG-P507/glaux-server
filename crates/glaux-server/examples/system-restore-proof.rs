//! Independent proof of isolated backup and restore of the first System (#26).
//!
//! The manifest is written from what this client submitted and observed on the
//! wire, then checked against the source before backup and the clone after
//! restore. Restored output never becomes its own expected answer.
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BINARY: &str = "/tmp/glaux-system-restore-server";
const SCRIPT: &str = "/tmp/glaux-system-restore.sh";
const SOURCE_DB: &str = "glaux_harness_test";
const CONTROL_DB: &str = "glaux_harness_control";
const CLONE: &str = "glaux_restore_clone";
const AUDIT_GAP: &str = "glaux_restore_audit_gap";
const ARTIFACT_BAD: &str = "glaux_restore_artifact_bad";
const ROLE: &str = "glaux_restore_app";
const OUTSIDER: &str = "glaux_restore_outsider";
const ENV: &str = "GLAUX_TEST_DATABASE_URL";
const ADDRESS: &str = "127.0.0.1:18827";
const PUBLIC: &str = "https://api.example.invalid/edge";
const SOURCE_A: &str = "urn:glaux:test:source-a";
const SOURCE_B: &str = "urn:glaux:test:source-b";
const GEOJSON: &str = "application/geo+json";
const SUPPLIED_ID: &str = "01890f20-7b5a-7cc3-98c4-dc0c0c220999";
const TABLES: [&str; 11] = [
    "migrations",
    "identity",
    "system",
    "source",
    "parent",
    "artifact",
    "revision",
    "audit",
    "work",
    "head",
    "retry",
];

fn passed(name: &str) {
    println!("System restore group passed: {name}");
}
fn safe(text: &str) {
    for secret in ["SyntheticRestoreSecret", "postgres://"] {
        assert!(
            !text.contains(secret),
            "private fixture detail reached response/output"
        );
    }
}
fn app_url(database: &str) -> String {
    format!(
        "postgres://{ROLE}:SyntheticRestoreSecret@localhost/{database}?host=/var/run/postgresql&sslmode=disable"
    )
}
fn admin_url(database: &str) -> String {
    format!("postgres://postgres@127.0.0.1:5432/{database}?sslmode=disable")
}
struct Fixture {
    directory: PathBuf,
    serial: usize,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("glaux-restore-proof-{}", std::process::id()));
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
        fs::remove_dir_all(&self.directory).expect("owned system-restore fixture cleanup failed");
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
                    "owned-system-restore-child",
                    &self.child.id().to_string()
                ])
                .status()
                .unwrap()
                .success(),
            "owned shutdown signal failed"
        );
        assert!(
            self.wait().success(),
            "graceful system-restore shutdown failed"
        );
        assert!(
            TcpStream::connect_timeout(&ADDRESS.parse().unwrap(), Duration::from_millis(200))
                .is_err(),
            "owned system-restore listener survived shutdown"
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

fn configuration(source: &str) -> Value {
    json!({
        "listener": ADDRESS, "database":{"url_env":ENV}, "health_timeout_ms":500,
        "authentication":"development",
        "development":{"subject":"writer-a","groups":["group-a"]},
        "http":{"public_api_root":PUBLIC},"discovery":true,
        "policy":{"grants":[
            {"issuer":"urn:glaux:development","group":"group-a","source":SOURCE_A,"actions":["create","read"]},
            {"issuer":"urn:glaux:development","group":"group-b","source":SOURCE_B,"actions":["create","read"]}
        ],"denial_audit":{"max_records":100,"max_per_window":100,"window_seconds":10}},
        "system_creation":{"source":source,"retry_retention_seconds":3600}
    })
}
fn start(fixture: &mut Fixture, database: &str, source: &str) -> Process {
    let path = fixture.file(&serde_json::to_vec(&configuration(source)).unwrap());
    let url = app_url(database);
    Process::spawn(fixture, &["serve", path.to_str().unwrap()], &url, true)
}
fn development_actor(subject: &str) -> String {
    serde_json::to_string(&["urn:glaux:development", subject, "development"]).unwrap()
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
    if let Some(length) = wire.header("content-length") {
        assert_eq!(wire.body.len(), length.parse::<usize>().unwrap());
    }
    safe(&String::from_utf8_lossy(&wire.body));
    wire
}
fn get(path: &str) -> Wire {
    request("GET", path, "", "")
}
fn post(body: &str, headers: &str) -> Wire {
    request(
        "POST",
        "/systems",
        &format!("Content-Type: application/geo+json\r\n{headers}"),
        body,
    )
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
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
/// Digest from the host's own coreutils, not the server or the database.
fn sha256sum(fixture: &mut Fixture, bytes: &[u8]) -> String {
    let path = fixture.file(bytes);
    let output = Command::new("sha256sum").arg(path).output().unwrap();
    assert!(output.status.success(), "independent digest tool failed");
    let text = String::from_utf8(output.stdout).unwrap();
    let digest = text.split_whitespace().next().unwrap().to_owned();
    assert_eq!(digest.len(), 64, "unexpected digest tool output");
    digest
}
/// One created System as this client submitted and observed it.
#[derive(Debug)]
struct Created {
    id: String,
    path: String,
    uid: String,
    name: String,
    tag: String,
    bytes: String,
    digest: String,
    correlation: String,
    retry_key: Option<String>,
}
fn create(
    fixture: &mut Fixture,
    submitted: &Value,
    retry_key: Option<&str>,
) -> Result<Created, Wire> {
    let bytes = submitted.to_string();
    let headers = retry_key.map_or(String::new(), |key| format!("Idempotency-Key: {key}\r\n"));
    let wire = post(&bytes, &headers);
    let location = wire.header("location").unwrap_or_default();
    let Some(id) = location.strip_prefix(&format!("{PUBLIC}/systems/")) else {
        return Err(wire);
    };
    if wire.status != 201 || !uuid_v7(id) {
        return Err(wire);
    }
    let properties = &submitted["properties"];
    Ok(Created {
        id: id.to_owned(),
        path: format!("/systems/{id}"),
        uid: properties["uid"].as_str().unwrap().to_owned(),
        name: properties["name"].as_str().unwrap().to_owned(),
        tag: properties["featureType"].as_str().unwrap().to_owned(),
        digest: sha256sum(fixture, bytes.as_bytes()),
        bytes,
        correlation: wire.header("x-request-id").unwrap().to_owned(),
        retry_key: retry_key.map(str::to_owned),
    })
}
fn feature(uid: &str, name: &str, tag: &str) -> Value {
    json!({"type":"Feature","geometry":null,"properties":{"uid":uid,"name":name,"featureType":tag}})
}
/// The documented retrieval representation, from the manifest only.
fn expected(system: &Created) -> Value {
    json!({
        "type":"Feature", "id":system.id, "geometry":null,
        "properties":{"uid":system.uid, "name":system.name, "featureType":system.tag},
        "links":[{"href":format!("{PUBLIC}{}", system.path), "rel":"self", "type":GEOJSON, "title":"This System"}]
    })
}
fn retrieved(wire: &Wire, system: &Created, when: &str) {
    let exact = wire.status == 200
        && wire.header("content-type") == Some(GEOJSON)
        && wire.header("cache-control") == Some("private, no-store")
        && serde_json::from_slice::<Value>(&wire.body).ok() == Some(expected(system));
    assert!(
        exact,
        "{when}: retrieved System differs from the manifest: {wire:?}"
    );
}
async fn execute(connection: &mut PgConnection, sql: &'static str) {
    sqlx::query(sql).execute(connection).await.unwrap();
}
async fn snapshot(database: &str) -> Value {
    let url = admin_url(database);
    let mut connection = PgConnection::connect(&url).await.unwrap();
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
        .fetch_one(&mut connection).await.unwrap();
    connection.close().await.unwrap();
    serde_json::from_str(&text).unwrap()
}
fn rows(snapshot: &Value, table: &str) -> Vec<Value> {
    snapshot[table].as_array().cloned().unwrap_or_default()
}
fn find(snapshot: &Value, table: &str, key: &str, value: &Value) -> Option<Value> {
    rows(snapshot, table)
        .into_iter()
        .find(|row| &row[key] == value)
}
/// Independent manifest check: every fact comes from this client's submissions
/// and observations. Returns the first missing or differing fact.
fn manifest_error(snapshot: &Value, created: &[Created], denied: &str) -> Option<String> {
    let actor = development_actor("writer-a");
    let keyed = created.iter().filter(|system| system.retry_key.is_some());
    for (table, count) in [
        ("identity", created.len()),
        ("system", created.len()),
        ("artifact", created.len()),
        ("revision", created.len()),
        ("head", created.len()),
        ("work", created.len()),
        ("audit", created.len() + 1),
        ("retry", keyed.count()),
    ] {
        if rows(snapshot, table).len() != count {
            return Some(format!("{table}: expected {count} rows"));
        }
    }
    for system in created {
        let id = json!(system.id);
        let fact = |table: &str| Some(format!("{}: {table} fact differs", system.uid));
        let identity = json!({"id":system.id, "family":"system", "uid":system.uid});
        if find(snapshot, "identity", "id", &id) != Some(identity) {
            return fact("identity");
        }
        let label = json!({"id":system.id, "label":system.name});
        if find(snapshot, "system", "id", &id) != Some(label) {
            return fact("system");
        }
        let Some(head) = find(snapshot, "head", "system_id", &id) else {
            return fact("head");
        };
        let Some(revision) = find(snapshot, "revision", "id", &head["revision_id"]) else {
            return fact("revision");
        };
        if revision["system_id"] != id || revision["artifact_id"] != head["artifact_id"] {
            return fact("revision");
        }
        let Some(artifact) = find(snapshot, "artifact", "id", &head["artifact_id"]) else {
            return fact("artifact");
        };
        if artifact["media_type"] != GEOJSON
            || artifact["bytes"] != json!(format!("\\x{}", hex(system.bytes.as_bytes())))
            || artifact["digest"] != json!(format!("\\x{}", system.digest))
        {
            return fact("artifact");
        }
        let Some(audit) = find(snapshot, "audit", "target_id", &id) else {
            return fact("audit");
        };
        if audit["actor"] != json!(actor)
            || audit["source"] != SOURCE_A
            || audit["operation"] != "system.create"
            || audit["outcome"] != "accepted"
            || audit["revision_id"] != revision["id"]
            || audit["correlation"] != json!(system.correlation)
            || audit["time_source"] != revision["receipt_source"]
        {
            return fact("audit");
        }
        let Some(work) = find(snapshot, "work", "audit_id", &audit["id"]) else {
            return fact("work");
        };
        if work["system_id"] != id
            || work["revision_id"] != revision["id"]
            || work["artifact_id"] != artifact["id"]
            || work["kind"] != "system.created"
            || work["outcome"] != "accepted"
        {
            return fact("work");
        }
        if let Some(key) = &system.retry_key {
            let Some(retry) = find(snapshot, "retry", "key", &json!(key)) else {
                return fact("retry");
            };
            if retry["system_id"] != id
                || retry["audit_id"] != audit["id"]
                || retry["actor"] != json!(actor)
            {
                return fact("retry");
            }
        }
    }
    let Some(denial) = find(snapshot, "audit", "correlation", &json!(denied)) else {
        return Some("denied attempt: audit fact missing".into());
    };
    if denial["outcome"] != "denied"
        || denial["actor"] != json!(actor)
        || !denial["target_id"].is_null()
        || !denial["revision_id"].is_null()
    {
        return Some("denied attempt: audit fact differs".into());
    }
    None
}
/// A clone is accepted only if it satisfies the independent manifest and every
/// table equals the backup inventory captured at the recovery point.
fn clone_error(
    clone: &Value,
    inventory: &Value,
    created: &[Created],
    denied: &str,
) -> Option<String> {
    if let Some(error) = manifest_error(clone, created, denied) {
        return Some(error);
    }
    TABLES
        .iter()
        .find(|table| clone[**table] != inventory[**table])
        .map(|table| format!("{table}: differs from the backup inventory"))
}
fn procedure(arguments: &[&str]) -> (bool, String) {
    let output = Command::new("/bin/sh")
        .arg(SCRIPT)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("restore procedure must execute");
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    safe(&text);
    (output.status.success(), text)
}
fn restore(dump: &str, target: &str) {
    let (succeeded, output) = procedure(&["restore", dump, SOURCE_DB, target, ROLE]);
    assert!(succeeded, "isolated restore failed: {output}");
}
async fn tamper(database: &str, sql: &'static str) {
    let url = admin_url(database);
    let mut connection = PgConnection::connect(&url).await.unwrap();
    // Administrative corruption of a disposable clone only, bypassing the
    // retained-history triggers that ordinary sessions cannot bypass.
    execute(&mut connection, "SET default_transaction_read_only = off").await;
    execute(&mut connection, "SET session_replication_role = replica").await;
    execute(&mut connection, sql).await;
    connection.close().await.unwrap();
}
async fn proof() {
    let mut fixture = Fixture::new();
    let url = admin_url(SOURCE_DB);
    let mut admin = PgConnection::connect(&url).await.unwrap();
    execute(&mut admin, "SET statement_timeout=5000").await;
    let mut migration = Process::spawn(
        &mut fixture,
        &["migrate"],
        "postgres://postgres@localhost/glaux_harness_test?host=/var/run/postgresql&sslmode=disable",
        false,
    );
    assert!(migration.wait().success(), "explicit migration failed");
    for sql in [
        "CREATE ROLE glaux_restore_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT",
        "CREATE ROLE glaux_restore_outsider LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT",
        "GRANT USAGE ON SCHEMA public TO glaux_restore_app",
        "GRANT SELECT ON public._sqlx_migrations TO glaux_restore_app",
        "GRANT SELECT,INSERT ON public.resource_identity,public.system_identity,public.source_identity,public.system_parent,public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work,public.system_write_head,public.system_create_retry TO glaux_restore_app",
        "GRANT UPDATE(digest,system_id,revision_id,artifact_id,audit_id,event_id,retained_at,expires_at) ON public.system_create_retry TO glaux_restore_app",
        "GRANT SELECT,UPDATE ON public.system_parent_write_guard TO glaux_restore_app",
    ] {
        execute(&mut admin, sql).await;
    }

    // The source workflow runs through the public write path only.
    let mut server = start(&mut fixture, SOURCE_DB, SOURCE_A);
    let first = feature(
        "urn:glaux:test:restore-a",
        "First restored System",
        "sosa:Sensor",
    );
    let mut second = feature(
        "urn:glaux:test:restore-b",
        "Second restored System",
        "http://www.w3.org/ns/sosa/Platform",
    );
    second["id"] = json!(SUPPLIED_ID);
    second["links"] =
        json!([{"href":"https://attacker.invalid/systems/forged", "rel":"alternate"}]);
    let created = vec![
        create(&mut fixture, &first, Some("restore-key")).expect("first System not created"),
        create(&mut fixture, &second, None).expect("second System not created"),
    ];
    server.stop();
    // A denied attempt leaves captured denial audit, not resource state.
    let mut server = start(&mut fixture, SOURCE_DB, SOURCE_B);
    let denied = create(
        &mut fixture,
        &feature("urn:glaux:test:restore-denied", "Denied", "sosa:Sensor"),
        None,
    )
    .expect_err("cross-source creation was admitted");
    assert_eq!(denied.status, 403, "denied attempt was not a 403");
    let denied = denied.header("x-request-id").unwrap().to_owned();
    server.stop();
    let inventory = snapshot(SOURCE_DB).await;
    assert_eq!(
        manifest_error(&inventory, &created, &denied),
        None,
        "source does not match the independent manifest"
    );
    passed("source-workflow-and-independent-manifest");

    let dump = fixture.directory.join("system.dump");
    let dump = dump.to_str().unwrap().to_owned();
    let (succeeded, output) = procedure(&["backup", SOURCE_DB, &dump]);
    assert!(succeeded, "backup failed: {output}");
    let (succeeded, output) = procedure(&["backup", SOURCE_DB, &dump]);
    assert!(
        !succeeded && output.contains("refusing to overwrite an existing dump file"),
        "existing dump file was overwritten: {output}"
    );
    for (target, refusal) in [
        (SOURCE_DB, "restore target is the source database"),
        (CONTROL_DB, "restore target already exists"),
    ] {
        let (succeeded, output) = procedure(&["restore", &dump, SOURCE_DB, target, ROLE]);
        assert!(
            !succeeded && output.contains(refusal),
            "unsafe restore target was not refused: {output}"
        );
    }
    restore(&dump, CLONE);
    assert_eq!(
        clone_error(&snapshot(CLONE).await, &inventory, &created, &denied),
        None,
        "restored clone differs from the manifest or backup inventory"
    );
    assert_eq!(
        snapshot(SOURCE_DB).await,
        inventory,
        "backup or restore changed the source"
    );
    passed("guarded-backup-and-isolated-restore");

    // Rerun the retrieval workflow from the root against the restored clone.
    let mut server = start(&mut fixture, CLONE, SOURCE_A);
    let landing = get("/").json();
    let links = landing["links"].as_array().unwrap();
    let description = links
        .iter()
        .find(|link| link["rel"] == "service-desc")
        .expect("restored root lacks its API description");
    let api_path = description["href"]
        .as_str()
        .unwrap()
        .strip_prefix(PUBLIC)
        .unwrap();
    let api = get(api_path).json();
    assert!(api["paths"]["/systems/{id}"]["get"].is_object());
    for system in &created {
        retrieved(&get(&system.path), system, "restored clone");
    }
    passed("restored-workflow-matches-manifest");

    // The clone is inspection-only: it cannot write, and outsiders cannot connect.
    let attempt = post(
        &feature("urn:glaux:test:clone-write", "Clone write", "sosa:Sensor").to_string(),
        "",
    );
    assert!(
        attempt.status != 201,
        "restored clone accepted a write: {attempt:?}"
    );
    assert_eq!(attempt.status, 503, "clone write refusal was not a 503");
    server.stop();
    assert_eq!(
        clone_error(&snapshot(CLONE).await, &inventory, &created, &denied),
        None,
        "clone state changed after a refused write"
    );
    let outsider = format!(
        "postgres://{OUTSIDER}@localhost/{CLONE}?host=/var/run/postgresql&sslmode=disable"
    );
    let connection = PgConnection::connect(&outsider).await;
    assert!(
        connection.is_err(),
        "outside role connected to the isolated clone"
    );
    let inspector = PgConnection::connect(&app_url(CLONE)).await;
    assert!(inspector.is_ok(), "inspection role could not connect");
    inspector.unwrap().close().await.unwrap();
    assert_eq!(
        snapshot(SOURCE_DB).await,
        inventory,
        "clone inspection changed the source"
    );
    passed("clone-refuses-effects-and-outside-access");

    // Incomplete or corrupt clones fail verification even when GET still works.
    restore(&dump, AUDIT_GAP);
    tamper(
        AUDIT_GAP,
        "DELETE FROM public.server_audit WHERE outcome = 'denied'",
    )
    .await;
    restore(&dump, ARTIFACT_BAD);
    tamper(
        ARTIFACT_BAD,
        "UPDATE public.source_artifact a SET bytes = n.forged, digest = sha256(n.forged)
         FROM (SELECT id, convert_to(replace(convert_from(bytes, 'UTF8'),
               'First restored System', 'Forged restored System'), 'UTF8') AS forged
               FROM public.source_artifact) n
         WHERE a.id = n.id AND n.forged <> a.bytes",
    )
    .await;
    for (database, problem) in [
        (AUDIT_GAP, "audit: expected 3 rows"),
        (
            ARTIFACT_BAD,
            "urn:glaux:test:restore-a: artifact fact differs",
        ),
    ] {
        let mut server = start(&mut fixture, database, SOURCE_A);
        for system in &created {
            retrieved(&get(&system.path), system, "corrupt clone");
        }
        server.stop();
        assert_eq!(
            clone_error(&snapshot(database).await, &inventory, &created, &denied),
            Some(problem.to_owned()),
            "incomplete or corrupt clone was not rejected for the intended reason"
        );
    }
    passed("incomplete-or-corrupt-clones-are-rejected");

    // Changes after the backup are outside its recovery point.
    let mut server = start(&mut fixture, SOURCE_DB, SOURCE_A);
    let later = create(
        &mut fixture,
        &feature("urn:glaux:test:restore-later", "Later System", "sosa:Sensor"),
        None,
    )
    .expect("post-backup System not created");
    retrieved(&get(&later.path), &later, "source after backup");
    server.stop();
    let mut server = start(&mut fixture, CLONE, SOURCE_A);
    let absent = get(&later.path);
    assert_eq!(
        absent.status, 404,
        "clone shows a System created after its backup"
    );
    retrieved(
        &get(&created[0].path),
        &created[0],
        "clone after source change",
    );
    server.stop();
    assert_eq!(
        clone_error(&snapshot(CLONE).await, &inventory, &created, &denied),
        None,
        "clone changed after the source moved on"
    );
    passed("recovery-point-excludes-later-changes");
    admin.close().await.unwrap();
    println!("Required System restore proof passed: 6 groups.");
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
            tokio::time::timeout(Duration::from_secs(140), proof())
                .await
                .expect("bounded System restore proof expired");
        });
}
