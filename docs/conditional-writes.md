# Initial conditional System writes

[Issue #16](https://github.com/DGIWG-P507/glaux-server/issues/16) implements Guide
v1.21 §§4.6 and 6.4 at the trusted application boundary. It adds one bounded
normalized System-label replacement, not full System PUT/PATCH or HTTP.
The caller must already authorize and validate the value, matching source
document and verified audit context.

## Optional condition

`application::update_system` takes an optional expected `RevisionId`. A match
permits the update; a mismatch returns `StorageError::PreconditionFailed` before
writing any resource, head, artifact, revision, audit or outgoing work. This is
the internal outcome intended for the later supplied-precondition HTTP 412
mapping, not an implemented response. Missing resources return `NotFound`;
existing low-level resources without a write head return
`UninitializedRevision` rather than guessing their current revision.

No condition is required. An unconditional valid update is permitted, even if
the caller read an older value. Row locking serializes writes but cannot detect
that stale client's intent; the later accepted value can overwrite the earlier
one. Accepted revisions and original artifacts remain retained.

**An internal revision ID is not an HTTP ETag.** Representation-specific,
authorized-view validators and authentication/authorization ordering remain
later HTTP work. Do not expose this as a global representation tag or infer
permissions from a match.

## Authoritative head and atomic transaction

Migration 0007 adds `system_write_head`, one per System, with a composite foreign
key binding the System, revision and artifact. It means the most recently
accepted write in this initial path, not temporal validity, freshness, greatest
UUID or timestamp. Low-level history insertion does not advance it.

Backfill uses only accepted CREATE work joined to its matching creation audit.
Multiple candidates fail the migration atomically; bare storage identities and
unattached historical revisions do not establish a head. Migrations 0001–0006
are unchanged. Reapplication does not reset an updated head. Migration remains
an explicit administrative operation, never a request action.

Creation inserts the head in its existing transaction. Updates own a READ
COMMITTED transaction on an idle SQLx-managed connection, irrespective of the
session default. They lock the System row, then read its head in a separate
statement so a waiter sees its predecessor's committed head before comparing.
This follows PostgreSQL's documented
[statement snapshots](https://www.postgresql.org/docs/18/transaction-iso.html#XACT-READ-COMMITTED)
and [row locks](https://www.postgresql.org/docs/18/explicit-locking.html#LOCKING-ROWS);
it is not distributed locking or scheduler fairness.

The new exact artifact and immutable revision, label, head, accepted
`system.update` audit and `system.updated` outgoing record commit together.
UID, source aliases and parent are unchanged. The database binds event kind to
audit operation, preserving the earlier CREATE-only guarantee. All failures roll
back; stale conditions append no separate denial audit. Receipts follow COMMIT;
transport loss during COMMIT can remain indeterminate, with no automatic retry.

No transaction crosses a device/network callback. Raw manually issued
transaction SQL and administrative/direct SQL mutation bypasses are unsupported
callers. Future accepted-write paths must preserve this head/lock/atomicity
contract rather than use low-level repositories as write handlers.

## Privileges, tests and limits

In addition to the [creation grants](atomic-write.md#serving-privilege-boundary),
the non-owner serving role needs SELECT/INSERT on the head and UPDATE of its
revision/artifact columns, plus UPDATE of the System label. It must not receive
retained-history mutation rights, ownership, trigger control or schema CREATE.
Migrations create no deployment roles or grants. Tests verify privileges only
inside the owned disposable database.

The [independent table](conditional-write-tests.md) defines exact expected
matching/stale/unconditional outcomes, rollback, migration discrimination and
bounded two-connection interleavings. A comparison-omission control must fail
the stale assertion after a passing baseline. Build/setup/timeout/cleanup
failure is not that proof. The issue/PR records actual execution.

On the approved GitHub-hosted Linux runner:

```sh
python3 -u scripts/check-execution.py conditional-write
python3 -u scripts/test-conditional-write-failures.py
```

No new dependency, permanent service or laptop installation is introduced.
HTTP, complete writable projections, worker delivery, valid-time
selection and backup/retention remain later tasks. No conformance is claimed.
Optional [creation retry identity](write-retries.md) does not extend to this
update operation or turn an internal revision into an HTTP validator.
