# Isolated backup and restore of the first System

[Issue #26](https://github.com/DGIWG-P507/glaux-server/issues/26) adds a
reproducible backup/restore example for the data the server holds today: the
System slice from [creation](system-create.md) and [retrieval](system-read.md),
with its identities, revisions, source artifacts, write heads, audit, retry
receipts and outgoing work. The restored copy is an **inspection clone**. It can
be read by an authorised test client, but it is not activated for serving.

This is not production disaster recovery, failover, command reconciliation,
broker delivery or continuity-token recovery. Those belong to later tasks.

## Tools and prerequisites

- The approved PostgreSQL/PostGIS host's own `pg_dump`, `pg_restore`, `createdb`
  and `psql`. The hosted proof uses the pinned test image; nothing is installed.
- Run [scripts/system-restore.sh](../scripts/system-restore.sh) as that host's
  administrative OS user, against databases holding synthetic test data only.
- Choose the **inspection role** deliberately. It gains the right to connect to
  the clone and read it.
  - It must already exist and must not be a superuser. The script refuses
    `public` and the `current_user`-style names.
  - It must not be the credential of a running service. Otherwise that service
    could connect to the clone like any other database.
  - The hosted proof reuses the test's own serving role, because only the proof
    uses that role.

## Procedure

```sh
# 1. Stop writers, or accept that the dump's snapshot is the recovery point.
sh scripts/system-restore.sh backup glaux_source /secure/path/system.dump

# 2. Restore into a new, separately named database.
sh scripts/system-restore.sh restore /secure/path/system.dump \
    glaux_source glaux_restore_clone glaux_inspector
```

`backup` writes one consistent custom-format dump, readable only by its owner.
It refuses to write over an existing file or a symbolic link.

`restore` refuses, before creating anything:
- a target with the same name as the source;
- a target that already exists, including any user database;
- a missing dump file, unsafe names, or an unsuitable inspection role.

The source check compares names only. The existing-database refusal is what
protects the source and any other database.

The restore then:

1. Creates the target from `template0`.
2. Revokes `CONNECT` from `PUBLIC` straight away, before any data arrives.
3. Restores the whole dump in one transaction, stopping at the first error.
4. Revokes `INSERT`, `UPDATE`, `DELETE` and `TRUNCATE` on every table from the
   inspection role. This is the real write barrier.
5. Sets `default_transaction_read_only = on`. This is a session default, which
   any session can override, so it is a second guard, not the barrier.
6. Grants `CONNECT` to the inspection role only. Administrators keep access.
7. Checks the effective result: the role must have no write privilege on any
   table, including through `PUBLIC` or role membership, and `PUBLIC` must not
   be able to connect.

If any step after creation fails, the script says the target is **not
isolated** and must be dropped. Rerunning is refused while that target exists.
No step alters the source.

## Inspecting the clone

Point a server at the clone with a loopback development identity, a policy
granting `read`, and the inspection role's database URL. For example, start
from the [creation configuration](system-create.md#enable-the-bounded-example)
and change only the database URL. Retrieval then works as usual. Any POST fails
with `503` because the database refuses the write, so no resource, audit, retry
or outgoing-work row can be added.

There is no delivery worker in this slice, so outgoing work is never delivered
from either copy. The clone keeps the backed-up rows unchanged.

A successful GET or restart of the clone is **not** permission to serve it
normally or export from it. Activation is not implemented here.

## What a valid restore means

Record a manifest before the backup, then check the clone against it. A clone
is valid only if both of these hold:

1. **Independent manifest.** It matches every fact the client submitted and
   observed:
   - the generated IDs from Location;
   - UIDs, names and exact `featureType` spellings;
   - the exact submitted bytes, with a digest from a separate tool;
   - each revision, write head and outgoing-work row, linked to its System;
   - accepted audit with actor, source, operation, target, revision, time and
     the correlation returned to the client;
   - the denial audit of a refused attempt;
   - the retry receipt for a keyed request.
2. **Backup inventory.** The captured source state at the recovery point
   matches the clone:
   - all ten System-slice tables and the migration record;
   - the parent-write guard row;
   - a catalog fingerprint listing tables, extensions and their versions,
     constraints, triggers and indexes. Constraint text is compared without
     parentheses, because a dump and restore re-parses CHECK expressions and
     can regroup equivalent `AND` terms.

   Grants are deliberately excluded, because the clone's grants are narrowed.

A clone missing captured audit, or holding altered artifact bytes, fails
verification even when its System GET still returns the expected body. A
restore that is missing required state is a failed restore, not a gap to
accept.

## Recovery point and later history

The recovery point is the dump's snapshot. The restore recovers nothing after
it: later Systems, later audit, later retry receipts and later retention actions
are absent. The clone does not reconstruct them from current resources or logs.

- **Executed example:** a System created in the source after the backup is
  absent from the clone. GET for it returns `404`, and the clone still equals
  the backup inventory.
- **Specified, not executed:** suppose a System in the backup is later deleted
  from the source. The clone would still show it, and its tombstone would be
  missing too. Guide §4.7 sets the boundary before such a scope may ever serve
  or export:
  - First, available authoritative later deletion evidence is reconciled.
    Known valid deletions are never waived.
  - Where later evidence is absent, partial or ambiguous, the scope stays
    isolated by default.
  - Only where deployment policy permits may a restore-authorised operator
    record a decision to accept a limited recovery-point view instead. The
    record names the actor, reason, scope, evidence gaps, and whether reads
    and/or export are authorised.
  - A successful comparison with the backup alone does not establish current
    deletion state.

This slice has no public DELETE, deletion replay, re-export, epoch or fencing
mechanism, and this task does not implement them. Later deletion replay and
query checks belong to #126 and #251, and activation to #252.

## Disposal

When inspection ends, or after a failed restore, remove the clone and the dump
as the administrative OS user:

```sh
dropdb --host=/var/run/postgresql glaux_restore_clone
rm /secure/path/system.dump
```

Never point `dropdb` at the source or any other database. The hosted proof
disposes of everything by removing its whole disposable container.

Controlling sources are [Guide §§4.7, 4.10, 4.12 and 8.2 scenario 6][Guide].
The [independent proof](system-restore-tests.md) records the expected answers,
refusals, corrupt clones and fault controls. Execution results and separate
review belong in issue #26 and its PR.

[Guide]: https://github.com/DGIWG-P507/glaux/blob/8801afa2a52a617b511e7418206c915b9da78014/Docs/Plans/glaux-server/glaux-server-implementation-guide.md
