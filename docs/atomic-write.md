# Initial atomic System creation

[Issue #15](https://github.com/DGIWG-P507/glaux-server/issues/15) adds a trusted
application boundary, not an HTTP endpoint or a policy engine. It implements
Guide §§2.3, 4.6–4.8, 4.10 and 6.1 for the initial System-create path only.
The caller must already have authorized the operation and supplied validated,
normalized System data, its matching exact source document and verified audit
context. Document attribution is not authenticated identity.

## One commit

`application::create_system` owns one transaction on an idle SQLx-managed
connection. It inserts the System identity and description, source aliases and
optional parent link, exact source artifact, immutable revision, accepted audit
and outgoing-work record. [Task #16](conditional-writes.md) additionally inserts
the authoritative write head here. It calls connection-scoped insert helpers, not
repository methods that commit independently. All succeed together; a failed
write or rejected commit cannot leave a partially accepted operation. A receipt
is returned only after COMMIT succeeds. Transport loss during COMMIT can leave
the caller uncertain; this API does not retry or invent an outcome.

Migration 0006 adds `server_audit` and `outgoing_work`; migrations 0001–0005
remain unchanged. Outgoing work has its own stable UUIDv7 ID, the System and
revision IDs, event kind `system.created`, and an immutable artifact reference.
Composite foreign keys require that the revision, artifact and accepted audit
belong to that same System. The relevant event time is the exact recorded audit
time reached through its immutable reference, not a generated sequence or UUID
timestamp. No delivery order, broker acknowledgment or worker state is implied.

The transaction has no transport dependency, device adapter, network callback
or worker. There is no external effect to schedule before commit. A separate
connection can see the outgoing row only after the transaction commits. Later
workers must preserve that boundary; this initial proof cannot establish their
future delivery behavior.

## Audit and denial

Audit records retain supplied actor/source, operation, target and revision,
exact context time, outcome and correlation. An accepted operation requires a
verified actor; an unavailable verified source stays absent. Context time is
not claimed to be the physical commit instant. IDs retain their separate Rust
types; none supplies provenance or time authority.

The metadata fields accept at most 256 UTF-8 bytes each, reject empty supplied
values and Unicode controls, and do not accept arbitrary reason/payload fields.
The System-create boundary additionally limits label size to 4096 bytes and
source aliases to 64. These are local storage budgets, not OGC requirements.
Existing source-artifact limits still apply. Bounded text is not automatic
secret detection: callers must supply only safe identifiers, never credentials
or raw request content. Context has no automatic Debug or Deserialize path.

`record_denied_system_create` appends minimal safe denial metadata in its own
transaction without creating a System, revision, artifact or outgoing work.
Unknown actor/source/target remain absent. An optional requested target is
recorded without an existence lookup. Failure to record denial cannot produce
success-side work. Endpoint policy selection and rate/storage controls belong
to #22; this function is not a public unrestricted logging endpoint.

Audit has no resource foreign key: nonexistent denied targets are legitimate,
and resource lifecycle must not cascade away accountability. Statement triggers
reject audit and outgoing-work UPDATE, DELETE and TRUNCATE. Audit is not sampled
as operational logging. Authorized retention, restoration and administrators
are separate responsibilities, not implemented purge paths or tamper-proofing.

## Serving privilege boundary

Migrations do not create deployment roles or change existing grants. Use a
non-owner serving role, distinct from the migration/administrative owner, with
USAGE on the public schema, SELECT on the migration ledger, and the SELECT/INSERT
rights needed by these repositories. The parent-write guard additionally needs
SELECT/UPDATE. Creation also needs SELECT/INSERT on `system_write_head`;
[conditional updates](conditional-writes.md) name their additional column grants.
Do not grant serving UPDATE/DELETE/TRUNCATE on retained audit,
revision, artifact or outgoing-work tables, table ownership, schema CREATE,
administrative membership, or superuser authority. Ordinary serving must not
disable the retention triggers. These deployment grants must be verified;
declaring a role name alone does not establish isolation.

Tests create an explicit restricted role only inside the owned disposable
database. They prove the application can create through it while audit mutation
and trigger-disabling are denied, and separately prove immutable triggers still
reject mutation under deliberately overbroad DML grants. An administrator can
alter schema or disable triggers; this is not a claim of protection from that
administrator, append-only infrastructure or cryptographic tamper evidence.

## Proof and limits

The [independent truth table](atomic-write-tests.md) precedes production changes.
The real SQLx proof compares exact stored facts, twelve injected rollback
boundaries and an explicitly synchronized second-connection snapshot. It also
checks denial failures, migration preservation, nested-transaction rejection and
serving privileges. A disposable source control omits outgoing insertion and
must fail the specific independent assertion after a passing baseline.

Run on the authorized GitHub-hosted Linux runner:

```sh
python3 -u scripts/check-execution.py atomic-write
python3 -u scripts/test-atomic-write-failures.py
```

No local installation or user database is required. Setup, build, timeout and
cleanup errors remain failures, not proof of behavioral detection.
Raw manually issued transaction SQL is not a supported caller contract.
Low-level storage repositories remain primitives; future accepted-write handlers
must use the application boundary rather than bypass it. Other
resource families, HTTP policy, public deletion, temporal selection, dispatch
and backup/retention remain later tasks. The initial conditional label update
and accepted-write head now exist under [task #16](conditional-writes.md).
Optional scoped creation retry identity now exists under [task #17](write-retries.md).
