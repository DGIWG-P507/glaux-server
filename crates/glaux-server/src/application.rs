//! Trusted initial System-write boundary; no HTTP policy or external delivery.
//!
//! Callers supply already authorized, normalized data and verified audit context.
//! These types do not establish authentication or validate SensorML semantics.

use crate::revisions::{
    ArtifactId, NewSourceArtifact, RevisionId, SystemRevision, insert_artifact, insert_revision,
    valid_artifact,
};
use crate::storage::{StorageError, SystemRecord, check_schema, insert_system};
use glaux_domain::identity::{GenerationError, IdentityError, LocalId};
use glaux_domain::temporal::ExactInstant;
use sqlx::{Connection, PgConnection};
use std::{fmt, str::FromStr};

macro_rules! transaction_id {
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

transaction_id!(AuditId);
transaction_id!(EventId);

/// Bounded safe metadata supplied by trusted application code, not request-body
/// attribution. Deliberately not Debug/Deserialize: no automatic logging or wire
/// admission. Missing denial identity remains unknown; never invent an actor.
#[derive(Clone)]
pub struct AuditContext {
    pub actor: Option<String>,
    pub source: Option<String>,
    pub correlation: String,
    pub time: ExactInstant,
}

pub struct CreateSystem {
    pub system: SystemRecord,
    pub artifact: NewSourceArtifact,
    pub revision: SystemRevision,
    pub audit_id: AuditId,
    pub event_id: EventId,
    pub audit: AuditContext,
}

/// Initial normalized label replacement only, not full System PUT/PATCH.
/// The caller supplies authorized data, matching source and trusted attribution.
pub struct UpdateSystem {
    pub system_id: LocalId,
    pub label: String,
    pub artifact: NewSourceArtifact,
    pub revision: SystemRevision,
    pub audit_id: AuditId,
    pub event_id: EventId,
    pub audit: AuditContext,
    /// Internal accepted-write revision, not an HTTP representation validator.
    /// None permits a valid unconditional write, including stale-client overwrite.
    pub expected_revision: Option<RevisionId>,
}

/// Returned only after COMMIT succeeds, not evidence of external delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteReceipt {
    pub system_id: LocalId,
    pub revision_id: RevisionId,
    pub artifact_id: ArtifactId,
    pub audit_id: AuditId,
    pub event_id: EventId,
}

/// Trusted configuration, not request-provided retention. No default horizon.
pub struct RetryKey {
    pub key: String,
    pub retention_seconds: u32,
}

/// Test-first API skeleton: receipt persistence is not implemented yet.
pub async fn create_system_with_retry<F>(
    connection: &mut PgConnection,
    input: &CreateSystem,
    retry: Option<&RetryKey>,
    mut authorize: F,
) -> Result<WriteReceipt, StorageError>
where
    F: FnMut(&WriteReceipt) -> bool,
{
    if retry.is_some_and(|retry| !valid_metadata(&retry.key) || retry.retention_seconds == 0) {
        return Err(StorageError::InvalidInput);
    }
    let candidate = WriteReceipt {
        system_id: input.system.id,
        revision_id: input.revision.id,
        artifact_id: input.artifact.id,
        audit_id: input.audit_id,
        event_id: input.event_id,
    };
    if !authorize(&candidate) {
        return Err(StorageError::Denied);
    }
    create_system(connection, input).await
}

pub struct DeniedSystemCreate {
    pub audit_id: AuditId,
    /// Safe requested identifier only, not a lookup or disclosure of existence.
    pub target: Option<LocalId>,
    pub audit: AuditContext,
}

fn valid_metadata(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn valid_context(context: &AuditContext) -> bool {
    context.actor.as_deref().is_none_or(valid_metadata)
        && context.source.as_deref().is_none_or(valid_metadata)
        && valid_metadata(&context.correlation)
}

async fn insert_audit(
    connection: &mut PgConnection,
    id: AuditId,
    target: Option<LocalId>,
    revision: Option<RevisionId>,
    context: &AuditContext,
    outcome: &str,
    operation: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO public.server_audit
         (id, actor, source, operation, target_id, revision_id,
          time_civil_second, time_leap, time_fraction, time_source, outcome, correlation)
         VALUES ($1::text::uuid, $2, $3, $12, $4::text::uuid, $5::text::uuid,
                 $6, $7, $8::text::numeric, $9, $10, $11)",
    )
    .bind(id.to_string())
    .bind(&context.actor)
    .bind(&context.source)
    .bind(target.map(|id| id.to_string()))
    .bind(revision.map(|id| id.to_string()))
    .bind(context.time.civil_second())
    .bind(context.time.is_leap_second())
    .bind(context.time.fraction_decimal())
    .bind(context.time.source_lexeme())
    .bind(outcome)
    .bind(&context.correlation)
    .bind(operation)
    .execute(connection)
    .await?;
    Ok(())
}

async fn insert_outgoing(
    connection: &mut PgConnection,
    input: &CreateSystem,
) -> Result<(), StorageError> {
    insert_work(
        connection,
        &WriteReceipt {
            system_id: input.system.id,
            revision_id: input.revision.id,
            artifact_id: input.artifact.id,
            audit_id: input.audit_id,
            event_id: input.event_id,
        },
        "system.created",
    )
    .await
}

async fn insert_work(
    connection: &mut PgConnection,
    receipt: &WriteReceipt,
    kind: &str,
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO public.outgoing_work
         (id, system_id, revision_id, artifact_id, audit_id, kind, outcome)
         VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid,
                 $4::text::uuid, $5::text::uuid, $6, 'accepted')",
    )
    .bind(receipt.event_id.to_string())
    .bind(receipt.system_id.to_string())
    .bind(receipt.revision_id.to_string())
    .bind(receipt.artifact_id.to_string())
    .bind(receipt.audit_id.to_string())
    .bind(kind)
    .execute(connection)
    .await?;
    Ok(())
}

/// Own the complete transaction on an idle, SQLx-managed connection. Low-level
/// repositories remain storage primitives, not substitutes for this boundary.
/// No retry is attempted: transport loss during COMMIT can be indeterminate.
pub async fn create_system(
    connection: &mut PgConnection,
    input: &CreateSystem,
) -> Result<WriteReceipt, StorageError> {
    if connection.is_in_transaction()
        || input.system.id != input.revision.system_id
        || input.artifact.id != input.revision.artifact_id
        || !valid_artifact(&input.artifact.media_type, &input.artifact.bytes)
        || !valid_context(&input.audit)
        || input.audit.actor.is_none()
        || input.system.label.len() > 4096
        || input.system.sources.len() > 64
    {
        return Err(StorageError::InvalidInput);
    }
    check_schema(connection).await?;
    let mut transaction = connection.begin_with("BEGIN").await?;
    let result = async {
        insert_system(&mut transaction, &input.system).await?;
        insert_artifact(&mut transaction, &input.artifact).await?;
        insert_revision(&mut transaction, &input.revision).await?;
        insert_audit(
            &mut transaction,
            input.audit_id,
            Some(input.system.id),
            Some(input.revision.id),
            &input.audit,
            "accepted",
            "system.create",
        )
        .await?;
        // Required outgoing work shares this transaction.
        insert_outgoing(&mut transaction, input).await?;
        sqlx::query(
            "INSERT INTO public.system_write_head (system_id, revision_id, artifact_id)
             VALUES ($1::text::uuid, $2::text::uuid, $3::text::uuid)",
        )
        .bind(input.system.id.to_string())
        .bind(input.revision.id.to_string())
        .bind(input.artifact.id.to_string())
        .execute(&mut *transaction)
        .await?;
        Ok::<_, StorageError>(())
    }
    .await;
    match result {
        Ok(()) => {
            transaction.commit().await?;
            Ok(WriteReceipt {
                system_id: input.system.id,
                revision_id: input.revision.id,
                artifact_id: input.artifact.id,
                audit_id: input.audit_id,
                event_id: input.event_id,
            })
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}

/// Append minimal safe denial context without looking up or creating resources
/// or outgoing work. Endpoint selection, rate/storage budgets and HTTP mapping
/// belong to the later policy boundary; this is not an unrestricted audit API.
pub async fn record_denied_system_create(
    connection: &mut PgConnection,
    input: &DeniedSystemCreate,
) -> Result<(), StorageError> {
    if connection.is_in_transaction() || !valid_context(&input.audit) {
        return Err(StorageError::InvalidInput);
    }
    check_schema(connection).await?;
    let mut transaction = connection.begin_with("BEGIN").await?;
    match insert_audit(
        &mut transaction,
        input.audit_id,
        input.target,
        None,
        &input.audit,
        "denied",
        "system.create",
    )
    .await
    {
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

fn check_revision(expected: Option<RevisionId>, current: RevisionId) -> Result<(), StorageError> {
    if expected.is_some_and(|expected| expected != current) {
        return Err(StorageError::PreconditionFailed);
    }
    Ok(())
}

/// Own a short READ COMMITTED transaction on an idle SQLx-managed connection.
/// Lock the existing System before obtaining its authoritative head. The head
/// read is a separate statement so it sees a predecessor that committed while
/// this writer waited. No missing-condition policy, retry or external effect.
pub async fn update_system(
    connection: &mut PgConnection,
    input: &UpdateSystem,
) -> Result<WriteReceipt, StorageError> {
    if connection.is_in_transaction()
        || input.system_id != input.revision.system_id
        || input.artifact.id != input.revision.artifact_id
        || !valid_artifact(&input.artifact.media_type, &input.artifact.bytes)
        || !valid_context(&input.audit)
        || input.audit.actor.is_none()
        || input.label.len() > 4096
    {
        return Err(StorageError::InvalidInput);
    }
    check_schema(connection).await?;
    let mut transaction = connection
        .begin_with("BEGIN ISOLATION LEVEL READ COMMITTED")
        .await?;
    let receipt = WriteReceipt {
        system_id: input.system_id,
        revision_id: input.revision.id,
        artifact_id: input.artifact.id,
        audit_id: input.audit_id,
        event_id: input.event_id,
    };
    let result = async {
        let locked: Option<String> = sqlx::query_scalar(
            "SELECT id::text FROM public.system_identity WHERE id = $1::text::uuid FOR UPDATE",
        )
        .bind(input.system_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        if locked.is_none() {
            return Err(StorageError::NotFound);
        }
        let current: Option<String> = sqlx::query_scalar(
            "SELECT revision_id::text FROM public.system_write_head WHERE system_id = $1::text::uuid",
        )
        .bind(input.system_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        let current = current
            .ok_or(StorageError::UninitializedRevision)?
            .parse()
            .map_err(|_| StorageError::InvalidStoredValue)?;
        check_revision(input.expected_revision, current)?;
        insert_artifact(&mut transaction, &input.artifact).await?;
        insert_revision(&mut transaction, &input.revision).await?;
        sqlx::query("UPDATE public.system_identity SET label = $2 WHERE id = $1::text::uuid")
            .bind(input.system_id.to_string())
            .bind(&input.label)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE public.system_write_head SET revision_id = $2::text::uuid, artifact_id = $3::text::uuid
             WHERE system_id = $1::text::uuid",
        )
        .bind(input.system_id.to_string())
        .bind(input.revision.id.to_string())
        .bind(input.artifact.id.to_string())
        .execute(&mut *transaction)
        .await?;
        insert_audit(
            &mut transaction,
            input.audit_id,
            Some(input.system_id),
            Some(input.revision.id),
            &input.audit,
            "accepted",
            "system.update",
        )
        .await?;
        insert_work(&mut transaction, &receipt, "system.updated").await?;
        Ok::<_, StorageError>(())
    }
    .await;
    match result {
        Ok(()) => {
            transaction.commit().await?;
            Ok(receipt)
        }
        Err(error) => {
            transaction.rollback().await?;
            Err(error)
        }
    }
}
