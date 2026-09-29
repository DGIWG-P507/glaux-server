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
- The serving role (here `glaux_app`) already exists in the cluster, because
  the dump restores its grants.

## Procedure

```sh
# 1. Stop writers, or accept that the dump's snapshot is the recovery point.
sh scripts/system-restore.sh backup glaux_source /secure/path/system.dump

# 2. Restore into a new, separately named database.
sh scripts/system-restore.sh restore /secure/path/system.dump \
    glaux_source glaux_restore_clone glaux_app
```

`backup` writes one consistent custom-format dump. It refuses to overwrite an
existing dump file.

`restore` refuses, before doing anything destructive:
- a target with the same name as the source;
- a target that already exists, including any user database;
- a missing dump file or unsafe names.

It then creates the target from `template0`, restores the whole dump in one
transaction (stopping at the first error), and isolates the clone:

- It revokes `INSERT`, `UPDATE`, `DELETE` and `TRUNCATE` on every table from
  the serving role.
- It sets `default_transaction_read_only = on` for the database.
- It revokes `CONNECT` from `PUBLIC` and grants it only to the named role.
  Administrators keep access for inspection.

No step alters the source.

## Inspecting the clone

Point a server at the clone with a loopback development identity, a policy
granting `read`, and the database URL of the named role. For example, start
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
2. **Backup inventory.** Every table equals the source's captured state at the
   recovery point, including the migration record.

A clone missing captured audit, or holding altered artifact bytes, fails
verification even when its System GET still returns the expected body. A
restore that is missing required state is a failed restore, not a gap to
accept.

## Recovery point and later history

The recovery point is the dump's snapshot. The restore recovers nothing after
it: later Systems, later audit and later retry receipts are absent. The clone
does not reconstruct them from current resources or logs.

- **Executed example:** a System created in the source after the backup is
  absent from the clone. GET for it returns `404`, and every clone table still
  equals the backup inventory.
- **Specified, not executed:** suppose a System in the backup is later deleted
  from the source. The clone would still show it, and its tombstone would be
  missing too.
  - Guide §4.7 therefore keeps such a clone non-serving and non-exporting by
    default.
  - Available authoritative later deletion evidence must be reconciled before
    that scope opens.
  - Where deployment policy permits, a restore-authorised operator may instead
    record a decision to accept a limited recovery-point view. That decision
    names its actor, reason, scope and gaps.
  - A successful comparison with the backup alone does not establish current
    deletion state.

This slice has no public DELETE, deletion replay, re-export, epoch or fencing
mechanism, and this task does not implement them. Later deletion replay and
query checks belong to #126 and #251, and activation to #252.

Controlling sources are [Guide §§4.7, 4.10, 4.12 and 8.2 scenario 6][Guide].
The [independent proof](system-restore-tests.md) records the expected answers,
refusals, corrupt clones and fault controls. Execution results and separate
review belong in issue #26 and its PR.

[Guide]: https://github.com/DGIWG-P507/glaux/blob/8801afa2a52a617b511e7418206c915b9da78014/Docs/Plans/glaux-server/glaux-server-implementation-guide.md
