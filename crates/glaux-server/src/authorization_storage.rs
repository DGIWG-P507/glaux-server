//! Initial authorized System selection, not a public resource endpoint.
//!
//! Scope JSON comes only from the configured policy adapter. Source aliases and
//! producer assertions are descriptive data, never the ownership predicate.

use crate::storage::{StorageError, SystemRecord, check_schema};
use glaux_domain::identity::{LocalId, SourceIdentity};
use sqlx::{Connection, PgConnection, Row};

#[derive(Debug)]
pub struct SystemPage {
    pub items: Vec<SystemRecord>,
    pub number_matched: u64,
}

/// The original accepted application CREATE establishes source ownership. A
/// bare identity, missing source or ambiguous CREATE evidence grants nothing.
/// Both retained tables are immutable under the normal serving privileges.
pub(crate) async fn system_source(
    connection: &mut PgConnection,
    id: LocalId,
) -> Result<Option<String>, StorageError> {
    check_schema(connection).await?;
    let source: Option<String> = sqlx::query_scalar(
        "SELECT min(a.source COLLATE \"C\")
         FROM public.outgoing_work w
         JOIN public.server_audit a
           ON a.id=w.audit_id AND a.target_id=w.system_id
          AND a.revision_id=w.revision_id AND a.outcome=w.outcome
          AND a.operation=w.audit_operation
         WHERE w.system_id=$1::text::uuid AND w.kind='system.created'
           AND w.outcome='accepted' AND a.operation='system.create'
         GROUP BY w.system_id HAVING count(*)=1 AND count(a.source)=1",
    )
    .bind(id.to_string())
    .fetch_optional(connection)
    .await?;
    Ok(source)
}

/// One statement authorizes rows and parent links before counting and limiting,
/// then hydrates only selected records. The statement snapshot keeps the count,
/// aliases, label and links consistent without independent follow-up reads.
/// `resources: null` grants that source's records; an empty array grants none.
pub(crate) async fn list_systems(
    connection: &mut PgConnection,
    scope_json: &str,
    id: Option<LocalId>,
    limit: u16,
) -> Result<SystemPage, StorageError> {
    if connection.is_in_transaction() || !(1..=100).contains(&limit) {
        return Err(StorageError::InvalidInput);
    }
    check_schema(connection).await?;
    let rows = sqlx::query(
        "WITH origins AS MATERIALIZED (
           SELECT w.system_id, min(a.source COLLATE \"C\") AS source
           FROM public.outgoing_work w JOIN public.server_audit a
             ON a.id=w.audit_id AND a.target_id=w.system_id
            AND a.revision_id=w.revision_id AND a.outcome=w.outcome
            AND a.operation=w.audit_operation
           WHERE w.kind='system.created' AND w.outcome='accepted'
             AND a.operation='system.create'
           GROUP BY w.system_id HAVING count(*)=1 AND count(a.source)=1
         ), grants AS MATERIALIZED (
           SELECT source, resources
           FROM jsonb_to_recordset($1::jsonb) AS g(source text, resources jsonb)
         ), readable AS MATERIALIZED (
           SELECT o.system_id FROM origins o
           WHERE EXISTS (
             SELECT 1 FROM grants g WHERE g.source COLLATE \"C\"=o.source
               AND (g.resources IS NULL OR g.resources='null'::jsonb
                    OR g.resources @> to_jsonb(ARRAY[o.system_id::text]))
           )
         ), visible AS MATERIALIZED (
           SELECT r.system_id FROM readable r
           WHERE NOT EXISTS (
             SELECT 1 FROM public.system_parent p
             WHERE p.child_id=r.system_id
               AND NOT EXISTS (
                 SELECT 1 FROM readable parent WHERE parent.system_id=p.parent_id
               )
           ) AND ($2::text IS NULL OR r.system_id=$2::text::uuid)
         ), selected AS MATERIALIZED (
           SELECT system_id FROM visible ORDER BY system_id LIMIT $3
         ), counted AS (
           SELECT count(*) AS number_matched FROM visible
         )
         SELECT c.number_matched, r.id::text AS id, r.uid, s.label,
                p.parent_id::text AS parent, a.authority, a.identifier
         FROM counted c LEFT JOIN selected selected ON true
         LEFT JOIN public.resource_identity r ON r.id=selected.system_id
         LEFT JOIN public.system_identity s ON s.id=selected.system_id
         LEFT JOIN public.system_parent p ON p.child_id=selected.system_id
         LEFT JOIN public.source_identity a ON a.resource_id=selected.system_id
         ORDER BY selected.system_id, a.authority COLLATE \"C\", a.identifier COLLATE \"C\"",
    )
    .bind(scope_json)
    .bind(id.map(|id| id.to_string()))
    .bind(i64::from(limit))
    .fetch_all(connection)
    .await?;
    let first = rows.first().ok_or(StorageError::InvalidStoredValue)?;
    let number_matched = u64::try_from(first.try_get::<i64, _>("number_matched")?)
        .map_err(|_| StorageError::InvalidStoredValue)?;
    let mut items: Vec<SystemRecord> = Vec::new();
    for row in rows {
        let Some(raw_id) = row.try_get::<Option<String>, _>("id")? else {
            continue;
        };
        let id: LocalId = raw_id
            .parse()
            .map_err(|_| StorageError::InvalidStoredValue)?;
        if items.last().is_none_or(|record| record.id != id) {
            items.push(SystemRecord {
                id,
                uid: row
                    .try_get::<String, _>("uid")?
                    .parse()
                    .map_err(|_| StorageError::InvalidStoredValue)?,
                label: row.try_get("label")?,
                parent: row
                    .try_get::<Option<String>, _>("parent")?
                    .map(|value| value.parse())
                    .transpose()
                    .map_err(|_| StorageError::InvalidStoredValue)?,
                sources: Vec::new(),
            });
        }
        if let Some(authority) = row.try_get::<Option<String>, _>("authority")? {
            let record = items.last_mut().ok_or(StorageError::InvalidStoredValue)?;
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
    Ok(SystemPage {
        items,
        number_matched,
    })
}
