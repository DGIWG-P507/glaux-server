//! Fixed synthetic real-database proof, run only inside the owned test container.
//! Initial expectations precede SQL; later discriminating fixture refinements
//! are distinguished in docs/system-storage-tests.md.
//! No URL override, skip-on-missing-storage or production serializer oracle.

use std::time::Duration;

use glaux_domain::identity::{LocalId, SourceIdentity, Uid};
use glaux_server::storage::{
    StorageError, SystemRecord, SystemRepository, check_schema, migrate, packaged_migrations,
};
use sqlx::{Connection, PgConnection};

const DSN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const LABEL: &str = "Shared label";

fn id(suffix: &str) -> LocalId {
    format!("01890f20-7b5a-7cc3-98c4-dc0c0c07{suffix}")
        .parse()
        .expect("literal fixture UUIDv7")
}

fn source(authority: &str, identifier: &str) -> SourceIdentity {
    SourceIdentity::new(authority.parse().unwrap(), identifier.parse().unwrap())
}

fn record(
    suffix: &str,
    uid: &str,
    sources: Vec<SourceIdentity>,
    parent: Option<LocalId>,
) -> SystemRecord {
    SystemRecord {
        id: id(suffix),
        uid: uid.parse().unwrap(),
        label: LABEL.to_owned(),
        sources,
        parent,
    }
}

fn parent() -> SystemRecord {
    record(
        "3901",
        "urn:glaux:fixture:system:P",
        vec![
            source("authority-A", "platform-7"),
            source("alternate-authority", "P-ALIAS"),
        ],
        None,
    )
}

fn first_child() -> SystemRecord {
    record(
        "3902",
        "urn:glaux:fixture:system:A",
        vec![
            source("authority-A", "sensor-1"),
            source("authority-A", "sensor-1-alias"),
        ],
        Some(id("3901")),
    )
}

fn other_authority() -> SystemRecord {
    record(
        "3903",
        "urn:glaux:fixture:system:B",
        vec![source("authority-B", "sensor-1")],
        None,
    )
}

// Fixed test-only xorshift64 bytes avoid a compressible 4 KiB fixture hiding
// PostgreSQL's B-tree index-row ceiling. This is not an identity generator.
fn lexical_bytes(mut state: u64, length: usize) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            ALPHABET[(state & 63) as usize] as char
        })
        .collect()
}

fn change_last(value: &str) -> String {
    format!(
        "{}{}",
        &value[..value.len() - 1],
        if value.ends_with('a') { 'b' } else { 'a' }
    )
}

// Expected data is constructed only from the committed literals, never from
// repository reads. Full table snapshots independently expose partial writes.
#[derive(Debug, Eq, PartialEq)]
struct Snapshot {
    identities: Vec<(String, String, String)>,
    systems: Vec<(String, String)>,
    sources: Vec<(String, String, String)>,
    parents: Vec<(String, String)>,
}

fn expected(records: &[&SystemRecord], identity_only: bool) -> Snapshot {
    let mut result = Snapshot {
        identities: Vec::new(),
        systems: Vec::new(),
        sources: Vec::new(),
        parents: Vec::new(),
    };
    for record in records {
        let local_id = record.id.to_string();
        result
            .identities
            .push((local_id.clone(), "system".into(), record.uid.to_string()));
        result
            .systems
            .push((local_id.clone(), record.label.clone()));
        for alias in &record.sources {
            result.sources.push((
                local_id.clone(),
                alias.authority().to_string(),
                alias.identifier().to_string(),
            ));
        }
        if let Some(parent) = record.parent {
            result.parents.push((local_id, parent.to_string()));
        }
    }
    if identity_only {
        result.identities.push((
            id("39f0").to_string(),
            "system".into(),
            "urn:glaux:fixture:identity-only".into(),
        ));
    }
    result.identities.sort();
    result.systems.sort();
    result.sources.sort();
    result.parents.sort();
    result
}

async fn snapshot(connection: &mut PgConnection, with_parents: bool) -> Snapshot {
    let identities = sqlx::query_as(
        "SELECT id::text, family, uid FROM resource_identity ORDER BY id::text COLLATE \"C\"",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    let systems = sqlx::query_as(
        "SELECT id::text, label FROM system_identity ORDER BY id::text COLLATE \"C\"",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    let sources = sqlx::query_as(
        "SELECT resource_id::text, authority, identifier FROM source_identity \
         ORDER BY resource_id::text COLLATE \"C\", authority COLLATE \"C\", identifier COLLATE \"C\"",
    ).fetch_all(&mut *connection).await.unwrap();
    let parents = if with_parents {
        sqlx::query_as(
            "SELECT child_id::text, parent_id::text FROM system_parent ORDER BY child_id::text COLLATE \"C\"",
        ).fetch_all(&mut *connection).await.unwrap()
    } else {
        Vec::new()
    };
    Snapshot {
        identities,
        systems,
        sources,
        parents,
    }
}

type LedgerRow = (i64, String, String, bool, String, i64);

async fn ledger(connection: &mut PgConnection) -> Vec<LedgerRow> {
    sqlx::query_as(
        "SELECT version, description, installed_on::text, success, encode(checksum, 'hex'), execution_time \
         FROM _sqlx_migrations ORDER BY version",
    ).fetch_all(&mut *connection).await.unwrap()
}

async fn public_tables(connection: &mut PgConnection) -> Vec<(String,)> {
    sqlx::query_as("SELECT tablename FROM pg_tables WHERE schemaname = 'public' ORDER BY tablename COLLATE \"C\"")
        .fetch_all(&mut *connection).await.unwrap()
}

fn assert_record(actual: &SystemRecord, expected: &SystemRecord) {
    assert_eq!(actual.id, expected.id, "canonical local ID");
    assert_eq!(actual.uid, expected.uid, "authoritative exact UID");
    assert_eq!(actual.label, expected.label, "label is not identity");
    assert_eq!(actual.parent, expected.parent, "typed parent association");
    let mut expected_sources: Vec<_> = expected
        .sources
        .iter()
        .map(|alias| (alias.authority().as_str(), alias.identifier().as_str()))
        .collect();
    expected_sources.sort();
    let actual_sources: Vec<_> = actual
        .sources
        .iter()
        .map(|alias| (alias.authority().as_str(), alias.identifier().as_str()))
        .collect();
    assert_eq!(
        actual_sources, expected_sources,
        "complete canonical source-pair order"
    );
}

async fn assert_lookup(connection: &mut PgConnection, expected: &SystemRecord) {
    let by_id = SystemRepository::get(&mut *connection, expected.id)
        .await
        .unwrap()
        .expect("known ID");
    assert_record(&by_id, expected);
    let by_uid = SystemRepository::find_uid(&mut *connection, &expected.uid)
        .await
        .unwrap()
        .expect("known UID");
    assert_record(&by_uid, expected);
    for alias in &expected.sources {
        let by_source = SystemRepository::find_source(&mut *connection, alias)
            .await
            .unwrap()
            .expect("known source pair");
        assert_record(&by_source, expected);
    }
}

async fn expect_conflict(
    connection: &mut PgConnection,
    attempted: &SystemRecord,
    unchanged: &Snapshot,
) {
    assert_eq!(&snapshot(connection, true).await, unchanged);
    let result = SystemRepository::create(&mut *connection, attempted).await;
    assert!(
        matches!(result, Err(StorageError::Conflict)),
        "expected identity conflict, got {result:?}"
    );
    assert_eq!(
        &snapshot(connection, true).await,
        unchanged,
        "conflict must leave every table unchanged"
    );
}

async fn expect_invalid_parent(
    connection: &mut PgConnection,
    attempted: &SystemRecord,
    unchanged: &Snapshot,
) {
    assert_eq!(&snapshot(connection, true).await, unchanged);
    let result = SystemRepository::create(&mut *connection, attempted).await;
    assert!(
        matches!(result, Err(StorageError::InvalidAssociation)),
        "expected invalid typed endpoint, got {result:?}"
    );
    assert_eq!(
        &snapshot(connection, true).await,
        unchanged,
        "parent failure must roll back the whole create"
    );
}

async fn proof() {
    let mut connection = PgConnection::connect(DSN)
        .await
        .expect("owned disposable PostgreSQL is required");
    let location: (String, String, String) =
        sqlx::query_as("SELECT current_database(), current_user, host(inet_server_addr())")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        location,
        (
            "glaux_harness_test".into(),
            "postgres".into(),
            "127.0.0.1".into()
        )
    );
    sqlx::query("SET statement_timeout = '5s'")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("SET lock_timeout = '1s'")
        .execute(&mut connection)
        .await
        .unwrap();

    let tables_before = public_tables(&mut connection).await;
    assert!(matches!(
        check_schema(&mut connection).await,
        Err(StorageError::IncompatibleSchema)
    ));
    assert_eq!(
        public_tables(&mut connection).await,
        tables_before,
        "schema check must not apply migrations"
    );
    sqlx::query("CREATE SCHEMA storage_probe")
        .execute(&mut connection)
        .await
        .unwrap();
    sqlx::query("SET search_path = storage_probe, public")
        .execute(&mut connection)
        .await
        .unwrap();
    assert!(
        matches!(
            migrate(&mut connection).await,
            Err(StorageError::IncompatibleSchema)
        ),
        "migration must reject a non-public creation namespace before any DDL"
    );
    assert_eq!(public_tables(&mut connection).await, tables_before);
    let other_objects: Vec<(String,)> = sqlx::query_as(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='storage_probe' ORDER BY c.relname",
    ).fetch_all(&mut connection).await.unwrap();
    assert!(
        other_objects.is_empty(),
        "rejected namespace must have no migration ledger or application objects"
    );
    sqlx::query("SET search_path = public, pg_catalog")
        .execute(&mut connection)
        .await
        .unwrap();

    let p = parent();
    packaged_migrations()
        .run_to(2, &mut connection)
        .await
        .expect("explicit predecessor migrations preserve the packaged ledger namespace");
    // Independent predecessor seed: do not depend on the repository create
    // function (whose latest implementation also writes the parent relation).
    sqlx::query(
        "INSERT INTO resource_identity (id, family, uid) VALUES ($1::text::uuid, 'system', $2)",
    )
    .bind(p.id.to_string())
    .bind(p.uid.as_str())
    .execute(&mut connection)
    .await
    .unwrap();
    sqlx::query("INSERT INTO system_identity (id, label) VALUES ($1::text::uuid, $2)")
        .bind(p.id.to_string())
        .bind(LABEL)
        .execute(&mut connection)
        .await
        .unwrap();
    for alias in &p.sources {
        sqlx::query("INSERT INTO source_identity (resource_id, authority, identifier) VALUES ($1::text::uuid, $2, $3)")
            .bind(p.id.to_string()).bind(alias.authority().as_str()).bind(alias.identifier().as_str())
            .execute(&mut connection).await.unwrap();
    }
    let predecessor_rows = expected(&[&p], false);
    assert_eq!(snapshot(&mut connection, false).await, predecessor_rows);
    let absent_parent: (Option<String>,) =
        sqlx::query_as("SELECT to_regclass('public.system_parent')::text")
            .fetch_one(&mut connection)
            .await
            .unwrap();
    assert_eq!(absent_parent, (None,));
    let predecessor_ledger = ledger(&mut connection).await;
    assert_eq!(
        predecessor_ledger
            .iter()
            .map(|row| row.0)
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert!(matches!(
        check_schema(&mut connection).await,
        Err(StorageError::IncompatibleSchema)
    ));
    assert_eq!(ledger(&mut connection).await, predecessor_ledger);
    assert_eq!(snapshot(&mut connection, false).await, predecessor_rows);
    migrate(&mut connection)
        .await
        .expect("explicit packaged upgrade");
    check_schema(&mut connection)
        .await
        .expect("latest compatible schema");
    assert_eq!(snapshot(&mut connection, true).await, predecessor_rows);
    assert_lookup(&mut connection, &p).await;
    println!("System storage group passed: migration-initial-preservation");

    let a = first_child();
    let b = other_authority();
    SystemRepository::create(&mut connection, &a).await.unwrap();
    SystemRepository::create(&mut connection, &b).await.unwrap();
    let initial = expected(&[&p, &a, &b], false);
    assert_eq!(snapshot(&mut connection, true).await, initial);
    for fixture in [&p, &a, &b] {
        assert_lookup(&mut connection, fixture).await;
    }
    assert!(
        SystemRepository::get(&mut connection, id("39ff"))
            .await
            .unwrap()
            .is_none()
    );
    let unknown_uid: Uid = "urn:glaux:fixture:unknown".parse().unwrap();
    assert!(
        SystemRepository::find_uid(&mut connection, &unknown_uid)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        SystemRepository::find_source(&mut connection, &source("authority-C", "sensor-1"))
            .await
            .unwrap()
            .is_none()
    );
    println!("System storage group passed: exact-identity-and-lookups");

    let duplicate_id = record(
        "3901",
        "urn:glaux:rejected:duplicate-id",
        vec![source("attempt", "duplicate-id")],
        None,
    );
    expect_conflict(&mut connection, &duplicate_id, &initial).await;
    let duplicate_uid = record(
        "3941",
        p.uid.as_str(),
        vec![source("attempt", "duplicate-uid")],
        Some(p.id),
    );
    expect_conflict(&mut connection, &duplicate_uid, &initial).await;
    let duplicate_source = record(
        "3942",
        "urn:glaux:rejected:duplicate-source",
        vec![
            source("aaa-new-authority", "fresh-only-then-conflict"),
            source("authority-A", "sensor-1"),
        ],
        Some(p.id),
    );
    expect_conflict(&mut connection, &duplicate_source, &initial).await;
    let repeated_alias = record(
        "3947",
        "urn:glaux:rejected:repeated-alias",
        vec![source("fresh", "twice"), source("fresh", "twice")],
        None,
    );
    expect_conflict(&mut connection, &repeated_alias, &initial).await;
    println!("System storage group passed: conflicts-atomic");

    let missing_parent = record(
        "3943",
        "urn:glaux:rejected:missing-parent",
        vec![source("attempt", "missing-parent")],
        Some(id("39ff")),
    );
    expect_invalid_parent(&mut connection, &missing_parent, &initial).await;
    sqlx::query("INSERT INTO resource_identity (id, family, uid) VALUES ($1::text::uuid, 'system', 'urn:glaux:fixture:identity-only')")
        .bind(id("39f0").to_string()).execute(&mut connection).await.unwrap();
    let with_identity_only = expected(&[&p, &a, &b], true);
    assert_eq!(snapshot(&mut connection, true).await, with_identity_only);
    let wrong_parent = record(
        "3944",
        "urn:glaux:rejected:identity-only-parent",
        vec![source("attempt", "identity-only-parent")],
        Some(id("39f0")),
    );
    expect_invalid_parent(&mut connection, &wrong_parent, &with_identity_only).await;
    let self_parent = record(
        "3948",
        "urn:glaux:rejected:self-parent",
        vec![source("attempt", "self-parent")],
        Some(id("3948")),
    );
    expect_invalid_parent(&mut connection, &self_parent, &with_identity_only).await;
    assert!(
        SystemRepository::get(&mut connection, id("39f0"))
            .await
            .unwrap()
            .is_none(),
        "common identity alone is not a System"
    );
    println!("System storage group passed: typed-parent-rollback");

    let long_uid = format!(
        "urn:glaux:long:{}",
        lexical_bytes(0x1234_5678_9abc_def1, 4096 - "urn:glaux:long:".len())
    );
    let other_long_uid = change_last(&long_uid);
    let long_authority = lexical_bytes(0x2345_6789_abcd_ef12, 4096);
    let long_identifier = lexical_bytes(0x3456_789a_bcde_f123, 4096);
    let other_long_identifier = change_last(&long_identifier);
    assert_eq!(long_uid.len(), 4096);
    assert_eq!(other_long_uid.len(), 4096);
    assert_eq!(long_authority.len(), 4096);
    assert_eq!(long_identifier.len(), 4096);
    assert_eq!(other_long_identifier.len(), 4096);
    let l1 = record(
        "3904",
        &long_uid,
        vec![source(&long_authority, &long_identifier)],
        Some(p.id),
    );
    let l2 = record(
        "3905",
        &other_long_uid,
        vec![source(&long_authority, &other_long_identifier)],
        None,
    );
    SystemRepository::create(&mut connection, &l1)
        .await
        .unwrap();
    SystemRepository::create(&mut connection, &l2)
        .await
        .unwrap();
    let c1 = record(
        "3906",
        "https://EXAMPLE.test/items/%41",
        vec![source("a", "bc"), source("a:", "b")],
        None,
    );
    let c2 = record(
        "3907",
        "https://example.test/items/%41",
        vec![source("ab", "c"), source("a", ":b")],
        None,
    );
    let c3 = record(
        "3908",
        "https://EXAMPLE.test/items/A",
        vec![source("Authority", "Sensor"), source("authority", "Sensor")],
        None,
    );
    let c4 = record(
        "3909",
        "https://EXAMPLE.test/items/%4a",
        vec![source("Authority", "sensor")],
        None,
    );
    let c5 = record("390a", "https://EXAMPLE.test/items/%4A", vec![], None);
    for fixture in [&c1, &c2, &c3, &c4, &c5] {
        SystemRepository::create(&mut connection, fixture)
            .await
            .unwrap();
    }
    let fixtures = [&p, &a, &b, &l1, &l2, &c1, &c2, &c3, &c4, &c5];
    let complete = expected(&fixtures, true);
    assert_eq!(snapshot(&mut connection, true).await, complete);
    for fixture in fixtures {
        assert_lookup(&mut connection, fixture).await;
    }
    let wide_values: (bool, bool, bool) = sqlx::query_as(
        "SELECT pg_column_size(r.uid)>3000, pg_column_size(s.authority)>3000, pg_column_size(s.identifier)>3000 \
         FROM resource_identity r JOIN source_identity s ON s.resource_id=r.id WHERE r.id=$1::text::uuid",
    ).bind(l1.id.to_string()).fetch_one(&mut connection).await.unwrap();
    assert_eq!(
        wide_values,
        (true, true, true),
        "long fixtures must not compress below the ordinary B-tree key limit"
    );
    let duplicate_long_uid = record("3945", &long_uid, vec![source("attempt", "long-uid")], None);
    expect_conflict(&mut connection, &duplicate_long_uid, &complete).await;
    let duplicate_long_source = record(
        "3946",
        "urn:glaux:rejected:long-source",
        vec![source(&long_authority, &long_identifier)],
        None,
    );
    expect_conflict(&mut connection, &duplicate_long_source, &complete).await;
    println!("System storage group passed: long-lexical-identities");

    let pristine_ledger = ledger(&mut connection).await;
    assert_eq!(
        pristine_ledger.iter().map(|row| row.0).collect::<Vec<_>>(),
        vec![1, 2, 3, 4]
    );
    migrate(&mut connection)
        .await
        .expect("reapplying packaged migrations is nondestructive");
    check_schema(&mut connection).await.unwrap();
    assert_eq!(
        ledger(&mut connection).await,
        pristine_ledger,
        "reapply must not rewrite migration evidence"
    );
    assert_eq!(snapshot(&mut connection, true).await, complete);
    sqlx::query("SET search_path = storage_probe, public")
        .execute(&mut connection)
        .await
        .unwrap();
    check_schema(&mut connection)
        .await
        .expect("read-only check always uses the public ledger");
    assert!(matches!(
        migrate(&mut connection).await,
        Err(StorageError::IncompatibleSchema)
    ));
    assert_eq!(ledger(&mut connection).await, pristine_ledger);
    assert_eq!(snapshot(&mut connection, true).await, complete);
    let other_objects: Vec<(String,)> = sqlx::query_as(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='storage_probe' ORDER BY c.relname",
    ).fetch_all(&mut connection).await.unwrap();
    assert!(other_objects.is_empty());
    sqlx::query("SET search_path = public, pg_catalog")
        .execute(&mut connection)
        .await
        .unwrap();
    for fixture in fixtures {
        assert_lookup(&mut connection, fixture).await;
    }
    println!("System storage group passed: migration-reapply-preservation");

    // Corruption is intentional only inside disposable transactions. Comparing
    // the polluted ledger before/after proves check_schema does not repair it.
    for mutation in [
        "UPDATE _sqlx_migrations SET success = false WHERE version = 3",
        "UPDATE _sqlx_migrations SET checksum = checksum || decode('00', 'hex') WHERE version = 3",
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (9999, 'unknown-fixture-version', true, decode('00', 'hex'), 0)",
    ] {
        sqlx::query("BEGIN").execute(&mut connection).await.unwrap();
        assert_eq!(
            sqlx::query(mutation)
                .execute(&mut connection)
                .await
                .unwrap()
                .rows_affected(),
            1
        );
        let polluted = ledger(&mut connection).await;
        assert_ne!(
            polluted, pristine_ledger,
            "corruption fixture must alter the ledger"
        );
        assert!(
            matches!(
                check_schema(&mut connection).await,
                Err(StorageError::IncompatibleSchema)
            ),
            "reject dirty, changed-checksum and unknown-version schemas"
        );
        assert_eq!(
            ledger(&mut connection).await,
            polluted,
            "compatibility check must be read-only"
        );
        assert_eq!(snapshot(&mut connection, true).await, complete);
        sqlx::query("ROLLBACK")
            .execute(&mut connection)
            .await
            .unwrap();
        check_schema(&mut connection).await.unwrap();
        assert_eq!(ledger(&mut connection).await, pristine_ledger);
        assert_eq!(snapshot(&mut connection, true).await, complete);
    }
    println!("System storage group passed: schema-compatibility-read-only");
    connection
        .close()
        .await
        .expect("explicit proof connection close");
    println!("Required System storage proof passed: 7 groups.");
}

fn main() {
    assert_eq!(
        std::env::args_os().len(),
        1,
        "no target or test-selection overrides are accepted"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(90), proof())
            .await
            .expect("System storage proof deadline exceeded");
    });
}
