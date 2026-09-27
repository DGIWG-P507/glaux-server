# Minimal System creation proof

Issue #24 / Roadmap 1.5.1 exercises the actual binary's initial POST /systems.
Guide §§4.2–4.3, 4.6, 6.2 and 6.4 govern the minimal GeoJSON contract; §8.1.1
requires independent expected results and demonstrated failure sensitivity.
This is not complete System CRUD or retrieval/restart evidence (#25).

Run on the authorised GitHub-hosted Linux runner:

```sh
python3 scripts/test_system_create.py
python3 scripts/test-system-create-failures.py
```

The fixture uses the existing pinned, disposable PostgreSQL/PostGIS harness,
with network disabled. Explicit migration and inspection use the owned
administrator connection; the actual serving binary uses a distinct role with
enumerated application permissions, no superuser/role/database-creation powers.
Fixtures contain synthetic callers and data only. Owned processes, listeners,
directories and database targets must clean up; cleanup failure is fatal.

## Independently expected results

The client uses raw HTTP and general JSON, not production request/response
types. A success has empty body, status 201, no promised ETag, and a canonical
configured-origin/prefix Location carrying a generated lowercase UUIDv7.
Wrong status/body/origin/identifier/validator/cache/correlation fixtures must fail
the wire oracle. Authenticated handler results, including errors, require exactly
`private, no-store`. Public fallback, failed authentication and early outer
boundary rejection require exactly `no-store`; the proof does not interchange
those expectations.

A minimal Feature has null geometry and properties uid, name and featureType.
Published SOSA short/full primary-type forms are preserved, not replaced with
SensorML process-class names. A structurally valid supplied local ID is ignored
as authority while its original source bytes remain retained; invalid shape
is rejected. Configured source and verified caller supply authorization/audit
context, not body fields or forged headers. Unsupported spatial/relationship
content is explicitly rejected rather than silently discarded.

Conditions refer to the POST request target, not its not-yet-created member.
This initial POST-only collection has no current representation or validator:
If-Match fails with 412, If-None-Match permits the operation, and malformed tags
fail with 400. A denied caller still receives 403 before a valid condition can
reveal a different result. If-Modified-Since does not make POST conditional.

| Ordered group | Evidence |
| --- | --- |
| independent-wire-oracle-controls | Seven known-bad success responses cannot satisfy the independently specified creation response. |
| empty201-canonical-location-and-atomic-records | Actual POST and canonical Location; exact UID/label, source bytes/digest, revision, audit actor/source/time/correlation, outgoing work and write-head joins; discovery advertises POST only, not future GET or conformance. |
| malformed-media-and-no-partial-writes | Duplicate UID, malformed/duplicate-key JSON, missing/wrong tag, invalid ID, unsupported geometry/relationship/forged source, wrong media and coding. Full ordered snapshots remain unchanged. |
| verified-callers-source-scope-and-safe-denials | Two development callers/groups and configured sources, plus an independently OpenSSL-signed JWT through the actual binary. Missing/bad credentials fail before a condition; accepted audit matches verified JWT context. Cross-source denial adds only safe denial audit, not resource/revision/retry/outgoing state. |
| optional-retry-and-forged-context | Unkeyed creation, exact same-key recovery without additional facts, retained-member resource authorization and revoked retry permission, changed-intent conflict, invalid key, generated identity despite supplied ID, all ten published primary-tag spellings. |
| precommit-failure-and-owned-cleanup | An owned database trigger fails outgoing-work insertion before commit; no 201 or partial identity/artifact/revision/audit/retry survives. Disabled route is absent and state remains unchanged. |

Snapshots include complete ordered rows, not only counts. Success also preserves
every preceding row and checks cross-table references, while denial auditing is
examined separately. No event-delivery worker is started.
The existing hosted OpenSSL signer generates a short-lived synthetic JWT; its
private key is removed before the public fixture enters the container. No
production signing or decoding types define the expected caller or wire body.

The fault run copies sources into an owned temporary directory, establishes a
complete passing baseline, bypasses the existing source-permission comparison,
and requires the named cross-source assertion to fail after the earlier groups.
It then restores the unchanged source and requires another complete pass.
Compilation/setup failure, an unrelated assertion, timeout or missing output
does not establish detection. This document describes required checks; actual
red/green execution and limitations belong in the issue/PR delivery record.
