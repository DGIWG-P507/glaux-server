# Immutable revision and source-artifact proof

This initial truth table precedes the production implementation for
[issue #14 / task 1.3.2](https://github.com/DGIWG-P507/glaux-server/issues/14).
It applies Guide v1.21 §§4.3, 4.7, 6.1 and 8.1.1: preserve original bounded
bytes separately from interpreted content, retain exact time and its source
context, and do not rewrite retained history. The issue/PR records execution;
authored expectations below are not claimed passes.

## Independent source documents

These ASCII fixtures are storage inputs, not claims of complete SensorML
validation. A and B have identical JSON meaning but different member order and
whitespace. C changes the label. No server serializer supplies expected bytes.
Digests were independently calculated with PowerShell/.NET SHA-256 before
production implementation; the hosted Python harness recomputes them with
`hashlib` before accepting execution evidence.

| Name | Exact bytes (escaped LF is one `0a` byte) | Length | SHA-256 |
|---|---|---:|---|
| A | `{"type":"PhysicalSystem","label":"Alpha","value":1}` | 51 | `8d3e448241a86daedf90f6ea36eebbc62c82829307bcb6eae1a9954c031ec215` |
| B | `{\n  "value": 1, "label": "Alpha", "type": "PhysicalSystem"\n}\n` | 61 | `27c281a95454810345d0780509dafb4154b3807ea473658aba7e5f09b41e9268` |
| C | `{"type":"PhysicalSystem","label":"Beta","value":1}` | 50 | `045014d3404cc5fa1f1d0b98c6d3852111a94bfbfa63356e451c8caa076e6c3b` |

The first two use media type `application/json`; C uses
`application/json; profile="urn:glaux:fixture"`, whose exact spelling must
survive storage. Artifact, revision and System identities are independently
supplied UUIDv7 literals, never a digest substituted for an identity.

## Exact-time expectations

Reuse the independent civil-second coordinates and precision boundary cases
from [the exact-time database proof](../scripts/test_time_database.py).
Semantic time and receipt time are separate assertions, not interchangeable.

| Source | UTC civil second | Leap slot | Fraction | Precision / source offset |
|---|---:|---|---|---|
| `1970-01-01T00:00:00.0000011Z` | 0 | false | `0.0000011` | 7 digits; 0; local offset not asserted by Z |
| `1970-01-01T00:00:00.0000012Z` | 0 | false | `0.0000012` | 7 digits; 0; local offset not asserted by Z |
| `1969-12-31T18:59:59.999999999-05:00` | -1 | false | `0.999999999` | 9 digits; -18000; known local offset |
| `2017-01-01T00:59:60.0000000001+01:00` | 1483228799 | true | `0.0000000001` | 10 digits; 3600; known local offset |
| `1970-01-01T00:00:00-00:00` | 0 | false | `0` | 0 digits; 0; unknown local offset |
| Epoch plus `000000001` + 90 zeroes + `1` fractional digits | 0 | false | All 100 digits retained | 100 digits, Z source |
| Epoch plus 4074 zeroes + `1` fractional digits | 0 | false | All 4075 digits retained | 4096-byte timestamp, Z source |

Assert keys directly from SQL against these answers, and assert source lexeme,
fraction digits and offset metadata explicitly after Rust reconstruction.
`ExactInstant` equality alone cannot prove lexical/precision preservation.
The two microsecond-neighbor values must remain unequal and on opposite sides
of an exact SQL boundary; ordinary PostgreSQL timestamps collapse them and
serve only as a deliberately lossy sensitivity control.

## Required behavior and discriminating cases

| Operation | Expected result and forbidden effects |
|---|---|
| Upgrade the existing identity schema | Existing System rows survive; revisions/artifacts start empty; migrations are explicit and repeatable |
| Insert A and B under different artifact IDs | Both exact documents, media types and independently expected digests survive; equivalent JSON does not merge them |
| Insert C and a new revision | Exact artifact/revision/System references persist; previous revision rows, times and source artifacts remain unchanged |
| Store an absent semantic instant | Every semantic key/source column is NULL; no default to receipt time |
| Store precision/offset/leap fixtures | Exact independent keys and lexical metadata survive real SQLx/PostgreSQL round trips |
| Append referencing missing artifact | Reject and retain complete before/after row snapshots; no partial revision |
| Atomically create artifact and revision for missing System | Reject and roll back the earlier artifact insert too |
| Use common identity without System family row | Reject typed revision ownership; no partial artifact/revision |
| Reuse revision ID or artifact ID | Conflict, not replacement; every retained row remains unchanged |
| Mismatch the artifact ID in the atomic pair | Reject without either insertion |
| Update retained artifact/revision, including historical bindings/times | Reject; compare every original row, not only count/ID |
| Delete or truncate retained artifact/revision tables | Reject at this append-only storage boundary; no generic deletion/purge API introduced |
| Deliberately disable the applicable immutability trigger inside a transaction | The otherwise forbidden change becomes observable; rollback restores both rows and enforcement |
| Corrupt digest or time key/source in a disposable transaction | Checked repository reconstruction rejects the inconsistency rather than silently normalizing it; rollback restores valid reads |
| Admit exact byte/media budgets, then exceed each | Boundary inputs persist unchanged; excess input rejects before mutation |
| Reapply migrations after data exists | All application rows, exact bytes and time context remain unchanged |

Snapshots use independent raw SQL projections of all affected application
tables, including complete bytes/digest and both time representations, in a
fixed order. Same-count mutations cannot satisfy this oracle. Missing-reference
tests include an earlier write where the API promises a paired transaction.

During implementation, reviewer-directed direct-SQL cases strengthen the
`atomic-rejection` group without claiming they were in the initial executable
proof. Two otherwise-valid INSERT controls establish that the fixtures really
insert. Eighteen mutations must then fail with SQLSTATE `23514` and the exact
owning constraint: one partially present semantic instant; six non-finite or
out-of-range fractions each in the semantic and receipt columns; a mismatched
32-byte digest and an incorrectly sized digest; and empty, LF-containing and
control-character media types. Every transaction rolls back and compares full
before/after snapshots, including valid controls. Invalid media forms are also
rejected by the Rust insertion boundary, without mutation. None of these
constraint checks substitutes for checked exact-time reconstruction.

## Execution and scope

The proof runs only inside the existing owned, pinned, network-isolated
PostgreSQL/PostGIS harness on GitHub-hosted Linux. No laptop Rust/Python/Docker
execution or installation is required. No caller-selected database/URL is
accepted. Setup, build, missing markers, failed assertions, SQL errors and
cleanup failures remain failures, never skipped/passed checks.

The intended behavioral red is a specific assertion that historical UPDATE
must reject, against the initially mutable tables. A compilation/setup failure
does not establish that red. Record actual first failure and subsequent green
on the PR, plus controlled-fault results from a passing baseline.

This leaf does not implement current-revision selection, full SensorML
valid-time intervals, public history, authorization, SWE contract compilation,
observation/command storage, resource/audit/outbox orchestration, retention or
restore. Optional semantic time is an instant only; interval-bearing source
bytes can be preserved without interpreting them. A privileged actor able to
disable triggers or change DDL is outside the ordinary immutability claim.
