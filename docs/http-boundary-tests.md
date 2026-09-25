# Shared HTTP boundary verification

Issue [#19](https://github.com/DGIWG-P507/glaux-server/issues/19) owns this proof.
It implements Guide §§4.1, 4.3, 6.2, 6.4, 7.2 and 8.1.1, including the issue's
approved wire-interpretation amendment. The [HTTP boundary contract](http-boundary.md)
documents the implemented subset. These are synthetic fixture routes, not an
implementation or conformance declaration for any CSAPI resource.

## What actually observes the boundary

The `http-boundary-proof` example binds an owned `127.0.0.1:0` TCP listener and
mounts synthetic fixture routes through the production shared Axum boundary.
It does not use a database, operational credentials, external target, existing
listener, reverse proxy or machine-global configuration. A separately authored
client uses `std::net::TcpStream` to send actual HTTP/1.1 request bytes. Response
status, headers, framing and JSON values are interpreted directly, without the
server's problem/response types or serializers being used as an oracle.

The server runs on the existing single-thread Tokio runtime. Blocking clients
run on a separate standard thread, signal completion through a channel, and do
not block the event loop. Listener binding establishes readiness without sleeps.
Even on assertion failure, the proof stops its owned listener, awaits bounded
cleanup, proves the address is closed, and then propagates the original failure.
The whole example has a 30-second deadline; the wrapper permits 40 seconds and
treats timeout, setup, build, thread or cleanup failures as failures, never passes.

## Required groups

Every group must occur once in this order, followed by one final success marker.

| Group | Independently asserted contract and meaningful wrong behavior |
| --- | --- |
| `independent-wire-oracle-controls` | Independently authored problem bytes have numeric `status`, exact documented type/title/detail, a correlation matching its header, and the right media/cache headers. Missing, string-typed and wrong-valued statuses fail. An unfamiliar permitted extension remains accepted. No production deserializer can hide these distinctions. |
| `media-and-json-contracts` | Missing/default, wildcard, case, quality, specificity/exclusion, repeated Accept fields and quoted comma parameters select exact status/media/value. A parameter-bearing offered representation exercises quality after parameters, charset case, quoted semicolons/escaped quotes, and parameter-specific exclusion over a broader match. Unsupported preferences return 406, unsupported submitted media/coding 415, invalid quality, duplicate Content-Type, duplicate JSON keys or malformed JSON 400. Repeated content-coding fields cannot hide gzip behind identity. Submitted extension members survive; the private-number-marker object is compared as exact raw bytes before even a general-purpose decoder can coerce it. Later schema/semantic validation is not implied. |
| `safe-problems-methods-and-head` | 404/405/500 use exact safe problem fields even when the client excludes their media type. 405 has the actual GET/HEAD Allow set. HEAD emits no body. Incoming correlation and private-looking request text are not echoed; separate requests have separate correlation values. |
| `bounded-bodies-headers-paths-and-timeouts` | Exactly 256 body bytes work; 257 fixed-length or chunked bytes fail. Oversized headers/URI, malformed percent escapes and encoded controls have exact rejection contracts. A partial body reaches 408; a never-completing handler reaches 503, each within the configured bound and runner allowance. |
| `configured-origin-link-isolation` | Exact configured HTTPS origin and API prefix survive forged Host, Forwarded and X-Forwarded headers. Segments and query values, including delimiters and UTF-8, have independently authored percent-encoded bytes. A dot-segment link fails instead of traversing the root. |
| `generated-accept-and-path-cases` | Seed `0x1901`, fixed wrapping LCG and 128 generated Accept cases exercise all four asserted semantic partitions: supported preference, unsupported parameter, explicit exclusion and invalid quality. Another 64 path cases distinguish safe escaped paths from encoded controls. Wrong status, selected media, values, error content or origin handling is observable, not merely a panic. |
| `no-extra-routes-and-clean-shutdown` | Discovery/resource/metrics paths remain absent. Missing public-root configuration fails link construction safely. Both owned listeners are closed after all assertions. |

Fixture limits are 256 body bytes, 2,048 header bytes, 1,024 URI bytes and 500 ms.
Independent client I/O has a three-second maximum and response collection stops
at 16,385 bytes, rejecting anything beyond 16,384. Fixed small cases are retained
in source as regression inputs; generator identity, seed and partitions are
fixed beside them. This is an initial bounded parser fuzz/property target using
the existing toolchain, not a libFuzzer campaign, exhaustive HTTP grammar proof,
proxy interoperability result or throughput claim.

## Execution and failure sensitivity

Run from the workspace root in the approved GitHub-hosted Linux environment:

```text
python3 scripts/test_http_boundary.py
python3 scripts/test-http-boundary-failures.py
```

No target, filter or selection override is accepted. The wrapper builds the
actual example with locked/offline Cargo and validates every required execution
marker. `cargo test` alone does not execute this listener proof. The existing CI
execution guard separately requires its complete marker set and success summary.

The controlled-fault script first proves a passing unmodified source copy. In
one owned temporary copy it replaces only the configured link origin with
`https://attacker.invalid/`; this must build successfully and fail specifically
at the independent exact-link assertion after the preceding four groups pass.
Compilation failure, setup error, timeout, a different assertion, or missing
expected failure is not fault detection. The real checkout is verified unchanged,
rebuilt, and run again to prove restored behavior. Temporary-source cleanup is
mandatory. Baseline, fault and restored logs plus a JSON outcome record enter the
ordinary CI evidence artifact; retention and exact run/commit are recorded in the
issue/PR, not inferred from this documentation.

The issue execution record distinguishes the actual pre-implementation
behavioral red from formatting/build preparation failures and identifies the
green and controlled-fault runs. This document does not assert that a run occurred.
Required database, runtime-health and earlier regression proofs remain separate
and must still pass. No resource authorization, discovery metadata, conditional
cache semantics, JWT verification, SWE codec or full CSAPI negotiation behavior
is added or claimed by these fixtures.
