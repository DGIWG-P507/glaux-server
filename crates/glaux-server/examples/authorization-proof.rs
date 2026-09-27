//! Independent raw HTTP and SQL evidence for bounded action/source admission.
use axum::{
    Router,
    extract::{Extension, Path, Request, State},
    http::StatusCode,
    response::Response,
    routing::{get, post},
};
use glaux_domain::identity::{LocalId, SourceIdentity};
use glaux_server::{
    application::{AuditContext, CreateSystem, RetryKey, UpdateSystem},
    authentication::{Authenticator, CallerContext, DevelopmentConfig},
    authorization::{
        AccessError, AccessPolicy, Action, Admission, ConfiguredPolicy, DenialLimits, GrantConfig,
        OperationContext, PermissionSet, PolicyConfig, PolicyError, RateClock,
    },
    http_boundary::{HttpBoundary, Limits, json_response},
    revisions::{NewSourceArtifact, SystemRevision},
    storage::{SystemRecord, SystemRepository, migrate},
};
use serde_json::{Value, json};
use sqlx::{AssertSqlSafe, Connection, PgConnection};
use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex as AsyncMutex, oneshot};

const DSN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const SOURCE_A: &str = "urn:glaux:test:source-a";
const SOURCE_B: &str = "urn:glaux:test:source-b";
const TIME: &str = "2026-09-26T12:00:00.123456789Z";
const CANARY: &str = "SyntheticAuthorizationSecretCanary";
const FINAL: &str = "Required authorization proof passed: 8 groups.";

fn id(number: u16) -> LocalId {
    format!("01890f20-7b5a-7cc3-98c4-dc0c0c22{number:04}")
        .parse()
        .unwrap()
}

struct TestClock(Mutex<Option<Duration>>);
impl RateClock for TestClock {
    fn now(&self) -> Option<Duration> {
        *self.0.lock().unwrap()
    }
}
impl TestClock {
    fn set(&self, value: Option<u64>) {
        *self.0.lock().unwrap() = value.map(Duration::from_secs);
    }
}

struct ControlledPolicy {
    policy: RwLock<ConfiguredPolicy>,
    unavailable: AtomicBool,
}
impl AccessPolicy for ControlledPolicy {
    fn permissions(
        &self,
        caller: &CallerContext,
        action: Action,
    ) -> Result<PermissionSet, PolicyError> {
        if self.unavailable.load(Ordering::SeqCst) {
            // A locally controlled unavailable adapter; never an enterprise service.
            return Err(PolicyError);
        }
        self.policy.read().unwrap().permissions(caller, action)
    }
}

fn limits(records: u32, rate: u32) -> DenialLimits {
    DenialLimits {
        max_records: records,
        max_per_window: rate,
        window_seconds: 10,
    }
}

fn policy() -> PolicyConfig {
    let grant = |subject: Option<&str>,
                 group: Option<&str>,
                 source: &str,
                 actions: Vec<Action>,
                 resources: Option<Vec<u16>>| GrantConfig {
        issuer: "urn:glaux:development".to_owned(),
        subject: subject.map(str::to_owned),
        group: group.map(str::to_owned),
        source: source.to_owned(),
        actions,
        resources: resources.map(|items| {
            items
                .into_iter()
                .map(|number| id(number).to_string())
                .collect()
        }),
    };
    PolicyConfig {
        grants: vec![
            grant(
                None,
                Some("group-a"),
                SOURCE_A,
                vec![Action::Read],
                Some(vec![101, 102, 104, 151, 201, 301]),
            ),
            // Sources and resource IDs must stay paired, not form a cross-product.
            grant(
                None,
                Some("group-a"),
                SOURCE_B,
                vec![Action::Read],
                Some(vec![101]),
            ),
            grant(
                Some("development-alice"),
                None,
                SOURCE_A,
                vec![Action::Create],
                None,
            ),
            grant(
                Some("development-alice"),
                None,
                SOURCE_A,
                vec![Action::Update],
                Some(vec![102]),
            ),
            grant(
                Some("development-alice"),
                None,
                SOURCE_A,
                vec![Action::SubmitCommand],
                Some(vec![101]),
            ),
            grant(
                Some("development-bob"),
                None,
                SOURCE_B,
                vec![Action::Read],
                None,
            ),
        ],
        denial_audit: limits(100, 100),
    }
}

fn audit(actor: &str, source: &str) -> AuditContext {
    AuditContext {
        actor: Some(actor.to_owned()),
        source: Some(source.to_owned()),
        time: TIME.parse().unwrap(),
        correlation: "untrusted-request-correlation".to_owned(),
    }
}

fn create(number: u16, source: &str, label: &str, parent: Option<u16>) -> CreateSystem {
    CreateSystem {
        system: SystemRecord {
            id: id(number),
            uid: format!("urn:glaux:test:permission-system-{number}")
                .parse()
                .unwrap(),
            label: label.to_owned(),
            parent: parent.map(id),
            sources: vec![SourceIdentity::new(
                source.parse().unwrap(),
                format!("upstream-{number}").parse().unwrap(),
            )],
        },
        artifact: NewSourceArtifact {
            id: id(1000 + number).to_string().parse().unwrap(),
            media_type: "application/json".to_owned(),
            bytes: br#"{"assertedProducer":"SyntheticAuthorizationSecretCanary"}"#.to_vec(),
        },
        revision: SystemRevision {
            id: id(2000 + number).to_string().parse().unwrap(),
            system_id: id(number),
            artifact_id: id(1000 + number).to_string().parse().unwrap(),
            semantic_time: None,
            receipt_time: TIME.parse().unwrap(),
        },
        audit_id: id(3000 + number).to_string().parse().unwrap(),
        event_id: id(4000 + number).to_string().parse().unwrap(),
        audit: audit("forged-request-actor", SOURCE_B),
    }
}

fn update(number: u16) -> UpdateSystem {
    let input = create(501, SOURCE_B, "Updated by Alice", None);
    let mut revision = input.revision;
    revision.system_id = id(number);
    UpdateSystem {
        system_id: id(number),
        label: "Updated by Alice".to_owned(),
        artifact: input.artifact,
        revision,
        audit_id: input.audit_id,
        event_id: input.event_id,
        audit: input.audit,
        expected_revision: None,
    }
}

struct Shared {
    admission: RwLock<Admission>,
    policy: Arc<ControlledPolicy>,
    clock: Arc<TestClock>,
}
impl Shared {
    fn admission(&self) -> Admission {
        self.admission.read().unwrap().clone()
    }
    fn reset_limiter(&self, bounds: DenialLimits, now: u64) {
        self.clock.set(Some(now));
        *self.admission.write().unwrap() =
            Admission::new(self.policy.clone(), bounds, self.clock.clone()).unwrap();
    }
}

struct Endpoint {
    shared: Arc<Shared>,
    connection: AsyncMutex<PgConnection>,
}

fn context(caller: CallerContext) -> OperationContext {
    OperationContext::new(caller, TIME.parse().unwrap()).unwrap()
}

fn representation(system: &SystemRecord) -> Value {
    let mut links = vec![json!({"rel":"self","href":format!("/items/{}", system.id)})];
    if let Some(parent) = system.parent {
        links.push(json!({"rel":"parent","href":format!("/items/{parent}")}));
    }
    json!({"id":system.id.to_string(),"uid":system.uid.as_str(),"label":system.label,"links":links})
}

async fn list(
    Extension(caller): Extension<CallerContext>,
    State(state): State<Arc<Endpoint>>,
) -> Result<Response, AccessError> {
    list_limit(caller, state, 20).await
}

async fn first(
    Extension(caller): Extension<CallerContext>,
    State(state): State<Arc<Endpoint>>,
) -> Result<Response, AccessError> {
    list_limit(caller, state, 1).await
}

async fn list_limit(
    caller: CallerContext,
    state: Arc<Endpoint>,
    limit: u16,
) -> Result<Response, AccessError> {
    let ctx = context(caller);
    let mut connection = state.connection.lock().await;
    let page = state
        .shared
        .admission()
        .list_systems(&mut connection, &ctx, limit)
        .await?;
    Ok(json_response(
        &json!({"items":page.items.iter().map(representation).collect::<Vec<_>>(),
        "numberMatched":page.number_matched}),
        "application/json",
    )
    .unwrap())
}

async fn get_item(
    Extension(caller): Extension<CallerContext>,
    State(state): State<Arc<Endpoint>>,
    Path(identifier): Path<String>,
) -> Result<Response, AccessError> {
    let ctx = context(caller);
    let mut connection = state.connection.lock().await;
    let system = state
        .shared
        .admission()
        .get_system(&mut connection, &ctx, identifier.parse().unwrap())
        .await?;
    Ok(json_response(&representation(&system), "application/json").unwrap())
}

async fn create_item(
    Extension(caller): Extension<CallerContext>,
    State(state): State<Arc<Endpoint>>,
    request: Request,
) -> Result<Response, AccessError> {
    assert!(!request.headers().contains_key("authorization"));
    let body = HttpBoundary::new(None, Limits::default())
        .unwrap()
        .read_json(request)
        .await
        .expect("fixed synthetic body must pass the bounded JSON boundary");
    let ctx = context(caller);
    let number = u16::try_from(body["number"].as_u64().unwrap()).unwrap();
    let source = body["source"].as_str().unwrap();
    let parent = body["parent"].as_u64().map(|n| u16::try_from(n).unwrap());
    let mut input = create(number, source, "Created by Alice", parent);
    if let Some(candidate) = body["candidate_number"].as_u64() {
        let candidate = u16::try_from(candidate).unwrap();
        input.system.id = id(candidate);
        input.artifact.id = id(1000 + candidate).to_string().parse().unwrap();
        input.revision.id = id(2000 + candidate).to_string().parse().unwrap();
        input.revision.system_id = id(candidate);
        input.revision.artifact_id = input.artifact.id;
        input.audit_id = id(3000 + candidate).to_string().parse().unwrap();
        input.event_id = id(4000 + candidate).to_string().parse().unwrap();
    }
    let retry = body["retry"].as_bool().unwrap_or(false).then(|| RetryKey {
        key: body["retry_key"]
            .as_str()
            .unwrap_or("fixture-retry-22")
            .to_owned(),
        retention_seconds: 60,
    });
    let mut connection = state.connection.lock().await;
    let receipt = state
        .shared
        .admission()
        .create_system(&mut connection, &ctx, source, input, retry.as_ref())
        .await?;
    let mut response = json_response(
        &json!({"id":receipt.system_id.to_string(),
        "revision":receipt.revision_id.to_string(),"correlation":ctx.correlation()}),
        "application/json",
    )
    .unwrap();
    *response.status_mut() = StatusCode::CREATED;
    Ok(response)
}

async fn update_item(
    Extension(caller): Extension<CallerContext>,
    State(state): State<Arc<Endpoint>>,
    Path(identifier): Path<String>,
) -> Result<Response, AccessError> {
    let ctx = context(caller);
    let number: u16 = identifier.parse().unwrap();
    let mut connection = state.connection.lock().await;
    let receipt = state
        .shared
        .admission()
        .update_system(&mut connection, &ctx, update(number))
        .await?;
    Ok(json_response(
        &json!({"id":receipt.system_id.to_string(),
        "revision":receipt.revision_id.to_string(),"correlation":ctx.correlation()}),
        "application/json",
    )
    .unwrap())
}

async fn authority(
    Extension(caller): Extension<CallerContext>,
    State(state): State<Arc<Endpoint>>,
) -> Response {
    let report = state
        .shared
        .policy
        .permissions(&caller, Action::ReportStatus)
        .unwrap();
    let submit = state
        .shared
        .policy
        .permissions(&caller, Action::SubmitCommand)
        .unwrap();
    json_response(
        &json!({"submit":submit.allows(SOURCE_A,id(101)),
        "report":report.allows(SOURCE_A,id(101)),"subject":caller.subject()}),
        "application/json",
    )
    .unwrap()
}

struct Listener {
    address: SocketAddr,
    shutdown: oneshot::Sender<()>,
    task: tokio::task::JoinHandle<std::io::Result<()>>,
}

async fn listener(shared: Arc<Shared>, subject: &str, group: &str) -> Listener {
    let mut connection = PgConnection::connect(DSN).await.unwrap();
    execute(&mut connection, "SET ROLE authorization_serving").await;
    execute(&mut connection, "SET statement_timeout=5000").await;
    execute(&mut connection, "SET lock_timeout=1000").await;
    let bound = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = bound.local_addr().unwrap();
    let auth = Authenticator::development(
        DevelopmentConfig {
            subject: subject.to_owned(),
            groups: vec![group.to_owned()],
            scopes: vec!["read".to_owned(), "write".to_owned()],
        },
        address,
    )
    .unwrap();
    let routes = Router::new()
        .route("/items", get(list))
        .route("/first", get(first))
        .route("/items/{identifier}", get(get_item))
        .route("/create", post(create_item))
        .route("/update/{identifier}", post(update_item))
        .route("/authority", get(authority))
        .with_state(Arc::new(Endpoint {
            shared,
            connection: AsyncMutex::new(connection),
        }));
    let routes = HttpBoundary::new(None, Limits::default())
        .unwrap()
        .router(auth.protect(routes));
    let (shutdown, stopped) = oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(
            bound,
            routes.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
    });
    Listener {
        address,
        shutdown,
        task,
    }
}

#[derive(Clone, Debug)]
struct Wire {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}
impl Wire {
    fn header(&self, name: &str) -> Option<&str> {
        let found: Vec<_> = self.headers.iter().filter(|(key, _)| key == name).collect();
        assert!(found.len() <= 1, "unexpected repeated response header");
        found.first().map(|(_, value)| value.as_str())
    }
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

fn decode(bytes: &[u8]) -> Wire {
    let end = bytes.windows(4).position(|v| v == b"\r\n\r\n").unwrap();
    let mut lines = std::str::from_utf8(&bytes[..end]).unwrap().split("\r\n");
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
            let (key, value) = line.split_once(':').unwrap();
            (key.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    let mut wire = Wire {
        status,
        headers,
        body: bytes[end + 4..].to_vec(),
    };
    if wire.header("transfer-encoding") == Some("chunked") {
        let mut remaining = wire.body.as_slice();
        let mut output = Vec::new();
        loop {
            let end = remaining.windows(2).position(|v| v == b"\r\n").unwrap();
            let size =
                usize::from_str_radix(std::str::from_utf8(&remaining[..end]).unwrap(), 16).unwrap();
            remaining = &remaining[end + 2..];
            if size == 0 {
                assert_eq!(remaining, b"\r\n");
                break;
            }
            assert!(remaining.len() >= size + 2);
            output.extend_from_slice(&remaining[..size]);
            assert_eq!(&remaining[size..size + 2], b"\r\n");
            remaining = &remaining[size + 2..];
        }
        wire.body = output;
    } else if let Some(length) = wire.header("content-length") {
        assert_eq!(wire.body.len(), length.parse::<usize>().unwrap());
    }
    wire
}

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Option<Value>,
    extra: &str,
) -> Wire {
    let method = method.to_owned();
    let path = path.to_owned();
    let extra = extra.to_owned();
    tokio::task::spawn_blocking(move || {
        let mut socket=TcpStream::connect_timeout(&address,Duration::from_secs(3)).unwrap();
        socket.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        socket.set_write_timeout(Some(Duration::from_secs(3))).unwrap();
        let body=body.map(|value|value.to_string()).unwrap_or_default();
        let bytes=format!("{method} {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{extra}\r\n{body}",body.len());
        socket.write_all(bytes.as_bytes()).unwrap();
        let mut response=Vec::new();socket.take(65537).read_to_end(&mut response).unwrap();
        assert!(response.len()<=65536,"HTTP response escaped finite bound");decode(&response)
    }).await.unwrap()
}

fn expected_item(number: u16, label: &str, parent: Option<u16>) -> Value {
    let mut links = vec![
        json!({"rel":"self","href":format!("/items/01890f20-7b5a-7cc3-98c4-dc0c0c22{number:04}")}),
    ];
    if let Some(parent) = parent {
        links.push(json!({"rel":"parent","href":format!("/items/01890f20-7b5a-7cc3-98c4-dc0c0c22{parent:04}")}));
    }
    json!({"id":format!("01890f20-7b5a-7cc3-98c4-dc0c0c22{number:04}"),
        "uid":format!("urn:glaux:test:permission-system-{number}"),"label":label,"links":links})
}

fn expected_a() -> Value {
    json!({"items":[expected_item(101,"A one",None),expected_item(102,"A two",Some(101))],"numberMatched":2})
}
fn matches_list(wire: &Wire, expected: &Value) -> bool {
    wire.status == 200
        && wire.header("content-type") == Some("application/json")
        && wire.json() == *expected
}

fn oracle_controls() {
    let good = Wire {
        status: 200,
        headers: vec![("content-type".into(), "application/json".into())],
        body: expected_a().to_string().into_bytes(),
    };
    assert!(matches_list(&good, &expected_a()));
    for changed in ["wrong-id", "extra-hidden", "wrong-count", "missing-field"] {
        let mut wrong = good.clone();
        let mut value = wrong.json();
        match changed {
            "wrong-id" => value["items"][0]["id"] = json!("01890f20-7b5a-7cc3-98c4-dc0c0c220201"),
            "extra-hidden" => {
                value["items"]
                    .as_array_mut()
                    .unwrap()
                    .push(expected_item(201, "B private", None))
            }
            "wrong-count" => value["numberMatched"] = json!(3),
            _ => {
                value["items"][0].as_object_mut().unwrap().remove("label");
            }
        }
        wrong.body = value.to_string().into_bytes();
        assert!(
            !matches_list(&wrong, &expected_a()),
            "known-bad authorization response escaped oracle"
        );
    }
    let mut wrong = good;
    wrong.status = 403;
    assert!(!matches_list(&wrong, &expected_a()));
    passed("independent-wire-oracle-controls");
}

fn problem(wire: &Wire, status: u16) -> Value {
    assert_eq!(
        wire.status, status,
        "unexpected safe policy status: {wire:?}"
    );
    assert_eq!(
        wire.header("content-type"),
        Some("application/problem+json")
    );
    assert_eq!(wire.header("cache-control"), Some("private, no-store"));
    assert_eq!(wire.header("www-authenticate"), None);
    let (slug, title, detail) = match status {
        403 => ("forbidden", "Forbidden", "The operation is not permitted."),
        404 => (
            "not-found",
            "Not Found",
            "The requested resource is unavailable.",
        ),
        503 => (
            "unavailable",
            "Service Unavailable",
            "The operation is temporarily unavailable.",
        ),
        _ => panic!("unexpected fixture status"),
    };
    let value = wire.json();
    let correlation = value["correlation"].as_str().unwrap();
    assert!(!correlation.is_empty() && correlation != "untrusted-request-correlation");
    assert_eq!(wire.header("x-request-id"), Some(correlation));
    assert_eq!(
        value,
        json!({"type":format!("urn:glaux:problem:{slug}"),"title":title,
        "status":status,"detail":detail,"correlation":correlation})
    );
    let text = std::str::from_utf8(&wire.body).unwrap();
    for secret in [
        CANARY,
        SOURCE_A,
        SOURCE_B,
        "development-alice",
        "development-bob",
        "forged-request-actor",
        "stack trace",
        "permission denied",
    ] {
        assert!(
            !text.contains(secret),
            "policy problem disclosed protected context"
        );
    }
    let mut comparable = value;
    comparable.as_object_mut().unwrap().remove("correlation");
    comparable
}

async fn execute(connection: &mut PgConnection, sql: &str) {
    // Callers supply only fixed fixture statements, never request values.
    sqlx::query(AssertSqlSafe(sql))
        .execute(connection)
        .await
        .unwrap();
}

async fn snapshot(connection: &mut PgConnection) -> Value {
    let text:String=sqlx::query_scalar("SELECT json_build_object(
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

fn without_audit(mut snapshot: Value) -> Value {
    snapshot.as_object_mut().unwrap().remove("audit");
    snapshot
}

async fn clear_denials(connection: &mut PgConnection) {
    execute(
        connection,
        "ALTER TABLE public.server_audit DISABLE TRIGGER server_audit_immutable",
    )
    .await;
    execute(
        connection,
        "DELETE FROM public.server_audit WHERE outcome='denied'",
    )
    .await;
    execute(
        connection,
        "ALTER TABLE public.server_audit ENABLE TRIGGER server_audit_immutable",
    )
    .await;
}

async fn denial_count(connection: &mut PgConnection) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM public.server_audit WHERE outcome='denied'")
        .fetch_one(connection)
        .await
        .unwrap()
}

async fn assert_denial(connection: &mut PgConnection, correlation: &str) {
    type DenialRow = (
        Option<String>,
        Option<String>,
        String,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
    );
    let row: DenialRow = sqlx::query_as(
        "SELECT actor,source,operation,target_id::text,revision_id::text,time_source,outcome,correlation
         FROM public.server_audit WHERE outcome='denied' ORDER BY id LIMIT 1").fetch_one(connection).await.unwrap();
    assert_eq!(
        row,
        (
            Some("[\"urn:glaux:development\",\"development-alice\",\"development\"]".to_owned()),
            None,
            "system.create".to_owned(),
            None,
            None,
            TIME.to_owned(),
            "denied".to_owned(),
            correlation.to_owned()
        ),
        "retained denial contains unsafe or inaccurate attribution"
    );
}

async fn assert_update_denial(connection: &mut PgConnection, correlation: &str, visible: bool) {
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT json_build_object('actor',actor,'source',source,'operation',operation,
         'target',target_id::text,'revision',revision_id::text,'time',time_source,
         'outcome',outcome,'correlation',correlation)::text
         FROM public.server_audit WHERE correlation=$1",
    )
    .bind(correlation)
    .fetch_all(connection)
    .await
    .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "eligible update denial was not retained exactly once"
    );
    let actual: Value = serde_json::from_str(&rows[0]).unwrap();
    assert_eq!(
        actual,
        json!({
            "actor":"[\"urn:glaux:development\",\"development-alice\",\"development\"]",
            "source":if visible {Some(SOURCE_A)} else {None},
            "operation":"system.update",
            "target":if visible {Some("01890f20-7b5a-7cc3-98c4-dc0c0c220101")} else {None},
            "revision":null,"time":TIME,"outcome":"denied","correlation":correlation,
        }),
        "denied update lost safe context or retained concealed target context"
    );
}

fn create_body(number: u16, source: &str, parent: Option<u16>, retry: bool) -> Value {
    json!({"number":number,"source":source,"parent":parent,"retry":retry,
        "actor":"forged-request-actor","group":"administrators","producer":CANARY,"statusReporter":SOURCE_B})
}

fn passed(group: &str) {
    println!("Authorization group passed: {group}");
}

async fn seed(connection: &mut PgConnection) {
    migrate(connection).await.unwrap();
    for (number, source, label, parent) in [
        (100, SOURCE_A, "A lower-sorting hidden", None),
        (101, SOURCE_A, "A one", None),
        (102, SOURCE_A, "A two", Some(101)),
        (103, SOURCE_A, "A hidden", None),
        (104, SOURCE_A, "A hidden-parent child", Some(103)),
        (201, SOURCE_B, "B private", None),
    ] {
        let mut input = create(number, source, label, parent);
        input.audit = audit("fixture-seed", source);
        // Conflicting source aliases are provenance, not accepted ownership.
        input.system.sources.push(SourceIdentity::new(
            if source == SOURCE_A {
                SOURCE_B
            } else {
                SOURCE_A
            }
            .parse()
            .unwrap(),
            format!("asserted-cross-source-{number}").parse().unwrap(),
        ));
        glaux_server::application::create_system(connection, &input)
            .await
            .unwrap();
    }
    SystemRepository::create(
        connection,
        &create(301, SOURCE_A, "No accepted owner", None).system,
    )
    .await
    .unwrap();
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT id::text,label FROM public.system_identity ORDER BY id")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    assert_eq!(
        rows,
        vec![
            (id(100).to_string(), "A lower-sorting hidden".to_owned()),
            (id(101).to_string(), "A one".to_owned()),
            (id(102).to_string(), "A two".to_owned()),
            (id(103).to_string(), "A hidden".to_owned()),
            (id(104).to_string(), "A hidden-parent child".to_owned()),
            (id(201).to_string(), "B private".to_owned()),
            (id(301).to_string(), "No accepted owner".to_owned())
        ]
    );
    for statement in [
        "CREATE ROLE authorization_serving NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT",
        "GRANT USAGE ON SCHEMA public TO authorization_serving",
        "GRANT SELECT ON public._sqlx_migrations TO authorization_serving",
        "GRANT SELECT,INSERT ON public.resource_identity,public.system_identity,public.source_identity,public.system_parent,
          public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work,public.system_write_head,
          public.system_create_retry TO authorization_serving",
        "GRANT UPDATE ON public.system_identity,public.system_write_head TO authorization_serving",
        "GRANT UPDATE(digest,system_id,revision_id,artifact_id,audit_id,event_id,retained_at,expires_at) ON public.system_create_retry TO authorization_serving",
        "GRANT SELECT,UPDATE ON public.system_parent_write_guard TO authorization_serving",
    ] {execute(connection,statement).await;}
}

async fn exact_queries(connection: &mut PgConnection, a: SocketAddr, b: SocketAddr) -> Value {
    let before = snapshot(connection).await;
    let wire = request(a, "GET", "/items", None, "").await;
    assert!(
        matches_list(&wire, &expected_a()),
        "allowed source query did not return exact authorized resources: {wire:?}"
    );
    assert_eq!(wire.header("cache-control"), Some("private, no-store"));
    assert!(
        matches_list(
            &request(a, "GET", "/first", None, "").await,
            &json!({"items":[expected_item(101,"A one",None)],"numberMatched":2})
        ),
        "limiting preceded authorization or changed authorized count"
    );
    let expected_b = json!({"items":[expected_item(201,"B private",None)],"numberMatched":1});
    assert!(matches_list(
        &request(b, "GET", "/items", None, "").await,
        &expected_b
    ));
    for number in [101, 102] {
        let wire = request(a, "GET", &format!("/items/{}", id(number)), None, "").await;
        assert_eq!(wire.status, 200);
        assert_eq!(
            wire.json(),
            expected_a()["items"][usize::from(number - 101)]
        );
    }
    let mut hidden = None;
    for number in [100, 103, 104, 201, 301, 999] {
        let wire = request(a, "GET", &format!("/items/{}", id(number)), None, "").await;
        let safe = problem(&wire, 404);
        if let Some(expected) = &hidden {
            assert_eq!(&safe, expected);
        } else {
            hidden = Some(safe);
        }
    }
    assert_eq!(
        snapshot(connection).await,
        before,
        "read admission mutated database or audited ordinary denials"
    );
    execute(connection,"UPDATE public.system_identity SET label='Changed hidden world' WHERE id IN
        ('01890f20-7b5a-7cc3-98c4-dc0c0c220103','01890f20-7b5a-7cc3-98c4-dc0c0c220104','01890f20-7b5a-7cc3-98c4-dc0c0c220201')").await;
    assert!(
        matches_list(&request(a, "GET", "/items", None, "").await, &expected_a()),
        "hidden changes altered visible resources/count/links"
    );
    let hidden_after = problem(
        &request(a, "GET", &format!("/items/{}", id(201)), None, "").await,
        404,
    );
    assert_eq!(Some(hidden_after), hidden);
    passed("exact-authorized-queries");
    expected_a()
}

async fn accepted_and_cross_source(
    connection: &mut PgConnection,
    shared: &Shared,
    a: SocketAddr,
) -> Value {
    let wire=request(a,"POST","/create",Some(create_body(151,SOURCE_A,Some(101),true)),
        "X-User: forged-request-actor\r\nX-Groups: administrators\r\nX-Source: urn:glaux:test:source-b\r\n").await;
    assert_eq!(wire.status, 201, "authorized creation failed");
    let value = wire.json();
    assert_eq!(value["id"], json!(id(151).to_string()));
    assert_eq!(value["revision"], json!(id(2151).to_string()));
    let correlation = value["correlation"].as_str().unwrap();
    let actual:(String,String,String,String,String,String,String,String,String,String)=sqlx::query_as(
        "SELECT r.uid,s.label,p.parent_id::text,a.actor,a.source,a.correlation,a.time_source,
                w.system_id::text,w.revision_id::text,w.artifact_id::text
         FROM public.resource_identity r JOIN public.system_identity s USING(id)
         JOIN public.system_parent p ON p.child_id=r.id
         JOIN public.server_audit a ON a.target_id=r.id AND a.operation='system.create' AND a.outcome='accepted'
         JOIN public.outgoing_work w ON w.audit_id=a.id
         WHERE r.id='01890f20-7b5a-7cc3-98c4-dc0c0c220151'").fetch_one(&mut *connection).await.unwrap();
    assert_eq!(
        actual,
        (
            "urn:glaux:test:permission-system-151".to_owned(),
            "Created by Alice".to_owned(),
            id(101).to_string(),
            "[\"urn:glaux:development\",\"development-alice\",\"development\"]".to_owned(),
            SOURCE_A.to_owned(),
            correlation.to_owned(),
            TIME.to_owned(),
            id(151).to_string(),
            id(2151).to_string(),
            id(1151).to_string()
        ),
        "accepted audit or outgoing binding trusted forged context"
    );
    let before = snapshot(connection).await;
    let retry = request(
        a,
        "POST",
        "/create",
        Some(create_body(151, SOURCE_A, Some(101), true)),
        "",
    )
    .await;
    assert_eq!(retry.status, 201);
    assert_eq!(retry.json()["revision"], json!(id(2151).to_string()));
    assert_eq!(
        snapshot(connection).await,
        before,
        "authorized retry created duplicate resource/audit/work"
    );
    let mut narrowed = policy();
    for grant in &mut narrowed.grants {
        if grant.actions.contains(&Action::Create) {
            grant.resources = Some(vec![id(101).to_string(), id(151).to_string()]);
        }
    }
    *shared.policy.policy.write().unwrap() = ConfiguredPolicy::new(narrowed).unwrap();
    let mut renewed_candidate = create_body(151, SOURCE_A, Some(101), true);
    renewed_candidate["candidate_number"] = json!(152);
    let replay = request(a, "POST", "/create", Some(renewed_candidate), "").await;
    assert_eq!(
        replay.status, 201,
        "retry authorized candidate instead of its original receipt"
    );
    assert_eq!(replay.json()["id"], json!(id(151).to_string()));
    assert_eq!(
        snapshot(connection).await,
        before,
        "new retry candidate created state or accountability"
    );
    let mut fresh_key = create_body(151, SOURCE_A, Some(101), true);
    fresh_key["candidate_number"] = json!(152);
    fresh_key["retry_key"] = json!("new-key-without-candidate-permission");
    let fresh = request(a, "POST", "/create", Some(fresh_key), "").await;
    assert_eq!(
        fresh.status, 403,
        "fresh retry key bypassed candidate permission"
    );
    problem(&fresh, 403);
    assert_eq!(
        without_audit(snapshot(connection).await),
        without_audit(before.clone()),
        "denied fresh retry key created resource, receipt or outgoing work"
    );
    *shared.policy.policy.write().unwrap() = ConfiguredPolicy::new(policy()).unwrap();
    let denied = request(
        a,
        "POST",
        "/create",
        Some(create_body(251, SOURCE_B, None, false)),
        "",
    )
    .await;
    assert_eq!(
        denied.status, 403,
        "cross-source create escaped authorization"
    );
    problem(&denied, 403);
    assert_eq!(
        without_audit(snapshot(connection).await),
        without_audit(before),
        "cross-source creation changed application state"
    );
    passed("accepted-write-and-source-boundary");
    json!({"items":[expected_item(101,"A one",None),expected_item(102,"A two",Some(101)),expected_item(151,"Created by Alice",Some(101))],"numberMatched":3})
}

async fn action_and_assertions(
    connection: &mut PgConnection,
    a: SocketAddr,
    b: SocketAddr,
) -> Value {
    let authority = request(
        a,
        "GET",
        "/authority",
        None,
        "X-Status-Reporter: urn:glaux:test:source-a\r\n",
    )
    .await;
    assert_eq!(authority.status, 200);
    assert_eq!(
        authority.json(),
        json!({"submit":true,"report":false,"subject":"development-alice"}),
        "identity or submission implied reporting authority"
    );
    let before = snapshot(connection).await;
    problem(
        &request(
            b,
            "POST",
            "/create",
            Some(create_body(252, SOURCE_B, None, false)),
            "",
        )
        .await,
        403,
    );
    for number in [101, 201, 999] {
        let denied = request(a, "POST", &format!("/update/{number}"), None, "").await;
        problem(&denied, if number == 101 { 403 } else { 404 });
        assert_update_denial(
            connection,
            denied.json()["correlation"].as_str().unwrap(),
            number == 101,
        )
        .await;
    }
    assert_eq!(
        without_audit(snapshot(connection).await),
        without_audit(before),
        "denied action/resource update mutated state"
    );
    let changed = request(a, "POST", "/update/102", None, "").await;
    assert_eq!(changed.status, 200);
    let rows:Vec<(String,String,String)>=sqlx::query_as("SELECT actor,source,correlation FROM public.server_audit
        WHERE target_id='01890f20-7b5a-7cc3-98c4-dc0c0c220102' AND operation='system.update' AND outcome='accepted'")
        .fetch_all(&mut *connection).await.unwrap();
    assert_eq!(
        rows,
        vec![(
            "[\"urn:glaux:development\",\"development-alice\",\"development\"]".to_owned(),
            SOURCE_A.to_owned(),
            changed.json()["correlation"].as_str().unwrap().to_owned()
        )]
    );
    let expected = json!({"items":[expected_item(101,"A one",None),expected_item(102,"Updated by Alice",Some(101)),expected_item(151,"Created by Alice",Some(101))],"numberMatched":3});
    assert!(matches_list(
        &request(a, "GET", "/items", None, "").await,
        &expected
    ));
    passed("action-resource-and-asserted-authority");
    expected
}

async fn unavailable_revocation(
    connection: &mut PgConnection,
    shared: &Shared,
    a: SocketAddr,
    expected: &Value,
) {
    let before = snapshot(connection).await;
    shared.policy.unavailable.store(true, Ordering::SeqCst);
    problem(&request(a, "GET", "/items", None, "").await, 503);
    problem(
        &request(a, "GET", &format!("/items/{}", id(101)), None, "").await,
        503,
    );
    problem(
        &request(
            a,
            "POST",
            "/create",
            Some(create_body(152, SOURCE_A, None, false)),
            "",
        )
        .await,
        503,
    );
    assert_eq!(
        snapshot(connection).await,
        before,
        "policy outage admitted or recorded unverified operation"
    );
    problem(&request(a, "POST", "/update/102", None, "").await, 503);
    assert_eq!(
        snapshot(connection).await,
        before,
        "policy outage admitted an update"
    );
    shared.policy.unavailable.store(false, Ordering::SeqCst);
    assert!(matches_list(
        &request(a, "GET", "/items", None, "").await,
        expected
    ));
    let mut revoked = policy();
    // Keep the parent's read permission. Only the original receipt is revoked,
    // so the replay's own read check, not a parent lookup, must prevent disclosure.
    for grant in &mut revoked.grants {
        if grant.actions.contains(&Action::Read)
            && let Some(resources) = &mut grant.resources
        {
            resources.retain(|resource| resource != &id(151).to_string());
        }
    }
    *shared.policy.policy.write().unwrap() = ConfiguredPolicy::new(revoked).unwrap();
    problem(
        &request(a, "GET", &format!("/items/{}", id(151)), None, "").await,
        404,
    );
    let replay = request(
        a,
        "POST",
        "/create",
        Some(create_body(151, SOURCE_A, Some(101), true)),
        "",
    )
    .await;
    assert_eq!(
        replay.status, 403,
        "revoked read permission leaked retry receipt"
    );
    problem(&replay, 403);
    let mut revoked_create = policy();
    for grant in &mut revoked_create.grants {
        if grant.actions.contains(&Action::Create) {
            grant.resources = Some(vec![id(101).to_string()]);
        }
    }
    *shared.policy.policy.write().unwrap() = ConfiguredPolicy::new(revoked_create).unwrap();
    let replay = request(
        a,
        "POST",
        "/create",
        Some(create_body(151, SOURCE_A, Some(101), true)),
        "",
    )
    .await;
    assert_eq!(
        replay.status, 403,
        "revoked create permission leaked retry receipt"
    );
    problem(&replay, 403);
    assert_eq!(
        without_audit(snapshot(connection).await),
        without_audit(before),
        "revoked retry mutated application state"
    );
    *shared.policy.policy.write().unwrap() = ConfiguredPolicy::new(policy()).unwrap();
    passed("policy-unavailable-and-revocation");
}

async fn denial_bounds(connection: &mut PgConnection, shared: &Shared, a: SocketAddr) {
    clear_denials(connection).await;
    shared.reset_limiter(limits(2, 1), 100);
    let baseline = without_audit(snapshot(connection).await);
    let first = request(
        a,
        "POST",
        "/create",
        Some(create_body(253, SOURCE_B, None, false)),
        "",
    )
    .await;
    problem(&first, 403);
    assert_eq!(denial_count(connection).await, 1);
    assert_denial(connection, first.json()["correlation"].as_str().unwrap()).await;
    assert_eq!(shared.admission().diagnostics().retained, 1);
    for _ in 0..4 {
        problem(
            &request(
                a,
                "POST",
                "/create",
                Some(create_body(253, SOURCE_B, None, false)),
                "",
            )
            .await,
            403,
        );
    }
    assert_eq!(denial_count(connection).await, 1);
    assert_eq!(shared.admission().diagnostics().rate_limited, 4);
    shared.clock.set(Some(110));
    problem(
        &request(
            a,
            "POST",
            "/create",
            Some(create_body(253, SOURCE_B, None, false)),
            "",
        )
        .await,
        403,
    );
    assert_eq!(denial_count(connection).await, 2);
    assert_eq!(shared.admission().diagnostics().retained, 2);
    shared.clock.set(Some(120));
    problem(
        &request(
            a,
            "POST",
            "/create",
            Some(create_body(253, SOURCE_B, None, false)),
            "",
        )
        .await,
        403,
    );
    assert_eq!(denial_count(connection).await, 2);
    assert_eq!(shared.admission().diagnostics().storage_limited, 1);
    assert_eq!(
        without_audit(snapshot(connection).await),
        baseline,
        "denial bounds changed non-audit state"
    );
    shared.clock.set(None);
    problem(
        &request(
            a,
            "POST",
            "/create",
            Some(create_body(253, SOURCE_B, None, false)),
            "",
        )
        .await,
        403,
    );
    assert_eq!(shared.admission().diagnostics().clock_unavailable, 1);
    assert_eq!(denial_count(connection).await, 2);
    passed("bounded-denial-audit");
}

async fn audit_failures(connection: &mut PgConnection, shared: &Shared, a: SocketAddr) {
    clear_denials(connection).await;
    shared.reset_limiter(limits(100, 100), 200);
    let before = snapshot(connection).await;
    execute(connection,"CREATE FUNCTION public.authorization_audit_failure() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'SyntheticAuthorizationSecretCanary audit-store failure'; END; $$").await;
    execute(connection,"CREATE TRIGGER authorization_denial_failure AFTER INSERT ON public.server_audit
        FOR EACH ROW WHEN (NEW.outcome='denied') EXECUTE FUNCTION public.authorization_audit_failure()").await;
    for _ in 0..3 {
        problem(
            &request(
                a,
                "POST",
                "/create",
                Some(create_body(254, SOURCE_B, None, false)),
                "",
            )
            .await,
            403,
        );
    }
    assert_eq!(
        snapshot(connection).await,
        before,
        "failed denial storage left partial audit or mutation"
    );
    assert_eq!(shared.admission().diagnostics().storage_failed, 3);
    execute(
        connection,
        "DROP TRIGGER authorization_denial_failure ON public.server_audit",
    )
    .await;
    execute(
        connection,
        "DROP FUNCTION public.authorization_audit_failure()",
    )
    .await;
    // A separate administrative connection owns the same lock namespace. The
    // recorder must report bounded contention instead of waiting or admitting.
    execute(connection, "SELECT pg_advisory_lock(1196183896,22)").await;
    problem(
        &request(
            a,
            "POST",
            "/create",
            Some(create_body(254, SOURCE_B, None, false)),
            "",
        )
        .await,
        403,
    );
    assert_eq!(snapshot(connection).await, before);
    assert_eq!(shared.admission().diagnostics().storage_busy, 1);
    let released: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(1196183896,22)")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(released);
    problem(
        &request(
            a,
            "POST",
            "/create",
            Some(create_body(254, SOURCE_B, None, false)),
            "",
        )
        .await,
        403,
    );
    assert_eq!(denial_count(connection).await, 1);
    assert_eq!(shared.admission().diagnostics().retained, 1);
    assert_eq!(
        without_audit(snapshot(connection).await),
        without_audit(before)
    );
    passed("audit-storage-failure-and-busy");
}

async fn scenario(mut connection: PgConnection, shared: Arc<Shared>, a: SocketAddr, b: SocketAddr) {
    oracle_controls();
    exact_queries(&mut connection, a, b).await;
    accepted_and_cross_source(&mut connection, &shared, a).await;
    let expected = action_and_assertions(&mut connection, a, b).await;
    unavailable_revocation(&mut connection, &shared, a, &expected).await;
    denial_bounds(&mut connection, &shared, a).await;
    audit_failures(&mut connection, &shared, a).await;
    assert!(matches_list(
        &request(a, "GET", "/items", None, "").await,
        &expected
    ));
    let before = snapshot(&mut connection).await;
    let invalid = request(
        a,
        "GET",
        "/items",
        None,
        "Authorization: Bearer SyntheticAuthorizationSecretCanary\r\n",
    )
    .await;
    assert_eq!(invalid.status, 401);
    assert_eq!(invalid.header("cache-control"), Some("no-store"));
    assert!(!String::from_utf8(invalid.body).unwrap().contains(CANARY));
    assert_eq!(snapshot(&mut connection).await, before);
}

async fn proof() {
    let mut connection = PgConnection::connect(DSN).await.unwrap();
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
    execute(&mut connection, "SET lock_timeout=1000").await;
    seed(&mut connection).await;
    let policy = Arc::new(ControlledPolicy {
        policy: RwLock::new(ConfiguredPolicy::new(policy()).unwrap()),
        unavailable: AtomicBool::new(false),
    });
    let clock = Arc::new(TestClock(Mutex::new(Some(Duration::from_secs(100)))));
    let admission = Admission::new(policy.clone(), limits(100, 100), clock.clone()).unwrap();
    let shared = Arc::new(Shared {
        admission: RwLock::new(admission),
        policy,
        clock,
    });
    let alice = listener(shared.clone(), "development-alice", "group-a").await;
    let bob = listener(shared.clone(), "development-bob", "group-b").await;
    let mut scenario_task = tokio::spawn(scenario(connection, shared, alice.address, bob.address));
    let result = tokio::time::timeout(Duration::from_secs(80), &mut scenario_task).await;
    if result.is_err() {
        scenario_task.abort();
        let _ = scenario_task.await;
    }
    for listener in [alice, bob] {
        listener.shutdown.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), listener.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(
            TcpStream::connect_timeout(&listener.address, Duration::from_millis(200)).is_err(),
            "owned HTTP listener survived shutdown"
        );
    }
    match result.expect("bounded authorization proof timed out") {
        Ok(()) => {}
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => panic!("authorization scenario cancelled: {error}"),
    }
    passed("restored-policy-and-clean-shutdown");
    println!("{FINAL}");
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
        .block_on(proof());
}
