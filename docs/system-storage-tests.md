# Initial System storage: independent expected results

The initial truth table was authored before the SQL implementation for
[issue #13 / task 1.3.1](https://github.com/DGIWG-P507/glaux-server/issues/13).
The controlling sources are [Guide v1.21 §§4.2, 4.7, 6.1 and 8.1.1][Guide]:
preserve distinct local IDs, exact URI UIDs and authority-qualified source IDs;
reject conflicts instead of merging; enforce typed association endpoints; and
apply explicit migrations without losing existing data.

Commit `74ad160` records that initial table. Later lexical, namespace and
concurrency cases refine the tests during implementation; they are not claimed
as pre-implementation execution or a behavioral red run.

These are expected results, not recorded successful execution. The issue/PR
must supply actual tested-head, runner, command, failure/retry and review evidence.
No Rust, Python or database runtime is used on the company laptop to author this
document or its companion [SQLx proof](../crates/glaux-server/examples/system-storage-proof.rs).

## Fixed synthetic identities

All local IDs below share the prefix `01890f20-7b5a-7cc3-98c4-dc0c0c07` and
append the four-character suffix shown. They are literal canonical UUIDv7
fixtures, not IDs minted by the repository under test. Source-pair order is not
an identity distinction; compare complete pairs in lexical authority/identifier
order. Every resource uses label `Shared label` to expose label-based merging.

| Fixture / suffix | Exact UID | Source authority / identifier pairs | Parent |
|---|---|---|---|
| P / `3901` | `urn:glaux:fixture:system:P` | `authority-A` / `platform-7`; `alternate-authority` / `P-ALIAS` | None |
| A / `3902` | `urn:glaux:fixture:system:A` | `authority-A` / `sensor-1`; `authority-A` / `sensor-1-alias` | P |
| B / `3903` | `urn:glaux:fixture:system:B` | `authority-B` / `sensor-1` | None |
| L1 / `3904` | `urn:glaux:long:` + deterministic ASCII to total 4,096 bytes | 4,096-byte authority / 4,096-byte identifier, generated below | P |
| L2 / `3905` | L1 UID with only its last byte changed | same long authority / L1 identifier with only its last byte changed | None |
| C1 / `3906` | `https://EXAMPLE.test/items/%41` | `a` / `bc`; `a:` / `b` | None |
| C2 / `3907` | `https://example.test/items/%41` | `ab` / `c`; `a` / `:b` | None |
| C3 / `3908` | `https://EXAMPLE.test/items/A` | `Authority` / `Sensor`; `authority` / `Sensor` | None |
| C4 / `3909` | `https://EXAMPLE.test/items/%4a` | `Authority` / `sensor` | None |
| C5 / `390a` | `https://EXAMPLE.test/items/%4A` | None | None |

The shared source-local name `sensor-1` belongs to two different authorities
and identifies A and B separately. A's second alias resolves to A, not another
resource. L1/L2 exercise the admitted maximum identity lengths and distinguish
values that differ only at the end; no truncation, case normalization, digest
identity substitution or accidental short-index limit is allowed.

Long values use fixed test-only xorshift64: update unsigned 64-bit state with
`state ^= state << 13`, then `state ^= state >> 7`, then `state ^= state << 17`;
select `state & 63` from
`ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_` per byte.
Seeds are `0x123456789abcdef1` (UID suffix), `0x23456789abcdef12` (authority),
and `0x3456789abcdef123` (identifier). L2 changes the last byte to `a`, or to `b`
if already `a`. No production generator or database output supplies expected
strings. Stored sizes must exceed 3,000 bytes for all three values: repeated
characters could compress below the ordinary B-tree key limit and hide a fault.

C1–C5 remain distinct despite semantically equivalent URI spellings. Source
pairs distinguish unseparated concatenation (`a` + `bc` versus `ab` + `c`) and
delimiter-only concatenation (`a:` + `b` versus `a` + `:b`). Case is preserved;
an empty alias set is valid.

## Pre-implementation truth table

| Operation or probe | Independently expected result and forbidden effects |
|---|---|
| Read schema compatibility before application migrations | `IncompatibleSchema`; no migration is applied by the check |
| Migrate with an alternate schema first in `search_path` | `IncompatibleSchema` before DDL; no public application/ledger tables or alternate-schema objects are created |
| Apply only packaged versions through 2; directly seed P and its aliases | Three-table rows exactly match P; the typed-parent table is not present yet |
| Check that predecessor against the latest application schema | `IncompatibleSchema`; P and aliases remain byte-for-byte equivalent as SQL text values |
| Apply latest packaged migration | Compatibility succeeds; P and both aliases are unchanged; new typed-parent relation is empty |
| Create A and B | Exact IDs, UIDs, labels and all aliases retained; only A points to P; three distinct Systems remain despite matching labels and source-local names |
| Read by local ID, UID and each complete source pair | The exact corresponding fixture, including its complete alias set and parent; an unknown ID/UID/pair returns `None` |
| Create another resource using P's local ID | `Conflict`; no new UID, System, alias or parent row and no modification of P |
| Create a fresh local ID using P's UID | `Conflict`; no partial identity/System/source/parent rows |
| Create a fresh ID/UID with a source pair already owned by A | `Conflict`; even an earlier fresh alias in that same attempted create is absent afterward |
| Supply the same fresh source pair twice in one create | `Conflict`; all earlier inserts roll back |
| Create a fresh ID/UID with a missing parent | `InvalidAssociation`; all earlier identity/System/source inserts in the attempted operation roll back |
| Create a fresh ID/UID with an identity-only target as parent | `InvalidAssociation`; an entry in the common identity table is insufficient without the target's System row; no partial child rows |
| Create a System naming its own fresh ID as parent | `InvalidAssociation`; all earlier inserts roll back |
| Create L1 and L2 and look them up by full UID/source pair | Both remain distinct; exact 4,096-byte UID, authority and identifier values survive SQL and repository reads |
| Create C1–C5 and look up each identity | Every lexical UID/source pair resolves only to its own exact System and aliases |
| Repeat L1's full UID or full source pair under another ID | `Conflict` and exact unchanged table snapshots, not a low-level index-size error or accepted duplicate |
| Reapply all packaged migrations | All four application tables and all seeded values/associations remain unchanged |
| Check latest schema with alternate schema first in `search_path`, then migrate | Read-only compatibility succeeds using the public ledger; migration rejects that creation namespace; exact rows and ledger remain unchanged |
| Mark a migration dirty, corrupt its checksum, or introduce an unknown version inside a disposable transaction | Each read-only compatibility check returns `IncompatibleSchema`, without fixing/deleting the evidence or changing application rows; rollback restores the pristine ledger |

The identity-only parent fixture uses suffix `39f0`, family `system`, UID
`urn:glaux:fixture:identity-only`, and deliberately has no `system_identity`
row. It tests the typed endpoint boundary without creating another resource
family or pretending an incomplete identity is a usable System. The missing
target uses suffix `39ff` and has no row at all.

Rejected attempts use dedicated IDs/UIDs/source pairs and are checked against
complete independently read SQL snapshots of `resource_identity`,
`system_identity`, `source_identity` and `system_parent`, ordered explicitly.
That detects stray partial rows, lost aliases, changed existing values and
unexpected links; matching row counts alone is insufficient. The snapshot's
queries are not repository lookup/serialization code. Positive reads are
compared with the literal fixtures above, not copied from earlier read results.

## Real-database execution boundary

The executable connects only to the fixed loopback DSN
`postgres://postgres@127.0.0.1:5432/glaux_harness_test?sslmode=disable` from
inside the [owned disposable PostgreSQL/PostGIS container](database-tests.md).
It accepts no URL/database override or command-line selection. The owning
harness must verify the exact nonce-labelled container, pinned image, private
tmpfs, `--network none`, absent published ports/host binds, and cleanup before
copying/executing the binary there. Trust authentication is synthetic-container
configuration, not deployment guidance. Do not run this proof against a user
database or directly on the laptop.

Setup, connection, SQL, assertion, schema-check or cleanup errors are failures,
not missing-database skips. The proof uses a finite deadline and SQL timeouts.
The harness removes its exact owned container on success or failure and treats
cleanup failure as fatal; the binary neither discovers nor resets external
databases. Ledger-corruption cases are rolled back on the owned connection.
No application update/delete API is introduced by those test-only SQL probes.

Run the complete suite on the authorised hosted Linux runner:

```sh
python3 -u scripts/check-execution.py system-storage
```

The [driver](../scripts/test_system_storage.py) builds the locked/offline proof
and administrative binary, copies them into the owned container, and runs the
SQLx proof plus independent psql checks. The CLI uses the container's Unix
socket; a network URL requesting `sslmode=disable` must still fail against the
non-TLS fixture, without applying migrations. This is no positive certificate
validation claim. CLI checks distinguish unmigrated rejection, nondestructive
reapplication, and migration on a freshly reset owned database: a no-op
`migrate` command cannot satisfy the last.

Seven SQL sensitivity cases reject the intended SQLSTATE and named
constraint/message, accept the same statement only after transactional removal
of its check, then roll back and reject it again: duplicate local ID, UID and
source pair; missing parent; identity-only parent and child; parent cycle.
Removing the local-ID primary key temporarily removes dependent foreign keys;
the whole DDL change rolls back before other cases execute. The eighth control
finds distinct strings sharing the PostgreSQL index hash within a bounded
400,000-candidate search. Both full values coexist while their exact duplicate
is rejected, proving full-text rechecking rather than cryptographic security.
Full row snapshots expose partial changes. Separate probes reject a second
parent, self-parent and missing cycle-serialization guard.

Four concurrency cases use backend state and blocking relationships as
barriers, not elapsed sleeps: competing UID owners, competing source-pair
owners, opposing parent edges at READ COMMITTED, and opposing parent edges at
REPEATABLE READ. The winner is held until the waiter is actually blocked, then
committed. The loser must report the intended conflict/cycle failure, or
SQLSTATE `40001` for a stale repeatable-read writer, and exact committed rows
must match. SERIALIZABLE isolation is not separately exercised. Session and
container cleanup errors fail the run.

The seven required completion markers are printed only after their assertions:

```text
System storage group passed: migration-initial-preservation
System storage group passed: exact-identity-and-lookups
System storage group passed: conflicts-atomic
System storage group passed: typed-parent-rollback
System storage group passed: long-lexical-identities
System storage group passed: migration-reapply-preservation
System storage group passed: schema-compatibility-read-only
Required System storage proof passed: 7 groups.
```

The execution guard must require every group and the final marker, a successful
process exit, and successful harness cleanup. Markers without those conditions
are not a pass. This example proves only the initial create/read identity and
typed-parent storage contract when it actually runs. It does not claim HTTP
behavior, authorization, full System descriptions, hierarchy mutation/cycle
workflows, revisions/artifacts, audit/outbox atomicity, restore, performance or
other resource families. Fault controls and independent SQL boundary probes
must record their actual outcomes separately; authored assertions alone are
not demonstrated behavioral failure sensitivity.

The outer suite prints `System storage database: all required checks passed.`
only after all seven Rust groups, eight SQL controls, four races, parent
invariant/CLI checks and owned-container cleanup succeed. CI requires that
marker and a successful exit. Authored tests are not recorded passes until a
hosted run executes them on the reviewed commit.

[Guide]: https://github.com/DGIWG-P507/glaux/blob/6c801a47227639d623d41297b9c9074ba0c91eb1/Docs/Plans/glaux-server/glaux-server-implementation-guide.md
