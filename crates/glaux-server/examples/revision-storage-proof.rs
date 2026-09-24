//! Fixed synthetic SQLx proof; run only inside the owned disposable container.
//! Expected source bytes/digests and time coordinates are not server output.

use std::time::Duration;

use glaux_domain::identity::LocalId;
use glaux_domain::temporal::ExactInstant;
use glaux_server::revisions::{
    ArtifactId, ArtifactRepository, NewSourceArtifact, RevisionId, RevisionRepository,
    SystemRevision,
};
use glaux_server::storage::{
    StorageError, SystemRepository, check_schema, migrate, packaged_migrations,
};
use sqlx::{Connection, PgConnection};

const DSN: &str = "postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable";
const A: &[u8] = b"{\"type\":\"PhysicalSystem\",\"label\":\"Alpha\",\"value\":1}";
const B: &[u8] = b"{\n  \"value\": 1, \"label\": \"Alpha\", \"type\": \"PhysicalSystem\"\n}\n";
const C: &[u8] = b"{\"type\":\"PhysicalSystem\",\"label\":\"Beta\",\"value\":1}";
const SHA_A: &str = "8d3e448241a86daedf90f6ea36eebbc62c82829307bcb6eae1a9954c031ec215";
const SHA_B: &str = "27c281a95454810345d0780509dafb4154b3807ea473658aba7e5f09b41e9268";
const SHA_C: &str = "045014d3404cc5fa1f1d0b98c6d3852111a94bfbfa63356e451c8caa076e6c3b";
const MEDIA: &str = "application/json";
const C_MEDIA: &str = "application/json; profile=\"urn:glaux:fixture\"";
const RECEIPT: &str = "2017-01-01T01:00:00.12345678901234567890+01:00";

fn local(suffix: &str) -> LocalId {
    format!("01890f20-7b5a-7cc3-98c4-dc0c0c07{suffix}")
        .parse()
        .unwrap()
}

fn artifact_id(suffix: &str) -> ArtifactId {
    local(suffix).to_string().parse().unwrap()
}

fn revision_id(suffix: &str) -> RevisionId {
    local(suffix).to_string().parse().unwrap()
}

fn sha(hex: &str) -> [u8; 32] {
    assert_eq!(hex.len(), 64);
    let mut result = [0; 32];
    for (index, target) in result.iter_mut().enumerate() {
        *target = u8::from_str_radix(&hex[index * 2..index * 2 + 2], 16).unwrap();
    }
    result
}

fn source(suffix: &str, media_type: &str, bytes: &[u8]) -> NewSourceArtifact {
    NewSourceArtifact {
        id: artifact_id(suffix),
        media_type: media_type.to_owned(),
        bytes: bytes.to_vec(),
    }
}

fn revision(suffix: &str, artifact: &str, semantic: Option<&str>) -> SystemRevision {
    SystemRevision {
        id: revision_id(suffix),
        system_id: local("4001"),
        artifact_id: artifact_id(artifact),
        semantic_time: semantic.map(|value| value.parse().unwrap()),
        receipt_time: RECEIPT.parse().unwrap(),
    }
}

fn passed(group: &str) {
    println!("Revision storage group passed: {group}");
}

async fn snapshot(connection: &mut PgConnection) -> String {
    // Complete independently selected values expose same-count corruption and
    // partial writes; no repository serializer supplies this snapshot.
    sqlx::query_scalar(
        "SELECT json_build_object(
          'identities',(SELECT coalesce(json_agg(t ORDER BY id),'[]')
                        FROM public.resource_identity t),
          'systems',(SELECT coalesce(json_agg(t ORDER BY id),'[]')
                     FROM public.system_identity t),
          'sources',(SELECT coalesce(json_agg(t ORDER BY resource_id,authority,identifier),'[]')
                     FROM public.source_identity t),
          'parents',(SELECT coalesce(json_agg(t ORDER BY child_id),'[]')
                     FROM public.system_parent t),
          'artifacts',(SELECT coalesce(json_agg(t ORDER BY id),'[]')
                       FROM public.source_artifact t),
          'revisions',(SELECT coalesce(json_agg(t ORDER BY id),'[]')
                       FROM public.system_revision t))::text",
    )
    .fetch_one(connection)
    .await
    .unwrap()
}

async fn check_artifact(connection: &mut PgConnection, expected: &NewSourceArtifact, digest: &str) {
    let actual = ArtifactRepository::get(connection, expected.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual.id, expected.id);
    assert_eq!(actual.media_type, expected.media_type);
    assert_eq!(actual.bytes, expected.bytes);
    assert_eq!(actual.sha256, sha(digest));
    let row: (String, String, Vec<u8>, Vec<u8>) = sqlx::query_as(
        "SELECT id::text,media_type,bytes,digest FROM public.source_artifact
         WHERE id=$1::text::uuid",
    )
    .bind(expected.id.to_string())
    .fetch_one(connection)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            expected.id.to_string(),
            expected.media_type.clone(),
            expected.bytes.clone(),
            sha(digest).to_vec(),
        )
    );
}

struct TimeFixture {
    source: String,
    second: i64,
    leap: bool,
    fraction: String,
    digits: usize,
    offset: i32,
    known: bool,
}

fn time_fixture(
    source: &str,
    second: i64,
    leap: bool,
    fraction: &str,
    digits: usize,
    offset: i32,
    known: bool,
) -> TimeFixture {
    TimeFixture {
        source: source.to_owned(),
        second,
        leap,
        fraction: fraction.to_owned(),
        digits,
        offset,
        known,
    }
}

fn time_fixtures() -> Vec<TimeFixture> {
    let hundred_a = format!("000000001{}1", "0".repeat(90));
    let hundred_b = format!("000000001{}2", "0".repeat(90));
    let maximum = format!("{}1", "0".repeat(4074));
    vec![
        time_fixture(
            "1970-01-01T00:00:00.0000011Z",
            0,
            false,
            "0.0000011",
            7,
            0,
            false,
        ),
        time_fixture(
            "1970-01-01T00:00:00.0000012Z",
            0,
            false,
            "0.0000012",
            7,
            0,
            false,
        ),
        time_fixture(
            "1969-12-31T18:59:59.999999999-05:00",
            -1,
            false,
            "0.999999999",
            9,
            -18000,
            true,
        ),
        time_fixture(
            "2017-01-01T00:59:60.0000000001+01:00",
            1483228799,
            true,
            "0.0000000001",
            10,
            3600,
            true,
        ),
        time_fixture("1970-01-01T00:00:00-00:00", 0, false, "0", 0, 0, false),
        time_fixture(
            &format!("1970-01-01T00:00:00.{hundred_a}Z"),
            0,
            false,
            &format!("0.{hundred_a}"),
            100,
            0,
            false,
        ),
        time_fixture(
            &format!("1970-01-01T00:00:00.{hundred_b}Z"),
            0,
            false,
            &format!("0.{hundred_b}"),
            100,
            0,
            false,
        ),
        time_fixture(
            &format!("1970-01-01T00:00:00.{maximum}Z"),
            0,
            false,
            &format!("0.{maximum}"),
            4075,
            0,
            false,
        ),
    ]
}

fn check_time(actual: &ExactInstant, expected: &TimeFixture) {
    assert_eq!(actual.civil_second(), expected.second);
    assert_eq!(actual.is_leap_second(), expected.leap);
    assert_eq!(actual.fraction_decimal(), expected.fraction);
    assert_eq!(actual.source_lexeme(), expected.source);
    assert_eq!(actual.fraction_digits(), expected.digits);
    assert_eq!(actual.offset_seconds(), expected.offset);
    assert_eq!(actual.offset_known(), expected.known);
}

async fn immutable_rejection(connection: &mut PgConnection, statement: &'static str) {
    let before = snapshot(connection).await;
    let mut transaction = connection.begin().await.unwrap();
    let result = sqlx::query(statement).execute(&mut *transaction).await;
    transaction.rollback().await.unwrap();
    let error = result.expect_err("retained history mutation must reject, not succeed");
    let database_error = error.as_database_error().unwrap();
    assert_eq!(database_error.code().as_deref(), Some("55000"));
    assert!(
        database_error
            .message()
            .contains("retained history is immutable")
    );
    assert_eq!(snapshot(connection).await, before);
}

fn check_constraint_failure(error: sqlx::Error, expected: &str) {
    let database_error = error.as_database_error().unwrap();
    assert_eq!(database_error.code().as_deref(), Some("23514"));
    assert_eq!(database_error.constraint(), Some(expected));
}

async fn revision_insert_constraint(
    connection: &mut PgConnection,
    semantic_leap: Option<bool>,
    semantic_fraction: &str,
    receipt_fraction: &str,
    expected: Option<&str>,
) {
    let before = snapshot(connection).await;
    let mut transaction = connection.begin().await.unwrap();
    // Copy the known-valid revision, varying only the named constraint input.
    // A fresh literal ID and existing System/artifact avoid unrelated failures.
    let result = sqlx::query(
        "INSERT INTO public.system_revision
         SELECT '01890f20-7b5a-7cc3-98c4-dc0c0c0746f0',system_id,artifact_id,
                semantic_civil_second,$1,$2::text::numeric,semantic_source,
                receipt_civil_second,receipt_leap,$3::text::numeric,receipt_source
         FROM public.system_revision
         WHERE id='01890f20-7b5a-7cc3-98c4-dc0c0c074201'",
    )
    .bind(semantic_leap)
    .bind(semantic_fraction)
    .bind(receipt_fraction)
    .execute(&mut *transaction)
    .await;
    transaction.rollback().await.unwrap();
    if let Some(constraint) = expected {
        check_constraint_failure(
            result.expect_err("invalid revision INSERT must reject"),
            constraint,
        );
    } else {
        assert_eq!(
            result.unwrap().rows_affected(),
            1,
            "valid SQL control did not insert"
        );
    }
    assert_eq!(snapshot(connection).await, before);
}

async fn artifact_insert_constraint(
    connection: &mut PgConnection,
    media_type: &str,
    digest: Vec<u8>,
    expected: Option<&str>,
) {
    let before = snapshot(connection).await;
    let mut transaction = connection.begin().await.unwrap();
    let result = sqlx::query(
        "INSERT INTO public.source_artifact(id,media_type,bytes,digest)
         VALUES ('01890f20-7b5a-7cc3-98c4-dc0c0c0746f1',$1,$2,$3)",
    )
    .bind(media_type)
    .bind(A)
    .bind(digest)
    .execute(&mut *transaction)
    .await;
    transaction.rollback().await.unwrap();
    if let Some(constraint) = expected {
        check_constraint_failure(
            result.expect_err("invalid artifact INSERT must reject"),
            constraint,
        );
    } else {
        assert_eq!(
            result.unwrap().rows_affected(),
            1,
            "valid SQL control did not insert"
        );
    }
    assert_eq!(snapshot(connection).await, before);
}

async fn direct_insert_constraints(connection: &mut PgConnection) {
    const SEMANTIC: &str = "0.0000011";
    const RECEIVED: &str = "0.12345678901234567890";
    revision_insert_constraint(connection, Some(false), SEMANTIC, RECEIVED, None).await;
    revision_insert_constraint(
        connection,
        None,
        SEMANTIC,
        RECEIVED,
        Some("semantic_time_presence"),
    )
    .await;
    for fraction in [
        "NaN",
        "Infinity",
        "-Infinity",
        "-0.0000000001",
        "1",
        "1.0000000001",
    ] {
        revision_insert_constraint(
            connection,
            Some(false),
            fraction,
            RECEIVED,
            Some("semantic_fraction_exact"),
        )
        .await;
        revision_insert_constraint(
            connection,
            Some(false),
            SEMANTIC,
            fraction,
            Some("receipt_fraction_exact"),
        )
        .await;
    }
    artifact_insert_constraint(connection, MEDIA, sha(SHA_A).to_vec(), None).await;
    for digest in [vec![0; 32], vec![0; 31]] {
        artifact_insert_constraint(
            connection,
            MEDIA,
            digest,
            Some("source_artifact_digest_matches"),
        )
        .await;
    }
    for media_type in ["", "application/json\n", "application/\u{0001}json"] {
        artifact_insert_constraint(
            connection,
            media_type,
            sha(SHA_A).to_vec(),
            Some("source_artifact_media"),
        )
        .await;
    }
    println!(
        "Revision INSERT constraints passed: 2 valid controls and 18 exact-constraint rejections"
    );
}

async fn run(connection: &mut PgConnection) {
    let identity: (String, String, String) =
        sqlx::query_as("SELECT current_database(),current_user,host(inet_server_addr())")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    assert_eq!(
        identity,
        (
            "glaux_harness_test".into(),
            "postgres".into(),
            "127.0.0.1".into()
        )
    );
    for statement in ["SET statement_timeout=5000", "SET lock_timeout=1000"] {
        sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    packaged_migrations().run_to(connection, 3).await.unwrap();
    for statement in [
        "INSERT INTO public.resource_identity VALUES
         ('01890f20-7b5a-7cc3-98c4-dc0c0c074001','system','urn:glaux:revision:system')",
        "INSERT INTO public.system_identity VALUES
         ('01890f20-7b5a-7cc3-98c4-dc0c0c074001','Revision fixture')",
        "INSERT INTO public.resource_identity VALUES
         ('01890f20-7b5a-7cc3-98c4-dc0c0c0740f0','system','urn:glaux:revision:bare')",
    ] {
        sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .unwrap();
    }
    assert!(matches!(
        check_schema(connection).await,
        Err(StorageError::IncompatibleSchema)
    ));
    migrate(connection).await.unwrap();
    let system = SystemRepository::get(connection, local("4001"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(system.uid.as_str(), "urn:glaux:revision:system");
    assert_eq!(system.label, "Revision fixture");
    assert_eq!(system.id, local("4001"));
    assert_eq!(system.parent, None);
    assert!(system.sources.is_empty());
    for column in ["semantic_fraction", "receipt_fraction"] {
        let sql_type: String = sqlx::query_scalar(
            "SELECT format_type(atttypid,atttypmod) FROM pg_attribute
             WHERE attrelid='public.system_revision'::regclass AND attname=$1",
        )
        .bind(column)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        assert_eq!(
            sql_type, "numeric",
            "fixed scale can round valid timestamps"
        );
    }
    passed("migration-preservation");

    let a = source("4101", MEDIA, A);
    let b = source("4102", MEDIA, B);
    let c = source("4103", C_MEDIA, C);
    ArtifactRepository::insert(connection, &a).await.unwrap();
    ArtifactRepository::insert(connection, &b).await.unwrap();
    check_artifact(connection, &a, SHA_A).await;
    check_artifact(connection, &b, SHA_B).await;
    assert!(
        ArtifactRepository::get(connection, artifact_id("41ff"))
            .await
            .unwrap()
            .is_none()
    );
    passed("exact-artifacts");

    let receipt = time_fixture(
        RECEIPT,
        1483228800,
        false,
        "0.12345678901234567890",
        20,
        3600,
        true,
    );
    let fixtures = time_fixtures();
    for (index, fixture) in fixtures.iter().enumerate() {
        let suffix = format!("42{:02x}", index + 1);
        let record = revision(&suffix, "4101", Some(&fixture.source));
        RevisionRepository::append(connection, &record)
            .await
            .unwrap();
        let actual = RevisionRepository::get(connection, record.id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actual.id, record.id);
        assert_eq!(actual.system_id, local("4001"));
        assert_eq!(actual.artifact_id, artifact_id("4101"));
        check_time(actual.semantic_time.as_ref().unwrap(), fixture);
        check_time(&actual.receipt_time, &receipt);
        let exact: bool = sqlx::query_scalar(
            "SELECT semantic_civil_second=$2 AND semantic_leap=$3
             AND semantic_fraction=$4::text::numeric AND semantic_source=$5
             AND receipt_civil_second=1483228800 AND NOT receipt_leap
             AND receipt_fraction=0.12345678901234567890 AND receipt_source=$6
             FROM public.system_revision WHERE id=$1::text::uuid",
        )
        .bind(record.id.to_string())
        .bind(fixture.second)
        .bind(fixture.leap)
        .bind(&fixture.fraction)
        .bind(&fixture.source)
        .bind(RECEIPT)
        .fetch_one(&mut *connection)
        .await
        .unwrap();
        assert!(
            exact,
            "raw SQL keys differ from independent time coordinates"
        );
    }
    let no_semantic = revision("4209", "4102", None);
    RevisionRepository::append(connection, &no_semantic)
        .await
        .unwrap();
    let actual = RevisionRepository::get(connection, no_semantic.id)
        .await
        .unwrap()
        .unwrap();
    assert!(actual.semantic_time.is_none());
    check_time(&actual.receipt_time, &receipt);
    let absent: bool = sqlx::query_scalar(
        "SELECT semantic_civil_second IS NULL AND semantic_leap IS NULL
         AND semantic_fraction IS NULL AND semantic_source IS NULL
         FROM public.system_revision WHERE id=$1::text::uuid",
    )
    .bind(no_semantic.id.to_string())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert!(absent, "unknown semantic time must not become receipt time");
    let selected: Vec<String> = sqlx::query_scalar(
        "SELECT id::text FROM public.system_revision
         WHERE (semantic_civil_second,semantic_leap,semantic_fraction)
          > (0::bigint,false,0.0000011::numeric)
         AND (semantic_civil_second,semantic_leap,semantic_fraction)
          <= (0::bigint,false,0.0000012::numeric) ORDER BY id",
    )
    .fetch_all(&mut *connection)
    .await
    .unwrap();
    assert_eq!(selected, vec![revision_id("4202").to_string()]);
    let lossy_equal: bool = sqlx::query_scalar(
        "SELECT TIMESTAMPTZ '1970-01-01T00:00:00.0000011Z'
         = TIMESTAMPTZ '1970-01-01T00:00:00.0000012Z'",
    )
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert!(
        lossy_equal,
        "the deliberate timestamp control must collapse these values"
    );
    assert!(
        RevisionRepository::get(connection, revision_id("42ff"))
            .await
            .unwrap()
            .is_none()
    );
    passed("exact-times-and-references");

    let previous: String =
        sqlx::query_scalar("SELECT json_agg(t ORDER BY id)::text FROM public.system_revision t")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
    let later = revision("4301", "4103", Some("2017-01-01T00:00:00Z"));
    RevisionRepository::create_with_artifact(connection, &c, &later)
        .await
        .unwrap();
    let still_previous: String = sqlx::query_scalar(
        "SELECT json_agg(t ORDER BY id)::text FROM public.system_revision t
         WHERE id<>$1::text::uuid",
    )
    .bind(later.id.to_string())
    .fetch_one(&mut *connection)
    .await
    .unwrap();
    assert_eq!(still_previous, previous);
    check_artifact(connection, &a, SHA_A).await;
    check_artifact(connection, &b, SHA_B).await;
    check_artifact(connection, &c, SHA_C).await;
    let actual = RevisionRepository::get(connection, later.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(actual.artifact_id, c.id);
    assert_eq!(actual.system_id, local("4001"));
    assert_eq!(
        actual.semantic_time.unwrap().source_lexeme(),
        "2017-01-01T00:00:00Z"
    );
    passed("append-preserves-history");

    let before = snapshot(connection).await;
    let missing_artifact = revision("4401", "41ff", None);
    assert!(matches!(
        RevisionRepository::append(connection, &missing_artifact).await,
        Err(StorageError::InvalidAssociation)
    ));
    assert_eq!(snapshot(connection).await, before);
    for owner in ["40ff", "40f0"] {
        let orphan_artifact = source("44f1", MEDIA, A);
        let mut orphan_revision = revision("44f2", "44f1", None);
        orphan_revision.system_id = local(owner);
        assert!(matches!(
            RevisionRepository::create_with_artifact(
                connection,
                &orphan_artifact,
                &orphan_revision
            )
            .await,
            Err(StorageError::InvalidAssociation)
        ));
        assert_eq!(snapshot(connection).await, before);
    }
    assert!(matches!(
        RevisionRepository::append(connection, &later).await,
        Err(StorageError::Conflict)
    ));
    assert_eq!(snapshot(connection).await, before);
    assert!(matches!(
        ArtifactRepository::insert(connection, &source("4101", C_MEDIA, C)).await,
        Err(StorageError::Conflict)
    ));
    assert_eq!(snapshot(connection).await, before);
    assert!(matches!(
        RevisionRepository::create_with_artifact(
            connection,
            &source("44f1", MEDIA, A),
            &revision("44f2", "4101", None),
        )
        .await,
        Err(StorageError::InvalidInput)
    ));
    assert_eq!(snapshot(connection).await, before);
    direct_insert_constraints(connection).await;
    passed("atomic-rejection");

    // Initial mutable implementation must fail here for this behavioral reason.
    // Roll back even an incorrectly accepted mutation before asserting failure.
    for statement in [
        "UPDATE public.source_artifact SET media_type='changed'",
        "UPDATE public.system_revision SET artifact_id='01890f20-7b5a-7cc3-98c4-dc0c0c074102'",
        "UPDATE public.system_revision SET receipt_source='1970-01-01T00:00:00Z'",
        "DELETE FROM public.system_revision",
        "DELETE FROM public.source_artifact",
        "TRUNCATE public.system_revision",
        "TRUNCATE public.source_artifact CASCADE",
    ] {
        immutable_rejection(connection, statement).await;
    }
    for (disable, statement) in [
        (
            "ALTER TABLE public.source_artifact DISABLE TRIGGER source_artifact_immutable",
            "UPDATE public.source_artifact SET media_type='changed'",
        ),
        (
            "ALTER TABLE public.system_revision DISABLE TRIGGER system_revision_immutable",
            "UPDATE public.system_revision SET receipt_source='1970-01-01T00:00:00Z'",
        ),
    ] {
        let before = snapshot(connection).await;
        sqlx::query("BEGIN")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(disable)
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(statement)
            .execute(&mut *connection)
            .await
            .unwrap();
        assert_ne!(
            snapshot(connection).await,
            before,
            "disabled guard must expose mutation"
        );
        sqlx::query("ROLLBACK")
            .execute(&mut *connection)
            .await
            .unwrap();
        assert_eq!(snapshot(connection).await, before);
        immutable_rejection(connection, statement).await;
    }
    passed("immutable-history");

    let before = snapshot(connection).await;
    for change in [
        "UPDATE public.system_revision SET semantic_civil_second=1 WHERE id=$1::text::uuid",
        "UPDATE public.system_revision SET semantic_leap=true WHERE id=$1::text::uuid",
        "UPDATE public.system_revision SET semantic_fraction=0.0000012 WHERE id=$1::text::uuid",
        "UPDATE public.system_revision SET semantic_source='1970-01-01T00:00:00Z'
         WHERE id=$1::text::uuid",
        "UPDATE public.system_revision SET receipt_fraction=0 WHERE id=$1::text::uuid",
        "UPDATE public.system_revision SET receipt_source='1970-01-01T00:00:00Z'
         WHERE id=$1::text::uuid",
    ] {
        sqlx::query("BEGIN")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query("ALTER TABLE public.system_revision DISABLE TRIGGER system_revision_immutable")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(change)
            .bind(revision_id("4201").to_string())
            .execute(&mut *connection)
            .await
            .unwrap();
        assert!(matches!(
            RevisionRepository::get(connection, revision_id("4201")).await,
            Err(StorageError::InvalidStoredValue)
        ));
        sqlx::query("ROLLBACK")
            .execute(&mut *connection)
            .await
            .unwrap();
        assert_eq!(snapshot(connection).await, before);
    }
    for change in [
        "UPDATE public.source_artifact SET digest=decode(repeat('00',32),'hex')
         WHERE id=$1::text::uuid",
        "UPDATE public.source_artifact SET bytes=bytes || decode('20','hex')
         WHERE id=$1::text::uuid",
    ] {
        sqlx::query("BEGIN")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query("ALTER TABLE public.source_artifact DISABLE TRIGGER source_artifact_immutable")
            .execute(&mut *connection)
            .await
            .unwrap();
        sqlx::query(
            "ALTER TABLE public.source_artifact DROP CONSTRAINT source_artifact_digest_matches",
        )
        .execute(&mut *connection)
        .await
        .unwrap();
        sqlx::query(change)
            .bind(a.id.to_string())
            .execute(&mut *connection)
            .await
            .unwrap();
        assert!(matches!(
            ArtifactRepository::get(connection, a.id).await,
            Err(StorageError::InvalidStoredValue)
        ));
        sqlx::query("ROLLBACK")
            .execute(&mut *connection)
            .await
            .unwrap();
        assert_eq!(snapshot(connection).await, before);
        check_artifact(connection, &a, SHA_A).await;
    }
    // A complete changed-byte/digest pair remains a valid artifact in isolation,
    // but cannot pass the independent literal-byte oracle for retained A.
    sqlx::query("BEGIN")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query("ALTER TABLE public.source_artifact DISABLE TRIGGER source_artifact_immutable")
        .execute(&mut *connection)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE public.source_artifact SET bytes=$2,digest=sha256($2) WHERE id=$1::text::uuid",
    )
    .bind(a.id.to_string())
    .bind(C)
    .execute(&mut *connection)
    .await
    .unwrap();
    let changed = ArtifactRepository::get(connection, a.id)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(
        changed.bytes, A,
        "literal-byte oracle must detect changed source"
    );
    assert_ne!(changed.sha256, sha(SHA_A));
    sqlx::query("ROLLBACK")
        .execute(&mut *connection)
        .await
        .unwrap();
    assert_eq!(snapshot(connection).await, before);
    check_artifact(connection, &a, SHA_A).await;
    passed("checked-reconstruction");

    let maximum = source(
        "4501",
        &format!("application/{}", "x".repeat(1012)),
        &vec![90; 1_048_576],
    );
    assert_eq!(maximum.media_type.len(), 1024);
    ArtifactRepository::insert(connection, &maximum)
        .await
        .unwrap();
    check_artifact(
        connection,
        &maximum,
        "bf63d8a95fcc2e64619813aae35fdcbe871fdd9264caa3f365eb3aed0f679129",
    )
    .await;
    let empty = source("4502", "application/octet-stream", b"");
    ArtifactRepository::insert(connection, &empty)
        .await
        .unwrap();
    check_artifact(
        connection,
        &empty,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    )
    .await;
    let before = snapshot(connection).await;
    for invalid in [
        source("45f1", MEDIA, &vec![90; 1_048_577]),
        source("45f2", &"x".repeat(1025), A),
        source("45f3", "", A),
        source("45f4", "application/json\n", A),
        source("45f5", "application/\u{0001}json", A),
    ] {
        assert!(matches!(
            ArtifactRepository::insert(connection, &invalid).await,
            Err(StorageError::InvalidInput)
        ));
        assert_eq!(snapshot(connection).await, before);
    }
    migrate(connection).await.unwrap();
    check_schema(connection).await.unwrap();
    assert_eq!(snapshot(connection).await, before);
    passed("limits-and-reapply");
    println!("Required revision storage proof passed: 8 groups.");
}

fn main() {
    assert_eq!(
        std::env::args().count(),
        1,
        "no selection or target override"
    );
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            tokio::time::timeout(Duration::from_secs(90), async {
                let mut connection = PgConnection::connect(DSN).await.unwrap();
                run(&mut connection).await;
                connection.close().await.unwrap();
            })
            .await
            .expect("revision storage proof exceeded its fixed deadline");
        });
}
