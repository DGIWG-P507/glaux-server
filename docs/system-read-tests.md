# Canonical System retrieval proof

Issue #25 / Roadmap 1.5.2 exercises the actual binary's `GET` and `HEAD`
`/systems/{id}` described in [the retrieval contract](system-read.md). Guide
§§4.1–4.3, 4.10 and 8.2 govern navigation, identity, meaning and safe access
responses; §8.1.1 requires independent expected results and demonstrated
failure sensitivity. Backup/restore (#26) and full CRUD are not covered.

Run on the authorised GitHub-hosted Linux runner:

```sh
python3 scripts/test_system_read.py
python3 scripts/test-system-read-failures.py
```

The fixture uses the existing pinned, disposable PostgreSQL/PostGIS harness,
with network disabled, and the same synthetic OpenSSL-signed JWT fixture as the
creation proof. The administrator connection runs migrations and snapshots; the
serving binary uses a distinct least-privilege role. Owned processes, listeners,
directories and database targets must clean up; cleanup failure is fatal.

## Independently expected results

The client is a separate raw HTTP/1.1 socket with general-purpose JSON parsing.
It uses no server types, schemas or response samples. Each System is created
through the public POST path, then its expected representation is written from
the submitted UID, name and exact `featureType` spelling, the generated ID taken
from Location, and the documented self link. A client-supplied `id` and `links`
must not appear.

A successful retrieval must be exactly that JSON, with status 200,
`application/geo+json`, `Cache-Control: private, no-store`, `Vary: Accept`, a
UUIDv7 correlation and no `ETag`, `Last-Modified` or `Location`. A missing or
concealed System must give a Problem whose status, header names, length, cache
directive and body (without its correlation) equal those of a missing ID. Each
correlation must be new and match `x-request-id`. The body must not contain the
hidden System's ID, UID or name.

| Ordered group | Evidence |
| --- | --- |
| independent-wire-oracle-controls | Ten wrong bodies (wrong ID, name, UID case, type spelling, geometry, self link or relation; extra field or link; missing links) and six wrong status or header cases fail the retrieval oracle. Eight wrong 404 variants (leaked detail, different title, extra member, Location, extra header, correlation not matching `x-request-id`, wrong cache directive, wrong status) fail the concealment oracle, while a different correlation alone passes. |
| root-navigation-create-and-exact-retrieval | From `/` only: `service-desc` → API root → POST path → Location → exact GET. Absent, exact and `*/*` Accept agree; `application/json` and `application/sml+json` give a safe 406. HEAD repeats the headers and length without a body. A second System keeps its full-URI type and ignores its supplied ID and link. The API description matches the listener: GET/HEAD only, one `id` path parameter, the Location template, media, cache and `Vary` headers, an example of the same shape, and 405 for undocumented methods and collection GET. |
| restart-retains-identity-and-meaning | The server process is stopped and started again on the same database. Both Systems return the same exact JSON and byte-identical bodies. The database snapshot is unchanged by the reads. |
| missing-and-concealed-are-indistinguishable | A caller of the other source gets identical 404s for a missing ID, the other source's System, an upper-case ID and a malformed ID, and identical 406s before lookup. None of these reads changes the database. The same caller reads its own new System, so the 404 is authorization. A resource-scoped grant and a create-only grant each conceal the first System. |
| verified-token-callers-and-unauthenticated-requests | With JWT authentication, missing and bad tokens give identical 401s for existing and missing IDs. A verified token returns the first System's exact bytes and conceals the other source's System. A forged subject header has no effect. |
| disabled-creation-removes-retrieval | Without `system_creation` the item route is absent (public 404) and the API description has no `/systems` paths. The database is unchanged. |

## Fault controls

The fault run copies the sources into an owned temporary directory and requires
a complete passing baseline. It then compiles and runs two separate disposable
faults in `system_http.rs`:

- **wrong-identity** replaces the body `id` with a fresh random UUIDv7 while
  keeping every other field and the self link. It must fail the
  "right after creation" retrieval assertion after the oracle controls pass.
- **process-memory** makes creation also record the System in process memory
  and makes GET read only from that memory, bypassing the database read. This
  is correct until the process stops, so it must pass navigation and exact
  retrieval, then fail the "after server restart" assertion.

Each mutation target must occur exactly once. The real source must be unchanged
afterwards, and a final restored build must pass completely. Compilation or setup
failure, an unrelated assertion, timeout or missing output does not count as
detection. Actual red/green execution and remaining limits are recorded in the
issue and PR delivery record, not here.
