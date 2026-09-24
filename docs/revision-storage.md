# Initial immutable revisions and source artifacts

[Issue #14](https://github.com/DGIWG-P507/glaux-server/issues/14) implements
Guide v1.21 §§4.3, 4.7 and 6.1's initial retention boundary for the existing
System storage path. Original bytes, their interpretation and generated wire
output are different things. This module stores bytes and revision context;
it does not parse a document or certify its contents.

## Exact sources, separate identities

`source_artifact` stores an independent UUIDv7 artifact ID, media type, `bytea`
document and SHA-256 digest. PostgreSQL's built-in
[SHA-256 function](https://www.postgresql.org/docs/18/functions-binarystring.html)
computes the digest directly from the bound bytes; a database constraint also
requires consistency. Reads verify that consistency. No additional Cargo
dependency, extension, live fetch or JSON reserialization is needed.

The digest is a byte-integrity check, not identity, signature verification,
source authority or proof of a producer. Equal documents under different IDs
remain separate artifacts; conflicting reuse of an ID fails, never upserts.
Member order, whitespace, binary bytes and exact media-type spelling survive.
The storage layer accepts opaque bytes, including an empty payload; it does not
thereby admit them as a valid System or advertise their media type publicly.

Artifact and revision IDs are distinct Rust wrappers around the existing
UUIDv7 primitive, separate from the System's local ID. No ID is sorted to infer
revision ancestry, receipt time or current state. Labels and hashes do not
merge identities.

Storage has a local 1 MiB document budget and a 1,024-byte, nonempty media-type
label budget with control characters rejected. These are bounded internal
storage choices, not standards limits or endpoint/media-validation policy.
The existing structural validator has its own smaller input budget. Endpoint
owners must enforce their applicable admission and authorization contracts;
this repository API never provides permission to serve raw protected bytes.

## Revision and time contract

`system_revision` binds an independent revision ID to an actual System row and
an existing source artifact. A common identity row without a System row cannot
own a System revision. Foreign keys do not cascade away retained history.

Each revision retains an optional semantic **instant** and a required receipt
instant separately. Receipt is trusted application-supplied context, not a
value inferred from source bytes, the UUID clock, semantic time or SQL `now()`.
An absent semantic instant stays absent. Neither ordering between those times
nor a monotonic-arrival guarantee is invented.

Each instant uses the existing [exact-time contract](exact-time.md): `bigint`
civil second, separate leap-slot boolean, unconstrained `numeric` fraction and
the exact supplied source string. SQLx binds fractions as decimal text and
PostgreSQL numeric, never through floating point, fixed scale or `timestamp`.
Reads reconstruct with `ExactInstant::from_storage_parts`, rejecting a key that
does not match its retained source. The source preserves trailing zeroes,
precision and offset spelling. Optional semantic fields are all absent or all
present; partial time records fail.

This is not full SensorML `validTime`, interval selection or temporal querying.
An interval-bearing source document can be retained byte-for-byte without
interpreting its interval, collapsing it to a start instant, or filling a gap
with receipt time. Later family and temporal owners supply those semantics.

## Append-only behavior and transactions

`ArtifactRepository::insert/get` and `RevisionRepository::append/get` provide
the narrow internal operations. `create_with_artifact` commits one new artifact
and its matching revision together, requiring an existing System. Duplicate
IDs, absent or wrong-family references and invalid inputs fail; the paired
operation rolls back its earlier artifact insert if revision insertion fails.
No update, deletion, current-revision selector or implicit retry is offered.

Ordinary SQL UPDATE, DELETE and TRUNCATE are also rejected on retained artifact
and revision tables by statement triggers. Later revisions add rows; they cannot
rewrite earlier bytes, media, time context or artifact bindings. This is not a
tamper-proof ledger: a privileged actor able to disable triggers or change DDL
is outside that boundary. There is no automatic purge. A future authorised
retention implementation must preserve dependencies under the Guide rather
than quietly removing these protections.

Migrations are new immutable SQL files, embedded in the explicit administrative
command. Existing identity and parent migrations remain byte-for-byte intact.
Repository access checks schema compatibility, without installing or repairing
it. Reapplication must preserve complete rows and source bytes.

No full System mutation/current selection, concurrency preconditions,
resource/revision/audit/outbox orchestration (#15/#16), public history route,
SWE compilation, other family storage, restore or conformance claim is added.

## Verification

The [independent truth table](revision-storage-tests.md) was committed before
production implementation. The hosted real-database proof checks literal bytes,
independently computed digests and exact time keys/source metadata, not merely
record counts or encode/decode agreement. It includes deliberately byte-distinct
equivalent JSON, later revisions, failed-reference rollback, historical mutation
rejection, corruption/fault controls and migration preservation.

The issue/PR records the actual behavioral-red and green outcomes, including
any setup/formatting failures separately. Authored checks alone are not passes.
Runtime uses only the existing owned, pinned GitHub-hosted PostgreSQL/PostGIS
harness; no company-laptop runtime/install or persistent service is required.
