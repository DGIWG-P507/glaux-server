# Action, source and resource permissions: independent checks

Issue [#22](https://github.com/DGIWG-P507/glaux-server/issues/22), including its
20 September denial-audit amendment, owns these checks against Guide v1.21
§4.10 and §8.1.1. This contract is authored before the production permission
evaluator. It specifies intended answers, not a claim that execution occurred.
Verified identity is an input to a permission decision, never the decision.

## Fixed synthetic access matrix

Two explicit loopback development identities represent separate configured
groups: `development-alice` in `group-a`, and `development-bob` in `group-b`.
Their source authorities are `urn:glaux:test:source-a` and
`urn:glaux:test:source-b`. Source authority is a configured permission scope;
putting that string in a request does not establish it. All identifiers,
payloads, clocks and credentials are fictional and local to an owned fixture.

Seed distinguishable named System records: two permitted A resources, one
otherwise valid A resource excluded by resource scope, and at least one B
resource. Use literal independently assigned local identifiers and labels,
with a second distinguishable hidden world changing denied labels and facts.
Expected answers name the exact permitted resources, not merely their count.
The issued proof's fixture constants document those identifiers; fixture
setup must verify the seeded facts before exercising admission.

| Verified caller and operation | Expected answer | Wrong behavior this distinguishes |
| --- | --- | --- |
| Alice lists or reads her configured A resources | Exactly the permitted A identities, labels and links; counts derive from that same authorized set | A correct count with the wrong members; filtering after generating links/counts; treating membership in one source as access to every resource |
| Bob lists or reads his configured B resources | Exactly the permitted B identities, labels and links | A globally fixed result set, caller/group mix-up, or source predicate inverted |
| Alice reads an excluded A resource, B resource or absent resource | The same safe `404` representation, with no protected target or policy reason | Existence disclosure through status, error details or target-bearing links |
| Alice creates a new allowed A System | Accepted resource, source context, artifact, revision, verified audit and outgoing work commit together | Permission disconnected from the actual application transaction |
| Alice attempts B creation, or a caller lacking create permission attempts a mutation | Safe `403`; no resource, artifact, revision, association or outgoing work; a bounded eligible denial may be retained separately | Identity accepted as source authority; mutation before authorization; blanket rollback assertion hiding legitimate denial auditing |
| Alice supplies forged caller/group/source/producer/reporting assertions | No expansion of the configured action/source/resource decision; authenticated actor remains Alice | Trusting request headers or content as authority; confusing uploader with producer or status reporter |
| Configured permission permits submission but not status reporting | Reporting remains denied | Inferring status authority from identity or a different action's permission; this is interface-level coverage, not a command/status endpoint claim |
| Controlled local policy adapter is unavailable | Safe `503`, no admitted mutation or protected read result | Failure defaulting to allow, or disguising an unavailable decision as a successful empty list |
| Permission is revoked between requests | The next request is denied and returns no previously permitted result | Caching an earlier authorization indefinitely or treating prior success as ongoing permission |

Anonymous read access is not selected by this initial adapter. It remains an
explicit later deployment option, not a passed fixture or an implicit fallback.

Missing or invalid authentication retains the earlier authentication adapter's
safe outcome, rather than manufacturing an anonymous or verified caller.
Policy configuration with unknown, malformed, unbounded or contradictory
entries must reject before serving. No wildcard or allow-all fallback is inferred
from an absent rule, absent decision, poisoned/unavailable state or unknown
action. A complete allowed resource is returned; the fixture must not redact
required fields into a malformed advertised representation.

## Safe denial auditing

The first eligible denied System mutation within the configured rate and
storage limits retains exactly one row through the existing audit path.
Inspect its fields using independently written SQL: verified actor and source
only where obtained safely, System-create operation category, denied outcome,
controlled time and correlation. Omit a target whose existence or ownership is
not authorized-to-record. A denied candidate's untrusted payload, forged actor,
producer declaration, bearer token, secret canary and internal policy reason
must not appear in retained audit or public errors.

Exercise these cases separately, rather than treating one generic failed audit
as all three:

1. **Retained denial:** within both limits and with available storage, exact safe
   denial facts appear; all non-audit application state remains unchanged.
2. **Rate exhaustion:** with retained storage still below its cap, the next
   eligible denial remains denied without another row or mutation. Advance the
   controlled clock into the next window and independently test recovery.
3. **Storage bound:** with rate capacity available, the retained-denial cap
   prevents another row without authorizing the request or removing old audit.
4. **Audit-store failure:** induce a real database insertion failure while the
   request otherwise reaches eligible denial recording. No partial audit or
   application state commits, and the requester still receives the same safe
   denial rather than database details or success.

Bounded protected diagnostics distinguish the recording limitation without
copying identity, target, payload or secrets; repeated failures cannot grow
an unbounded diagnostic collection. Ordinary denied reads do not automatically
create denial audit rows. No independent spool, policy server, audit service,
background retention process or tamper-proof ledger is introduced.

## Observation and execution boundaries

Use the existing owned, pinned PostgreSQL/PostGIS harness and actual Rust SQLx
application/storage calls. Database snapshots are independently written SQL
over all affected ordered row values, including internal coordination, audit,
retry and outgoing state; row counts alone are insufficient. Setup/reset and
cleanup failures are fatal, not skipped proof. The test fixture owns all reset
privileges and never accepts a caller-selected or operational database.

The real HTTP listener mounts synthetic read/create routes through production
authentication, policy/admission and HTTP-boundary components. An independent
TCP client sends HTTP bytes and checks status, headers and general-purpose JSON,
not production response types. Expected resource values, complete safe problems,
links and membership are authored independently. Known-bad wire fixtures with a
wrong identity, extra hidden member or changed status must fail the same oracle.
Changing only denied facts must not change Alice's permitted response, count,
links or safe concealed error. Check protected-response `private, no-store` and
denial `no-store` behavior independently of permission selection.

Use explicit completion/barrier signals and finite waits, not sleeps as ordering
evidence. The listener binds only ephemeral loopback, shuts down even after a
failed assertion, and is verified closed before the owned database is removed.
The only execution environment is approved GitHub-hosted Linux; the company
laptop performs no Rust, Python, OpenSSL or Docker runtime or installation.

## Behavioral red and fault sensitivity

The first compiling implementation is intentionally deny-all. A legitimate
allowed request must fail at a named exact-resource assertion after setup and
the independent wire-oracle controls pass. A compiler, migration, service,
authentication or cleanup failure is not that behavioral red. The final issue
record identifies the actual commit and result rather than assuming execution.

After the complete unmodified proof passes, create a disposable source copy,
bypass the reviewed source-permission comparison, compile successfully and
require the specific cross-source/incorrect-membership assertion to fail.
Require the preceding oracle/setup markers and absence of the final success
marker. A different panic, process timeout or infrastructure failure cannot
count as a detected authorization fault. Preserve the original source bytes,
rebuild and rerun them to prove restoration; keep baseline/fault/restored raw
logs and a concise result in the ordinary CI artifact.

## Limits

This is shared admission and System-storage fixture coverage, not new public
CSAPI routes. The production listener remains health-only until its route-owning
tasks. It does not establish coverage for unimplemented observations, commands,
status reporting, exports, provenance graphs, streaming or diagnostic endpoints.
Those owners reuse and extend the same permission boundaries. No enterprise
policy administration, national/NATO labeling adapter, real identity provider,
new credential format or complete standards-conformance claim is added.
