//! Trusted initial System-create boundary; no HTTP policy or external delivery.
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

/// Returned only after COMMIT succeeds, not evidence of external delivery.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WriteReceipt {
    pub system_id: LocalId,
    pub revision_id: RevisionId,
    pub artifact_id: ArtifactId,
    pub audit_id: AuditId,
    pub event_id: EventId,
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
) -> Result<(), StorageError> {
    sqlx::query(
        "INSERT INTO public.server_audit
         (id, actor, source, operation, target_id, revision_id,
          time_civil_second, time_leap, time_fraction, time_source, outcome, correlation)
         VALUES ($1::text::uuid, $2, $3, 'system.create', $4::text::uuid, $5::text::uuid,
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
        )
        .await?;
        // Required outgoing work shares this transaction.
        // Intentionally absent in the initial behavioral-red candidate.
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
