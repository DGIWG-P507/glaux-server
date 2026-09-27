# Initial action, source and resource permissions

[Issue #22](https://github.com/DGIWG-P507/glaux-server/issues/22) implements the
shared permission boundary from Guide §4.10 and the approved denial-audit
amendment. Authentication establishes **who called**. A separately configured
policy establishes **what that caller may do, for which source and resources**.
Being authenticated, uploading a document, naming a producer, or having command
submission permission never implicitly grants another action or reporting role.

This is a library boundary exercised through synthetic HTTP routes and actual
System storage. The production binary remains health-only. The route-owning
tasks must use this admission boundary; the older application/repository
functions are trusted internal persistence primitives, not request handlers.

## Explicit local policy

An optional `policy` object in the existing configuration selects the initial
in-process adapter. It performs no network calls or enterprise policy
administration. For example, this fragment permits one fictional development
group to read Systems owned by one source:

```json
{
  "policy": {
    "grants": [{
      "issuer": "urn:glaux:development",
      "group": "example-group",
      "source": "urn:glaux:test:source-a",
      "actions": ["read"],
      "resources": null
    }],
    "denial_audit": {
      "max_records": 10000,
      "max_per_window": 60,
      "window_seconds": 60
    }
  }
}
```

Each grant matches the exact verified issuer and **either** a subject **or** a
group from that issuer, never both. Source and resource restrictions remain
paired within that grant; permissions from different grants are not combined
into a broader source/resource cross-product. `resources: null` permits every
resource in that source for the named actions; a list restricts the grant to
those canonical local IDs. An empty list grants no resource. Actions are
explicit: `read`, `create`, `update`, `submit_command`, `report_status`, `publish`,
`export` and `administer`. Naming a later action does not implement its endpoints.
Source labels are exact bounded configured identifiers, not automatically
dereferenced URLs or a required URI syntax. The example uses a fictional URN.

The adapter accepts at most 256 grants, eight distinct actions per grant,
1024 resource IDs per grant and 4096 across the policy. Issuer/subject text is
bounded to 1024 bytes and group/source text to 256, without control characters.
Denial limits are 1–100000 retained rows, 1–10000 attempts per process window and
a 1–86400-second window. Missing policy uses deny-all with finite denial defaults
of 10000 rows and 60 attempts per 60 seconds, not unlimited recording.

Omission of `policy` grants nothing. Explicit null, unknown fields, malformed
identities or limits fail startup validation; no anonymous or allow-all fallback
is inferred. This slice does not select the Guide's optional anonymous-read
mode. Configuration checking remains offline. `Configuration::admission()`
shares the validated policy and denial limiter across handlers rather than
creating a fresh allowance per request.

## Reads and writes

For the current System foundation, source ownership comes from the original
accepted creation's audit/outgoing binding. It does **not** come from descriptive
source aliases, a later uploader or a claimed producer in a document. Missing or
ambiguous creation evidence grants no read/update authority. This is the current
internal ownership binding, not a public provenance claim or ownership-transfer
API. Trusted low-level fixture/import calls must not be exposed as bypasses.

Query selection applies paired source/resource permissions before counting,
limiting or building output. A child whose direct parent is not readable is
withheld as a whole resource; required fields are not stripped into an invalid
representation. Selection, counts and hydration share one SQL statement's
snapshot. A concealed item and an absent item receive the same safe `404`
problem, apart from a fresh server correlation identifier. Later filter, paging
and domain-family tasks extend this boundary with their own checks.

Creation binds the selected source only after permission is established.
Accepted creation/update audit identity, source, correlation and time come from
verified/trusted operation context, not submitted audit fields. The initial
update wrapper requires both readable result context and update permission
because it returns an existing-resource receipt. It does not prescribe that
every future CSAPI mutation require a general read permission. Optional creation
retries reauthorize the original outcome before disclosure. Persistence still
uses the existing atomic resource/revision/audit/outgoing transaction.

The local policy is consulted on every operation. A controlled unavailable
adapter produces safe `503`, never an allowed mutation or a successful empty
query. Policy implementations must remain bounded local computations; a future
network adapter needs its own explicit timeout/cancellation contract. No external
call is performed inside a database write transaction.

## Bounded denied-mutation records

Selected denied System creates/updates reuse the existing `server_audit` path.
They retain only safe verified actor context, operation, denied outcome, trusted
time and server-generated correlation, with source/target omitted when not safe
to record. Request bodies, bearer tokens, producer claims and internal policy
reasons are not copied. Ordinary denied reads do not automatically create rows.

The actor is an exact JSON tuple of verified issuer, subject and caller kind,
not a truncation or a request-provided label. If that tuple cannot fit the
existing bounded audit field, a write cannot be accepted without valid audit
context. Reads do not gain permission from that limitation.

The configured rate is process-local and shared by cloned admission handlers;
restarting the process restarts its rate window. The retained-denial row cap is
database-wide and serialized by a nonblocking transaction advisory lock. A busy
lock, full capacity, exhausted rate or unavailable audit store does not authorize
the request. Finite statement/lock timeouts bound this separate append. There is
no purge, spool, background retry or promise of one durable record per attempt.
Fixed-cardinality saturating diagnostic counters distinguish limitations without
growing per-actor labels or copying protected details. They are an internal
inspection interface, not a new public diagnostics endpoint.

Separate append failure does not expose SQL/policy reasons to the requester or
create a resource or outgoing event. Normal resource permissions do not grant
audit-reading or audit-administration privileges. Database provisioning must
keep serving privileges separate from owner/migration privileges.

## Verification and limits

The [independent test contract](authorization-tests.md) defines exact two-source
answers, hidden-world comparisons, real-database before/after facts, four
separate denial-audit outcomes and a compiled permission fault. Execution is
GitHub-hosted with synthetic identities and an owned disposable database. The
issue/PR records distinguish authored tests from actual red/green/fault results.

No enterprise policy product, national/NATO labels, provenance graph, new public
CSAPI operation, audit service, complete threat model or conformance claim is
introduced. Command/reporting, streaming, export and other resource families
remain explicitly unimplemented and must acquire their own admission coverage.
