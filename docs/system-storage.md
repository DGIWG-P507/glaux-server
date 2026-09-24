# Initial System identity storage

[Task #13](https://github.com/DGIWG-P507/glaux-server/issues/13) implements the
first Rust-to-PostgreSQL repository path, following Guide §§4.2, 4.7 and 6.1.
It creates and reads System identities, their source identifiers and an optional
System parent. This is an internal foundation, not a CSAPI endpoint or complete
System description model.

## Preserved identities and relationships

- The shared `resource_identity` table owns the canonical UUIDv7 and exact URI
  UID. Only the System family is admitted by this initial migration. A separate
  `system_identity` row supplies the initial family boundary and label.
- A label is not identity. A UID is not normalized, truncated or scoped to its
  source authority. Duplicate local IDs or exact UIDs cause a conflict; no
  upsert, label-based merge or automatic identity reconciliation occurs.
- Source identity is the exact `(authority, identifier)` pair. Two authorities
  can use the same identifier for different Systems. One System can have
  several aliases, including several under one authority. Assigning an existing
  exact pair to another System conflicts. This unambiguous pair ownership is a
  narrow Glaux storage choice, not an additional OGC uniqueness requirement.
- `system_parent` permits zero or one parent, with both foreign keys referencing
  actual System rows. A common identity row alone is not a valid endpoint.
  Self-parenting and cycles fail. Reverse lookup has an index; full hierarchy
  queries, mutation workflows and other relationship families are later tasks.

The UID and source fields retain the existing 4,096-byte per-field domain budget.
Native PostgreSQL `EXCLUDE USING hash (... WITH =)` constraints compare full
lexical values using the `C` collation. The source pair uses a byte-length-prefixed
expression, so `('a','bc')` and `('ab','c')` cannot become the same key. A hash
collision is not identity equality: PostgreSQL rechecks the full expression.
This avoids a B-tree entry-size restriction on otherwise valid long identities.
See PostgreSQL's [hash index behavior](https://www.postgresql.org/docs/18/hash-index.html)
and [exclusion constraints](https://www.postgresql.org/docs/18/sql-createtable.html#SQL-CREATETABLE-EXCLUDE).

Only parent-edge mutations serialize through one guard row. Its actual update
is held until transaction completion: following volatile trigger queries see
the current graph at READ COMMITTED, while a stale higher-isolation writer must
abort rather than accept a stale graph. Missing guard state fails closed.
Privileged DDL, trigger disabling and administrative corruption are not defended
against by ordinary constraints. This is not a tamper-proof database.

## Rust operations and transaction boundary

`SystemRepository::create` inserts identity, System, aliases and optional parent
in one transaction. Any rejected constraint rolls everything back. There is no
automatic retry, merge, update, delete, revision, audit or outbox operation here.
`get`, `find_uid` and `find_source` return checked domain identities with all
aliases in lexical order. The composite record read uses one SQL statement;
source/UID lookup first resolves a local ID. Concurrent reassignments are not an
offered operation in this slice; later mutation work must preserve consistent
lookup semantics when introducing them.

SQLx `0.9.0` and Tokio `1.53.1` are pinned in the server package only. Domain and
standards libraries do not gain database dependencies. The dependency inventory
records their actual transitive graph/features/notices. No compile-time query
macro or development database is needed to build the packaged SQL.

## Explicit administrative commands

On the approved runtime, configure `GLAUX_DATABASE_URL` for the intended database
using the deployment's secret-handling practice; do not put credentials in
arguments, issue comments or logs. These commands do not provision a database:

```sh
glaux-server migrate
glaux-server check-schema
```

`migrate` applies the embedded immutable migrations explicitly, with SQLx's
transaction/checksum/locking machinery and ledger in `public._sqlx_migrations`.
This initial layout is public-schema-only. A read-only preflight requires
`current_schema()` to be `public` before any migration writes, including the
unchanged version-1 extension command. A role-owned schema first in the search
path must be deliberately corrected by the administrator; it is not migrated.
Version 1 remains unchanged; version 2 adds identity/System/source storage and
version 3 adds parent storage. Existing version-2 identities and aliases must
survive version 3 exactly. Future schema changes need a new migration, not an
edit to an applied file. Failed migrations are failures, not silently skipped.

`check-schema` only reads compatibility evidence. Missing, incomplete, unknown,
dirty or checksum-mismatched ledger entries fail; it never repairs a ledger or
auto-upgrades a schema. It is not a full catalog-tampering or backup-integrity
check. Repository operations also check this compatibility boundary. A startup
with no explicit command still fails: no listening service exists yet.

Network database commands force TLS certificate/hostname verification. An
explicit local Unix-socket connection uses that socket's local security boundary.
The command has a 30-second deadline and 10-second statement/5-second lock limits;
timeouts remain failures requiring inspection, not automatic retries. Public
diagnostics omit connection strings and database error details. Full typed
configuration, role provisioning and service startup belong to later tasks.

## Verification and limits

The [independent truth table and executable proof](system-storage-tests.md) were
specified before SQL implementation. The hosted suite uses the existing owned,
network-isolated PostgreSQL/PostGIS harness: the Rust binary is copied into its
validated container, never connected to an external database. It checks exact
values and whole-table rollback, not merely matching counts or self-roundtrips.
Separate SQL controls remove selected constraints only in rolled-back disposable
transactions, showing that the same invalid operation would otherwise succeed.
Concurrent cases synchronize on observed database locks, not assumed delays.

No company-laptop runtime or installation is required. Results, initial failures,
tested commits and separate review are recorded on the issue/PR; authored tests
alone are not passing evidence. Full System schemas/endpoints, authorization,
immutable revisions/source artifacts (#14), audit/outbox, non-reuse after future
deletion, restore and other resource families remain with their owning tasks.
No performance, release readiness or CSAPI conformance is claimed.
