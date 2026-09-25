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
use sqlx::{Connection, PgConnection, Row};
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

/// Optional terminal-creation retry. Callers reauthenticate before every call.
/// The local trusted callback authorizes the selected original outcome on replay,
/// or the candidate on admission. It must not perform external I/O or disclose
/// its argument to an untrusted caller. HTTP policy wiring remains separate.
pub async fn create_system_with_retry<F>(
    connection: &mut PgConnection,
    input: &CreateSystem,
    retry: Option<&RetryKey>,
    authorize: F,
) -> Result<WriteReceipt, StorageError>
where
    F: FnMut(&WriteReceipt) -> bool,
{
    create_system_inner(connection, input, retry, authorize).await
}

// Versioned, unambiguous length framing; no serializers or concatenated fields.
// Alias order is not persisted. Duplicate aliases remain invalid, not deduped.
// Semantic time uses its exact source lexeme; this conservative initial contract
// does not promise JSON, URI, media-type or time-spelling equivalence.
fn retry_content(input: &CreateSystem) -> Result<Vec<u8>, StorageError> {
    fn field(output: &mut Vec<u8>, value: &[u8]) {
        output.extend_from_slice(&(value.len() as u64).to_be_bytes());
        output.extend_from_slice(value);
    }
    let mut aliases: Vec<_> = input
        .system
        .sources
        .iter()
        .map(|source| (source.authority().as_str(), source.identifier().as_str()))
        .collect();
    aliases.sort_unstable();
    if aliases.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(StorageError::InvalidInput);
    }
    let mut content = b"glaux.system-create-intent.v1".to_vec();
    field(&mut content, input.system.uid.as_str().as_bytes());
    field(&mut content, input.system.label.as_bytes());
    field(
        &mut content,
        input
            .system
            .parent
            .map(|id| id.to_string())
            .unwrap_or_default()
            .as_bytes(),
    );
    content.extend_from_slice(&(aliases.len() as u64).to_be_bytes());
    for (authority, identifier) in aliases {
        field(&mut content, authority.as_bytes());
        field(&mut content, identifier.as_bytes());
    }
    field(
        &mut content,
        input
            .revision
            .semantic_time
            .as_ref()
            .map(|time| time.source_lexeme())
            .unwrap_or("")
            .as_bytes(),
    );
    field(&mut content, input.artifact.media_type.as_bytes());
    field(&mut content, &input.artifact.bytes);
    Ok(content)
}

struct PreparedRetry<'a> {
    actor: &'a str,
    source: Option<&'a str>,
    target: String,
    key: &'a RetryKey,
    digest: Vec<u8>,
}

impl PreparedRetry<'_> {
    async fn replay<F>(
        &self,
        connection: &mut PgConnection,
        authorize: &mut F,
    ) -> Result<Option<WriteReceipt>, StorageError>
    where
        F: FnMut(&WriteReceipt) -> bool,
    {
        // Acquire before any resource/parent lock. Hash collisions only serialize;
        // exact fields below, not the hash, decide identity. Transaction release
        // handles success, error and disconnect; no persistent placeholder claim.
        sqlx::query(
            "SELECT pg_advisory_xact_lock(hashtextextended(
             'glaux.system-create-retry.v1' ||
             jsonb_build_array($1::text,$2::text,'system.create',$3::text,$4::text)::text, 0))",
        )
        .bind(self.actor)
        .bind(self.source)
        .bind(&self.target)
        .bind(&self.key.key)
        .execute(&mut *connection)
        .await?;
        // A separate READ COMMITTED statement sees a predecessor after waiting.
        // Database wall clock is evaluated here, not before acquiring the lock.
        let row = sqlx::query(
            "SELECT digest, system_id::text, revision_id::text, artifact_id::text,
                    audit_id::text, event_id::text
             FROM public.system_create_retry
             WHERE actor=$1 AND source IS NOT DISTINCT FROM $2
               AND operation='system.create' AND target=$3 AND key=$4
               AND expires_at > clock_timestamp()",
        )
        .bind(self.actor)
        .bind(self.source)
        .bind(&self.target)
        .bind(&self.key.key)
        .fetch_optional(&mut *connection)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let receipt = WriteReceipt {
            system_id: row
                .try_get::<String, _>("system_id")?
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue)?,
            revision_id: row
                .try_get::<String, _>("revision_id")?
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue)?,
            artifact_id: row
                .try_get::<String, _>("artifact_id")?
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue)?,
            audit_id: row
                .try_get::<String, _>("audit_id")?
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue)?,
            event_id: row
                .try_get::<String, _>("event_id")?
                .parse()
                .map_err(|_| StorageError::InvalidStoredValue)?,
        };
        // Revoke disclosure before distinguishing equal from conflicting intent.
        if !authorize(&receipt) {
            return Err(StorageError::Denied);
        }
        let stored_digest: Vec<u8> = row.try_get("digest")?;
        let digest = &self.digest;
        if stored_digest != *digest {
            return Err(StorageError::Conflict);
        }
        Ok(Some(receipt))
    }

    async fn store(
        &self,
        connection: &mut PgConnection,
        receipt: &WriteReceipt,
    ) -> Result<(), StorageError> {
        // No automatic purge: only this expired scoped receipt may be replaced.
        // Original resources, revisions, audit and outgoing evidence remain.
        // The retention clock sample is taken once at receipt recording.
        let recorded: Option<bool> = sqlx::query_scalar(
            "WITH sampled AS MATERIALIZED (SELECT clock_timestamp() AS now)
             INSERT INTO public.system_create_retry
             (actor,source,operation,target,key,digest,system_id,revision_id,
              artifact_id,audit_id,event_id,retained_at,expires_at)
             SELECT $1,$2,'system.create',$3,$4,$5,$6::text::uuid,$7::text::uuid,
                    $8::text::uuid,$9::text::uuid,$10::text::uuid,now,
                    now + make_interval(secs => $11::double precision) FROM sampled
             ON CONFLICT ON CONSTRAINT system_create_retry_scope DO UPDATE
             SET digest=EXCLUDED.digest,system_id=EXCLUDED.system_id,
                 revision_id=EXCLUDED.revision_id,artifact_id=EXCLUDED.artifact_id,
                 audit_id=EXCLUDED.audit_id,event_id=EXCLUDED.event_id,
                 retained_at=EXCLUDED.retained_at,expires_at=EXCLUDED.expires_at
             WHERE system_create_retry.expires_at <= EXCLUDED.retained_at
             RETURNING true",
        )
        .bind(self.actor)
        .bind(self.source)
        .bind(&self.target)
        .bind(&self.key.key)
        .bind(&self.digest)
        .bind(receipt.system_id.to_string())
        .bind(receipt.revision_id.to_string())
        .bind(receipt.artifact_id.to_string())
        .bind(receipt.audit_id.to_string())
        .bind(receipt.event_id.to_string())
        .bind(f64::from(self.key.retention_seconds))
        .fetch_optional(connection)
        .await?;
        if recorded != Some(true) {
            return Err(StorageError::Conflict);
        }
        Ok(())
    }
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
    create_system_inner(connection, input, None, |_| true).await
}

async fn create_system_inner<F>(
    connection: &mut PgConnection,
    input: &CreateSystem,
    retry: Option<&RetryKey>,
    mut authorize: F,
) -> Result<WriteReceipt, StorageError>
where
    F: FnMut(&WriteReceipt) -> bool,
{
    if connection.is_in_transaction()
        || input.system.id != input.revision.system_id
        || input.artifact.id != input.revision.artifact_id
        || !valid_artifact(&input.artifact.media_type, &input.artifact.bytes)
        || !valid_context(&input.audit)
        || input.audit.actor.is_none()
        || input.system.label.len() > 4096
        || input.system.sources.len() > 64
        || retry.is_some_and(|retry| !valid_metadata(&retry.key) || retry.retention_seconds == 0)
    {
        return Err(StorageError::InvalidInput);
    }
    check_schema(connection).await?;
    let content = retry.map(|_| retry_content(input)).transpose()?;
    let prepared = if let (Some(key), Some(content)) = (retry, content) {
        Some(PreparedRetry {
            actor: input
                .audit
                .actor
                .as_deref()
                .ok_or(StorageError::InvalidInput)?,
            source: input.audit.source.as_deref(),
            target: input
                .system
                .parent
                .map(|id| id.to_string())
                .unwrap_or_default(),
            key,
            digest: sqlx::query_scalar("SELECT sha256($1::bytea)")
                .bind(content)
                .fetch_one(&mut *connection)
                .await?,
        })
    } else {
        None
    };
    let receipt = WriteReceipt {
        system_id: input.system.id,
        revision_id: input.revision.id,
        artifact_id: input.artifact.id,
        audit_id: input.audit_id,
        event_id: input.event_id,
    };
    let mut transaction = connection
        .begin_with("BEGIN ISOLATION LEVEL READ COMMITTED")
        .await?;
    let result = async {
        if let Some(prepared) = &prepared
            && let Some(recorded) = prepared.replay(&mut transaction, &mut authorize).await?
        {
            return Ok(recorded);
        }
        if !authorize(&receipt) {
            return Err(StorageError::Denied);
        }
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
        if let Some(prepared) = &prepared {
            prepared.store(&mut transaction, &receipt).await?;
        }
        Ok::<_, StorageError>(receipt)
    }
    .await;
    match result {
        Ok(receipt) => {
            transaction.commit().await?;
            Ok(receipt)
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
