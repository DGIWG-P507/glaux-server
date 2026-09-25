# Shared HTTP boundary

[Issue #19](https://github.com/DGIWG-P507/glaux-server/issues/19) implements
Roadmap 1.4.2 and Guide §§4.1/4.3/6.2/6.4/7.2/8.1.1. This is reusable
request/response handling, **not resource operations or a conformance claim**.
The production listener still exposes only health routes. A separate example
mounts synthetic fixture routes through the same production boundary for
[independent real-listener checks](http-boundary-tests.md); those routes are
not in the production binary.

## Request limits and parsing

The boundary checks URI and aggregate header lengths, then consumes the actual
body under a byte bound (including chunked bodies), before invoking the handler.
No decompression is enabled. Unsupported content coding returns 415 and
`Accept-Encoding: identity`. JSON input must declare `application/json`;
the registration defines no optional parameters, so parameters do not change
JSON's encoding. A media type with a `+json` suffix is not automatically the
same contract. The JSON helper reuses the existing bounded safe parser,
preserving exact numbers and wire object kinds, rejecting duplicate keys and
retaining its existing depth/node/member/string limits. It performs no schema
selection, network/file retrieval, resource semantics or authorization.

| Limit | Default | Configurable range |
| --- | --- | --- |
| Body bytes | 65,536 | 1–8,388,608 |
| Aggregate parsed header bytes | 16,384 | 256–65,536 |
| Request-target bytes | 4,096 | 128–16,384 |
| Body/handler deadline milliseconds | 15,000 | 10–60,000 |

Header accounting includes every name/value occurrence. URI/header rejection
happens before body consumption. The deadline is shared between body collection
and obtaining the handler response, not a promise to cancel committed work or a
streaming-response delivery deadline. No resource writes exist here. The JSON
parser's additional 262,144-byte ceiling remains in force even when a deployment
allows a larger non-JSON request. Malformed/ambiguous media fields and malformed
URI escapes are 400. Oversized bodies are 413, request targets 414 and headers
431. Body timeout is 408; handler-budget exhaustion is a safe 503.

These are Glaux configuration bounds, not standard-imposed sizes. Checks start
after Hyper parses the HTTP head; malformed framing or its own transport limits
may fail before Axum can produce a problem. This does not promise comprehensive
connection/slow-header/traffic admission control or replace the later deployment
and security tasks.

## Media selection

The helper selects only from representations supplied by the implementing
handler. Missing Accept means the first offered representation; a present empty
Accept accepts none. Repeated fields are combined. Type/subtype and parameter
names compare case-insensitively; parameter values remain case-sensitive except
charset. Quoted values retain embedded delimiters and escapes.

For each representation, the most specific matching range supplies its quality:
exact type/subtype outranks a subtype wildcard, then `*/*`; matching media
parameters increase specificity. Thus an explicit `application/json;q=0`
cannot be undone by `*/*;q=1`. Quality uses exact thousandths, never floating
point. Highest quality wins, then specificity, then server offer order.
Identical-specificity duplicate ranges use the highest quality, an explicit
deterministic choice for otherwise equivalent preferences. Unsupported
parameters do not silently match an incompatible offer. Invalid/duplicate q or
parameter syntax is 400; valid preferences with no acceptable offer are 406.
At most 128 ranges, 16 parameters per range and 16,384 media-field bytes are
parsed. Successful negotiated JSON responses carry `Vary: Accept` and
`Cache-Control: no-store`. Later representation/caching owners add their own
contracts without treating this helper as completed codec support.

## Safe errors and correlation

Ordinary boundary errors use `application/problem+json` and `no-store`.
The fixed catalog supplies `type`, `title`, numeric `status`, safe `detail`
and a server-generated `correlation` matching `X-Request-Id`. Client request
IDs are not echoed. Types use the stable `urn:glaux:problem:` namespace below;
they identify problems, not fetchable schema or policy resources.

| Status | Type suffix | Fixed detail |
| --- | --- | --- |
| 400 | bad-request | The request is malformed. |
| 404 | not-found | The requested resource is unavailable. |
| 405 | method-not-allowed | The method is unavailable on this route. |
| 406 | not-acceptable | No offered representation is acceptable. |
| 408 | request-timeout | The request body did not complete within its limit. |
| 413 | payload-too-large | The request body exceeds its limit. |
| 414 | uri-too-long | The request target exceeds its limit. |
| 415 | unsupported-media-type | The request media type or coding is unsupported. |
| 431 | headers-too-large | The request headers exceed their limit. |
| 500 | internal | The operation could not be completed. |
| 503 | unavailable | The operation is temporarily unavailable. |

No API accepts arbitrary detail, SQL, schema paths, policy explanations, request
targets or parser text for the catalog. 405 retains the route's Allow field;
HEAD responses contain no body even on boundary failures. Error problems are
sent even if Accept omits problem+json, the fallback permitted by RFC 9457 §3,
rather than replacing the real failure with recursive negotiation errors.
The existing plain-text health responses retain their separate minimal contract.
Correlation values are diagnostic identifiers, not credentials, resource IDs,
ordering evidence or authority. They do not echo protected data.

## Configured public links

Add an optional `http` object to runtime configuration:

```json
{
  "http": {
    "public_api_root": "https://example.test/deployment/api",
    "limits": {
      "body_bytes": 65536,
      "header_bytes": 16384,
      "uri_bytes": 4096,
      "timeout_ms": 15000
    }
  }
}
```

This fragment supplements the required fields in [runtime configuration](runtime-configuration.md).
Omitting `http` keeps the safe default bounds with **no configured public root**.
Link generation then fails safely; it never guesses an origin. If supplied,
`limits` contains all four fields and rejects unknown fields. An omitted
`limits` uses defaults. An omitted public root does not prevent health serving.

Roots are absolute HTTP(S) URLs with a valid authority, optional path prefix and
no userinfo, query, fragment, backslash, empty interior segment or dot segment.
The bounded ASCII configuration-path syntax permits unreserved characters;
percent-escaped prefixes are deliberately not a supported deployment setting.
A trailing slash is normalized; the prefix is retained when appending links.
Callers supply individual path segments and query name/value pairs, never an
arbitrary relative URL to resolve. UTF-8 bytes and reserved delimiters are
percent-encoded; empty/dot/control-containing path segments are rejected.
Case of resource IDs, paths and query values is retained.

Host, Forwarded and every X-Forwarded-* field are untrusted and never used to
derive links. No proxy-trust adapter is implemented in this slice: deployments
configure the externally visible root explicitly, including its prefix, while
any proxy mounts/rewrites the backend path independently. This does not configure
TLS termination, ingress or routing through that proxy.

## Sources and limits

- [RFC 9110 §§5, 8.3–8.4, 12.4–12.5, 15](https://www.rfc-editor.org/rfc/rfc9110.html):
  field/media semantics, quality, status and Allow.
- [RFC 9457 §§3–5](https://www.rfc-editor.org/rfc/rfc9457.html):
  problem fields, extension members and safe reporting.
- [RFC 3986 §§2–5](https://www.rfc-editor.org/rfc/rfc3986.html):
  URI components and percent encoding; this implementation avoids relative
  reference resolution for untrusted path data.
- [RFC 8259 §§8.1, 11](https://www.rfc-editor.org/rfc/rfc8259.html):
  UTF-8 JSON and application/json registration.

No new dependency, runtime proxy, JWT/policy integration, CSAPI route, discovery
document, codec, compression, database mutation or conformance class is added.
Fault campaigns preserve an executed baseline, the intended behavioral failure
and a restored pass. Setup/compiler/formatter failures remain separate evidence.
