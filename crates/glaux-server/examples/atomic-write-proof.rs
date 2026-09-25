//! Real SQLx application boundary, fixed independent facts, no external target.

use std::time::Duration;

use glaux_domain::identity::{LocalId, SourceIdentity};
use glaux_server::application::{
    AuditContext, AuditId, CreateSystem, DeniedSystemCreate, EventId, create_system,
    record_denied_system_create,
};
use glaux_server::revisions::{NewSourceArtifact, SystemRevision};
use glaux_server::storage::{
    StorageError, SystemRecord, SystemRepository, migrate, packaged_migrations,
};
use sqlx::{Connection, PgConnection, Row};

const DSN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const BYTES: &[u8] = b"{\"type\":\"PhysicalSystem\",\"label\":\"Alpha\",\"value\":1}";
const DIGEST: &str = "8d3e448241a86daedf90f6ea36eebbc62c82829307bcb6eae1a9954c031ec215";
const SEMANTIC: &str = "1970-01-01T00:00:00.0000011Z";
const RECEIPT: &str = "2017-01-01T01:00:00.12345678901234567890+01:00";

fn id(suffix: &str) -> LocalId {
    format!("01890f20-7b5a-7cc3-98c4-dc0c0c07{suffix}")
        .parse()
        .unwrap()
}

fn audit_id(suffix: &str) -> AuditId {
    id(suffix).to_string().parse().unwrap()
}

fn event_id(suffix: &str) -> EventId {
    id(suffix).to_string().parse().unwrap()
}

fn context() -> AuditContext {
    AuditContext {
        actor: Some("fixture-actor".to_owned()),
        source: Some("fixture-source".to_owned()),
        correlation: "fixture-correlation-15".to_owned(),
        time: RECEIPT.parse().unwrap(),
    }
}

fn parent() -> SystemRecord {
    SystemRecord {
        id: id("0101"),
        uid: "urn:glaux:test:atomic-parent".parse().unwrap(),
        label: "Parent fixture".to_owned(),
        parent: None,
        sources: Vec::new(),
    }
}

fn fixture() -> CreateSystem {
    CreateSystem {
        system: SystemRecord {
            id: id("0102"),
            uid: "urn:glaux:test:atomic-child".parse().unwrap(),
            label: "Atomic child".to_owned(),
            parent: Some(id("0101")),
            sources: vec![
                SourceIdentity::new(
                    "urn:glaux:test:source-a".parse().unwrap(),
                    "upstream-17".parse().unwrap(),
                ),
                SourceIdentity::new(
                    "urn:glaux:test:source-b".parse().unwrap(),
                    "upstream-29".parse().unwrap(),
                ),
            ],
        },
        artifact: NewSourceArtifact {
            id: id("0201").to_string().parse().unwrap(),
            media_type: "application/json".to_owned(),
            bytes: BYTES.to_vec(),
        },
        revision: SystemRevision {
            id: id("0301").to_string().parse().unwrap(),
            system_id: id("0102"),
            artifact_id: id("0201").to_string().parse().unwrap(),
            semantic_time: Some(SEMANTIC.parse().unwrap()),
            receipt_time: RECEIPT.parse().unwrap(),
        },
        audit_id: audit_id("0401"),
        event_id: event_id("0501"),
        audit: context(),
    }
}

fn denial() -> DeniedSystemCreate {
    DeniedSystemCreate {
        audit_id: audit_id("0402"),
        target: None,
        audit: AuditContext {
            actor: None,
            source: None,
            correlation: "denial-correlation-15".to_owned(),
            time: RECEIPT.parse().unwrap(),
        },
    }
}

async fn execute(connection: &mut PgConnection, statement: &'static str) {
    sqlx::query(statement).execute(connection).await.unwrap();
}

fn passed(group: &str) {
    println!("Atomic write group passed: {group}");
}

async fn snapshot(connection: &mut PgConnection) -> String {
    sqlx::query_scalar(
        "SELECT json_build_object(
         'identity',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.resource_identity t),
         'system',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.system_identity t),
         'source',(SELECT coalesce(json_agg(t ORDER BY resource_id,authority,identifier),'[]') FROM public.source_identity t),
         'parent',(SELECT coalesce(json_agg(t ORDER BY child_id),'[]') FROM public.system_parent t),
         'guard',(SELECT coalesce(json_agg(t ORDER BY singleton),'[]') FROM public.system_parent_write_guard t),
         'artifact',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.source_artifact t),
         'revision',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.system_revision t),
         'audit',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.server_audit t),
         'work',(SELECT coalesce(json_agg(t ORDER BY id),'[]') FROM public.outgoing_work t),
         'head',(SELECT coalesce(json_agg(t ORDER BY system_id),'[]') FROM public.system_write_head t))::text",
    )
    .fetch_one(connection)
    .await
    .unwrap()
}

async fn reset(connection: &mut PgConnection) {
    // Privileged fixture reset, never serving or retention behavior.
    for statement in [
        "ALTER TABLE public.source_artifact DISABLE TRIGGER source_artifact_immutable",
        "ALTER TABLE public.system_revision DISABLE TRIGGER system_revision_immutable",
        "ALTER TABLE public.server_audit DISABLE TRIGGER server_audit_immutable",
        "ALTER TABLE public.outgoing_work DISABLE TRIGGER outgoing_work_immutable",
        "TRUNCATE public.system_create_retry, public.system_write_head, public.outgoing_work, public.server_audit, public.system_revision,
         public.source_artifact, public.system_parent, public.source_identity,
         public.system_identity, public.resource_identity",
        "ALTER TABLE public.source_artifact ENABLE TRIGGER source_artifact_immutable",
        "ALTER TABLE public.system_revision ENABLE TRIGGER system_revision_immutable",
        "ALTER TABLE public.server_audit ENABLE TRIGGER server_audit_immutable",
        "ALTER TABLE public.outgoing_work ENABLE TRIGGER outgoing_work_immutable",
    ] {
        execute(connection, statement).await;
    }
    SystemRepository::create(connection, &parent())
        .await
        .unwrap();
}

async fn assert_accepted(connection: &mut PgConnection) {
    // These raw queries do not use production deserializers or derive expected
    // facts from the just-returned receipt.
    let system: (String, String, String, String, String) = sqlx::query_as(
        "SELECT r.id::text,r.family,r.uid,s.label,p.parent_id::text
         FROM public.resource_identity r JOIN public.system_identity s USING(id)
         JOIN public.system_parent p ON p.child_id=r.id
         WHERE r.id='01890f20-7b5a-7cc3-98c4-dc0c0c070102'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        system,
        (
            id("0102").to_string(),
            "system".to_owned(),
            "urn:glaux:test:atomic-child".to_owned(),
            "Atomic child".to_owned(),
            id("0101").to_string()
        )
    );
    let sources: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT resource_id::text,authority,identifier FROM public.source_identity
         ORDER BY authority COLLATE \"C\",identifier COLLATE \"C\"",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        sources,
        vec![
            (
                id("0102").to_string(),
                "urn:glaux:test:source-a".to_owned(),
                "upstream-17".to_owned()
            ),
            (
                id("0102").to_string(),
                "urn:glaux:test:source-b".to_owned(),
                "upstream-29".to_owned()
            ),
        ]
    );
    let artifact: (String, String, Vec<u8>, String) = sqlx::query_as(
        "SELECT id::text,media_type,bytes,encode(digest,'hex') FROM public.source_artifact",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        artifact,
        (
            id("0201").to_string(),
            "application/json".to_owned(),
            BYTES.to_vec(),
            DIGEST.to_owned()
        )
    );
    let revision = sqlx::query(
        "SELECT id::text,system_id::text,artifact_id::text,
         semantic_civil_second,semantic_leap,semantic_fraction::text,semantic_source,
         receipt_civil_second,receipt_leap,receipt_fraction::text,receipt_source
         FROM public.system_revision",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    for (column, expected) in [
        ("id", id("0301").to_string()),
        ("system_id", id("0102").to_string()),
        ("artifact_id", id("0201").to_string()),
        ("semantic_fraction", "0.0000011".to_owned()),
        ("semantic_source", SEMANTIC.to_owned()),
        ("receipt_fraction", "0.12345678901234567890".to_owned()),
        ("receipt_source", RECEIPT.to_owned()),
    ] {
        assert_eq!(
            revision.get::<String, _>(column),
            expected,
            "revision {column}"
        );
    }
    assert_eq!(revision.get::<i64, _>("semantic_civil_second"), 0);
    assert_eq!(
        revision.get::<i64, _>("receipt_civil_second"),
        1_483_228_800
    );
    assert!(!revision.get::<bool, _>("semantic_leap"));
    assert!(!revision.get::<bool, _>("receipt_leap"));
    let audit = sqlx::query(
        "SELECT id::text,actor,source,operation,target_id::text,revision_id::text,
         time_civil_second,time_leap,time_fraction::text,time_source,outcome,correlation
         FROM public.server_audit WHERE id='01890f20-7b5a-7cc3-98c4-dc0c0c070401'",
    )
    .fetch_optional(&mut *connection)
    .await
    .unwrap();
    assert!(audit.is_some(), "required accepted audit facts missing");
    let audit = audit.unwrap();
    for (column, expected) in [
        ("id", id("0401").to_string()),
        ("actor", "fixture-actor".to_owned()),
        ("source", "fixture-source".to_owned()),
        ("operation", "system.create".to_owned()),
        ("target_id", id("0102").to_string()),
        ("revision_id", id("0301").to_string()),
        ("time_fraction", "0.12345678901234567890".to_owned()),
        ("time_source", RECEIPT.to_owned()),
        ("outcome", "accepted".to_owned()),
        ("correlation", "fixture-correlation-15".to_owned()),
    ] {
        assert_eq!(audit.get::<String, _>(column), expected, "audit {column}");
    }
    assert_eq!(audit.get::<i64, _>("time_civil_second"), 1_483_228_800);
    assert!(!audit.get::<bool, _>("time_leap"));
    let work: Option<(String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT id::text,system_id::text,revision_id::text,artifact_id::text,audit_id::text,kind,outcome
         FROM public.outgoing_work WHERE id='01890f20-7b5a-7cc3-98c4-dc0c0c070501'",
    )
    .fetch_optional(&mut *connection)
    .await
    .unwrap();
    assert!(work.is_some(), "required outgoing work facts missing");
    assert_eq!(
        work.unwrap(),
        (
            id("0501").to_string(),
            id("0102").to_string(),
            id("0301").to_string(),
            id("0201").to_string(),
            id("0401").to_string(),
            "system.created".to_owned(),
            "accepted".to_owned()
        )
    );
    let linked: (String, String, String, Vec<u8>, String) = sqlx::query_as(
        "SELECT w.system_id::text,r.id::text,a.time_source,s.bytes,encode(s.digest,'hex')
         FROM public.outgoing_work w
         JOIN public.system_revision r ON r.id=w.revision_id AND r.system_id=w.system_id AND r.artifact_id=w.artifact_id
         JOIN public.source_artifact s ON s.id=r.artifact_id
         JOIN public.server_audit a ON a.id=w.audit_id AND a.target_id=w.system_id AND a.revision_id=w.revision_id",
    )
    .fetch_one(connection)
    .await
    .unwrap();
    assert_eq!(
        linked,
        (
            id("0102").to_string(),
            id("0301").to_string(),
            RECEIPT.to_owned(),
            BYTES.to_vec(),
            DIGEST.to_owned()
        )
    );
}

async fn accepted(connection: &mut PgConnection) {
    reset(connection).await;
    let receipt = create_system(connection, &fixture()).await.unwrap();
    assert_eq!(receipt.system_id, id("0102"));
    assert_eq!(receipt.artifact_id.to_string(), id("0201").to_string());
    assert_eq!(receipt.revision_id.to_string(), id("0301").to_string());
    assert_eq!(receipt.audit_id, audit_id("0401"));
    assert_eq!(receipt.event_id, event_id("0501"));
    assert_accepted(connection).await;
    let before = snapshot(connection).await;
    assert!(matches!(
        create_system(connection, &fixture()).await,
        Err(StorageError::Conflict)
    ));
    assert_eq!(
        snapshot(connection).await,
        before,
        "duplicate attempt changed committed context"
    );
    for statement in [
        "INSERT INTO public.outgoing_work(id,system_id,revision_id,artifact_id,audit_id,kind,outcome) VALUES('01890f20-7b5a-7cc3-98c4-dc0c0c070502',
         '01890f20-7b5a-7cc3-98c4-dc0c0c070101','01890f20-7b5a-7cc3-98c4-dc0c0c070301',
         '01890f20-7b5a-7cc3-98c4-dc0c0c070201','01890f20-7b5a-7cc3-98c4-dc0c0c070401','system.created','accepted')",
        "INSERT INTO public.outgoing_work(id,system_id,revision_id,artifact_id,audit_id,kind,outcome) VALUES('01890f20-7b5a-7cc3-98c4-dc0c0c070502',
         '01890f20-7b5a-7cc3-98c4-dc0c0c070102','01890f20-7b5a-7cc3-98c4-dc0c0c070301',
         '01890f20-7b5a-7cc3-98c4-dc0c0c070202','01890f20-7b5a-7cc3-98c4-dc0c0c070401','system.created','accepted')",
        "INSERT INTO public.outgoing_work(id,system_id,revision_id,artifact_id,audit_id,kind,outcome) VALUES('01890f20-7b5a-7cc3-98c4-dc0c0c070502',
         '01890f20-7b5a-7cc3-98c4-dc0c0c070102','01890f20-7b5a-7cc3-98c4-dc0c0c070301',
         '01890f20-7b5a-7cc3-98c4-dc0c0c070201','01890f20-7b5a-7cc3-98c4-dc0c0c070402','system.created','accepted')",
    ] {
        let error = sqlx::query(statement).execute(&mut *connection).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|e| e.code()).as_deref(), Some("23503"));
        assert_eq!(snapshot(connection).await, before, "mismatched outgoing binding changed state");
    }
    for statement in [
        "UPDATE public.outgoing_work SET audit_id='01890f20-7b5a-7cc3-98c4-dc0c0c070402'",
        "DELETE FROM public.outgoing_work",
        "TRUNCATE public.system_create_retry, public.outgoing_work",
    ] {
        let error = sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .unwrap_err();
        let database_error = error
            .as_database_error()
            .expect("database immutability error");
        assert_eq!(database_error.code().as_deref(), Some("55000"));
        assert_eq!(database_error.message(), "retained history is immutable");
        assert_eq!(
            snapshot(connection).await,
            before,
            "ordinary SQL changed retained outgoing work"
        );
    }
    // Reopening a connection establishes ordinary durable visibility, not backup recovery.
    let mut reopened = PgConnection::connect(DSN).await.unwrap();
    assert_accepted(&mut reopened).await;
    passed("accepted-exact-facts");
}

async fn insert_failures(connection: &mut PgConnection) {
    execute(
        connection,
        "CREATE FUNCTION public.fixture_write_failure() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN RAISE EXCEPTION 'fixture write boundary' USING ERRCODE='P0001'; END; $$",
    )
    .await;
    execute(connection,
        "CREATE FUNCTION public.fixture_second_alias_failure() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN IF NEW.identifier='upstream-29' THEN
         RAISE EXCEPTION 'fixture second alias' USING ERRCODE='P0001'; END IF; RETURN NEW; END; $$").await;
    let cases = [
        (
            "identity",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.resource_identity FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.resource_identity",
        ),
        (
            "system",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.system_identity FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.system_identity",
        ),
        (
            "first-alias",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.source_identity FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.source_identity",
        ),
        (
            "second-alias",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.source_identity FOR EACH ROW EXECUTE FUNCTION public.fixture_second_alias_failure()",
            "DROP TRIGGER fixture_fail ON public.source_identity",
        ),
        (
            "parent-guard",
            "CREATE TRIGGER fixture_fail AFTER UPDATE ON public.system_parent_write_guard FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.system_parent_write_guard",
        ),
        (
            "parent",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.system_parent FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.system_parent",
        ),
        (
            "artifact",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.source_artifact FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.source_artifact",
        ),
        (
            "revision",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.system_revision FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.system_revision",
        ),
        (
            "audit",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.server_audit FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.server_audit",
        ),
        (
            "outgoing",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.outgoing_work FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.outgoing_work",
        ),
        (
            "head",
            "CREATE TRIGGER fixture_fail AFTER INSERT ON public.system_write_head FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.system_write_head",
        ),
        (
            "commit",
            "CREATE CONSTRAINT TRIGGER fixture_fail AFTER INSERT ON public.outgoing_work DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()",
            "DROP TRIGGER fixture_fail ON public.outgoing_work",
        ),
    ];
    for (name, install, remove) in cases {
        reset(connection).await;
        let before = snapshot(connection).await;
        execute(connection, install).await;
        let error = create_system(connection, &fixture())
            .await
            .expect_err("injected write must fail");
        assert!(
            matches!(&error, StorageError::Database(inner)
            if inner.as_database_error().and_then(|e| e.code()).as_deref() == Some("P0001")),
            "wrong failure at {name}: {error}"
        );
        execute(connection, remove).await;
        assert_eq!(
            snapshot(connection).await,
            before,
            "atomic rollback failed at {name}"
        );
        assert!(
            !connection.is_in_transaction(),
            "failed transaction remained open at {name}"
        );
        println!("Atomic rollback boundary passed: {name}");
    }
    passed("all-write-boundaries-and-commit");
}

async fn invalid_context(connection: &mut PgConnection) {
    reset(connection).await;
    let before = snapshot(connection).await;
    for case in 0..11 {
        let mut request = fixture();
        match case {
            0 => request.revision.system_id = id("0101"),
            1 => request.revision.artifact_id = id("0202").to_string().parse().unwrap(),
            2 => request.audit.actor = None,
            3 => request.audit.actor = Some(String::new()),
            4 => request.audit.source = Some("x".repeat(257)),
            5 => request.audit.correlation = String::new(),
            6 => request.audit.correlation = "x".repeat(257),
            7 => request.audit.actor = Some("safe\u{0085}unsafe".to_owned()),
            8 => request.system.label = "x".repeat(4097),
            9 => request.system.sources = vec![request.system.sources[0].clone(); 65],
            10 => request.artifact.bytes = vec![0; 1_048_577],
            _ => unreachable!(),
        }
        assert!(
            matches!(
                create_system(connection, &request).await,
                Err(StorageError::InvalidInput)
            ),
            "invalid context case {case}"
        );
        assert_eq!(
            snapshot(connection).await,
            before,
            "invalid context changed state case {case}"
        );
    }
    let mut transaction = connection.begin().await.unwrap();
    assert!(matches!(
        create_system(&mut transaction, &fixture()).await,
        Err(StorageError::InvalidInput)
    ));
    assert!(matches!(
        record_denied_system_create(&mut transaction, &denial()).await,
        Err(StorageError::InvalidInput)
    ));
    assert_eq!(
        snapshot(&mut transaction).await,
        before,
        "ambient transaction was mutated"
    );
    transaction.rollback().await.unwrap();
    assert_eq!(snapshot(connection).await, before);
    let mut invalid_denial = denial();
    invalid_denial.audit.correlation = String::new();
    assert!(matches!(
        record_denied_system_create(connection, &invalid_denial).await,
        Err(StorageError::InvalidInput)
    ));
    assert_eq!(snapshot(connection).await, before);
    let mut optional_source = fixture();
    optional_source.audit.source = None;
    create_system(connection, &optional_source).await.unwrap();
    let actual_source: Option<String> =
        sqlx::query_scalar("SELECT source FROM public.server_audit")
            .fetch_one(connection)
            .await
            .unwrap();
    assert!(
        actual_source.is_none(),
        "missing verified source must stay unknown"
    );
    passed("invalid-context-and-owned-transaction");
}

async fn visibility(connection: &mut PgConnection) {
    reset(connection).await;
    execute(
        connection,
        "CREATE FUNCTION public.fixture_visibility_barrier() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN PERFORM pg_advisory_xact_lock(150015); RETURN NEW; END; $$",
    )
    .await;
    execute(connection,
        "CREATE TRIGGER fixture_visibility AFTER INSERT ON public.outgoing_work FOR EACH ROW EXECUTE FUNCTION public.fixture_visibility_barrier()").await;
    let before = snapshot(connection).await;
    execute(connection, "SELECT pg_advisory_lock(150015)").await;
    let mut writer = PgConnection::connect(DSN).await.unwrap();
    execute(&mut writer, "SET statement_timeout=10000").await;
    let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut writer)
        .await
        .unwrap();
    let task = tokio::spawn(async move { create_system(&mut writer, &fixture()).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND locktype='advisory'
                 AND classid=0 AND objid=150015 AND objsubid=1 AND NOT granted)",
            )
            .bind(writer_pid)
            .fetch_one(&mut *connection)
            .await
            .unwrap();
            if waiting {
                break;
            }
            assert!(
                !task.is_finished(),
                "writer ended before reaching its explicit visibility barrier"
            );
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("writer did not reach advisory barrier within bound");
    assert_eq!(
        snapshot(connection).await,
        before,
        "uncommitted application facts became visible"
    );
    let unlocked: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(150015)")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(unlocked);
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    execute(
        connection,
        "DROP TRIGGER fixture_visibility ON public.outgoing_work",
    )
    .await;
    assert_accepted(connection).await;
    passed("separate-observer-before-commit");
}

async fn denied(connection: &mut PgConnection) {
    reset(connection).await;
    let before = snapshot(connection).await;
    execute(connection,
        "CREATE TRIGGER fixture_denial_fail AFTER INSERT ON public.server_audit FOR EACH ROW EXECUTE FUNCTION public.fixture_write_failure()").await;
    let error = record_denied_system_create(connection, &denial())
        .await
        .unwrap_err();
    assert!(
        matches!(&error, StorageError::Database(inner)
        if inner.as_database_error().and_then(|e| e.code()).as_deref() == Some("P0001")),
        "wrong denial-store failure: {error}"
    );
    execute(
        connection,
        "DROP TRIGGER fixture_denial_fail ON public.server_audit",
    )
    .await;
    assert_eq!(
        snapshot(connection).await,
        before,
        "failed denial audit changed state"
    );
    record_denied_system_create(connection, &denial())
        .await
        .unwrap();
    let row = sqlx::query(
        "SELECT id::text,actor,source,operation,target_id::text,revision_id::text,time_civil_second,
         time_leap,time_fraction::text,time_source,outcome,correlation FROM public.server_audit",
    ).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(row.get::<String, _>("id"), id("0402").to_string());
    for column in ["actor", "source", "target_id", "revision_id"] {
        assert!(
            row.get::<Option<String>, _>(column).is_none(),
            "denial invented {column}"
        );
    }
    for (column, expected) in [
        ("operation", "system.create"),
        ("outcome", "denied"),
        ("time_fraction", "0.12345678901234567890"),
        ("time_source", RECEIPT),
        ("correlation", "denial-correlation-15"),
    ] {
        assert_eq!(row.get::<String, _>(column), expected);
    }
    assert_eq!(row.get::<i64, _>("time_civil_second"), 1_483_228_800);
    assert!(!row.get::<bool, _>("time_leap"));
    let counts: (i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM public.resource_identity),
         (SELECT count(*) FROM public.system_identity),(SELECT count(*) FROM public.source_identity),
         (SELECT count(*) FROM public.source_artifact),(SELECT count(*) FROM public.system_revision),
         (SELECT count(*) FROM public.outgoing_work)",
    ).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(counts, (1, 1, 0, 0, 0, 0));
    // Remove the known denial in privileged fixture cleanup, then compare every
    // remaining field, so unchanged counts are not the sole no-mutation oracle.
    execute(
        connection,
        "ALTER TABLE public.server_audit DISABLE TRIGGER server_audit_immutable",
    )
    .await;
    execute(connection, "DELETE FROM public.server_audit").await;
    execute(
        connection,
        "ALTER TABLE public.server_audit ENABLE TRIGGER server_audit_immutable",
    )
    .await;
    assert_eq!(
        snapshot(connection).await,
        before,
        "denial mutated non-audit state"
    );
    let mut known = denial();
    known.target = Some(id("0199"));
    known.audit = context();
    record_denied_system_create(connection, &known)
        .await
        .unwrap();
    let known_row: (
        Option<String>,
        Option<String>,
        Option<String>,
        Option<String>,
        String,
    ) = sqlx::query_as(
        "SELECT actor,source,target_id::text,revision_id::text,outcome FROM public.server_audit",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        known_row,
        (
            Some("fixture-actor".to_owned()),
            Some("fixture-source".to_owned()),
            Some(id("0199").to_string()),
            None,
            "denied".to_owned()
        )
    );
    passed("safe-denial-storage-and-failure");
}

async fn permissions(connection: &mut PgConnection) {
    reset(connection).await;
    for statement in [
        "CREATE ROLE atomic_serving NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT",
        "GRANT USAGE ON SCHEMA public TO atomic_serving",
        "GRANT SELECT ON public._sqlx_migrations TO atomic_serving",
        "GRANT SELECT,INSERT ON public.resource_identity,public.system_identity,public.source_identity,
         public.system_parent,public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work,public.system_write_head TO atomic_serving",
        "GRANT SELECT,UPDATE ON public.system_parent_write_guard TO atomic_serving",
    ] { execute(connection, statement).await; }
    let mut serving = PgConnection::connect(DSN).await.unwrap();
    execute(&mut serving, "SET ROLE atomic_serving").await;
    create_system(&mut serving, &fixture()).await.unwrap();
    assert_accepted(&mut serving).await;
    let before = snapshot(connection).await;
    for statement in [
        "UPDATE public.server_audit SET outcome='denied'",
        "DELETE FROM public.server_audit",
        "TRUNCATE public.server_audit",
        "ALTER TABLE public.server_audit DISABLE TRIGGER server_audit_immutable",
    ] {
        let error = sqlx::query(statement)
            .execute(&mut serving)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
        assert_eq!(
            snapshot(connection).await,
            before,
            "serving role changed earlier audit"
        );
    }
    // Even a mistakenly broad DML grant does not bypass immutable storage.
    execute(connection, "GRANT UPDATE,DELETE,TRUNCATE ON public.server_audit,public.outgoing_work TO atomic_serving").await;
    execute(connection, "GRANT TRUNCATE ON public.system_create_retry TO atomic_serving").await;
    for statement in [
        "UPDATE public.server_audit SET actor='replacement'",
        "DELETE FROM public.server_audit",
        "TRUNCATE public.server_audit CASCADE",
    ] {
        let error = sqlx::query(statement)
            .execute(&mut serving)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("55000")
        );
        assert_eq!(
            snapshot(connection).await,
            before,
            "immutable audit guard failed"
        );
    }
    // The existing System/revision constraints may refuse this removal, but it
    // must never silently remove accountability. Actual public DELETE is later.
    let deletion = sqlx::query(
        "DELETE FROM public.system_identity WHERE id='01890f20-7b5a-7cc3-98c4-dc0c0c070102'",
    )
    .execute(&mut *connection)
    .await;
    assert!(deletion.is_err());
    assert_eq!(snapshot(connection).await, before);
    let audit_fks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_constraint WHERE conrelid='public.server_audit'::regclass AND contype='f'",
    ).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(
        audit_fks, 0,
        "audit retention must not depend on resource existence"
    );
    passed("serving-permissions-and-audit-retention-boundary");
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
    packaged_migrations()
        .run_to(5, &mut connection)
        .await
        .unwrap();
    assert!(matches!(
        SystemRepository::create(&mut connection, &parent()).await,
        Err(StorageError::IncompatibleSchema)
    ));
    // The prior-version repository intentionally refuses a newer binary's old
    // ledger; seed the existing-schema fixture through fixed administrative SQL.
    execute(&mut connection,
        "INSERT INTO public.resource_identity VALUES('01890f20-7b5a-7cc3-98c4-dc0c0c070101','system','urn:glaux:test:atomic-parent')").await;
    execute(&mut connection,
        "INSERT INTO public.system_identity VALUES('01890f20-7b5a-7cc3-98c4-dc0c0c070101','Parent fixture')").await;
    migrate(&mut connection).await.unwrap();
    assert_eq!(
        SystemRepository::get(&mut connection, id("0101"))
            .await
            .unwrap(),
        Some(parent())
    );
    let before = snapshot(&mut connection).await;
    migrate(&mut connection).await.unwrap();
    assert_eq!(
        snapshot(&mut connection).await,
        before,
        "migration reapply changed data"
    );
    passed("migration-preservation");
    accepted(&mut connection).await;
    insert_failures(&mut connection).await;
    invalid_context(&mut connection).await;
    visibility(&mut connection).await;
    denied(&mut connection).await;
    permissions(&mut connection).await;
    println!("Required atomic write proof passed: 7 groups.");
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
                .expect("bounded atomic write proof timed out");
        });
}
