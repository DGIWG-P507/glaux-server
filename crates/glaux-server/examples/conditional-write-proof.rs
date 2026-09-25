//! Independently expected conditional writes against an owned real database.

use std::time::Duration;

use glaux_domain::identity::{LocalId, SourceIdentity};
use glaux_server::application::{
    AuditContext, CreateSystem, UpdateSystem, create_system, update_system,
};
use glaux_server::revisions::{NewSourceArtifact, RevisionId, SystemRevision};
use glaux_server::storage::{
    StorageError, SystemRecord, SystemRepository, migrate, packaged_migrations,
};
use sqlx::{AssertSqlSafe, Connection, PgConnection, Row};

const DSN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const TIME: &str = "2017-01-01T01:00:00.12345678901234567890+01:00";

fn id(suffix: u16) -> LocalId {
    format!("01890f20-7b5a-7cc3-98c4-dc0c0c08{suffix:04}")
        .parse()
        .unwrap()
}

fn revision(number: u16) -> RevisionId {
    id(300 + number).to_string().parse().unwrap()
}

fn label(number: u16) -> &'static str {
    match number {
        1 => "Original",
        2 => "Writer A",
        3 => "Writer B",
        _ => unreachable!(),
    }
}

fn bytes(number: u16) -> &'static [u8] {
    match number {
        1 => b"original-bytes",
        2 => b"writer-A-bytes",
        3 => b"writer-B-bytes",
        _ => unreachable!(),
    }
}

fn update(number: u16, expected: Option<u16>) -> UpdateSystem {
    UpdateSystem {
        system_id: id(102),
        label: label(number).to_owned(),
        artifact: NewSourceArtifact {
            id: id(200 + number).to_string().parse().unwrap(),
            media_type: "application/octet-stream".to_owned(),
            bytes: bytes(number).to_vec(),
        },
        revision: SystemRevision {
            id: revision(number),
            system_id: id(102),
            artifact_id: id(200 + number).to_string().parse().unwrap(),
            semantic_time: None,
            receipt_time: TIME.parse().unwrap(),
        },
        audit_id: id(400 + number).to_string().parse().unwrap(),
        event_id: id(500 + number).to_string().parse().unwrap(),
        audit: AuditContext {
            actor: Some("conditional-actor".to_owned()),
            source: Some("conditional-source".to_owned()),
            correlation: format!("conditional-{number}"),
            time: TIME.parse().unwrap(),
        },
        expected_revision: expected.map(revision),
    }
}

fn parent() -> SystemRecord {
    SystemRecord {
        id: id(101),
        uid: "urn:glaux:test:conditional-parent".parse().unwrap(),
        label: "Parent".to_owned(),
        parent: None,
        sources: vec![],
    }
}

fn original() -> CreateSystem {
    let input = update(1, None);
    CreateSystem {
        system: SystemRecord {
            id: id(102),
            uid: "urn:glaux:test:conditional-child".parse().unwrap(),
            label: "Original".to_owned(),
            parent: Some(id(101)),
            sources: vec![SourceIdentity::new(
                "urn:glaux:test:conditional-source".parse().unwrap(),
                "upstream-child".parse().unwrap(),
            )],
        },
        artifact: input.artifact,
        revision: input.revision,
        audit_id: input.audit_id,
        event_id: input.event_id,
        audit: input.audit,
    }
}

async fn execute(connection: &mut PgConnection, statement: &str) {
    // Private fixture helper: every caller uses literals or the closed table/
    // trigger-action lists below. No external input enters SQL identifiers.
    sqlx::query(AssertSqlSafe(statement))
        .execute(connection)
        .await
        .unwrap();
}

fn passed(group: &str) {
    println!("Conditional write group passed: {group}");
}

async fn snapshot(connection: &mut PgConnection, head: bool) -> String {
    let head_sql = if head {
        "(SELECT coalesce(json_agg(t ORDER BY system_id),'[]') FROM public.system_write_head t)"
    } else {
        "'[]'::json"
    };
    // Only the two fixed head_sql expressions above are interpolated.
    sqlx::query_scalar(AssertSqlSafe(format!(
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
         'head',{head_sql})::text")))
        .fetch_one(connection).await.unwrap()
}

async fn reset(connection: &mut PgConnection) {
    for statement in [
        "ALTER TABLE public.source_artifact DISABLE TRIGGER source_artifact_immutable",
        "ALTER TABLE public.system_revision DISABLE TRIGGER system_revision_immutable",
        "ALTER TABLE public.server_audit DISABLE TRIGGER server_audit_immutable",
        "ALTER TABLE public.outgoing_work DISABLE TRIGGER outgoing_work_immutable",
        "TRUNCATE public.system_write_head,public.outgoing_work,public.server_audit,public.system_revision,
         public.source_artifact,public.system_parent,public.source_identity,public.system_identity,public.resource_identity",
        "ALTER TABLE public.source_artifact ENABLE TRIGGER source_artifact_immutable",
        "ALTER TABLE public.system_revision ENABLE TRIGGER system_revision_immutable",
        "ALTER TABLE public.server_audit ENABLE TRIGGER server_audit_immutable",
        "ALTER TABLE public.outgoing_work ENABLE TRIGGER outgoing_work_immutable",
    ] { execute(connection, statement).await; }
    SystemRepository::create(connection, &parent())
        .await
        .unwrap();
    create_system(connection, &original()).await.unwrap();
}

async fn assert_facts(connection: &mut PgConnection, current: u16, history: &[u16]) {
    // Actual values use ordinary SQL decoding, not production record decoders.
    let identity: (String, String, String, String, String, String) = sqlx::query_as(
        "SELECT r.id::text,r.uid,s.label,p.parent_id::text,a.authority,a.identifier
         FROM public.resource_identity r JOIN public.system_identity s USING(id)
         JOIN public.system_parent p ON p.child_id=r.id
         JOIN public.source_identity a ON a.resource_id=r.id
         WHERE r.id='01890f20-7b5a-7cc3-98c4-dc0c0c080102'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (
            id(102).to_string(),
            "urn:glaux:test:conditional-child".to_owned(),
            label(current).to_owned(),
            id(101).to_string(),
            "urn:glaux:test:conditional-source".to_owned(),
            "upstream-child".to_owned()
        )
    );
    let head: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT system_id::text,revision_id::text,artifact_id::text FROM public.system_write_head ORDER BY system_id")
        .fetch_all(&mut *connection).await.unwrap();
    assert_eq!(
        head,
        vec![(
            id(102).to_string(),
            id(300 + current).to_string(),
            id(200 + current).to_string()
        )]
    );
    for (table, base) in [
        ("source_artifact", 200),
        ("system_revision", 300),
        ("server_audit", 400),
        ("outgoing_work", 500),
    ] {
        let ids: Vec<String> = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT id::text FROM public.{table} ORDER BY id"
        )))
        .fetch_all(&mut *connection)
        .await
        .unwrap();
        assert_eq!(
            ids,
            history
                .iter()
                .map(|number| id(base + number).to_string())
                .collect::<Vec<_>>(),
            "exact retained set for {table}"
        );
    }
    for number in history {
        let row = sqlx::query(
            "SELECT r.system_id::text,r.artifact_id::text,r.semantic_source,
             r.receipt_civil_second,r.receipt_leap,r.receipt_fraction::text,r.receipt_source,
             s.media_type,s.bytes,a.actor,a.source,a.operation,a.target_id::text,
             a.revision_id::text,a.time_civil_second,a.time_leap,a.time_fraction::text,a.time_source,a.outcome,a.correlation,
             w.system_id::text AS work_system,w.revision_id::text AS work_revision,
             w.artifact_id::text AS work_artifact,w.audit_id::text AS work_audit,w.kind,w.outcome AS work_outcome,w.audit_operation
             FROM public.system_revision r JOIN public.source_artifact s ON s.id=r.artifact_id
             JOIN public.server_audit a ON a.revision_id=r.id
             JOIN public.outgoing_work w ON w.audit_id=a.id WHERE r.id=$1::text::uuid")
            .bind(id(300 + number).to_string()).fetch_one(&mut *connection).await.unwrap();
        for (column, expected) in [
            ("system_id", id(102).to_string()),
            ("artifact_id", id(200 + number).to_string()),
            ("receipt_fraction", "0.12345678901234567890".to_owned()),
            ("receipt_source", TIME.to_owned()),
            ("media_type", "application/octet-stream".to_owned()),
            ("actor", "conditional-actor".to_owned()),
            ("source", "conditional-source".to_owned()),
            ("target_id", id(102).to_string()),
            ("revision_id", id(300 + number).to_string()),
            ("time_fraction", "0.12345678901234567890".to_owned()),
            ("time_source", TIME.to_owned()),
            ("outcome", "accepted".to_owned()),
            ("correlation", format!("conditional-{number}")),
            ("work_system", id(102).to_string()),
            ("work_revision", id(300 + number).to_string()),
            ("work_artifact", id(200 + number).to_string()),
            ("work_audit", id(400 + number).to_string()),
            ("work_outcome", "accepted".to_owned()),
        ] {
            assert_eq!(
                row.get::<String, _>(column),
                expected,
                "retained {number} {column}"
            );
        }
        let operation = if *number == 1 {
            "system.create"
        } else {
            "system.update"
        };
        assert_eq!(row.get::<String, _>("operation"), operation);
        assert_eq!(row.get::<String, _>("audit_operation"), operation);
        assert_eq!(
            row.get::<String, _>("kind"),
            if *number == 1 {
                "system.created"
            } else {
                "system.updated"
            }
        );
        assert_eq!(row.get::<Vec<u8>, _>("bytes"), bytes(*number));
        assert!(row.get::<Option<String>, _>("semantic_source").is_none());
        for column in ["receipt_civil_second", "time_civil_second"] {
            assert_eq!(row.get::<i64, _>(column), 1_483_228_800);
        }
        for column in ["receipt_leap", "time_leap"] {
            assert!(!row.get::<bool, _>(column));
        }
    }
}

async fn migration(connection: &mut PgConnection) {
    packaged_migrations()
        .run_to(6, &mut *connection)
        .await
        .unwrap();
    execute(connection, "INSERT INTO public.resource_identity VALUES('01890f20-7b5a-7cc3-98c4-dc0c0c080102','system','urn:glaux:test:migrated')").await;
    execute(connection, "INSERT INTO public.system_identity VALUES('01890f20-7b5a-7cc3-98c4-dc0c0c080102','Migrated')").await;
    for number in [1, 3] {
        sqlx::query("INSERT INTO public.source_artifact VALUES($1::text::uuid,'application/octet-stream',$2,sha256($2))")
            .bind(id(200 + number).to_string()).bind(bytes(number)).execute(&mut *connection).await.unwrap();
        sqlx::query("INSERT INTO public.system_revision VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,
                    NULL,NULL,NULL,NULL,$4,false,0,$5)")
            .bind(id(300 + number).to_string()).bind(id(102).to_string()).bind(id(200 + number).to_string())
            .bind(1_893_456_000_i64 + i64::from(number)).bind(format!("2030-01-01T00:00:0{number}Z"))
            .execute(&mut *connection).await.unwrap();
        sqlx::query("INSERT INTO public.server_audit VALUES($1::text::uuid,'migration-actor',NULL,'system.create',
                    $2::text::uuid,$3::text::uuid,1483228800,false,0,'2017-01-01T00:00:00Z','accepted','migration')")
            .bind(id(400 + number).to_string()).bind(id(102).to_string()).bind(id(300 + number).to_string())
            .execute(&mut *connection).await.unwrap();
        sqlx::query("INSERT INTO public.outgoing_work(id,system_id,revision_id,artifact_id,audit_id,kind,outcome)
                    VALUES($1::text::uuid,$2::text::uuid,$3::text::uuid,$4::text::uuid,$5::text::uuid,'system.created','accepted')")
            .bind(id(500 + number).to_string()).bind(id(102).to_string()).bind(id(300 + number).to_string())
            .bind(id(200 + number).to_string()).bind(id(400 + number).to_string())
            .execute(&mut *connection).await.unwrap();
    }
    let before = snapshot(connection, false).await;
    let Err(StorageError::Migration(sqlx::migrate::MigrateError::ExecuteMigration(error, 7))) =
        migrate(connection).await
    else {
        panic!("ambiguous migration must fail at migration 7");
    };
    let database_error = error
        .as_database_error()
        .expect("expected database rejection");
    assert_eq!(database_error.code().as_deref(), Some("23505"));
    assert_eq!(database_error.constraint(), Some("system_write_head_pkey"));
    let absent: bool = sqlx::query_scalar("SELECT to_regclass('public.system_write_head') IS NULL")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(absent, "failed migration retained new schema");
    assert_eq!(
        snapshot(connection, false).await,
        before,
        "failed migration mutated schema-6 data"
    );
    let versions: Vec<i64> =
        sqlx::query_scalar("SELECT version FROM public._sqlx_migrations ORDER BY version")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    assert_eq!(versions, vec![1, 2, 3, 4, 5, 6]);
    for statement in [
        "ALTER TABLE public.outgoing_work DISABLE TRIGGER outgoing_work_immutable",
        "DELETE FROM public.outgoing_work WHERE id='01890f20-7b5a-7cc3-98c4-dc0c0c080503'",
        "ALTER TABLE public.outgoing_work ENABLE TRIGGER outgoing_work_immutable",
        "ALTER TABLE public.server_audit DISABLE TRIGGER server_audit_immutable",
        "DELETE FROM public.server_audit WHERE id='01890f20-7b5a-7cc3-98c4-dc0c0c080403'",
        "ALTER TABLE public.server_audit ENABLE TRIGGER server_audit_immutable",
    ] {
        execute(connection, statement).await;
    }
    migrate(connection).await.unwrap();
    let head: (String, String) =
        sqlx::query_as("SELECT revision_id::text,artifact_id::text FROM public.system_write_head")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(
        head,
        (id(301).to_string(), id(201).to_string()),
        "migration selected newest unattached history"
    );
    let retained: Vec<String> =
        sqlx::query_scalar("SELECT id::text FROM public.system_revision ORDER BY id")
            .fetch_all(&mut *connection)
            .await
            .unwrap();
    assert_eq!(retained, vec![id(301).to_string(), id(303).to_string()]);
    let before = snapshot(connection, true).await;
    migrate(connection).await.unwrap();
    assert_eq!(snapshot(connection, true).await, before);
    passed("migration-authoritative-head-and-ambiguity");
}

async fn sequential(connection: &mut PgConnection) {
    reset(connection).await;
    assert_facts(connection, 1, &[1]).await;
    let receipt = update_system(connection, &update(2, Some(1)))
        .await
        .unwrap();
    assert_eq!(receipt.system_id, id(102));
    assert_eq!(receipt.revision_id, revision(2));
    assert_eq!(receipt.artifact_id.to_string(), id(202).to_string());
    assert_eq!(receipt.audit_id.to_string(), id(402).to_string());
    assert_eq!(receipt.event_id.to_string(), id(502).to_string());
    assert_facts(connection, 2, &[1, 2]).await;
    let before = snapshot(connection, true).await;
    assert!(
        matches!(
            update_system(connection, &update(3, Some(1))).await,
            Err(StorageError::PreconditionFailed)
        ),
        "stale supplied condition was not rejected"
    );
    assert_eq!(
        snapshot(connection, true).await,
        before,
        "stale rejection changed state"
    );
    assert!(!connection.is_in_transaction());
    update_system(connection, &update(3, None)).await.unwrap();
    assert_facts(connection, 3, &[1, 2, 3]).await;
    let before = snapshot(connection, true).await;
    let error = sqlx::query("INSERT INTO public.outgoing_work(id,system_id,revision_id,artifact_id,audit_id,kind,outcome)
        VALUES('01890f20-7b5a-7cc3-98c4-dc0c0c080599','01890f20-7b5a-7cc3-98c4-dc0c0c080102',
        '01890f20-7b5a-7cc3-98c4-dc0c0c080301','01890f20-7b5a-7cc3-98c4-dc0c0c080201',
        '01890f20-7b5a-7cc3-98c4-dc0c0c080401','system.updated','accepted')")
        .execute(&mut *connection).await.unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("23503")
    );
    assert_eq!(snapshot(connection, true).await, before);
    migrate(connection).await.unwrap();
    assert_eq!(
        snapshot(connection, true).await,
        before,
        "migration reapply rewound an advanced head"
    );
    passed("matching-stale-unconditional-and-exact-history");
}

async fn invalid(connection: &mut PgConnection) {
    reset(connection).await;
    let before = snapshot(connection, true).await;
    for (target, missing) in [(id(199), true), (id(101), false)] {
        let mut input = update(2, None);
        input.system_id = target;
        input.revision.system_id = target;
        let error = update_system(connection, &input).await.unwrap_err();
        assert!(if missing {
            matches!(error, StorageError::NotFound)
        } else {
            matches!(error, StorageError::UninitializedRevision)
        });
        assert_eq!(snapshot(connection, true).await, before);
    }
    for case in 0..6 {
        let mut input = update(2, Some(1));
        match case {
            0 => input.revision.system_id = id(101),
            1 => input.revision.artifact_id = id(299).to_string().parse().unwrap(),
            2 => input.audit.actor = None,
            3 => input.label = "x".repeat(4097),
            4 => input.audit.correlation = String::new(),
            5 => input.artifact.bytes = vec![0; 1_048_577],
            _ => unreachable!(),
        }
        assert!(
            matches!(
                update_system(connection, &input).await,
                Err(StorageError::InvalidInput)
            ),
            "invalid case {case}"
        );
        assert_eq!(snapshot(connection, true).await, before);
    }
    let mut transaction = connection.begin().await.unwrap();
    assert!(matches!(
        update_system(&mut transaction, &update(2, Some(1))).await,
        Err(StorageError::InvalidInput)
    ));
    assert_eq!(snapshot(&mut transaction, true).await, before);
    transaction.rollback().await.unwrap();
    passed("missing-uninitialized-invalid-and-owned-transaction");
}

async fn rollback(connection: &mut PgConnection) {
    execute(
        connection,
        "CREATE FUNCTION public.conditional_failure() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'conditional fixture boundary' USING ERRCODE='P0001'; END; $$",
    )
    .await;
    for (name, table, action, deferred) in [
        ("label", "system_identity", "UPDATE", false),
        ("artifact", "source_artifact", "INSERT", false),
        ("revision", "system_revision", "INSERT", false),
        ("audit", "server_audit", "INSERT", false),
        ("outgoing", "outgoing_work", "INSERT", false),
        ("head", "system_write_head", "UPDATE", false),
        ("commit", "outgoing_work", "INSERT", true),
    ] {
        reset(connection).await;
        let before = snapshot(connection, true).await;
        let prefix = if deferred { "CONSTRAINT " } else { "" };
        let suffix = if deferred {
            "DEFERRABLE INITIALLY DEFERRED "
        } else {
            ""
        };
        execute(connection, &format!("CREATE {prefix}TRIGGER conditional_fail AFTER {action} ON public.{table} {suffix}FOR EACH ROW EXECUTE FUNCTION public.conditional_failure()")).await;
        let error = update_system(connection, &update(2, Some(1)))
            .await
            .unwrap_err();
        assert!(
            matches!(&error, StorageError::Database(inner) if inner.as_database_error().and_then(|e| e.code()).as_deref()==Some("P0001")),
            "wrong rollback failure {name}: {error}"
        );
        execute(
            connection,
            &format!("DROP TRIGGER conditional_fail ON public.{table}"),
        )
        .await;
        assert_eq!(
            snapshot(connection, true).await,
            before,
            "partial update after {name}"
        );
        assert!(!connection.is_in_transaction());
        println!("Conditional rollback boundary passed: {name}");
    }
    passed("all-update-boundaries-and-commit");
}

async fn wait_for_lock(observer: &mut PgConnection, pid: i32, blocker: Option<i32>) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = if let Some(blocker) = blocker {
                sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1)) AND EXISTS(
                    SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted AND locktype IN ('transactionid','tuple'))")
                    .bind(pid).bind(blocker).fetch_one(&mut *observer).await.unwrap()
            } else {
                sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND locktype='advisory'
                    AND classid=0 AND objid=160016 AND objsubid=1 AND NOT granted)")
                    .bind(pid).fetch_one(&mut *observer).await.unwrap()
            };
            if waiting { break; }
            tokio::task::yield_now().await;
        }
    }).await.expect("writer did not reach required database lock barrier");
}

async fn concurrent(connection: &mut PgConnection, first: u16, conditional: bool) {
    reset(connection).await;
    execute(connection, "CREATE OR REPLACE FUNCTION public.conditional_barrier() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN PERFORM pg_advisory_xact_lock(160016); RETURN NEW; END; $$").await;
    execute(connection, "CREATE TRIGGER conditional_wait AFTER INSERT ON public.outgoing_work FOR EACH ROW EXECUTE FUNCTION public.conditional_barrier()").await;
    let before = snapshot(connection, true).await;
    execute(connection, "SELECT pg_advisory_lock(160016)").await;
    let mut winner = PgConnection::connect(DSN).await.unwrap();
    let mut waiter = PgConnection::connect(DSN).await.unwrap();
    for writer in [&mut winner, &mut waiter] {
        execute(writer, "SET statement_timeout=15000").await;
        // The application owns READ COMMITTED even when a caller's default differs.
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
    let winner_task =
        tokio::spawn(async move { update_system(&mut winner, &update(first, Some(1))).await });
    wait_for_lock(connection, winner_pid, None).await;
    let second = if first == 2 { 3 } else { 2 };
    let waiter_task = tokio::spawn(async move {
        update_system(
            &mut waiter,
            &update(second, if conditional { Some(1) } else { None }),
        )
        .await
    });
    wait_for_lock(connection, waiter_pid, Some(winner_pid)).await;
    assert!(!winner_task.is_finished() && !waiter_task.is_finished());
    assert_eq!(
        snapshot(connection, true).await,
        before,
        "uncommitted update became visible"
    );
    let unlocked: bool = sqlx::query_scalar("SELECT pg_advisory_unlock(160016)")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    assert!(unlocked);
    let winner_receipt = tokio::time::timeout(Duration::from_secs(5), winner_task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(winner_receipt.revision_id, revision(first));
    let result = tokio::time::timeout(Duration::from_secs(5), waiter_task)
        .await
        .unwrap()
        .unwrap();
    if conditional {
        assert!(
            matches!(result, Err(StorageError::PreconditionFailed)),
            "conditional waiter did not reject stale head"
        );
        assert_facts(connection, first, &[1, first]).await;
    } else {
        assert_eq!(result.unwrap().revision_id, revision(second));
        assert_facts(connection, second, &[1, 2, 3]).await;
    }
    execute(
        connection,
        "DROP TRIGGER conditional_wait ON public.outgoing_work",
    )
    .await;
    println!(
        "Conditional race passed: first-{first}-{}",
        if conditional {
            "conditional"
        } else {
            "unconditional"
        }
    );
}

async fn serving(connection: &mut PgConnection) {
    reset(connection).await;
    for statement in [
        "CREATE ROLE conditional_serving NOLOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOINHERIT",
        "GRANT USAGE ON SCHEMA public TO conditional_serving",
        "GRANT SELECT ON public._sqlx_migrations,public.resource_identity,public.system_identity,public.source_identity,public.system_parent,public.system_parent_write_guard,public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work,public.system_write_head TO conditional_serving",
        "GRANT INSERT ON public.source_artifact,public.system_revision,public.server_audit,public.outgoing_work TO conditional_serving",
        "GRANT UPDATE(label) ON public.system_identity TO conditional_serving",
        "GRANT UPDATE(revision_id,artifact_id) ON public.system_write_head TO conditional_serving",
    ] {
        execute(connection, statement).await;
    }
    let mut writer = PgConnection::connect(DSN).await.unwrap();
    execute(&mut writer, "SET ROLE conditional_serving").await;
    update_system(&mut writer, &update(2, Some(1)))
        .await
        .unwrap();
    assert_facts(connection, 2, &[1, 2]).await;
    let before = snapshot(connection, true).await;
    assert!(matches!(
        update_system(&mut writer, &update(3, Some(1))).await,
        Err(StorageError::PreconditionFailed)
    ));
    assert_eq!(snapshot(connection, true).await, before);
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
    migration(&mut connection).await;
    sequential(&mut connection).await;
    invalid(&mut connection).await;
    rollback(&mut connection).await;
    concurrent(&mut connection, 2, true).await;
    concurrent(&mut connection, 3, true).await;
    concurrent(&mut connection, 2, false).await;
    passed("both-conditional-orders-and-unconditional-race");
    serving(&mut connection).await;
    println!("Required conditional write proof passed: 6 groups.");
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
                .expect("bounded conditional write proof timed out");
        });
}
