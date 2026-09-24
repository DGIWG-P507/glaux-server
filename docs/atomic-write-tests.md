# Atomic System write: independently specified checks

Issue [#15](https://github.com/DGIWG-P507/glaux-server/issues/15), including its
20 September audit amendment, implements Guide v1.21 §§2.3, 4.6–4.8 and 4.10.
This truth table was authored before its production application transaction.
It is an intended test contract, not a claim that execution has occurred.

## Fixed synthetic facts

Use UUIDv7-shaped fixture identifiers with the common prefix
`01890f20-7b5a-7cc3-98c4-dc0c0c07` and these independently assigned suffixes:
parent System `0101`, child System `0102`, artifact `0201`, revision `0301`,
audit `0401`, outgoing work `0501`. Identifiers convey neither clock authority
nor commit order. The parent exists before the attempted child write.

The child UID is `urn:glaux:test:atomic-child`, label `Atomic child`; its two
source aliases are (`urn:glaux:test:source-a`, `upstream-17`) and
(`urn:glaux:test:source-b`, `upstream-29`). Both are provenance assertions,
not the verified caller. The supplied safe caller is `fixture-actor`, with
verified source `fixture-source`; correlation is `fixture-correlation-15`.
The accepted operation is System creation, its outcome is accepted, and its
target/revision are exactly the child and `0301`. The outgoing record describes
that System creation and refers to the same child/revision/artifact, not a
fresh serialization or mutable current System row.

The artifact contains exactly these 51 ASCII bytes, with no trailing newline:

```json
{"type":"PhysicalSystem","label":"Alpha","value":1}
```

Media type is `application/json`; SHA-256 is
`8d3e448241a86daedf90f6ea36eebbc62c82829307bcb6eae1a9954c031ec215`.
These are existing independently authored source fixtures, reused rather than
regenerated from a production serializer. The deliberately different source
label is opaque-storage content; this issue does not validate full SensorML or
assert that this synthetic document is a complete admitted System description.

Semantic time is `1970-01-01T00:00:00.0000011Z`: civil second `0`, not a leap
second, fractional value `0.0000011`, seven digits and known UTC offset zero.
Receipt/audit/event time is `2017-01-01T01:00:00.12345678901234567890+01:00`:
civil second `1483228800`, not a leap second, fraction
`0.12345678901234567890`, twenty digits and known offset `3600`. Compare the
lexical spelling as well as the numeric coordinate; `ExactInstant` equality
alone cannot establish source preservation.

## Truth table and discriminating failures

| Case | Independently expected result | Wrong behavior detected |
|---|---|---|
| Accepted child creation | Exact resource, System, both aliases, parent edge, original artifact, revision, audit and outgoing-work facts all commit together; supplied actor/source and correlation remain distinct from payload assertions | Missing/mismatched audit, association or work; reserialization, rounding, invented actor, wrong revision binding |
| Failure at each implemented INSERT boundary | Test-owned `AFTER INSERT` trigger raises after the database actually accepted the target row; complete table-value snapshot equals the pre-operation snapshot | Autocommit/nested independent commits, omitted rollback, partially retained alias, resource/context/audit/work |
| Second source-alias insertion fails | The first alias and every earlier write roll back too | Testing only an empty association list or first-row failure |
| Deferred constraint trigger fails at COMMIT | The operation returns failure and the entire independently observed snapshot is unchanged | Returning success before COMMIT or leaving committed earlier work |
| Separate observer while writer is blocked after outgoing insertion | Explicit advisory-lock wait and backend identity establish the writer reached the boundary; observer sees no new resource, revision, audit or outgoing work; after release it sees the exact committed result | Treating queued/uncommitted outgoing work as durable or using sleep as ordering proof |
| Invalid/mismatched context or existing IDs | No write occurs; complete snapshot unchanged | Partial state from precondition/input rejection or cross-resource revision binding |
| Safely supplied denied operation | One denied audit row can be recorded without a mutation; optional unavailable actor/source/target context stays absent; no resource, revision, alias or work appears | Invented identity, requiring a successful mutation, treating denial as an accepted change |
| Denial audit insertion/storage failure | Failure is explicit, complete state unchanged; no mutation or outgoing work | Failing open or creating partial denial evidence |
| Restricted serving role | Can append its permitted audit and perform the accepted transaction; cannot UPDATE, DELETE, TRUNCATE or disable protection on earlier audit | Owner-only testing, rewrite/deletion through normal SQL privileges |
| Public-resource removal simulation | Ordinary removal attempt never cascades earlier audit; administrative test cleanup remains explicitly separate | Audit FK cascade or resource deletion doubling as audit retention |
| Controlled omission from a passing baseline | Independently expected audit/outgoing facts are absent and the named behavioral assertion fails | Tests accepting only success/counts or comparing server output to itself |

Snapshots use independently written SQL over all affected table values and
ordered rows, not production record serialization. Table-count equality alone
is insufficient. Fixture resets are isolated, preserve baseline parent facts,
and must fail fatally if setup or cleanup fails. Each proof has bounded waits.

## External-effect and scope limits

The production boundary has no delivery worker, network callback or external
adapter. Its ordinary code and dependency paths are inspected for that absence;
there is therefore no production external-effect hook to instrument. The
test-owned database barrier and network-isolated harness establish committed
visibility, not broker delivery, device effects or all possible future network
behavior. No artificial callback or fail-switch is added to production merely
for this test.

HTTP/policy denial selection, rate and storage bounds, diagnostic aggregation,
authentication and protected response handling remain #22's responsibility.
These tests establish only the safely supplied audit storage boundary and
its failure invariants. Precondition/concurrency semantics, retry records,
publication ordering/log handoff, workers, restoration and retention execution
belong to their later tasks. Audit durability is not a tamper-proof guarantee
against an administrative database owner.

## Execution route

Runtime is only the approved GitHub-hosted Linux job, using the existing owned,
network-isolated pinned PostgreSQL/PostGIS harness and real Rust SQLx calls.
No caller-selected database, operational data, laptop runtime or installation
is used. The eventual execution record must distinguish actual passed groups,
controlled behavioral failures, setup/build failures and unexecuted claims.
