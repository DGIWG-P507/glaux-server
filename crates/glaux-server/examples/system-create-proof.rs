//! Independent raw HTTP and SQL proof of the real minimal System write path.
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const BINARY: &str = "/tmp/glaux-system-create-server";
const ADMIN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const APP: &str = "postgres://glaux_create_app:SyntheticCreateSecret@localhost/glaux_harness_test?host=/var/run/postgresql&sslmode=disable";
const ENV: &str = "GLAUX_TEST_DATABASE_URL";
const ADDRESS: &str = "127.0.0.1:18825";
const PUBLIC: &str = "https://api.example.invalid/edge";
const SOURCE_A: &str = "urn:glaux:test:source-a";
const SOURCE_B: &str = "urn:glaux:test:source-b";
const SUPPLIED_ID: &str = "01890f20-7b5a-7cc3-98c4-dc0c0c220999";

fn passed(name: &str) {
    println!("System create group passed: {name}");
}
fn safe(text: &str) {
    for secret in [
        "SyntheticCreateSecret",
        "postgres://",
        "forced-precommit-secret",
    ] {
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
            std::env::temp_dir().join(format!("glaux-system-create-proof-{}", std::process::id()));
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
        fs::remove_dir_all(&self.directory).expect("owned system-create fixture cleanup failed");
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
                    "owned-system-create-child",
                    &self.child.id().to_string()
                ])
                .status()
                .unwrap()
                .success(),
            "owned shutdown signal failed"
        );
        assert!(
            self.wait().success(),
            "graceful system-create shutdown failed"
        );
        assert!(
            TcpStream::connect_timeout(&ADDRESS.parse().unwrap(), Duration::from_millis(200))
                .is_err(),
            "owned system-create listener survived shutdown"
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

fn configuration(subject: &str, source: &str, enabled: bool) -> Value {
    let mut value = json!({
        "listener": ADDRESS, "database":{"url_env":ENV}, "health_timeout_ms":500,
        "authentication":"development",
        "development":{"subject":subject,"groups":[if subject=="writer-a" {"group-a"} else {"group-b"}]},
        "http":{"public_api_root":PUBLIC},"discovery":true,
        "policy":{"grants":[
            {"issuer":"urn:glaux:development","group":"group-a","source":SOURCE_A,"actions":["create","read"]},
            {"issuer":"urn:glaux:development","group":"group-b","source":SOURCE_B,"actions":["create","read"]}
        ],"denial_audit":{"max_records":100,"max_per_window":100,"window_seconds":10}}
    });
    if enabled {
        value["system_creation"] = json!({"source":source,"retry_retention_seconds":3600});
    }
    value
}
fn start(fixture: &mut Fixture, subject: &str, source: &str, enabled: bool) -> Process {
    start_config(fixture, &configuration(subject, source, enabled))
}
fn start_config(fixture: &mut Fixture, config: &Value) -> Process {
    let path = fixture.file(&serde_json::to_vec(config).unwrap());
    Process::spawn(fixture, &["serve", path.to_str().unwrap()], APP, true)
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
        serde_json::from_slice(&self.body).unwrap()
    }
}
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
    if let Some(length) = wire.header("content-length") {
        assert_eq!(wire.body.len(), length.parse::<usize>().unwrap());
    }
    safe(std::str::from_utf8(&wire.body).unwrap());
    wire
}
fn post(body: &str, extra: &str) -> Wire {
    request(
        "POST",
        "/systems",
        &format!("Content-Type: application/geo+json\r\n{extra}"),
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
fn creation_id(wire: &Wire) -> Option<String> {
    if wire.status != 201
        || !wire.body.is_empty()
        || wire.header("etag").is_some()
        || wire.header("cache-control") != Some("private, no-store")
        || !wire.header("x-request-id").is_some_and(uuid_v7)
    {
        return None;
    }
    let id = wire
        .header("location")?
        .strip_prefix(&format!("{PUBLIC}/systems/"))?;
    uuid_v7(id).then(|| id.to_owned())
}
fn oracle_controls() {
    let good = Wire {
        status: 201,
        headers: vec![
            ("location".into(), format!("{PUBLIC}/systems/{SUPPLIED_ID}")),
            ("cache-control".into(), "private, no-store".into()),
            ("x-request-id".into(), SUPPLIED_ID.into()),
        ],
        body: vec![],
    };
    assert_eq!(creation_id(&good), Some(SUPPLIED_ID.into()));
    let mut wrong = good.clone();
    wrong.status = 200;
    assert!(creation_id(&wrong).is_none());
    let mut wrong = good.clone();
    wrong.body = b"{}".to_vec();
    assert!(creation_id(&wrong).is_none());
    let mut wrong = good.clone();
    wrong.headers[0].1 =
        "https://attacker.invalid/systems/01890f20-7b5a-7cc3-98c4-dc0c0c220999".into();
    assert!(creation_id(&wrong).is_none());
    let mut wrong = good.clone();
    wrong.headers[0].1 = format!("{PUBLIC}/systems/not-an-id");
    assert!(creation_id(&wrong).is_none());
    let mut wrong = good.clone();
    wrong.headers.push(("etag".into(), "\"unpromised\"".into()));
    assert!(creation_id(&wrong).is_none());
    let mut wrong = good.clone();
    wrong.headers[1].1 = "no-store".into();
    assert!(creation_id(&wrong).is_none());
    let mut wrong = good;
    wrong.headers[2].1 = "caller-supplied-correlation".into();
    assert!(creation_id(&wrong).is_none());
    passed("independent-wire-oracle-controls");
}
fn body(uid: &str, tag: &str) -> String {
    serde_json::to_string(&json!({"type":"Feature","geometry":null,"properties":{"uid":uid,"name":"First System","featureType":tag}})).unwrap()
}
fn problem(wire: &Wire, status: u16) {
    problem_with_cache(wire, status, "no-store");
}
fn protected_problem(wire: &Wire, status: u16) {
    problem_with_cache(wire, status, "private, no-store");
}
fn problem_with_cache(wire: &Wire, status: u16, cache_control: &str) {
    assert_eq!(wire.status, status, "wrong HTTP rejection: {wire:?}");
    assert_eq!(
        wire.header("content-type"),
        Some("application/problem+json")
    );
    assert_eq!(wire.header("cache-control"), Some(cache_control));
    let value = wire.json();
    assert_eq!(value["status"], json!(status));
    assert!(value["detail"].is_string() && value["title"].is_string());
    assert_eq!(value["correlation"].as_str(), wire.header("x-request-id"));
    assert!(wire.header("location").is_none());
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
      'guard',(SELECT coalesce(json_agg(t ORDER BY singleton),'[]') FROM public.system_parent_write_guard t),
      'artifact',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.source_artifact t),
      'revision',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.system_revision t),
      'audit',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.server_audit t),
      'work',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.outgoing_work t),
      'head',(SELECT coalesce(json_agg(t ORDER BY system_id),'[]') FROM public.system_write_head t),
      'retry',(SELECT coalesce(json_agg(t ORDER BY actor,source,operation,target,key),'[]') FROM public.system_create_retry t))::text")
        .fetch_one(connection).await.unwrap();
    serde_json::from_str(&text).unwrap()
}

fn without_denials(mut value: Value) -> Value {
    value["audit"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row["outcome"] != "denied");
    value
}
fn additions<'a>(before: &Value, after: &'a Value, table: &str) -> Vec<&'a Value> {
    let prior = before[table].as_array().unwrap();
    after[table]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| !prior.contains(row))
        .collect()
}
async fn exact_commit(
    connection: &mut PgConnection,
    before: &Value,
    wire: &Wire,
    submitted: &str,
    actor: &str,
    source: &str,
) -> String {
    let id = creation_id(wire)
        .expect("minimal System POST did not return empty 201 with canonical UUIDv7 Location");
    let input: Value = serde_json::from_str(submitted).unwrap();
    let after = snapshot(connection).await;
    for table in [
        "identity", "system", "artifact", "revision", "audit", "work", "head",
    ] {
        assert_eq!(
            additions(before, &after, table).len(),
            1,
            "successful creation did not commit exactly one coherent {table} row"
        );
        assert_eq!(
            after[table].as_array().unwrap().len(),
            before[table].as_array().unwrap().len() + 1
        );
        assert!(
            before[table]
                .as_array()
                .unwrap()
                .iter()
                .all(|row| after[table].as_array().unwrap().contains(row)),
            "existing rows were changed"
        );
    }
    for table in ["source", "parent", "guard", "migrations"] {
        assert_eq!(before[table], after[table], "unrequested state changed");
    }
    let identity = additions(before, &after, "identity")[0];
    assert_eq!(
        identity,
        &json!({"id":id,"family":"system","uid":input["properties"]["uid"]})
    );
    assert_eq!(
        additions(before, &after, "system")[0],
        &json!({"id":id,"label":"First System"})
    );
    let revision = additions(before, &after, "revision")[0];
    let artifact = additions(before, &after, "artifact")[0];
    let audit = additions(before, &after, "audit")[0];
    let event = additions(before, &after, "work")[0];
    assert_eq!(revision["system_id"], json!(id));
    assert_eq!(revision["artifact_id"], artifact["id"]);
    assert_eq!(artifact["media_type"], json!("application/geo+json"));
    let stored:(String,bool)=sqlx::query_as("SELECT convert_from(bytes,'UTF8'),digest=sha256(bytes) FROM public.source_artifact WHERE id::text=$1")
        .bind(artifact["id"].as_str().unwrap()).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(
        stored,
        (submitted.to_owned(), true),
        "exact source bytes/digest changed"
    );
    let stored_json: Value = serde_json::from_str(&stored.0).unwrap();
    assert_eq!(
        stored_json["properties"]["featureType"],
        input["properties"]["featureType"]
    );
    for key in [
        "semantic_civil_second",
        "semantic_leap",
        "semantic_fraction",
        "semantic_source",
    ] {
        assert!(revision[key].is_null());
    }
    assert!(revision["receipt_source"].as_str().unwrap().ends_with('Z'));
    assert_eq!(audit["actor"], json!(actor));
    assert_eq!(audit["source"], json!(source));
    assert_eq!(audit["operation"], json!("system.create"));
    assert_eq!(audit["outcome"], json!("accepted"));
    assert_eq!(audit["target_id"], json!(id));
    assert_eq!(audit["revision_id"], revision["id"]);
    assert_eq!(audit["time_source"], revision["receipt_source"]);
    assert!(uuid_v7(audit["correlation"].as_str().unwrap()));
    assert_eq!(audit["correlation"].as_str(), wire.header("x-request-id"));
    assert_eq!(event["system_id"], json!(id));
    assert_eq!(event["revision_id"], revision["id"]);
    assert_eq!(event["artifact_id"], artifact["id"]);
    assert_eq!(event["audit_id"], audit["id"]);
    assert_eq!(event["kind"], json!("system.created"));
    assert_eq!(event["outcome"], json!("accepted"));
    assert_eq!(
        additions(before, &after, "head")[0],
        &json!({"system_id":id,"revision_id":revision["id"],"artifact_id":artifact["id"]})
    );
    id
}
async fn rejected(connection: &mut PgConnection, submitted: &str, headers: &str, status: u16) {
    let before = snapshot(connection).await;
    protected_problem(&post(submitted, headers), status);
    assert_eq!(
        snapshot(connection).await,
        before,
        "rejected write left resource, revision, audit, retry or outgoing state"
    );
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
        "CREATE ROLE glaux_create_app LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT",
        "GRANT USAGE ON SCHEMA public TO glaux_create_app",
        "GRANT SELECT ON public._sqlx_migrations TO glaux_create_app",
        "GRANT SELECT,INSERT ON public.resource_identity,public.system_identity,public.source_identity,public.system_parent,public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work,public.system_write_head,public.system_create_retry TO glaux_create_app",
        "GRANT UPDATE(digest,system_id,revision_id,artifact_id,audit_id,event_id,retained_at,expires_at) ON public.system_create_retry TO glaux_create_app",
        "GRANT SELECT,UPDATE ON public.system_parent_write_guard TO glaux_create_app",
    ] {
        execute(&mut connection, sql).await;
    }
    let privileges: (bool, bool, bool) = sqlx::query_as(
        "SELECT rolsuper,rolcreatedb,rolcreaterole FROM pg_roles WHERE rolname='glaux_create_app'",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(privileges, (false, false, false));
    let mut server = start(&mut fixture, "writer-a", SOURCE_A, true);
    let first = body("urn:glaux:test:created-a", "sosa:Sensor");
    let before = snapshot(&mut connection).await;
    let wire = post(
        &first,
        "Accept: text/plain\r\nForwarded: host=attacker.invalid;proto=http\r\n",
    );
    let first_id = exact_commit(
        &mut connection,
        &before,
        &wire,
        &first,
        &development_actor("writer-a"),
        SOURCE_A,
    )
    .await;
    assert_eq!(
        snapshot(&mut connection).await["retry"],
        before["retry"],
        "unkeyed POST manufactured retry state"
    );
    // Canonical member GET (#25) has its own proof; no collection GET is added.
    let api = request("GET", "/api", "", "").json();
    assert!(api["paths"]["/systems"]["post"].is_object());
    assert!(api["paths"]["/systems"].get("get").is_none());
    assert_eq!(
        request("GET", "/conformance", "", "").json()["conformsTo"],
        json!([])
    );
    passed("empty201-canonical-location-and-atomic-records");

    rejected(&mut connection, &first, "", 409).await;
    rejected(
        &mut connection,
        &body("urn:glaux:test:condition-star", "sosa:Sensor"),
        "If-Match: *\r\n",
        412,
    )
    .await;
    rejected(
        &mut connection,
        &body("urn:glaux:test:condition-tag", "sosa:Sensor"),
        "If-Match: \"unknown\"\r\n",
        412,
    )
    .await;
    rejected(
        &mut connection,
        &body("urn:glaux:test:condition-empty", "sosa:Sensor"),
        "If-Match: \r\n",
        412,
    )
    .await;
    rejected(
        &mut connection,
        &body("urn:glaux:test:condition-malformed", "sosa:Sensor"),
        "If-Match: not-quoted\r\n",
        400,
    )
    .await;
    rejected(&mut connection, "{", "", 400).await;
    rejected(
        &mut connection,
        r#"{"type":"Feature","type":"Feature","geometry":null,"properties":{}}"#,
        "",
        400,
    )
    .await;
    for mutation in [
        "missing-tag",
        "bad-tag",
        "bad-id",
        "geometry",
        "relation",
        "forged-source",
    ] {
        let mut value: Value =
            serde_json::from_str(&body(&format!("urn:glaux:test:{mutation}"), "sosa:Sensor"))
                .unwrap();
        let expected = match mutation {
            "missing-tag" => {
                value["properties"]
                    .as_object_mut()
                    .unwrap()
                    .remove("featureType");
                400
            }
            "bad-tag" => {
                value["properties"]["featureType"] = json!("PhysicalSystem");
                400
            }
            "bad-id" => {
                value["id"] = json!(true);
                400
            }
            "geometry" => {
                value["geometry"] = json!({"type":"Point","coordinates":[0,0]});
                422
            }
            "relation" => {
                value["properties"]["systemKind@link"] =
                    json!({"href":"https://example.invalid/procedure"});
                422
            }
            _ => {
                value["properties"]["source"] = json!(SOURCE_B);
                422
            }
        };
        rejected(&mut connection, &value.to_string(), "", expected).await;
    }
    let before = snapshot(&mut connection).await;
    protected_problem(
        &request("POST", "/systems", "Content-Type: text/plain\r\n", &first),
        415,
    );
    problem(&post(&first, "Content-Encoding: gzip\r\n"), 415);
    assert_eq!(snapshot(&mut connection).await, before);
    passed("malformed-media-and-no-partial-writes");

    server.stop();
    let mut server = start(&mut fixture, "writer-a", SOURCE_B, true);
    let before = snapshot(&mut connection).await;
    let denied = post(
        &body("urn:glaux:test:cross-source", "sosa:Sensor"),
        "X-Glaux-Source: urn:glaux:test:source-a\r\nX-Glaux-Subject: writer-b\r\n",
    );
    assert_eq!(
        denied.status, 403,
        "cross-source System creation was admitted"
    );
    protected_problem(&denied, 403);
    let after = snapshot(&mut connection).await;
    assert_eq!(
        without_denials(after.clone()),
        without_denials(before.clone()),
        "denied source mutated resource/outgoing state"
    );
    let denials = additions(&before, &after, "audit");
    assert_eq!(denials.len(), 1);
    assert_eq!(denials[0]["outcome"], json!("denied"));
    assert_eq!(
        denials[0]["actor"],
        json!("[\"urn:glaux:development\",\"writer-a\",\"development\"]")
    );
    assert!(
        denials[0]["source"].is_null()
            && denials[0]["target_id"].is_null()
            && denials[0]["revision_id"].is_null()
    );
    assert_eq!(
        denials[0]["correlation"].as_str(),
        denied.header("x-request-id")
    );
    let denied_condition = post(
        &body("urn:glaux:test:denied-condition", "sosa:Sensor"),
        "If-Match: *\r\n",
    );
    protected_problem(&denied_condition, 403);
    assert_eq!(
        without_denials(snapshot(&mut connection).await),
        without_denials(after),
        "precondition handling bypassed denial isolation"
    );
    let before = snapshot(&mut connection).await;
    let denied_malformed = post("{", "");
    protected_problem(&denied_malformed, 403);
    let after = snapshot(&mut connection).await;
    assert_eq!(
        without_denials(after.clone()),
        without_denials(before.clone()),
        "malformed unauthorized write changed resource or outgoing state"
    );
    let denials = additions(&before, &after, "audit");
    assert_eq!(denials.len(), 1);
    assert_eq!(denials[0]["outcome"], json!("denied"));
    assert_eq!(denials[0]["actor"], json!(development_actor("writer-a")));
    assert!(
        denials[0]["source"].is_null()
            && denials[0]["target_id"].is_null()
            && denials[0]["revision_id"].is_null()
    );
    assert_eq!(
        denials[0]["correlation"].as_str(),
        denied_malformed.header("x-request-id")
    );
    server.stop();
    let mut server = start(&mut fixture, "writer-b", SOURCE_B, true);
    let before = snapshot(&mut connection).await;
    let second = body(
        "urn:glaux:test:created-b",
        "http://www.w3.org/ns/sosa/System",
    );
    let wire = post(&second, "");
    exact_commit(
        &mut connection,
        &before,
        &wire,
        &second,
        &development_actor("writer-b"),
        SOURCE_B,
    )
    .await;
    server.stop();
    let fixture_bytes = fs::read("/tmp/glaux-system-create-fixtures.json").unwrap();
    assert!(fixture_bytes.len() < 16384);
    let signed: Value = serde_json::from_slice(&fixture_bytes).unwrap();
    let mut jwt = configuration("writer-a", SOURCE_A, true);
    jwt.as_object_mut().unwrap().remove("development");
    jwt["authentication"] = json!("jwt");
    jwt["jwt"] = json!({"issuer":"https://issuer.example.test","audience":"https://api.example.test","keys":[signed["jwk"]],"required_scopes":["write"]});
    jwt["policy"]["grants"] = json!([{"issuer":"https://issuer.example.test","group":"group-a","source":SOURCE_A,"actions":["create","read"]}]);
    let mut server = start_config(&mut fixture, &jwt);
    let value = body("urn:glaux:test:jwt-created", "sosa:Sensor");
    let before = snapshot(&mut connection).await;
    problem(&post(&value, "If-Match: *\r\n"), 401);
    problem(
        &post(
            &value,
            &format!(
                "Authorization: Bearer {}\r\nIf-Match: *\r\n",
                signed["bad_token"].as_str().unwrap()
            ),
        ),
        401,
    );
    assert_eq!(
        snapshot(&mut connection).await,
        before,
        "unverified caller caused writes"
    );
    let wire = post(
        &value,
        &format!(
            "Authorization: Bearer {}\r\nX-Glaux-Subject: attacker\r\n",
            signed["token"].as_str().unwrap()
        ),
    );
    exact_commit(
        &mut connection,
        &before,
        &wire,
        &value,
        "[\"https://issuer.example.test\",\"jwt-writer\",\"jwt\"]",
        SOURCE_A,
    )
    .await;
    server.stop();
    let mut server = start(&mut fixture, "writer-b", SOURCE_B, true);
    passed("verified-callers-source-scope-and-safe-denials");

    let mut supplied: Value =
        serde_json::from_str(&body("urn:glaux:test:supplied-id", "sosa:Platform")).unwrap();
    supplied["id"] = json!(SUPPLIED_ID);
    let supplied = supplied.to_string();
    let before = snapshot(&mut connection).await;
    let wire = post(
        &supplied,
        "Idempotency-Key: retry-one\r\nX-Request-ID: forged-correlation\r\n",
    );
    let actual_id = exact_commit(
        &mut connection,
        &before,
        &wire,
        &supplied,
        &development_actor("writer-b"),
        SOURCE_B,
    )
    .await;
    assert_ne!(
        actual_id, SUPPLIED_ID,
        "client-supplied local ID became authoritative"
    );
    let after = snapshot(&mut connection).await;
    assert_eq!(additions(&before, &after, "retry").len(), 1);
    let retry = post(&supplied, "Idempotency-Key: retry-one\r\n");
    assert_eq!(creation_id(&retry), Some(actual_id.clone()));
    assert_eq!(
        snapshot(&mut connection).await,
        after,
        "same-key retry duplicated accepted facts"
    );
    rejected(
        &mut connection,
        &body("urn:glaux:test:changed-intent", "sosa:Platform"),
        "Idempotency-Key: retry-one\r\n",
        409,
    )
    .await;
    rejected(
        &mut connection,
        &body("urn:glaux:test:bad-retry", "sosa:Sensor"),
        "Idempotency-Key: \r\n",
        400,
    )
    .await;
    server.stop();
    // A replay is authorized against its retained member, never a fresh UUID.
    let mut restricted = configuration("writer-b", SOURCE_B, true);
    restricted["policy"]["grants"][1]["resources"] = json!([actual_id]);
    let mut server = start_config(&mut fixture, &restricted);
    assert_eq!(
        creation_id(&post(&supplied, "Idempotency-Key: retry-one\r\n")),
        Some(actual_id)
    );
    assert_eq!(
        snapshot(&mut connection).await,
        after,
        "authorized retained-ID replay changed committed facts"
    );
    server.stop();
    restricted["policy"]["grants"][1]["resources"] = json!([first_id]);
    let mut server = start_config(&mut fixture, &restricted);
    protected_problem(&post(&supplied, "Idempotency-Key: retry-one\r\n"), 403);
    assert_eq!(
        without_denials(snapshot(&mut connection).await),
        without_denials(after),
        "revoked retry permission disclosed or changed accepted facts"
    );
    server.stop();
    let mut server = start(&mut fixture, "writer-b", SOURCE_B, true);
    for (index, tag) in [
        "sosa:Sensor",
        "sosa:Actuator",
        "sosa:Sampler",
        "sosa:Platform",
        "sosa:System",
        "http://www.w3.org/ns/sosa/Sensor",
        "http://www.w3.org/ns/sosa/Actuator",
        "http://www.w3.org/ns/sosa/Sampler",
        "http://www.w3.org/ns/sosa/Platform",
        "http://www.w3.org/ns/sosa/System",
    ]
    .iter()
    .enumerate()
    {
        let value = body(&format!("urn:glaux:test:type-{index}"), tag);
        let before = snapshot(&mut connection).await;
        let wire = post(
            &value,
            if index == 0 {
                "If-None-Match: *\r\nIf-Modified-Since: Sat, 01 Jan 2000 00:00:00 GMT\r\n"
            } else if index == 1 {
                "If-None-Match: \r\n"
            } else {
                ""
            },
        );
        exact_commit(
            &mut connection,
            &before,
            &wire,
            &value,
            &development_actor("writer-b"),
            SOURCE_B,
        )
        .await;
        assert_eq!(snapshot(&mut connection).await["retry"], before["retry"]);
    }
    passed("optional-retry-and-forged-context");

    execute(&mut connection,"CREATE FUNCTION public.glaux_proof_fail_work() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'forced-precommit-secret'; END $$").await;
    execute(&mut connection,"CREATE TRIGGER glaux_proof_fail_work BEFORE INSERT ON public.outgoing_work FOR EACH ROW EXECUTE FUNCTION public.glaux_proof_fail_work()").await;
    let before = snapshot(&mut connection).await;
    protected_problem(
        &post(
            &body("urn:glaux:test:precommit-failure", "sosa:Sensor"),
            "Idempotency-Key: rollback-key\r\n",
        ),
        503,
    );
    assert_eq!(
        snapshot(&mut connection).await,
        before,
        "precommit failure left partial accepted writes"
    );
    execute(
        &mut connection,
        "DROP TRIGGER glaux_proof_fail_work ON public.outgoing_work",
    )
    .await;
    execute(
        &mut connection,
        "DROP FUNCTION public.glaux_proof_fail_work()",
    )
    .await;
    server.stop();
    let before = snapshot(&mut connection).await;
    let mut disabled = start(&mut fixture, "writer-a", SOURCE_A, false);
    problem(
        &post(&body("urn:glaux:test:disabled", "sosa:Sensor"), ""),
        404,
    );
    assert!(
        request("GET", "/api", "", "").json()["paths"]
            .get("/systems")
            .is_none()
    );
    disabled.stop();
    assert_eq!(snapshot(&mut connection).await, before);
    passed("precommit-failure-and-owned-cleanup");
    println!("Required System create proof passed: 6 groups.");
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
                .expect("bounded System create proof expired");
        });
}
