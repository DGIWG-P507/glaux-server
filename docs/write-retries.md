# Optional retries for initial System creation

[Issue #17](https://github.com/DGIWG-P507/glaux-server/issues/17) implements
Guide v1.21 §§4.6/6.4 on the existing trusted application boundary. It does not
add an HTTP header parser, authentication service, command execution or a public
receipt API. The future HTTP extension remains optional.

## Same request, same committed outcome

`application::create_system_with_retry` accepts an optional `RetryKey`. The
existing `create_system` remains unkeyed: equal values alone never mean retry.
A key belongs to the **verified actor, optional verified source, creation
operation and target**. The target is the root System collection or the exact
parent System, derived from the submitted association; it is not the freshly
generated resource ID. Missing source and a supplied source are different scopes.
The only supported retry operation is `system.create`; update and future command
operations cannot borrow a creation receipt.

Within that scope, a live key and identical intent return the original
`WriteReceipt`: the same System, revision, artifact, audit and outgoing-work IDs.
The retry creates none of them again, even if it arrives with new generated IDs,
receipt/audit times or correlation metadata. It returns the original accepted
outcome, not a fresh snapshot of a subsequently updated System. Different intent
returns a safe `Conflict` without accepted mutation.

Intent consists of the exact UID, label, parent, sorted authority/identifier
alias pairs, optional semantic-time source spelling, artifact media type and
exact bytes. Fields are length-framed under a versioned domain prefix and hashed
with SHA-256. Alias order is immaterial; duplicate aliases are invalid. Generated
IDs, receipt/audit times and correlation are excluded. This initial conservative
contract does **not** promise equivalence for differently serialized JSON,
URI/media-type spelling or offset-equivalent time spelling. Request normalization
and document/semantic consistency remain the trusted caller's responsibility.

Every call requires fresh authentication and operation authorization from its
trusted caller. Its synchronous local authorization callback examines the selected
original receipt before replay or content-conflict disclosure, or the candidate
before a new admission. Returning false yields `Denied` without mutation. This
callback must use verified current policy context, not request-body claims; it
must neither expose its argument to an untrusted caller nor call an external
service while the transaction is open. This is the integration seam, not a
completed HTTP/policy implementation. Later handlers must not bypass it via the
unkeyed or low-level storage APIs.

## Atomicity, concurrency and expiry

Migration 0008 adds `system_create_retry`; migrations 0001–0007 are unchanged.
A unique constraint includes absent-source equality. A composite foreign key
binds the recorded outcome to the exact matching creation work and its existing
revision/audit bindings. The retry record commits in the same transaction as the
resource, source, revision, relationships, head, audit and outgoing work.
An error, including rejected COMMIT, rolls the entire admission back.

The boundary owns a READ COMMITTED transaction on an idle SQLx-managed connection.
A transaction advisory lock over the scoped key precedes resource/parent locks;
the subsequent exact-field lookup sees a predecessor's committed receipt.
Hash collisions can cause additional waiting, never identity equivalence.
See PostgreSQL's [transaction advisory locks](https://www.postgresql.org/docs/18/explicit-locking.html#ADVISORY-LOCKS),
[statement snapshots](https://www.postgresql.org/docs/18/transaction-iso.html#XACT-READ-COMMITTED)
and [unique null handling](https://www.postgresql.org/docs/18/ddl-constraints.html#DDL-CONSTRAINTS-UNIQUE-CONSTRAINTS).
Administrative/direct SQL writers are not supported application callers.

`retention_seconds` is trusted configuration, not a client request field. It is a
positive whole-second `u32`; no universal or implicit default is selected. The
database samples its wall clock when recording the receipt, and stores that
instant plus the configured interval. A replay neither extends the deadline nor
applies changed retention configuration to a live record. Eligibility is checked
against database wall time **after** any lock wait; equality with the deadline is
expired. Receipt lifetime is measured from recording, not a claimed exact COMMIT
instant. Clock accuracy/adjustments are deployment concerns.

An expired key can admit a new operation, subject to ordinary identity and
authorization rules. Its scoped receipt may then be replaced atomically; original
resource, revision, audit and outgoing evidence remain. There is no automatic
purge job or global deletion policy. Identical content with an existing immutable
UID may conflict rather than create another resource; after expiry no retry
identity guarantee remains. A lost receipt or rollback to an earlier backup
likewise ends that guarantee. Retrying after a lost response recovers an outcome
only if its committed receipt is still retained and disclosure is authorized.
No automatic network retry, command-retention policy or exactly-once external
effect is introduced.

## Serving privileges and proof

In addition to the existing [creation privileges](atomic-write.md#serving-privilege-boundary),
the serving role needs SELECT/INSERT on the retry table and UPDATE only of its
digest, outcome IDs and retention timestamp columns. Scope columns cannot be
changed by that role. Do not grant DELETE/TRUNCATE, ownership, trigger control or
schema CREATE; migrations do not create roles or grant deployment privileges.
Existing immutable evidence remains protected.

The independent expected-state table is in
[`retry-write-proof.rs`](../crates/glaux-server/examples/retry-write-proof.rs).
It exercises sequential and concurrent retries, changed content/scope,
lost-response recovery, denied disclosure, configured expiry, transaction
failure and bounded generated sequences against the owned real database.
A disposable content-comparison fault must fail its specific assertion between
passing original/restored runs. Setup, compile, timeout or cleanup failure does
not establish behavioral detection. Actual outcomes belong to the issue/PR,
not an assumed pass in this document.

On the approved GitHub-hosted Linux environment:

```sh
python3 -u scripts/check-execution.py retry-write
python3 -u scripts/test-retry-write-failures.py
```

No new dependency, company-laptop installation or permanent service is needed.
Public HTTP replay responses, other resource families, command/broker retry,
deletion, restore and broader policy enforcement remain their owning tasks.
