# Minimal System creation over HTTP

[Issue #24](https://github.com/DGIWG-P507/glaux-server/issues/24) connects the
existing validation, caller permissions and atomic transaction to `POST /systems`.
This is one explicitly enabled creation slice, not complete System CRUD or an
OGC conformance-class claim. [Canonical retrieval](system-read.md) (#25) reads the
created System back.

## Enable the bounded example

On an already approved runtime, after the existing explicit migration step,
use this synthetic, loopback-only configuration:

```json
{
  "listener": "127.0.0.1:8080",
  "authentication": "development",
  "development": {"subject": "example-caller", "groups": ["source-a"]},
  "database": {"url_env": "GLAUX_DATABASE_URL"},
  "health_timeout_ms": 1000,
  "http": {"public_api_root": "http://127.0.0.1:8080"},
  "discovery": true,
  "system_creation": {
    "source": "urn:glaux:example:source-a",
    "retry_retention_seconds": 3600
  },
  "policy": {
    "grants": [{
      "issuer": "urn:glaux:development", "group": "source-a",
      "source": "urn:glaux:example:source-a", "actions": ["create", "read"],
      "resources": null
    }],
    "denial_audit": {"max_records": 100, "max_per_window": 10, "window_seconds": 60}
  }
}
```

Omit `system_creation` to leave the write route absent. When present, it is a
strict object, not a Boolean or null. It requires non-disabled authentication and
an explicit public API root. `source` is nonempty, control-free and at most 256
UTF-8 bytes; retention is a positive `u32` number of seconds. These are Glaux
operational bounds, not OGC requirements or a mandated retention policy. The
one-hour example is illustrative. Existing configurations remain unchanged.

The source is trusted deployment configuration, not a request header or a
submitted producer claim. Permission for that source and the create action is
still required. A cheap action/source screen precedes body/schema processing;
the selected member or retained retry outcome is authorized again at the atomic
write boundary. Different source-configured listeners can use the same store;
this slice introduces no public source-selection parameter. JWT deployments use
the existing issuer/audience/key checks. Development callers remain explicit,
loopback-only test identities: never publish or proxy that listener externally.

The serving role needs the existing atomic-write tables' required read/insert
permissions, retry-row update permission and the existing parent-write-guard
read/update permission, not schema/migration ownership. The proof enumerates
these grants separately from its administrative inspection connection.
Run migrations separately with the administrative role. No permanent database,
cloud service or company-laptop installation is required for the hosted tests.

## Request and success contract

Send `Content-Type: application/geo+json` and exactly one minimal Feature:

```json
{
  "type": "Feature",
  "geometry": null,
  "properties": {
    "uid": "urn:glaux:example:thermometer",
    "name": "Example thermometer",
    "featureType": "sosa:Sensor"
  }
}
```

The UID is an absolute URI, preserved without normalization; Glaux limits it
and the name to 4096 UTF-8 bytes each. A name must be nonempty. `featureType`
accepts the five published `sosa:` tags (Sensor, Actuator, Sampler, Platform,
System) or their exact `http://www.w3.org/ns/sosa/` forms. Preserve the supplied
valid spelling. Neither a tag nor an identifier grants permission.

Use the published structural projection before the documented partial-slice
semantic check. Non-null geometry, extra description properties, relations,
foreign members and other unsupported content receive `422` rather than being
silently lost. This is a current implementation limit, not a claim that those
members are forbidden by GeoJSON or CSAPI generally. Malformed input or invalid
known-field shape receives `400`. A schema-valid supplied outer `id` is ignored
in favor of a fresh UUIDv7; an invalid supplied `id` still fails. Supplied root
`links` are structurally checked and removed as generated association input;
they are not authority. The original complete bytes remain restricted evidence,
not an unfiltered public representation. No request-time links or schemas are fetched.

After a successful commit, return **`201 Created` with an empty body** and
`Location: {configured-public-root}/systems/{generated-local-id}`. The response
has `Cache-Control: private, no-store` and safe correlation metadata. It does not
emit an ETag or claim a generated GeoJSON response. `Accept` does not select a
resource representation for this empty success. The canonical URL identifies
the created resource; [GET on it](system-read.md) returns the System. A configured
path prefix is retained, and the deployment proxy must strip it before forwarding.
Host/forwarding headers never choose the canonical origin.

The System identity, original artifact and digest, revision, verified audit
context, outgoing work and any retry receipt commit together through the existing
application boundary. The stored receipt/audit time is a trusted database UTC
sample during operation processing, not a client timestamp, UUID-derived time,
or a claim of exact network-arrival time. No event worker or external delivery
runs as part of the transaction. A failure must not become an early `201`.

## Rejections and optional retries

Missing/invalid credentials receive `401`; authenticated source/action denial
receives `403`. Conflicting UID or retry intent receives a safe `409` without
revealing competing records. Request bounds, unsupported media/coding and required
dependency failures use `413`, `415` and `503` respectively. Common boundary
limits and safe Problem Details continue to apply. Rejected pre-admission input
creates no System or outgoing work; bounded safe denial auditing is separate.

`Idempotency-Key` is optional. It uses the existing bounded, caller/source-scoped
retry contract: the same retained key and intent returns the original canonical
Location after current authorization; different intent conflicts. The digest
includes original request bytes, so differently formatted JSON is not promised
equivalent. An unkeyed repeat of an existing UID conflicts; equal names or values
never merge different Systems. Retention is configured explicitly, and expiry
ends the duplicate-prevention guarantee. See [retry limitations](write-retries.md).

For this explicitly POST-only `/systems` target, no current collection
representation, ETag or modification date exists yet. `If-Match`, including `*`,
therefore fails with `412`; `If-None-Match`, including `*`, passes. These conditions
concern the request target, not the future item named by Location. Evaluate them
after normal authentication/permission checks; a denied caller must not get an
authorized precondition result. A present empty entity-tag list is valid:
If-Match still fails and If-None-Match passes. Malformed tags fail with `400`.
`If-Modified-Since` is ignored for POST; `If-Unmodified-Since` is ignored without
an available modification date. No conditional header is mandatory. Revisit this
explicit representation state when the collection representation is implemented.

## Discovery, sources and verification

If discovery is enabled, its API description includes the actual POST request,
empty success, errors and authentication mode. It advertises no list GET,
complete class or experimental capability; the [item GET](system-read.md) is
described with retrieval. Root navigation reaches this
description without pretending that a POST-only endpoint is a browsable list.
The locally served renderer remains read-only; interactive submission is disabled.

Controlling sources are [Guide §§4.2–4.3, 4.6, 6.2 and 6.4][Guide],
[CSAPI Part 1 §19.1][Part1], the [pinned creation response][CreateResponse]
and [client-ID permission][ClientId], and [RFC 9110 §§13.1–13.2][Conditions].
The [independent actual-binary proof](system-create-tests.md) records the expected
answers, exact database comparisons, injected failures and scope limits.
Execution results and separate review belong in issue #24 and its PR; this
document alone is not evidence that a check ran or passed.

[Guide]: https://github.com/DGIWG-P507/glaux/blob/273002a311a978569b280697133d0b0d5c2ec756/Docs/Plans/glaux-server/glaux-server-implementation-guide.md
[Part1]: https://docs.ogc.org/is/23-001/23-001.html
[CreateResponse]: https://github.com/opengeospatial/ogcapi-features/blob/9ca25f56a58ed822ea8a685a7a41afa7181aaa8b/extensions/transactions/create-replace-update-delete/standard/requirements/create-replace-delete/create/REQ_response.adoc
[ClientId]: https://github.com/opengeospatial/ogcapi-features/blob/9ca25f56a58ed822ea8a685a7a41afa7181aaa8b/extensions/transactions/create-replace-update-delete/standard/recommendations/create-replace-delete/create/PER_rid.adoc
[Conditions]: https://www.rfc-editor.org/rfc/rfc9110.html#section-13.1
