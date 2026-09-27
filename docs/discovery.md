# Initial discovery and API description

## Source-derived test contract (written before the route module)

Issue #23 implements Guide §§4.1, 4.1.1 and 7.3, not later collection or
resource handlers. Common 19-072 requirement 13 supplies the full conformance
relation URI; `service-desc` and `service-doc` identify machine and human API
documentation. The initial implementation selects these observable answers:

| Fixture | Independently expected result | Wrong behavior to detect |
|---|---|---|
| Discovery enabled with an explicit public root | Root has the exact configured-root self, full OGC conformance, `service-desc` and `service-doc` links; each target works | Hard-coded origin, bare `conformance` substitution, dangling advertised capability |
| Conformance endpoint | `conformsTo` is an empty array | Claiming Common, CSAPI, JSON or OAS 3.0 classes from incomplete discovery alone |
| Document versus listener | OpenAPI 3.1 names exactly every enabled route, its GET and HEAD methods and actual success media; each advertised target is requested | Documentation copied from the upstream full implementation example; phantom routes or undocumented enabled methods |
| Explicit path-prefixed root through a stripping proxy | Every discovery link, OpenAPI server URL and documentation asset uses that prefix | Trusting hostile Host/Forwarded headers, dropping or duplicating the prefix |
| Disabled discovery | Root, document, schemas, examples and renderer assets are absent; health remains unchanged | Advertising disabled resources, an always-on documentation route |
| Negotiation, methods and safe boundary | Offered media succeeds; unacceptable media returns 406; malformed Accept returns 400; unsupported method returns safe 405; HEAD contains no body | Relabeling an unacceptable response, raw framework error or accidental body on HEAD |
| Downloadable schema and examples | Original local schema and both examples are reachable, parse independently and express this initial contract | Remote schema fetching or schemas/examples for unimplemented resource families |
| Local renderer | Same-origin pinned script/CSS and external initializer; API URL from configuration; remote validator, query override and Try-it-out disabled | CDN dependency, remote spec injection, unrequested mutation controls |
| Misleading declaration fault | A compiled wrong conformance declaration fails its exact wire assertion, and restoration passes | A test that merely checks JSON shape or computes expectations from production metadata |

The hosted listener proof uses raw HTTP and general-purpose JSON, not production
wire types or route metadata to generate its expected inventory. Source/header
inspection does not by itself establish that a browser rendered the page.

Controlling sources: [Guide §4.1](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#41-discovery-navigation-and-api-description),
[Guide §7.3](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#73-release-declarations-and-evidence),
[Common 19-072](https://docs.ogc.org/is/19-072/19-072.html), and
[OpenAPI 3.1.0](https://spec.openapis.org/oas/v3.1.0.html).
The OpenAPI specification treats `Accept` header parameters and `Content-Type`
response-header definitions as ignored. Accordingly GET media is represented
through response `content`, while HEAD documents the corresponding GET headers
without advertising a response body; negotiation is described as HTTP behavior,
not an invented query parameter.

The OpenAPI document uses the specification's default OAS 3.1 base dialect,
which extends JSON Schema 2020-12 and is the dialect tooling must support
(§4.8.24.2.3). This also avoids the pinned renderer's unsupported-dialect
warning. The separately downloadable discovery schema retains its own
JSON Schema 2020-12 declaration; no schema constraints change.

## Scope and configuration

Discovery is an explicit opt-in (`discovery: true`) and requires
`http.public_api_root`. Existing health-only configurations retain their previous
behavior. The configured public root is the sole absolute-link authority; the
backend router remains unprefixed, so a prefix deployment must strip its prefix
before forwarding. No forwarded origin header is trusted.

The first surface has no collections or CSAPI resource operations, no standards
conformance declarations, and no draft experiments. Health is described as an
operational extension, not a collection or conformance class. The human page and
machine document describe only this running surface.

## Runtime and renderer boundary

Small typed route definitions sit with the handlers. The same definitions
assemble GET/HEAD routes, discovery links and OpenAPI paths, while independently
written tests check their claims. Parameters, representations, resource family
and conformance dependencies are explicit; initial dependencies are empty because
no class is yet complete. This is code metadata, not a registration service.

`/docs` uses the pinned, locally hosted Swagger UI bundle and stylesheet recorded
in the asset manifest. Its initializer is an external local script, with no
request-derived values. Validation services, query configuration, credential
persistence, syntax highlighting and mutation controls are disabled. Content
security policy restricts scripts and connections to the same origin; the page
also provides ordinary links to the machine document and offline downloads.
Third-party licences/notices remain with the vendored renderer and are served as
downloadable text. No upstream example OpenAPI is served or copied as the
implementation's definition.

JSON Schema and examples here are original Glaux discovery assets, not copies of
the OGC/SensorML corpus. They make no resource-schema or conformance claim.

## Evidence

This file initially records the intended assertions, not an executed result.
The issue/PR execution record identifies the actual behavioral red, passing
hosted checks, deliberate fault/restoration and separate review. Build/setup
failures are not behavioral red; HTTP fetching alone is not browser execution.
