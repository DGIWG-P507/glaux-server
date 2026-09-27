//! Source-derived actual-binary discovery checks, independent of production types.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use glaux_server::storage::{SystemRecord, SystemRepository};
use serde_json::{Value, json};
use sqlx::{Connection, PgConnection};

const BINARY: &str = "/tmp/glaux-discovery-server";
const ADMIN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const APP: &str = "postgres://glaux_discovery_app:SyntheticDiscoverySecret@localhost/glaux_harness_test?host=/var/run/postgresql&sslmode=disable";
const ENV: &str = "GLAUX_TEST_DATABASE_URL";
const ADDRESS: &str = "127.0.0.1:18823";
const DIRECT_ROOT: &str = "http://127.0.0.1:18823";
const PREFIX_ROOT: &str = "http://127.0.0.1:18824/edge";
const CONFORMANCE_REL: &str = "http://www.opengis.net/def/rel/ogc/1.0/conformance";
const MAX_RESPONSE: usize = 4_000_000;

// Independently authored from the issue/Guide contract before handler wiring.
// Never import the production route table into this client.
const INVENTORY: [(&str, &str); 15] = [
    ("/", "application/json"),
    ("/conformance", "application/json"),
    ("/api", "application/json"),
    ("/docs", "text/html;charset=utf-8"),
    ("/docs/init.js", "text/javascript;charset=utf-8"),
    (
        "/docs/swagger-ui-bundle.js",
        "text/javascript;charset=utf-8",
    ),
    ("/docs/swagger-ui.css", "text/css;charset=utf-8"),
    ("/docs/LICENSE", "text/plain;charset=utf-8"),
    ("/docs/NOTICE", "text/plain;charset=utf-8"),
    (
        "/docs/swagger-ui-bundle.js.LICENSE.txt",
        "text/plain;charset=utf-8",
    ),
    ("/schemas/discovery.json", "application/schema+json"),
    ("/examples/landing.json", "application/json"),
    ("/examples/conformance.json", "application/json"),
    ("/health/live", "text/plain; charset=utf-8"),
    ("/health/ready", "text/plain; charset=utf-8"),
];

fn passed(name: &str) {
    println!("Discovery group passed: {name}");
}

fn safe(text: &str) {
    assert!(
        !text.contains("SyntheticDiscoverySecret"),
        "secret reached output"
    );
    assert!(!text.contains("postgres://"), "database URL reached output");
}

struct Fixture {
    directory: PathBuf,
    serial: usize,
}

impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("glaux-discovery-proof-{}", std::process::id()));
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
        fs::remove_dir_all(&self.directory).expect("owned discovery fixture cleanup failed");
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
                    "owned-discovery-child",
                    &self.child.id().to_string()
                ])
                .status()
                .unwrap()
                .success(),
            "owned shutdown signal failed"
        );
        assert!(self.wait().success(), "graceful discovery shutdown failed");
        assert!(
            TcpStream::connect_timeout(&ADDRESS.parse().unwrap(), Duration::from_millis(200))
                .is_err(),
            "owned discovery listener survived shutdown"
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

fn configuration(root: Option<&str>, enabled: Option<Value>) -> Value {
    let mut result = json!({"listener":ADDRESS,"authentication":"disabled","database":{"url_env":ENV},"health_timeout_ms":500});
    if let Some(root) = root {
        result["http"] = json!({"public_api_root":root});
    }
    if let Some(enabled) = enabled {
        result["discovery"] = enabled;
    }
    result
}

fn check_configuration(fixture: &mut Fixture, document: Value, valid: bool) {
    let path = fixture.file(&serde_json::to_vec(&document).unwrap());
    let mut process = Process::spawn(
        fixture,
        &["check-config", path.to_str().unwrap()],
        APP,
        false,
    );
    assert_eq!(
        process.wait().code(),
        Some(if valid { 0 } else { 2 }),
        "discovery configuration acceptance differs"
    );
    if valid {
        assert_eq!(process.output(), "Configuration valid; secrets redacted.\n");
    }
}

fn start(fixture: &mut Fixture, root: &str, enabled: Option<Value>) -> Process {
    let path = fixture.file(&serde_json::to_vec(&configuration(Some(root), enabled)).unwrap());
    Process::spawn(fixture, &["serve", path.to_str().unwrap()], APP, true)
}

#[derive(Debug)]
struct Wire {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Wire {
    fn header(&self, key: &str) -> Option<&str> {
        let values: Vec<_> = self
            .headers
            .iter()
            .filter(|(name, _)| name == key)
            .collect();
        assert!(
            values.len() <= 1,
            "duplicated independently checked header: {key}"
        );
        values.first().map(|(_, value)| value.as_str())
    }
    fn media(&self) -> &str {
        self.header("content-type")
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .trim()
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("general JSON parser rejected response")
    }
}

fn decode(bytes: Vec<u8>) -> Wire {
    let boundary = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("complete HTTP headers required");
    let head = std::str::from_utf8(&bytes[..boundary]).unwrap();
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
    let result = Wire {
        status,
        headers,
        body: bytes[boundary + 4..].to_vec(),
    };
    assert!(
        result.header("transfer-encoding").is_none(),
        "bounded fixed document unexpectedly uses transfer coding"
    );
    safe(std::str::from_utf8(&result.body).unwrap());
    result
}

fn request(url: &str, method: &str, accept: Option<&str>, forged: bool) -> Wire {
    let address_path = url
        .strip_prefix("http://")
        .expect("fixture follows only owned loopback HTTP targets");
    let (address, rest) = address_path.split_once('/').unwrap_or((address_path, ""));
    assert!(
        [ADDRESS, "127.0.0.1:18824"].contains(&address),
        "advertised URL escaped owned listeners"
    );
    let address: SocketAddr = address.parse().unwrap();
    let path = format!("/{rest}");
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    let origin = if forged {
        "attacker.invalid"
    } else {
        "localhost"
    };
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {origin}\r\nConnection: close\r\n"
    )
    .unwrap();
    if let Some(accept) = accept {
        write!(stream, "Accept: {accept}\r\n").unwrap();
    }
    if forged {
        write!(stream,"Forwarded: host=attacker.invalid;proto=https\r\nX-Forwarded-Host: attacker.invalid\r\nX-Forwarded-Proto: https\r\nX-Forwarded-Prefix: /escape\r\n").unwrap();
    }
    write!(stream, "\r\n").unwrap();
    let mut bytes = Vec::new();
    stream
        .take((MAX_RESPONSE + 1) as u64)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        bytes.len() <= MAX_RESPONSE,
        "response exceeded independent bound"
    );
    decode(bytes)
}

fn url(root: &str, path: &str) -> String {
    format!("{root}{path}")
}

fn linked(value: &Value, relation: &str, media: &str) -> Option<String> {
    let links = value.get("links")?.as_array()?;
    let selected: Vec<_> = links
        .iter()
        .filter(|link| {
            link.get("rel").and_then(Value::as_str) == Some(relation)
                && link
                    .get("type")
                    .and_then(Value::as_str)
                    .map(|value| value.split(';').next().unwrap().trim())
                    == Some(media)
        })
        .collect();
    if selected.len() != 1 {
        return None;
    }
    selected[0].get("href")?.as_str().map(str::to_owned)
}

fn landing_matches(value: &Value, root: &str) -> bool {
    value
        .get("title")
        .and_then(Value::as_str)
        .is_some_and(|title| !title.is_empty())
        && linked(value, "self", "application/json") == Some(root.to_owned())
        && linked(value, CONFORMANCE_REL, "application/json") == Some(url(root, "/conformance"))
        && linked(value, "service-desc", "application/json") == Some(url(root, "/api"))
        && linked(value, "service-doc", "text/html") == Some(url(root, "/docs"))
}

fn exact_links(value: &Value, expected: &[(&str, String, &str)]) {
    let links = value["links"]
        .as_array()
        .expect("discovery links must be an array");
    assert_eq!(
        links.len(),
        expected.len(),
        "discovery advertised missing/extra/duplicate links"
    );
    let actual = links
        .iter()
        .map(|link| {
            assert!(
                link["title"]
                    .as_str()
                    .is_some_and(|title| !title.is_empty()),
                "link title missing"
            );
            [
                link["rel"].as_str().unwrap().to_owned(),
                link["href"].as_str().unwrap().to_owned(),
                link["type"].as_str().unwrap().to_owned(),
            ]
        })
        .collect::<BTreeSet<_>>();
    let expected = expected
        .iter()
        .map(|(rel, target, media)| [(*rel).to_owned(), target.clone(), (*media).to_owned()])
        .collect();
    assert_eq!(
        actual, expected,
        "discovery relation/target/media differs from source-derived edge table"
    );
    for link in links {
        let wire = request(
            link["href"].as_str().unwrap(),
            "GET",
            Some(link["type"].as_str().unwrap()),
            false,
        );
        assert_eq!(
            wire.status, 200,
            "advertised discovery link is not reachable"
        );
        assert_eq!(wire.header("content-type"), link["type"].as_str());
    }
}

fn no_classes(value: &Value) -> bool {
    value.get("conformsTo") == Some(&json!([]))
}

fn oracle_controls() {
    let root = DIRECT_ROOT;
    let mut valid = json!({"title":"Source-authored example","links":[
        {"rel":"self","href":root,"type":"application/json"},
        {"rel":CONFORMANCE_REL,"href":url(root,"/conformance"),"type":"application/json"},
        {"rel":"service-desc","href":url(root,"/api"),"type":"application/json"},
        {"rel":"service-doc","href":url(root,"/docs"),"type":"text/html"}]});
    assert!(landing_matches(&valid, root));
    for (field, wrong) in [
        ("rel", "conformance"),
        ("type", "text/html"),
        ("href", "http://attacker.invalid/conformance"),
    ] {
        let mut candidate = valid.clone();
        candidate["links"][1][field] = json!(wrong);
        assert!(
            !landing_matches(&candidate, root),
            "wire oracle accepted wrong relationship/media/target"
        );
    }
    valid["links"].as_array_mut().unwrap().reverse();
    valid["extension"] = json!({"allowed":"untouched"});
    assert!(
        landing_matches(&valid, root),
        "valid reordered links/extension rejected"
    );
    assert!(no_classes(&json!({"conformsTo":[]})));
    for wrong in [
        json!({}),
        json!({"conformsTo":null}),
        json!({"conformsTo":["http://www.opengis.net/spec/ogcapi-connectedsystems-1/1.0/conf/api-common"]}),
    ] {
        assert!(
            !no_classes(&wrong),
            "declaration oracle accepted missing/mistyped/unfinished class"
        );
    }
    passed("independent-wire-oracle-controls");
}

struct Proxy {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Proxy {
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:18824").unwrap();
        listener.set_nonblocking(true).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = stop.clone();
        let thread = std::thread::spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                match listener.accept() {
                    Ok((mut client, _)) => {
                        client
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        client
                            .set_write_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        let mut incoming = Vec::new();
                        while !incoming.ends_with(b"\r\n\r\n") {
                            let mut byte = [0];
                            assert_eq!(
                                client.read(&mut byte).unwrap(),
                                1,
                                "proxy request truncated"
                            );
                            incoming.push(byte[0]);
                            assert!(incoming.len() <= 32_768, "proxy request bound exceeded");
                        }
                        let incoming = String::from_utf8(incoming).unwrap();
                        let (line, headers) = incoming.split_once("\r\n").unwrap();
                        let words: Vec<_> = line.split_whitespace().collect();
                        assert_eq!(words.len(), 3);
                        let backend_path = words[1]
                            .strip_prefix("/edge")
                            .expect("prefix proxy received outside path");
                        assert!(backend_path.starts_with('/') || backend_path.is_empty());
                        let backend_path = if backend_path.is_empty() {
                            "/"
                        } else {
                            backend_path
                        };
                        let mut backend = TcpStream::connect_timeout(
                            &ADDRESS.parse().unwrap(),
                            Duration::from_secs(2),
                        )
                        .unwrap();
                        backend
                            .set_read_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        backend
                            .set_write_timeout(Some(Duration::from_secs(3)))
                            .unwrap();
                        write!(
                            backend,
                            "{} {backend_path} {}\r\n{headers}",
                            words[0], words[2]
                        )
                        .unwrap();
                        let mut response = Vec::new();
                        backend
                            .take((MAX_RESPONSE + 1) as u64)
                            .read_to_end(&mut response)
                            .unwrap();
                        assert!(
                            response.len() <= MAX_RESPONSE,
                            "proxy response bound exceeded"
                        );
                        client.write_all(&response).unwrap();
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(2))
                    }
                    Err(error) => panic!("owned proxy failed: {error}"),
                }
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.thread
            .take()
            .unwrap()
            .join()
            .expect("owned prefix proxy cleanup failed");
        assert!(
            TcpStream::connect_timeout(
                &"127.0.0.1:18824".parse().unwrap(),
                Duration::from_millis(200)
            )
            .is_err(),
            "owned proxy survived shutdown"
        );
    }
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

fn problem(wire: &Wire, status: u16) {
    assert_eq!(wire.status, status);
    assert_eq!(wire.media(), "application/problem+json");
    assert_eq!(wire.json()["status"], json!(status));
    assert!(
        !std::str::from_utf8(&wire.body)
            .unwrap()
            .contains("attacker.invalid")
    );
}

fn discover(root: &str) -> (Value, Value, Value) {
    let root_wire = request(&url(root, "/"), "GET", None, false);
    assert!(
        root_wire.status == 200
            && root_wire.media() == "application/json"
            && landing_matches(&root_wire.json(), root),
        "enabled discovery root did not return the source-derived landing document: {root_wire:?}"
    );
    let landing = root_wire.json();
    assert_eq!(landing["title"], json!("Glaux Server"));
    exact_links(
        &landing,
        &[
            ("self", root.to_owned(), "application/json"),
            (
                CONFORMANCE_REL,
                url(root, "/conformance"),
                "application/json",
            ),
            ("service-desc", url(root, "/api"), "application/json"),
            ("service-doc", url(root, "/docs"), "text/html;charset=utf-8"),
            (
                "describedby",
                url(root, "/schemas/discovery.json"),
                "application/schema+json",
            ),
            (
                "related",
                url(root, "/examples/landing.json"),
                "application/json",
            ),
            (
                "related",
                url(root, "/examples/conformance.json"),
                "application/json",
            ),
        ],
    );
    let conformance = request(
        &linked(&landing, CONFORMANCE_REL, "application/json").unwrap(),
        "GET",
        Some("application/json"),
        false,
    );
    assert_eq!(conformance.status, 200);
    let conformance = conformance.json();
    assert!(
        no_classes(&conformance),
        "unfinished conformance class advertised"
    );
    exact_links(
        &conformance,
        &[
            ("self", url(root, "/conformance"), "application/json"),
            ("up", root.to_owned(), "application/json"),
        ],
    );
    let api = request(
        &linked(&landing, "service-desc", "application/json").unwrap(),
        "GET",
        Some("application/json"),
        false,
    );
    assert_eq!(api.status, 200);
    let api = api.json();
    assert_eq!(api["openapi"], json!("3.1.0"));
    assert_eq!(api["servers"], json!([{"url":root}]));
    let paths = api["paths"].as_object().unwrap();
    assert_eq!(
        paths.keys().map(String::as_str).collect::<BTreeSet<_>>(),
        INVENTORY.iter().map(|(path, _)| *path).collect(),
        "API advertised nonexistent or omitted implemented route"
    );
    (landing, conformance, api)
}

fn routes(root: &str, api: &Value) {
    for (path, media) in INVENTORY {
        let advertised = &api["paths"][path];
        assert_eq!(
            advertised
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .filter(|key| [
                    "get", "head", "post", "put", "patch", "delete", "options", "trace"
                ]
                .contains(key))
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["get", "head"]),
            "advertised methods differ"
        );
        let content = advertised["get"]["responses"]["200"]["content"]
            .as_object()
            .unwrap();
        assert_eq!(
            content.keys().map(String::as_str).collect::<Vec<_>>(),
            [media],
            "advertised response media differs"
        );
        assert!(
            advertised["head"]["responses"]["200"]
                .get("content")
                .is_none(),
            "HEAD advertises an entity body"
        );
        for method in ["get", "head"] {
            assert_eq!(
                advertised[method]["x-glaux-conformance-dependencies"],
                json!([]),
                "unfinished route conformance prerequisite advertised"
            );
            assert_eq!(
                advertised[method]["security"],
                json!([]),
                "public discovery advertised unavailable authentication requirement"
            );
        }
        let get = request(&url(root, path), "GET", Some(media), false);
        assert_eq!(get.status, 200, "advertised GET failed: {path}");
        assert_eq!(get.header("content-type"), Some(media));
        assert!(!get.body.is_empty());
        let head = request(&url(root, path), "HEAD", Some(media), false);
        assert_eq!(head.status, 200, "advertised HEAD failed: {path}");
        assert_eq!(head.header("content-type"), get.header("content-type"));
        assert_eq!(head.header("content-length"), get.header("content-length"));
        assert!(head.body.is_empty(), "HEAD returned entity bytes");
        for method in ["POST", "PUT", "PATCH", "DELETE", "OPTIONS"] {
            let denied = request(&url(root, path), method, None, false);
            problem(&denied, 405);
            let allowed = denied
                .header("allow")
                .unwrap()
                .split(',')
                .map(str::trim)
                .collect::<BTreeSet<_>>();
            assert_eq!(allowed, BTreeSet::from(["GET", "HEAD"]));
        }
        // Health media negotiation predates this task and remains its documented
        // fixed probe contract; discovery routes must enforce their offered type.
        if !path.starts_with("/health/") {
            problem(
                &request(&url(root, path), "GET", Some("application/json;q=banana"), false),
                400,
            );
            problem(
                &request(
                    &url(root, path),
                    "GET",
                    Some("application/x-glaux-unsupported"),
                    false,
                ),
                406,
            );
            problem(
                &request(
                    &url(root, path),
                    "GET",
                    Some(&format!("{media};q=0,*/*;q=1")),
                    false,
                ),
                406,
            );
            assert_eq!(
                request(
                    &url(root, path),
                    "GET",
                    Some(&format!(
                        "application/x-glaux-unsupported;q=1,{media};q=0.5"
                    )),
                    false
                )
                .status,
                200
            );
            assert_eq!(
                request(&url(root, path), "GET", Some("*/*"), false).status,
                200
            );
        }
    }
    for path in [
        "/systems",
        "/collections",
        "/datastreams",
        "/observations",
        "/controlstreams",
        "/commands",
        "/asyncapi",
        "/samplingFeatures",
        "/does-not-exist",
    ] {
        problem(&request(&url(root, path), "GET", None, false), 404);
    }
}

fn hrefs(html: &str, attribute: &str) -> Vec<String> {
    html.split(&format!("{attribute}=\""))
        .skip(1)
        .map(|tail| tail.split('"').next().unwrap().replace("&amp;", "&"))
        .collect()
}

fn validate_downloads(fixture: &mut Fixture, schema: Value, examples: Vec<Value>) {
    const MARKER: &str = "Downloaded discovery schema: 2 valid examples; missing links, unfinished class, invalid schema and retrieval controls detected.\n";
    let input = serde_json::to_vec(&json!({"schema":schema,"examples":examples})).unwrap();
    assert!(input.len() <= 32_768, "downloaded schema fixture bound exceeded");
    let input = fixture.file(&input);
    let stdout = fixture.file(b"");
    let stderr = fixture.file(b"");
    let child = Command::new("/tmp/glaux-discovery-schema-proof")
        .stdin(File::open(input).unwrap())
        .stdout(File::create(&stdout).unwrap())
        .stderr(File::create(&stderr).unwrap())
        .spawn().expect("owned schema proof binary unavailable");
    let mut process = Process { child, stdout, stderr, pump:None };
    assert!(process.wait().success(), "downloaded schema/example compilation proof failed: {}", process.output());
    assert_eq!(process.output(), MARKER, "schema proof execution marker missing or unexpected output");
    print!("{MARKER}");
}

fn downloads(fixture: &mut Fixture, root: &str, landing: &Value, conformance: &Value) {
    let page = request(
        &linked(landing, "service-doc", "text/html").unwrap(),
        "GET",
        Some("text/html"),
        false,
    );
    assert_eq!(page.status, 200);
    let html = std::str::from_utf8(&page.body).unwrap();
    let links = hrefs(html, "href");
    for path in [
        "/api",
        "/schemas/discovery.json",
        "/examples/landing.json",
        "/examples/conformance.json",
    ] {
        assert!(
            links.contains(&url(root, path)),
            "documentation omitted local download {path}"
        );
    }
    for target in hrefs(html, "src").into_iter().chain(links.iter().cloned()) {
        if target.starts_with('#') {
            continue;
        }
        assert!(
            target.starts_with(&format!("{root}/")),
            "documentation depends on external or unprefixed asset"
        );
        assert_eq!(
            request(&target, "GET", None, false).status,
            200,
            "local documentation target unavailable"
        );
    }
    let landing_example = request(&url(root, "/examples/landing.json"), "GET", None, false).json();
    let conformance_example = request(&url(root, "/examples/conformance.json"), "GET", None, false).json();
    assert_eq!(landing_example, *landing);
    assert_eq!(conformance_example, *conformance);
    let schema = request(&url(root, "/schemas/discovery.json"), "GET", None, false).json();
    assert_eq!(
        schema["$schema"],
        json!("https://json-schema.org/draft/2020-12/schema")
    );
    assert_eq!(schema["$id"], json!(url(root, "/schemas/discovery.json")));
    for definition in ["landing", "conformance", "link"] {
        assert!(
            schema["$defs"][definition].is_object(),
            "offline schema lacks {definition}"
        );
    }
    validate_downloads(fixture, schema, vec![landing_example, conformance_example]);
    let initializer = request(&url(root, "/docs/init.js"), "GET", None, false);
    let initializer = std::str::from_utf8(&initializer.body).unwrap();
    assert!(
        initializer.contains(&url(root, "/api")),
        "renderer points outside deployed API prefix"
    );
    assert!(!initializer.contains("attacker.invalid"));
    // The hosted wrapper independently verifies these files against the pinned
    // upstream manifest. Here actual served bytes must equal those exact assets.
    for (path, expected) in [
        (
            "/docs/swagger-ui-bundle.js",
            include_bytes!("../assets/swagger-ui/swagger-ui-bundle.js").as_slice(),
        ),
        (
            "/docs/swagger-ui.css",
            include_bytes!("../assets/swagger-ui/swagger-ui.css").as_slice(),
        ),
        (
            "/docs/LICENSE",
            include_bytes!("../assets/swagger-ui/LICENSE").as_slice(),
        ),
        (
            "/docs/NOTICE",
            include_bytes!("../assets/swagger-ui/NOTICE").as_slice(),
        ),
        (
            "/docs/swagger-ui-bundle.js.LICENSE.txt",
            include_bytes!("../assets/swagger-ui/swagger-ui-bundle.js.LICENSE.txt").as_slice(),
        ),
    ] {
        assert_eq!(
            request(&url(root, path), "GET", None, false).body,
            expected,
            "locally served renderer asset bytes differ: {path}"
        );
    }
}

async fn proof() {
    let mut fixture = Fixture::new();
    for enabled in [None, Some(json!(false)), Some(json!(true))] {
        check_configuration(
            &mut fixture,
            configuration(Some(DIRECT_ROOT), enabled),
            true,
        );
    }
    check_configuration(&mut fixture, configuration(None, Some(json!(true))), false);
    for invalid in [
        Value::Null,
        json!("true"),
        json!(1),
        json!({"enabled":true}),
    ] {
        check_configuration(
            &mut fixture,
            configuration(Some(DIRECT_ROOT), Some(invalid)),
            false,
        );
    }
    oracle_controls();
    let mut connection = PgConnection::connect(ADMIN).await.unwrap();
    let identity: (String, String, String) =
        sqlx::query_as("SELECT current_database(),current_user::text,host(inet_server_addr())")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        identity,
        (
            "glaux_harness_test".to_owned(),
            "postgres".to_owned(),
            "127.0.0.1".to_owned()
        )
    );
    execute(&mut connection, "SET statement_timeout=5000").await;
    let mut migrate = Process::spawn(
        &mut fixture,
        &["migrate"],
        "postgres://postgres@localhost/glaux_harness_test?host=/var/run/postgresql&sslmode=disable",
        false,
    );
    assert!(migrate.wait().success(), "explicit migration failed");
    execute(&mut connection, "CREATE ROLE glaux_discovery_app LOGIN").await;
    execute(
        &mut connection,
        "GRANT USAGE ON SCHEMA public TO glaux_discovery_app",
    )
    .await;
    execute(
        &mut connection,
        "GRANT SELECT ON public._sqlx_migrations TO glaux_discovery_app",
    )
    .await;
    let privileges:(bool,bool,bool)=sqlx::query_as("SELECT rolsuper,rolcreatedb,rolcreaterole FROM pg_roles WHERE rolname='glaux_discovery_app'").fetch_one(&mut connection).await.unwrap();
    assert_eq!(privileges, (false, false, false));
    let excessive:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM information_schema.role_table_grants WHERE grantee='glaux_discovery_app' AND (table_name<>'_sqlx_migrations' OR privilege_type<>'SELECT'))").fetch_one(&mut connection).await.unwrap();
    assert!(
        !excessive,
        "discovery role gained application write/read privileges"
    );
    SystemRepository::create(
        &mut connection,
        &SystemRecord {
            id: "01890f20-7b5a-7cc3-98c4-dc0c0c230101".parse().unwrap(),
            uid: "urn:glaux:test:discovery-sentinel".parse().unwrap(),
            label: "Discovery must not reveal or change this System".to_owned(),
            sources: vec![],
            parent: None,
        },
    )
    .await
    .unwrap();
    let before = snapshot(&mut connection).await;
    assert_eq!(before["migrations"].as_array().unwrap().len(), 8);

    let mut server = start(&mut fixture, DIRECT_ROOT, Some(json!(true)));
    let (landing, conformance, api) = discover(DIRECT_ROOT);
    passed("root-navigation-and-honest-declarations");
    routes(DIRECT_ROOT, &api);
    assert_eq!(
        request(&url(DIRECT_ROOT, "/"), "GET", None, true).json(),
        landing,
        "forged origin changed root links"
    );
    assert_eq!(
        request(&url(DIRECT_ROOT, "/api"), "GET", None, true).json(),
        api,
        "forged origin changed API server root"
    );
    passed("actual-method-media-and-origin-contract");
    downloads(&mut fixture, DIRECT_ROOT, &landing, &conformance);
    assert_eq!(
        snapshot(&mut connection).await,
        before,
        "discovery mutated authoritative data"
    );
    passed("offline-schema-examples-and-local-assets");
    server.stop();

    let mut prefixed = start(&mut fixture, PREFIX_ROOT, Some(json!(true)));
    let proxy = Proxy::start();
    let (landing, conformance, api) = discover(PREFIX_ROOT);
    routes(PREFIX_ROOT, &api);
    downloads(&mut fixture, PREFIX_ROOT, &landing, &conformance);
    assert_eq!(
        request(&url(PREFIX_ROOT, "/"), "GET", None, true).json(),
        landing,
        "forwarded forged origin changed prefixed links"
    );
    drop(proxy);
    prefixed.stop();
    passed("actual-prefix-proxy-and-forged-headers");
    for setting in [None, Some(json!(false))] {
        let mut disabled = start(&mut fixture, DIRECT_ROOT, setting);
        for (path, _) in INVENTORY {
            let result = request(&url(DIRECT_ROOT, path), "GET", None, false);
            if path.starts_with("/health/") {
                assert_eq!(result.status, 200);
            } else {
                problem(&result, 404);
            }
        }
        disabled.stop();
    }
    assert_eq!(
        snapshot(&mut connection).await,
        before,
        "discovery/prefix/disabled requests changed data or migrations"
    );
    passed("disabled-capabilities-clean-shutdown-and-unchanged-data");
    println!("Required discovery proof passed: 6 groups.");
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
                .expect("bounded discovery proof timed out");
        });
}
