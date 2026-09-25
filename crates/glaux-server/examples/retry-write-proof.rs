//! Independent retry truth table, Guide 4.6/6.4 and issue 17:
//! live (caller, source, create, parent, key) + equal intent => original outcome;
//! same scope + changed intent => conflict/no effect; other scope => independent;
//! denied disclosure => no outcome/effect; expired or absent key => fresh attempt.
//! Generated IDs and accepted-time metadata are not client intent. Byte/media,
//! semantic-time source spelling, UID, label and source aliases are intent.

use std::time::Duration;

use glaux_domain::identity::{LocalId, SourceIdentity};
use glaux_server::application::{
    AuditContext, CreateSystem, RetryKey, UpdateSystem, WriteReceipt, create_system_with_retry,
    update_system,
};
use glaux_server::revisions::{NewSourceArtifact, SystemRevision};
use glaux_server::storage::{StorageError, SystemRecord, SystemRepository, migrate};
use sqlx::{AssertSqlSafe, Connection, PgConnection, Row};

const DSN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const TIME: &str = "2017-01-01T01:00:00.12345678901234567890+01:00";

fn id(value: u16) -> LocalId {
    format!("01890f20-7b5a-7cc3-98c4-dc0c0c09{value:04x}")
        .parse()
        .unwrap()
}

fn input(generation: u16, logical: u16) -> CreateSystem {
    CreateSystem {
        system: SystemRecord {
            id: id(1000 + generation),
            uid: format!("urn:glaux:test:retry:{logical}").parse().unwrap(),
            label: "Exact retry fixture".to_owned(),
            sources: vec![
                SourceIdentity::new(
                    "urn:glaux:retry:a".parse().unwrap(),
                    format!("a-{logical}").parse().unwrap(),
                ),
                SourceIdentity::new(
                    "urn:glaux:retry:b".parse().unwrap(),
                    format!("b-{logical}").parse().unwrap(),
                ),
            ],
            parent: None,
        },
        artifact: NewSourceArtifact {
            id: id(2000 + generation).to_string().parse().unwrap(),
            media_type: "application/octet-stream".to_owned(),
            bytes: b"independent-original-bytes".to_vec(),
        },
        revision: SystemRevision {
            id: id(3000 + generation).to_string().parse().unwrap(),
            system_id: id(1000 + generation),
            artifact_id: id(2000 + generation).to_string().parse().unwrap(),
            semantic_time: Some(TIME.parse().unwrap()),
            receipt_time: TIME.parse().unwrap(),
        },
        audit_id: id(4000 + generation).to_string().parse().unwrap(),
        event_id: id(5000 + generation).to_string().parse().unwrap(),
        audit: AuditContext {
            actor: Some("retry-actor".to_owned()),
            source: Some("retry-source".to_owned()),
            correlation: format!("request-{generation}"),
            time: TIME.parse().unwrap(),
        },
    }
}

fn expected(generation: u16) -> WriteReceipt {
    WriteReceipt {
        system_id: id(1000 + generation),
        artifact_id: id(2000 + generation).to_string().parse().unwrap(),
        revision_id: id(3000 + generation).to_string().parse().unwrap(),
        audit_id: id(4000 + generation).to_string().parse().unwrap(),
        event_id: id(5000 + generation).to_string().parse().unwrap(),
    }
}

fn key() -> RetryKey {
    RetryKey {
        key: "opaque-retry-key".to_owned(),
        retention_seconds: 3600,
    }
}

async fn execute(connection: &mut PgConnection, sql: &str) {
    // Only closed fixture literals and boolean-selected trigger syntax enter SQL.
    sqlx::query(AssertSqlSafe(sql))
        .execute(connection)
        .await
        .unwrap();
}

fn passed(name: &str) {
    println!("Retry write group passed: {name}");
}

async fn reset(connection: &mut PgConnection) {
    for sql in [
        "ALTER TABLE public.source_artifact DISABLE TRIGGER source_artifact_immutable",
        "ALTER TABLE public.system_revision DISABLE TRIGGER system_revision_immutable",
        "ALTER TABLE public.server_audit DISABLE TRIGGER server_audit_immutable",
        "ALTER TABLE public.outgoing_work DISABLE TRIGGER outgoing_work_immutable",
        "TRUNCATE public.system_create_retry,public.system_write_head,public.outgoing_work,public.server_audit,public.system_revision,public.source_artifact,public.system_parent,public.source_identity,public.system_identity,public.resource_identity",
        "ALTER TABLE public.source_artifact ENABLE TRIGGER source_artifact_immutable",
        "ALTER TABLE public.system_revision ENABLE TRIGGER system_revision_immutable",
        "ALTER TABLE public.server_audit ENABLE TRIGGER server_audit_immutable",
        "ALTER TABLE public.outgoing_work ENABLE TRIGGER outgoing_work_immutable",
    ] {
        execute(connection, sql).await;
    }
}

async fn snapshot(connection: &mut PgConnection) -> String {
    sqlx::query_scalar("SELECT json_build_object(
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
        'retry',(SELECT coalesce(json_agg(t ORDER BY actor,source,target,key),'[]') FROM public.system_create_retry t))::text")
        .fetch_one(connection).await.unwrap()
}

async fn assert_records(connection: &mut PgConnection, generations: &[u16]) {
    for (table, offset) in [
        ("resource_identity", 1000),
        ("source_artifact", 2000),
        ("system_revision", 3000),
        ("server_audit", 4000),
        ("outgoing_work", 5000),
    ] {
        let actual: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT id::text FROM public.{table} ORDER BY id"
        )))
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        let mut expected: Vec<String> = generations
            .iter()
            .map(|n| id(offset + n).to_string())
            .collect();
        expected.sort();
        assert_eq!(actual, expected, "exact independent ID set for {table}");
    }
    for generation in generations {
        let row = sqlx::query("SELECT r.system_id::text,r.artifact_id::text,r.semantic_source,r.receipt_source,
            a.media_type,a.bytes,u.actor,u.source,u.operation,u.target_id::text,u.revision_id::text,u.outcome,u.correlation,
            w.id::text AS event,w.system_id::text AS work_system,w.revision_id::text AS work_revision,w.artifact_id::text AS work_artifact,w.audit_id::text AS work_audit,w.kind,
            h.revision_id::text AS head_revision,h.artifact_id::text AS head_artifact
            FROM public.system_revision r JOIN public.source_artifact a ON a.id=r.artifact_id
            JOIN public.server_audit u ON u.revision_id=r.id JOIN public.outgoing_work w ON w.audit_id=u.id
            JOIN public.system_write_head h ON h.system_id=r.system_id WHERE r.id=$1::text::uuid")
            .bind(id(3000 + generation).to_string()).fetch_one(&mut *connection).await.unwrap();
        for (column, value) in [
            ("system_id", id(1000 + generation).to_string()),
            ("artifact_id", id(2000 + generation).to_string()),
            ("semantic_source", TIME.to_owned()),
            ("receipt_source", TIME.to_owned()),
            ("media_type", "application/octet-stream".to_owned()),
            ("actor", "retry-actor".to_owned()),
            ("source", "retry-source".to_owned()),
            ("operation", "system.create".to_owned()),
            ("target_id", id(1000 + generation).to_string()),
            ("revision_id", id(3000 + generation).to_string()),
            ("outcome", "accepted".to_owned()),
            ("correlation", format!("request-{generation}")),
            ("event", id(5000 + generation).to_string()),
            ("work_system", id(1000 + generation).to_string()),
            ("work_revision", id(3000 + generation).to_string()),
            ("work_artifact", id(2000 + generation).to_string()),
            ("work_audit", id(4000 + generation).to_string()),
            ("kind", "system.created".to_owned()),
            ("head_revision", id(3000 + generation).to_string()),
            ("head_artifact", id(2000 + generation).to_string()),
        ] {
            assert_eq!(
                row.get::<String, _>(column),
                value,
                "exact persisted {column}"
            );
        }
        assert_eq!(
            row.get::<Vec<u8>, _>("bytes"),
            b"independent-original-bytes"
        );
    }
}

async fn replay(connection: &mut PgConnection) {
    reset(connection).await;
    // A fresh admission commits; discard its response and connection. Recovery
    // must use the durable receipt, not a retained object or the original socket.
    let mut lost = PgConnection::connect(DSN).await.unwrap();
    create_system_with_retry(&mut lost, &input(1, 1), Some(&key()), |_| true)
        .await
        .unwrap();
    lost.close().await.unwrap();
    let before = snapshot(connection).await;
    let mut retry = input(2, 1);
    retry.system.sources.reverse();
    retry.audit.time = "2030-01-01T00:00:00Z".parse().unwrap();
    retry.revision.receipt_time = "2030-01-01T00:00:00Z".parse().unwrap();
    let result = create_system_with_retry(connection, &retry, Some(&key()), |_| true).await;
    assert!(
        matches!(result,Ok(value) if value==expected(1)),
        "identical retry did not return original receipt"
    );
    assert_eq!(
        snapshot(connection).await,
        before,
        "replay changed committed state"
    );
    let mut reconnected = PgConnection::connect(DSN).await.unwrap();
    assert_eq!(
        create_system_with_retry(&mut reconnected, &input(4, 1), Some(&key()), |_| true)
            .await
            .unwrap(),
        expected(1)
    );
    assert_eq!(snapshot(connection).await, before);
    assert_records(connection, &[1]).await;
    let binding: (String,String,String,String,String,String,String,String,String,bool) = sqlx::query_as(
        "SELECT actor,source,operation,target,key,system_id::text,revision_id::text,artifact_id::text,audit_id::text,
        event_id='01890f20-7b5a-7cc3-98c4-dc0c0c091389'::uuid AND octet_length(digest)=32
        AND expires_at-retained_at=interval '3600 seconds' AND retained_at<=clock_timestamp() AND expires_at>clock_timestamp()
        FROM public.system_create_retry")
        .fetch_one(&mut *connection).await.unwrap();
    assert_eq!(
        binding,
        (
            "retry-actor".to_owned(),
            "retry-source".to_owned(),
            "system.create".to_owned(),
            String::new(),
            key().key,
            id(1001).to_string(),
            id(3001).to_string(),
            id(2001).to_string(),
            id(4001).to_string(),
            true
        )
    );
    migrate(connection).await.unwrap();
    assert_eq!(
        snapshot(connection).await,
        before,
        "migration reapply changed receipt"
    );
    passed("original-outcome-generated-ids-lost-response");
}

async fn content(connection: &mut PgConnection) {
    reset(connection).await;
    create_system_with_retry(connection, &input(1, 1), Some(&key()), |_| true)
        .await
        .unwrap();
    let before = snapshot(connection).await;
    let mut duplicate_alias = input(2, 1);
    duplicate_alias.system.sources.push(SourceIdentity::new(
        "urn:glaux:retry:a".parse().unwrap(),
        "a-1".parse().unwrap(),
    ));
    assert!(
        matches!(
            create_system_with_retry(connection, &duplicate_alias, Some(&key()), |_| true).await,
            Err(StorageError::InvalidInput)
        ),
        "live replay silently deduplicated invalid aliases"
    );
    assert_eq!(snapshot(connection).await, before);
    for case in 0..9 {
        let mut changed = input(2, 1);
        match case {
            0 => changed.system.label = "Different label".to_owned(),
            1 => changed.system.uid = "urn:glaux:test:changed".parse().unwrap(),
            2 => {
                changed.system.sources.pop();
            }
            3 => {
                changed.system.sources[0] = SourceIdentity::new(
                    "urn:glaux:changed".parse().unwrap(),
                    "a-1".parse().unwrap(),
                )
            }
            4 => {
                changed.system.sources[0] = SourceIdentity::new(
                    "urn:glaux:retry:a".parse().unwrap(),
                    "changed".parse().unwrap(),
                )
            }
            5 => changed.artifact.media_type = "application/json".to_owned(),
            6 => changed.artifact.bytes = b"different bytes".to_vec(),
            7 => changed.revision.semantic_time = None,
            8 => {
                changed.revision.semantic_time =
                    Some("2017-01-01T00:00:00.12345678901234567890Z".parse().unwrap())
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(
                create_system_with_retry(connection, &changed, Some(&key()), |_| true).await,
                Err(StorageError::Conflict)
            ),
            "changed retry intent was not rejected: case {case}"
        );
        assert_eq!(
            snapshot(connection).await,
            before,
            "content conflict mutated state: {case}"
        );
    }
    passed("every-content-field-conflicts-without-effect");
}

async fn scopes(connection: &mut PgConnection) {
    reset(connection).await;
    create_system_with_retry(connection, &input(1, 1), Some(&key()), |_| true)
        .await
        .unwrap();
    let before = snapshot(connection).await;
    let mut authorized_target = None;
    assert!(
        matches!(
            create_system_with_retry(connection, &input(2, 1), Some(&key()), |r| {
                authorized_target = Some(*r);
                false
            })
            .await,
            Err(StorageError::Denied)
        ),
        "saved receipt disclosed before reauthorization"
    );
    assert_eq!(
        authorized_target,
        Some(expected(1)),
        "reauthorization used new generated target"
    );
    assert_eq!(snapshot(connection).await, before);
    let mut changed = input(2, 1);
    changed.artifact.bytes = b"denied changed intent".to_vec();
    assert!(
        matches!(
            create_system_with_retry(connection, &changed, Some(&key()), |_| false).await,
            Err(StorageError::Denied)
        ),
        "content-conflict disclosure preceded reauthorization"
    );
    assert_eq!(snapshot(connection).await, before);
    for case in 0..3 {
        let generation = 2 + case;
        let mut request = input(generation, generation);
        match case {
            0 => request.audit.actor = Some("other-actor".to_owned()),
            1 => request.audit.source = Some("other-source".to_owned()),
            2 => request.audit.source = None,
            _ => unreachable!(),
        }
        assert_eq!(
            create_system_with_retry(connection, &request, Some(&key()), |_| true)
                .await
                .unwrap(),
            expected(generation),
            "different caller/source reused another scope's receipt"
        );
        let before = snapshot(connection).await;
        assert_eq!(
            create_system_with_retry(connection, &request, Some(&key()), |_| true)
                .await
                .unwrap(),
            expected(generation)
        );
        assert_eq!(snapshot(connection).await, before);
    }
    let parent = SystemRecord {
        id: id(999),
        uid: "urn:glaux:retry:parent".parse().unwrap(),
        label: "Parent".to_owned(),
        sources: vec![],
        parent: None,
    };
    SystemRepository::create(connection, &parent).await.unwrap();
    let mut child = input(5, 5);
    child.system.parent = Some(parent.id);
    assert_eq!(
        create_system_with_retry(connection, &child, Some(&key()), |_| true)
            .await
            .unwrap(),
        expected(5)
    );
    let target: String = sqlx::query_scalar(
        "SELECT target FROM public.system_create_retry WHERE system_id=$1::text::uuid",
    )
    .bind(id(1005).to_string())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(target, parent.id.to_string());
    let before = snapshot(connection).await;
    assert!(matches!(
        create_system_with_retry(connection, &input(6, 6), Some(&RetryKey {
            key: "fresh-denied".to_owned(), ..key()
        }), |candidate| {
            assert_eq!(*candidate, expected(6));
            false
        }).await,
        Err(StorageError::Denied)
    ));
    assert_eq!(snapshot(connection).await, before);
    let request = input(7, 1);
    let update = UpdateSystem {
        system_id: id(1001),
        label: "Updated independently".to_owned(),
        artifact: request.artifact,
        revision: SystemRevision {
            system_id: id(1001),
            ..request.revision
        },
        audit_id: request.audit_id,
        event_id: request.event_id,
        audit: request.audit,
        expected_revision: None,
    };
    assert_eq!(
        update_system(connection, &update)
            .await
            .unwrap()
            .revision_id,
        id(3007).to_string().parse().unwrap()
    );
    assert_eq!(
        create_system_with_retry(connection, &input(8, 1), Some(&key()), |_| true)
            .await
            .unwrap(),
        expected(1),
        "update consumed or rewrote create retry outcome"
    );
    passed("caller-source-target-operation-and-reauthorization");
}

async fn expiry(connection: &mut PgConnection) {
    reset(connection).await;
    create_system_with_retry(connection, &input(1, 1), Some(&key()), |_| true)
        .await
        .unwrap();
    let before = snapshot(connection).await;
    let extended = RetryKey {
        retention_seconds: 7200,
        ..key()
    };
    assert_eq!(
        create_system_with_retry(connection, &input(2, 1), Some(&extended), |_| true)
            .await
            .unwrap(),
        expected(1)
    );
    assert_eq!(
        snapshot(connection).await,
        before,
        "replay extended existing retention"
    );
    execute(connection,"UPDATE public.system_create_retry SET retained_at=clock_timestamp()-interval '2 hours',expires_at=clock_timestamp()-interval '1 hour'").await;
    let expired: bool =
        sqlx::query_scalar("SELECT expires_at<clock_timestamp() FROM public.system_create_retry")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert!(
        expired,
        "expiry fixture does not precede actual database clock"
    );
    assert_eq!(
        create_system_with_retry(connection, &input(2, 2), Some(&key()), |_| true)
            .await
            .unwrap(),
        expected(2)
    );
    assert_records(connection, &[1, 2]).await;
    let receipt_ids: Vec<String> =
        sqlx::query_scalar("SELECT system_id::text FROM public.system_create_retry")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    assert_eq!(receipt_ids, vec![id(1002).to_string()]);
    reset(connection).await;
    for generation in [1, 2] {
        assert_eq!(
            create_system_with_retry(connection, &input(generation, generation), None, |_| true)
                .await
                .unwrap(),
            expected(generation)
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM public.system_create_retry")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert_eq!(count, 0, "optional key became mandatory or manufactured");
    let before = snapshot(connection).await;
    let mut duplicate_alias = input(3, 3);
    duplicate_alias.system.sources.push(SourceIdentity::new(
        "urn:glaux:retry:a".parse().unwrap(),
        "a-3".parse().unwrap(),
    ));
    assert!(
        matches!(
            create_system_with_retry(connection, &duplicate_alias, Some(&key()), |_| true).await,
            Err(StorageError::InvalidInput)
        ),
        "invalid duplicate aliases were normalized into accepted intent"
    );
    assert_eq!(snapshot(connection).await, before);
    for retry in [
        RetryKey {
            key: String::new(),
            retention_seconds: 3600,
        },
        RetryKey {
            key: "x".repeat(257),
            retention_seconds: 3600,
        },
        RetryKey {
            key: "bad\nkey".to_owned(),
            retention_seconds: 3600,
        },
        RetryKey {
            key: "valid".to_owned(),
            retention_seconds: 0,
        },
    ] {
        assert!(matches!(
            create_system_with_retry(connection, &input(3, 3), Some(&retry), |_| true).await,
            Err(StorageError::InvalidInput)
        ));
        assert_eq!(snapshot(connection).await, before);
    }
    let mut transaction = connection.begin().await.unwrap();
    assert!(matches!(
        create_system_with_retry(&mut transaction, &input(3, 3), Some(&key()), |_| true).await,
        Err(StorageError::InvalidInput)
    ));
    transaction.rollback().await.unwrap();
    assert_eq!(snapshot(connection).await, before);
    passed("expiry-database-clock-optional-key-input-bounds");
}

async fn rollback_and_constraints(connection: &mut PgConnection) {
    execute(connection,"CREATE FUNCTION public.retry_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'retry boundary fault'; END; $$").await;
    for deferred in [false, true] {
        reset(connection).await;
        let before = snapshot(connection).await;
        let prefix = if deferred { "CONSTRAINT " } else { "" };
        let suffix = if deferred {
            "DEFERRABLE INITIALLY DEFERRED "
        } else {
            ""
        };
        execute(connection,&format!("CREATE {prefix}TRIGGER retry_fail AFTER INSERT ON public.system_create_retry {suffix}FOR EACH ROW EXECUTE FUNCTION public.retry_fail()")).await;
        let error = create_system_with_retry(connection, &input(1, 1), Some(&key()), |_| true)
            .await
            .unwrap_err();
        assert!(
            matches!(&error,StorageError::Database(e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0001")),
            "wrong receipt rollback failure: {error}"
        );
        execute(
            connection,
            "DROP TRIGGER retry_fail ON public.system_create_retry",
        )
        .await;
        assert_eq!(
            snapshot(connection).await,
            before,
            "receipt or commit failure left partial state"
        );
        assert!(!connection.is_in_transaction());
        println!(
            "Retry rollback boundary passed: {}",
            if deferred { "commit" } else { "receipt" }
        );
    }
    create_system_with_retry(connection, &input(1, 1), Some(&key()), |_| true)
        .await
        .unwrap();
    let other_key = RetryKey {
        key: "independent-other".to_owned(),
        ..key()
    };
    create_system_with_retry(connection, &input(2, 2), Some(&other_key), |_| true)
        .await
        .unwrap();
    let before = snapshot(connection).await;
    for (sql, code) in [
        (
            "UPDATE public.system_create_retry SET event_id='01890f20-7b5a-7cc3-98c4-dc0c0c09138a' WHERE key='opaque-retry-key'",
            "23503",
        ),
        (
            "UPDATE public.system_create_retry SET operation='system.update'",
            "23514",
        ),
        (
            "UPDATE public.system_create_retry SET digest=decode('00','hex')",
            "23514",
        ),
    ] {
        let error = sqlx::query(sql)
            .execute(&mut *connection)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some(code)
        );
        assert_eq!(snapshot(connection).await, before);
    }
    passed("receipt-and-commit-rollback-and-exact-bindings");
}

async fn wait_for_lock(observer: &mut PgConnection, pid: i32, blocker: Option<i32>) {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let waiting: bool=if let Some(blocker)=blocker {
                sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1)) AND EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted)")
                    .bind(pid).bind(blocker).fetch_one(&mut *observer).await.unwrap()
            } else {
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND locktype='advisory' AND classid=0 AND objid=170017 AND objsubid=1 AND NOT granted)")
                    .bind(pid).fetch_one(&mut *observer).await.unwrap()
            };
            if waiting {break;}
            tokio::task::yield_now().await;
        }
    }).await.expect("retry writer did not reach required database barrier");
}

async fn concurrent(connection: &mut PgConnection, first: u16, conflicting: bool) {
    reset(connection).await;
    execute(connection,"CREATE OR REPLACE FUNCTION public.retry_barrier() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_advisory_xact_lock(170017); RETURN NEW; END; $$").await;
    execute(connection,"CREATE TRIGGER retry_wait AFTER INSERT ON public.outgoing_work FOR EACH ROW EXECUTE FUNCTION public.retry_barrier()").await;
    let before = snapshot(connection).await;
    execute(connection, "SELECT pg_advisory_lock(170017)").await;
    let mut winner = PgConnection::connect(DSN).await.unwrap();
    let mut waiter = PgConnection::connect(DSN).await.unwrap();
    for writer in [&mut winner, &mut waiter] {
        execute(writer, "SET statement_timeout=15000").await;
        execute(
            writer,
            "SET default_transaction_isolation='repeatable read'",
        )
        .await;
    }
    let winner_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut winner)
        .await
        .unwrap();
    let waiter_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut waiter)
        .await
        .unwrap();
    let winner_task = tokio::spawn(async move {
        create_system_with_retry(&mut winner, &input(first, 1), Some(&key()), |_| true).await
    });
    wait_for_lock(connection, winner_pid, None).await;
    let second = if first == 1 { 2 } else { 1 };
    let waiter_task = tokio::spawn(async move {
        let mut request = input(second, 1);
        if conflicting {
            request.system.label = "Conflicting concurrent intent".to_owned();
        }
        create_system_with_retry(&mut waiter, &request, Some(&key()), |_| true).await
    });
    wait_for_lock(connection, waiter_pid, Some(winner_pid)).await;
    assert!(!winner_task.is_finished() && !waiter_task.is_finished());
    assert_eq!(
        snapshot(connection).await,
        before,
        "uncommitted retry exposed partial state"
    );
    let unlocked: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(170017)")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(unlocked);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), winner_task)
            .await
            .unwrap()
            .unwrap()
            .unwrap(),
        expected(first)
    );
    let result = tokio::time::timeout(Duration::from_secs(5), waiter_task)
        .await
        .unwrap()
        .unwrap();
    if conflicting {
        assert!(
            matches!(result, Err(StorageError::Conflict)),
            "concurrent changed intent was not rejected"
        );
    } else {
        assert_eq!(
            result.unwrap(),
            expected(first),
            "concurrent identical retry did not reuse original outcome"
        );
    }
    assert_records(connection, &[first]).await;
    execute(
        connection,
        "DROP TRIGGER retry_wait ON public.outgoing_work",
    )
    .await;
    println!(
        "Retry race passed: first-{first}-{}",
        if conflicting {
            "conflicting"
        } else {
            "identical"
        }
    );
}

async fn concurrent_scope(connection: &mut PgConnection, change_actor: bool) {
    reset(connection).await;
    execute(connection,"CREATE TRIGGER retry_wait AFTER INSERT ON public.outgoing_work FOR EACH ROW EXECUTE FUNCTION public.retry_barrier()").await;
    let before = snapshot(connection).await;
    execute(connection, "SELECT pg_advisory_lock(170017)").await;
    let mut first = PgConnection::connect(DSN).await.unwrap();
    let mut second = PgConnection::connect(DSN).await.unwrap();
    for writer in [&mut first, &mut second] {
        execute(writer, "SET statement_timeout=15000").await;
    }
    let first_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut first)
        .await
        .unwrap();
    let second_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut second)
        .await
        .unwrap();
    let first_task = tokio::spawn(async move {
        create_system_with_retry(&mut first, &input(1, 1), Some(&key()), |_| true).await
    });
    wait_for_lock(connection, first_pid, None).await;
    let second_task = tokio::spawn(async move {
        let mut request = input(2, 2);
        if change_actor {
            request.audit.actor = Some("other-actor".to_owned());
        } else {
            request.audit.source = Some("other-source".to_owned());
        }
        create_system_with_retry(&mut second, &request, Some(&key()), |_| true).await
    });
    // Both scopes must reach the controlled outgoing barrier independently.
    // A global key lock or foreign-scope replay cannot satisfy this observation.
    wait_for_lock(connection, second_pid, None).await;
    assert!(!first_task.is_finished() && !second_task.is_finished());
    assert_eq!(snapshot(connection).await, before);
    let unlocked: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(170017)")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(unlocked);
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), first_task)
            .await.unwrap().unwrap().unwrap(),
        expected(1)
    );
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(5), second_task)
            .await.unwrap().unwrap().unwrap(),
        expected(2)
    );
    let actual: Vec<(String, String, String, String, String, String, String)> = sqlx::query_as(
        "SELECT t.actor,t.source,t.system_id::text,t.revision_id::text,t.artifact_id::text,t.audit_id::text,t.event_id::text
         FROM public.system_create_retry t JOIN public.outgoing_work w ON w.id=t.event_id
         JOIN public.server_audit a ON a.id=t.audit_id
         WHERE a.actor=t.actor AND a.source=t.source AND a.target_id=t.system_id
         AND a.revision_id=t.revision_id AND a.operation='system.create' AND a.outcome='accepted'
         AND w.system_id=t.system_id AND w.revision_id=t.revision_id AND w.artifact_id=t.artifact_id
         AND w.audit_id=t.audit_id AND w.kind='system.created' ORDER BY t.system_id")
        .fetch_all(&mut *connection).await.unwrap();
    let expected_rows = vec![
        ("retry-actor".to_owned(), "retry-source".to_owned(), id(1001).to_string(),
         id(3001).to_string(), id(2001).to_string(), id(4001).to_string(), id(5001).to_string()),
        (if change_actor { "other-actor" } else { "retry-actor" }.to_owned(),
         if change_actor { "retry-source" } else { "other-source" }.to_owned(),
         id(1002).to_string(), id(3002).to_string(), id(2002).to_string(),
         id(4002).to_string(), id(5002).to_string()),
    ];
    assert_eq!(actual, expected_rows, "concurrent contexts borrowed or mixed another outcome");
    for (table, offset) in [
        ("resource_identity", 1000), ("source_artifact", 2000), ("system_revision", 3000),
        ("server_audit", 4000), ("outgoing_work", 5000),
    ] {
        let ids: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT id::text FROM public.{table} ORDER BY id"
        )))
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        assert_eq!(ids, vec![id(offset + 1).to_string(), id(offset + 2).to_string()]);
    }
    execute(connection, "DROP TRIGGER retry_wait ON public.outgoing_work").await;
    println!("Retry race passed: different-{}", if change_actor { "actor" } else { "source" });
}

fn sequence(seed: u32) -> Vec<u32> {
    let mut value = seed;
    (0..24)
        .map(|_| {
            value = value.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (value >> 16) % 4
        })
        .collect()
}

async fn model(connection: &mut PgConnection) {
    // v1 deterministic bounded generator: replay, changed intent, denial, expiry.
    // Exact model state is (current receipt generation, retained generations).
    let mut partitions = [0; 4];
    for seed in [17, 91, 314] {
        reset(connection).await;
        let operations = sequence(seed);
        assert_eq!(operations, sequence(seed), "generator not deterministic");
        let mut current = 1;
        let mut retained = vec![1];
        create_system_with_retry(connection, &input(1, 1), Some(&key()), |_| true)
            .await
            .unwrap();
        for (index, operation) in operations.into_iter().enumerate() {
            partitions[operation as usize] += 1;
            let candidate = 10 + u16::try_from(index).unwrap();
            let before = snapshot(connection).await;
            match operation {
                0 => assert_eq!(
                    create_system_with_retry(
                        connection,
                        &input(candidate, current),
                        Some(&key()),
                        |_| true
                    )
                    .await
                    .unwrap(),
                    expected(current)
                ),
                1 => {
                    let mut request = input(candidate, current);
                    request.artifact.bytes = b"model changed content".to_vec();
                    assert!(matches!(
                        create_system_with_retry(connection, &request, Some(&key()), |_| true)
                            .await,
                        Err(StorageError::Conflict)
                    ));
                }
                2 => assert!(matches!(
                    create_system_with_retry(
                        connection,
                        &input(candidate, current),
                        Some(&key()),
                        |_| false
                    )
                    .await,
                    Err(StorageError::Denied)
                )),
                3 => {
                    execute(connection,"UPDATE public.system_create_retry SET retained_at=clock_timestamp()-interval '2 hours',expires_at=clock_timestamp()-interval '1 hour'").await;
                    assert_eq!(
                        create_system_with_retry(
                            connection,
                            &input(candidate, candidate),
                            Some(&key()),
                            |_| true
                        )
                        .await
                        .unwrap(),
                        expected(candidate)
                    );
                    current = candidate;
                    retained.push(candidate);
                }
                _ => unreachable!(),
            }
            if operation != 3 {
                assert_eq!(
                    snapshot(connection).await,
                    before,
                    "model non-effect transition changed state"
                );
            }
            assert_records(connection, &retained).await;
        }
    }
    assert!(
        partitions.iter().all(|count| *count > 0),
        "generator missed a transition partition"
    );
    println!("Retry model passed: v1 seeds 17,91,314; 72 transitions; partitions {partitions:?}");
    passed("bounded-generated-retry-state-sequences");
}

async fn serving(connection: &mut PgConnection) {
    reset(connection).await;
    for sql in [
        "CREATE ROLE retry_serving NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT",
        "GRANT USAGE ON SCHEMA public TO retry_serving",
        "GRANT SELECT ON public._sqlx_migrations,public.resource_identity,public.system_identity,public.source_identity,public.system_parent,public.system_parent_write_guard,public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work,public.system_write_head,public.system_create_retry TO retry_serving",
        "GRANT INSERT ON public.resource_identity,public.system_identity,public.source_identity,public.system_parent,public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work,public.system_write_head,public.system_create_retry TO retry_serving",
        "GRANT UPDATE ON public.system_parent_write_guard TO retry_serving",
        "GRANT UPDATE(digest,system_id,revision_id,artifact_id,audit_id,event_id,retained_at,expires_at) ON public.system_create_retry TO retry_serving",
    ] {
        execute(connection, sql).await;
    }
    let mut writer = PgConnection::connect(DSN).await.unwrap();
    execute(&mut writer, "SET ROLE retry_serving").await;
    assert_eq!(
        create_system_with_retry(&mut writer, &input(1, 1), Some(&key()), |_| true)
            .await
            .unwrap(),
        expected(1)
    );
    assert_eq!(
        create_system_with_retry(&mut writer, &input(2, 1), Some(&key()), |_| true)
            .await
            .unwrap(),
        expected(1)
    );
    assert_records(connection, &[1]).await;
    let before = snapshot(connection).await;
    assert!(matches!(
        create_system_with_retry(&mut writer, &input(2, 2), Some(&key()), |_| true).await,
        Err(StorageError::Conflict)
    ));
    assert_eq!(snapshot(connection).await, before);
    for sql in [
        "DELETE FROM public.system_create_retry",
        "UPDATE public.system_create_retry SET actor='other'",
    ] {
        let error = sqlx::query(sql).execute(&mut writer).await.unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
        assert_eq!(snapshot(connection).await, before);
    }
    execute(connection,"UPDATE public.system_create_retry SET retained_at=clock_timestamp()-interval '2 hours',expires_at=clock_timestamp()-interval '1 hour'").await;
    assert_eq!(
        create_system_with_retry(&mut writer, &input(2, 2), Some(&key()), |_| true)
            .await
            .unwrap(),
        expected(2)
    );
    assert_records(connection, &[1, 2]).await;
    passed("restricted-serving-role");
}

async fn proof() {
    let mut connection = PgConnection::connect(DSN).await.unwrap();
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
    execute(&mut connection, "SET statement_timeout=10000").await;
    migrate(&mut connection).await.unwrap();
    replay(&mut connection).await;
    content(&mut connection).await;
    scopes(&mut connection).await;
    expiry(&mut connection).await;
    rollback_and_constraints(&mut connection).await;
    for (first, conflicting) in [(1, false), (2, false), (1, true), (2, true)] {
        concurrent(&mut connection, first, conflicting).await;
    }
    concurrent_scope(&mut connection, true).await;
    concurrent_scope(&mut connection, false).await;
    passed("both-orders-identical-and-conflicting-concurrency");
    model(&mut connection).await;
    serving(&mut connection).await;
    println!("Required retry write proof passed: 8 groups.");
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
                .expect("bounded retry write proof timed out");
        });
}
