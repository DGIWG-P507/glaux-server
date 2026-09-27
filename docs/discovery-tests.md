# Initial discovery proof

Issue #23 / Roadmap 1.4.6 implements partial discovery, not complete CSAPI or OGC
Common conformance. This expected contract was authored before the discovery
handler was wired into the running server. Guide §§4.1, 4.1.1 and 7.3 require
root navigation, configuration-true methods/media and honest declarations;
§8.1.1 requires independent interpretation and demonstrated failure detection.

## Independently expected contract

The Common discovery link identifies the conformance document by
`http://www.opengis.net/def/rel/ogc/1.0/conformance`; `service-desc` and
`service-doc` identify the API definition and human-readable documentation.
The endpoint names below are Glaux implementation choices, not additional OGC
requirements. The client starts with only the root URL and discovers those
targets before comparing them with this independently specified inventory.

| Route | GET response media | Methods |
|---|---|---|
| `/`, `/conformance`, `/api` | `application/json` | GET, HEAD |
| `/docs` | `text/html` | GET, HEAD |
| `/docs/init.js`, `/docs/swagger-ui-bundle.js` | `text/javascript` | GET, HEAD |
| `/docs/swagger-ui.css` | `text/css` | GET, HEAD |
| `/docs/LICENSE`, `/docs/NOTICE`, `/docs/swagger-ui-bundle.js.LICENSE.txt` | `text/plain` | GET, HEAD |
| `/schemas/discovery.json` | `application/schema+json` | GET, HEAD |
| `/examples/landing.json`, `/examples/conformance.json` | `application/json` | GET, HEAD |
| `/health/live`, `/health/ready` | `text/plain` | GET, HEAD |

HEAD returns GET's media metadata and no entity body. Other methods are rejected.
OpenAPI is exactly 3.1.0, has exactly these implemented paths, and uses the
configured public API root. There are no advertised CSAPI, OAS 3.0, Common,
filtering or experimental conformance classes yet: `conformsTo` is exactly `[]`.
There are no Systems/collections/resource routes, placeholder schemas for those
resources, or advertised unfinished extensions.

Enabled discovery requires an explicit public API root. An omitted or false
`discovery` setting keeps the existing health-only listener. The same deployment
contract applies through an actual prefix-stripping loopback proxy; attacker
Host/Forwarded/X-Forwarded-* values cannot replace configured links.

## Execution and limitations

`python3 scripts/test_discovery.py` builds the actual server and independent Rust
proof on the approved hosted runner. The proof executes inside the existing
pinned, owned PostGIS container with `--network none`. It explicitly migrates
the isolated database, seeds a sentinel through the existing trusted fixture
helper, and runs the server as a role that can read only the migration table.
It compares complete ordered application-table rows and migration facts before
and after discovery, rather than checking only row counts.

The client uses raw HTTP and general JSON values, never production discovery
types or route metadata. Known-bad wire fixtures exercise exact link and
declaration assertions. Actual requests cover every advertised method/media,
negative methods, unsupported and malformed Accept negotiation, unknown/unfinished paths, disabled discovery,
forged origins, prefix proxy navigation, schema/example downloads and exact local
renderer asset bytes. Asset hashes are checked independently against their
pinned manifest before execution. Browser execution is a separate hosted proof;
the container proof does not claim that parsing HTML executes JavaScript.

`python3 scripts/test-discovery-browser.py` separately uses the hosted runner's
already-provisioned Chrome and records its actual version and runner image.
Its version/setup probe makes one attempt with a 30-second deadline and an owned
process group, separately from the unchanged 20-second rendering deadline and
180-second suite bound. Timeout kills only that group and allows five seconds
for pipe cleanup and reaping. Empty output, nonzero exit, oversized diagnostics and
cleanup failure remain fatal; there is no automatic retry. Monotonic elapsed
time and bounded diagnostic tails are recorded for both success and failure.
Hosted controls exercise a known version, nonzero and empty rejection, and an
actually started sleeping probe that must time out and be killed/reaped.
The earlier five-second Chrome-version timeout remains an observed setup
failure with an unidentified cause, not a rendering defect or a claimed fix
to Chrome itself.
It follows the root's documentation link and checks the actual initial rendered
title, all 30 independently expected GET/HEAD operations, partial-implementation
notice and absence of renderer errors and initial execution controls. Six
known-bad DOM fixtures exercise these assertions. Same-origin and external
specification/configuration query overrides must leave the rendered contract
unchanged and must not request the test-controlled replacement targets.

The browser uses fresh owned profiles, an owned deny-only proxy, disabled QUIC
and non-loopback DNS resolution. An external canary first proves the proxy's
rejection. Every subsequent proxy request is also denied; there is no external
hostname exception list or upstream forwarding path. The evidence records all
blocked attempts without attributing them to the page or the browser. Chrome
can attempt external services despite its background-network disabling flags;
successful rendering with those requests denied proves no successful external
response is needed, not that the process attempted no telemetry. Unexpected
proxy errors fail the check; a client disconnect while writing an already
recorded denial is retained as such. Exact local request inventory checks,
finite child-process deadlines and listener/profile/proxy cleanup remain
required. This initial-DOM smoke does not click through collapsed operations;
the initializer's disabled submit-method configuration is checked separately.

The test-only `glaux-standards` example `discovery-schema-proof` compiles the
actual downloaded discovery schema and validates both actual downloaded
examples, for direct and prefixed deployments. It reuses the existing pinned
JSON Schema validator dependency without widening the production validator API.
Bounded stdin, local-reference preflight and a denying retriever prohibit remote
schema access. Missing-link and nonempty-conformance examples must fail; an
invalid schema type must fail compilation; a deliberately remote reference must
be refused. Both complete executions have an exact required output marker.

`python3 scripts/test-discovery-failures.py` starts from a passing disposable
source-copy baseline, advertises an unfinished class in a compiled server,
requires the named declaration assertion to fail, then rebuilds the untouched
source and requires a complete restored pass. Compilation/setup failure or a
different assertion is not successful fault detection. All child listeners,
proxy threads and database targets have bounded, fatal cleanup.

Execution results and initial behavioral red are recorded in the issue/PR.
Nothing in this document is itself a passed run or a certification claim.
