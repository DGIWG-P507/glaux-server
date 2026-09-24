//! Initial System identity storage. No HTTP, revisions, audit, or deletion API.

use glaux_domain::identity::{LocalId, SourceIdentity, Uid};
use sqlx::migrate::{Migration, MigrationType, Migrator};
use sqlx::{Connection, PgConnection, Row, SqlSafeStr};
use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemRecord {
    pub id: LocalId,
    pub uid: Uid,
    pub label: String,
    pub sources: Vec<SourceIdentity>,
    pub parent: Option<LocalId>,
}

#[derive(Debug)]
pub enum StorageError {
    Conflict,
    InvalidAssociation,
    InvalidInput,
    Immutable,
    IncompatibleSchema,
    InvalidStoredValue,
    Database(sqlx::Error),
    Migration(sqlx::migrate::MigrateError),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Database details may contain protected values: do not emit them here.
        f.write_str(match self {
            Self::Conflict => "identity or association conflict",
            Self::InvalidAssociation => "invalid System association",
            Self::InvalidInput => "storage input violates its bounded contract",
            Self::Immutable => "retained history is immutable",
            Self::IncompatibleSchema => "database schema is not compatible",
            Self::InvalidStoredValue => "stored value violates its typed contract",
            Self::Database(_) => "database operation failed",
            Self::Migration(_) => "database migration failed",
        })
    }
}
impl std::error::Error for StorageError {}

impl From<sqlx::Error> for StorageError {
    fn from(error: sqlx::Error) -> Self {
        match error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref()
        {
            Some("23505" | "23P01") => Self::Conflict,
            Some("23503" | "23514") => Self::InvalidAssociation,
            Some("55000") => Self::Immutable,
            _ => Self::Database(error),
        }
    }
}

/// Immutable SQL embedded in the binary; no runtime directory or schema fetch.
pub fn packaged_migrations() -> Migrator {
    let mut migrator = Migrator::with_migrations(vec![
        Migration::new(
            1,
            "enable PostGIS".into(),
            MigrationType::Simple,
            include_str!("../migrations/0001_enable_postgis.sql").into_sql_str(),
            false,
        ),
        Migration::new(
            2,
            "System identity".into(),
            MigrationType::Simple,
            include_str!("../migrations/0002_system_identity.sql").into_sql_str(),
            false,
        ),
        Migration::new(
            3,
            "System parent".into(),
            MigrationType::Simple,
            include_str!("../migrations/0003_system_parent.sql").into_sql_str(),
            false,
        ),
        Migration::new(
            4,
            "System revisions and source artifacts".into(),
            MigrationType::Simple,
            include_str!("../migrations/0004_system_revisions.sql").into_sql_str(),
            false,
        ),
        Migration::new(
            5,
            "immutable retained history".into(),
            MigrationType::Simple,
            include_str!("../migrations/0005_immutable_history.sql").into_sql_str(),
            false,
        ),
    ]);
    migrator.dangerous_set_table_name("public._sqlx_migrations");
    migrator
}

/// Explicit administrative operation. Normal repository access never calls it.
pub async fn migrate(connection: &mut PgConnection) -> Result<(), StorageError> {
    // Version 1 is immutable and installs PostGIS in the current schema.
    // Reject another target before any DDL; do not migrate a caller-selected namespace.
    let schema: Option<String> = sqlx::query_scalar("SELECT current_schema()::text")
        .fetch_one(&mut *connection)
        .await?;
    if schema.as_deref() != Some("public") {
        return Err(StorageError::IncompatibleSchema);
    }
    packaged_migrations()
        .run(connection)
        .await
        .map_err(StorageError::Migration)
}

/// Read-only compatibility check; missing, dirty, edited, or unknown migrations fail.
pub async fn check_schema(connection: &mut PgConnection) -> Result<(), StorageError> {
    let exists: bool =
        sqlx::query_scalar("SELECT to_regclass('public._sqlx_migrations') IS NOT NULL")
            .fetch_one(&mut *connection)
            .await?;
    if !exists {
        return Err(StorageError::IncompatibleSchema);
    }
    let rows = sqlx::query(
        "SELECT version, checksum, success FROM public._sqlx_migrations ORDER BY version",
    )
    .fetch_all(&mut *connection)
    .await?;
    let migrations = packaged_migrations();
    if rows.len() != migrations.migrations.len() {
        return Err(StorageError::IncompatibleSchema);
    }
    for (row, expected) in rows.iter().zip(migrations.iter()) {
        if row.try_get::<i64, _>("version")? != expected.version
            || row.try_get::<Vec<u8>, _>("checksum")?.as_slice() != expected.checksum.as_ref()
            || !row.try_get::<bool, _>("success")?
        {
            return Err(StorageError::IncompatibleSchema);
        }
    }
    Ok(())
}

pub struct SystemRepository;

impl SystemRepository {
    /// Create, never upsert/merge. Any identity, alias or parent failure rolls back all rows.
    pub async fn create(
        connection: &mut PgConnection,
        record: &SystemRecord,
    ) -> Result<(), StorageError> {
        check_schema(connection).await?;
        let mut transaction = connection.begin().await?;
        let result = async {
            sqlx::query("INSERT INTO public.resource_identity(id, family, uid) VALUES ($1::text::uuid, 'system', $2)")
                .bind(record.id.to_string()).bind(record.uid.as_str()).execute(&mut *transaction).await?;
            sqlx::query("INSERT INTO public.system_identity(id, label) VALUES ($1::text::uuid, $2)")
                .bind(record.id.to_string()).bind(&record.label).execute(&mut *transaction).await?;
            for source in &record.sources {
                sqlx::query("INSERT INTO public.source_identity(resource_id, authority, identifier) VALUES ($1::text::uuid, $2, $3)")
                    .bind(record.id.to_string()).bind(source.authority().as_str()).bind(source.identifier().as_str())
                    .execute(&mut *transaction).await?;
            }
            if let Some(parent) = record.parent {
                sqlx::query("INSERT INTO public.system_parent(child_id, parent_id) VALUES ($1::text::uuid, $2::text::uuid)")
                    .bind(record.id.to_string()).bind(parent.to_string()).execute(&mut *transaction).await?;
            }
            Ok::<(), sqlx::Error>(())
        }.await;
        match result {
            Ok(()) => {
                transaction.commit().await?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error.into())
            }
        }
    }

    /// All constituent rows are read by one statement, including zero/multiple aliases.
    pub async fn get(
        connection: &mut PgConnection,
        id: LocalId,
    ) -> Result<Option<SystemRecord>, StorageError> {
        check_schema(connection).await?;
        let rows = sqlx::query(
            "SELECT r.id::text AS id, r.uid, s.label, p.parent_id::text AS parent, a.authority, a.identifier
             FROM public.system_identity s JOIN public.resource_identity r ON r.id=s.id
             LEFT JOIN public.system_parent p ON p.child_id=s.id
             LEFT JOIN public.source_identity a ON a.resource_id=s.id
             WHERE s.id=$1::text::uuid ORDER BY a.authority COLLATE \"C\", a.identifier COLLATE \"C\""
        ).bind(id.to_string()).fetch_all(connection).await?;
        let Some(first) = rows.first() else {
            return Ok(None);
        };
        let mut record = SystemRecord {
            id: first
                .try_get::<String, _>("id")?
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue)?,
            uid: first
                .try_get::<String, _>("uid")?
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue)?,
            label: first.try_get("label")?,
            parent: first
                .try_get::<Option<String>, _>("parent")?
                .map(|value| value.parse())
                .transpose()
                .map_err(|_| StorageError::InvalidStoredValue)?,
            sources: Vec::new(),
        };
        for row in rows {
            if let Some(authority) = row.try_get::<Option<String>, _>("authority")? {
                record.sources.push(SourceIdentity::new(
                    authority
                        .parse()
                        .map_err(|_| StorageError::InvalidStoredValue)?,
                    row.try_get::<String, _>("identifier")?
                        .parse()
                        .map_err(|_| StorageError::InvalidStoredValue)?,
                ));
            }
        }
        Ok(Some(record))
    }

    pub async fn find_uid(
        connection: &mut PgConnection,
        uid: &Uid,
    ) -> Result<Option<SystemRecord>, StorageError> {
        check_schema(connection).await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT id::text FROM public.resource_identity WHERE uid=$1 COLLATE \"C\"",
        )
        .bind(uid.as_str())
        .fetch_optional(&mut *connection)
        .await?;
        match id {
            Some(id) => {
                Self::get(
                    connection,
                    id.parse().map_err(|_| StorageError::InvalidStoredValue)?,
                )
                .await
            }
            None => Ok(None),
        }
    }

    pub async fn find_source(
        connection: &mut PgConnection,
        source: &SourceIdentity,
    ) -> Result<Option<SystemRecord>, StorageError> {
        check_schema(connection).await?;
        let id: Option<String> = sqlx::query_scalar(
            "SELECT resource_id::text FROM public.source_identity
             WHERE (octet_length(authority)::text || ':' || authority || identifier) COLLATE \"C\"
                 = (octet_length($1::text)::text || ':' || $1 || $2) COLLATE \"C\"",
        )
        .bind(source.authority().as_str())
        .bind(source.identifier().as_str())
        .fetch_optional(&mut *connection)
        .await?;
        match id {
            Some(id) => {
                Self::get(
                    connection,
                    id.parse().map_err(|_| StorageError::InvalidStoredValue)?,
                )
                .await
            }
            None => Ok(None),
        }
    }
}
