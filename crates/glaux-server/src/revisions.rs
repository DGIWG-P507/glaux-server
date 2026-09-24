//! Internal exact source/revision storage; not public history or access policy.

use crate::storage::{StorageError, check_schema};
use glaux_domain::identity::{GenerationError, IdentityError, LocalId};
use glaux_domain::temporal::ExactInstant;
use sqlx::postgres::PgRow;
use sqlx::{Connection, PgConnection, Row};
use std::{fmt, str::FromStr};

/// Local storage budgets, not standards limits or public media admission rules.
pub const MAX_SOURCE_BYTES: usize = 1_048_576;
pub const MAX_MEDIA_TYPE_BYTES: usize = 1024;

macro_rules! stored_id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
        pub struct $name(LocalId);

        impl $name {
            pub fn generate() -> Result<Self, GenerationError> {
                LocalId::generate().map(Self)
            }
        }

        impl FromStr for $name {
            type Err = IdentityError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                value.parse().map(Self)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

stored_id!(ArtifactId);
stored_id!(RevisionId);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewSourceArtifact {
    pub id: ArtifactId,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceArtifact {
    pub id: ArtifactId,
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub sha256: [u8; 32],
}

/// Semantic time is an optional instant, not full SensorML validTime support.
/// Receipt time is supplied by trusted application context, never inferred
/// from a UUID, source document, semantic time or database clock.
#[derive(Clone, Debug)]
pub struct SystemRevision {
    pub id: RevisionId,
    pub system_id: LocalId,
    pub artifact_id: ArtifactId,
    pub semantic_time: Option<ExactInstant>,
    pub receipt_time: ExactInstant,
}

fn valid_artifact(media_type: &str, bytes: &[u8]) -> bool {
    !media_type.is_empty()
        && media_type.len() <= MAX_MEDIA_TYPE_BYTES
        && !media_type.chars().any(char::is_control)
        && bytes.len() <= MAX_SOURCE_BYTES
}

fn artifact_from_row(row: &PgRow) -> Result<SourceArtifact, StorageError> {
    let media_type: String = row.try_get("media_type")?;
    let bytes: Vec<u8> = row.try_get("bytes")?;
    if !valid_artifact(&media_type, &bytes) || !row.try_get::<bool, _>("digest_matches")? {
        return Err(StorageError::InvalidStoredValue);
    }
    Ok(SourceArtifact {
        id: row
            .try_get::<String, _>("id")?
            .parse()
            .map_err(|_| StorageError::InvalidStoredValue)?,
        media_type,
        bytes,
        sha256: row
            .try_get::<Vec<u8>, _>("digest")?
            .try_into()
            .map_err(|_| StorageError::InvalidStoredValue)?,
    })
}

async fn insert_artifact(
    connection: &mut PgConnection,
    artifact: &NewSourceArtifact,
) -> Result<SourceArtifact, StorageError> {
    let row = sqlx::query(
        "INSERT INTO public.source_artifact(id, media_type, bytes, digest)
         VALUES ($1::text::uuid, $2, $3, sha256($3))
         RETURNING id::text, media_type, bytes, digest, digest = sha256(bytes) AS digest_matches",
    )
    .bind(artifact.id.to_string())
    .bind(&artifact.media_type)
    .bind(&artifact.bytes)
    .fetch_one(connection)
    .await?;
    artifact_from_row(&row)
}

pub struct ArtifactRepository;

impl ArtifactRepository {
    /// Byte-preserving storage only: no parsing, normalization, fetch or deduplication.
    pub async fn insert(
        connection: &mut PgConnection,
        artifact: &NewSourceArtifact,
    ) -> Result<SourceArtifact, StorageError> {
        if !valid_artifact(&artifact.media_type, &artifact.bytes) {
            return Err(StorageError::InvalidInput);
        }
        check_schema(connection).await?;
        let mut transaction = connection.begin().await?;
        match insert_artifact(&mut transaction, artifact).await {
            Ok(stored) => {
                transaction.commit().await?;
                Ok(stored)
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }

    /// Internal read; endpoint owners must authorize access before disclosing bytes.
    pub async fn get(
        connection: &mut PgConnection,
        id: ArtifactId,
    ) -> Result<Option<SourceArtifact>, StorageError> {
        check_schema(connection).await?;
        let row = sqlx::query(
            "SELECT id::text, media_type, bytes, digest,
                    digest = sha256(bytes) AS digest_matches
             FROM public.source_artifact WHERE id=$1::text::uuid",
        )
        .bind(id.to_string())
        .fetch_optional(connection)
        .await?;
        row.as_ref().map(artifact_from_row).transpose()
    }
}

async fn insert_revision(
    connection: &mut PgConnection,
    revision: &SystemRevision,
) -> Result<(), StorageError> {
    let semantic = revision.semantic_time.as_ref();
    let receipt = &revision.receipt_time;
    sqlx::query(
        "INSERT INTO public.system_revision
         (id, system_id, artifact_id,
          semantic_civil_second, semantic_leap, semantic_fraction, semantic_source,
          receipt_civil_second, receipt_leap, receipt_fraction, receipt_source)
         VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid,
                 $4, $5, $6::text::numeric, $7, $8, $9, $10::text::numeric, $11)",
    )
    .bind(revision.id.to_string())
    .bind(revision.system_id.to_string())
    .bind(revision.artifact_id.to_string())
    .bind(semantic.map(ExactInstant::civil_second))
    .bind(semantic.map(ExactInstant::is_leap_second))
    .bind(semantic.map(ExactInstant::fraction_decimal))
    .bind(semantic.map(ExactInstant::source_lexeme))
    .bind(receipt.civil_second())
    .bind(receipt.is_leap_second())
    .bind(receipt.fraction_decimal())
    .bind(receipt.source_lexeme())
    .execute(connection)
    .await?;
    Ok(())
}

fn revision_from_row(row: &PgRow) -> Result<SystemRevision, StorageError> {
    let semantic_second: Option<i64> = row.try_get("semantic_civil_second")?;
    let semantic_leap: Option<bool> = row.try_get("semantic_leap")?;
    let semantic_fraction: Option<String> = row.try_get("semantic_fraction")?;
    let semantic_source: Option<String> = row.try_get("semantic_source")?;
    let semantic_time = match (
        semantic_second,
        semantic_leap,
        semantic_fraction,
        semantic_source,
    ) {
        (None, None, None, None) => None,
        (Some(second), Some(leap), Some(fraction), Some(source)) => Some(
            ExactInstant::from_storage_parts(second, leap, &fraction, &source)
                .map_err(|_| StorageError::InvalidStoredValue)?,
        ),
        _ => return Err(StorageError::InvalidStoredValue),
    };
    let receipt_time = ExactInstant::from_storage_parts(
        row.try_get("receipt_civil_second")?,
        row.try_get("receipt_leap")?,
        &row.try_get::<String, _>("receipt_fraction")?,
        &row.try_get::<String, _>("receipt_source")?,
    )
    .map_err(|_| StorageError::InvalidStoredValue)?;
    Ok(SystemRevision {
        id: row
            .try_get::<String, _>("id")?
            .parse()
            .map_err(|_| StorageError::InvalidStoredValue)?,
        system_id: row
            .try_get::<String, _>("system_id")?
            .parse()
            .map_err(|_| StorageError::InvalidStoredValue)?,
        artifact_id: row
            .try_get::<String, _>("artifact_id")?
            .parse()
            .map_err(|_| StorageError::InvalidStoredValue)?,
        semantic_time,
        receipt_time,
    })
}

pub struct RevisionRepository;

impl RevisionRepository {
    /// Append against an existing artifact. No current-state mutation or ordering inference.
    pub async fn append(
        connection: &mut PgConnection,
        revision: &SystemRevision,
    ) -> Result<(), StorageError> {
        check_schema(connection).await?;
        let mut transaction = connection.begin().await?;
        match insert_revision(&mut transaction, revision).await {
            Ok(()) => {
                transaction.commit().await?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }

    /// Narrow atomic artifact/revision pair. The System must already exist.
    /// Resource/revision/audit/outbox orchestration belongs to the next task.
    pub async fn create_with_artifact(
        connection: &mut PgConnection,
        artifact: &NewSourceArtifact,
        revision: &SystemRevision,
    ) -> Result<(), StorageError> {
        if artifact.id != revision.artifact_id
            || !valid_artifact(&artifact.media_type, &artifact.bytes)
        {
            return Err(StorageError::InvalidInput);
        }
        check_schema(connection).await?;
        let mut transaction = connection.begin().await?;
        let result = async {
            insert_artifact(&mut transaction, artifact).await?;
            insert_revision(&mut transaction, revision).await
        }
        .await;
        match result {
            Ok(()) => {
                transaction.commit().await?;
                Ok(())
            }
            Err(error) => {
                transaction.rollback().await?;
                Err(error)
            }
        }
    }

    /// Reconstruct checked exact instants. Source/key inconsistencies are failures.
    pub async fn get(
        connection: &mut PgConnection,
        id: RevisionId,
    ) -> Result<Option<SystemRevision>, StorageError> {
        check_schema(connection).await?;
        let row = sqlx::query(
            "SELECT id::text, system_id::text, artifact_id::text,
                    semantic_civil_second, semantic_leap,
                    semantic_fraction::text, semantic_source,
                    receipt_civil_second, receipt_leap,
                    receipt_fraction::text, receipt_source
             FROM public.system_revision WHERE id=$1::text::uuid",
        )
        .bind(id.to_string())
        .fetch_optional(connection)
        .await?;
        row.as_ref().map(revision_from_row).transpose()
    }
}
