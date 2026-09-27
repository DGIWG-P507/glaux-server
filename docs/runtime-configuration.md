# Runtime configuration and health-only serving

[Issue #18](https://github.com/DGIWG-P507/glaux-server/issues/18) establishes
Roadmap 1.4.1's startup/health foundation. [Issue #20](https://github.com/DGIWG-P507/glaux-server/issues/20)
adds configured JWT verification and explicit development callers under Guide
§4.10. The binary still exposes **only the two health routes below**, not CSAPI
resource operations or a production deployment. No conformance is advertised.

## Commands and configuration

Build with the [approved hosted instructions](ci.md). On an already approved
development host with the same prerequisites, the resulting binary accepts:

```sh
glaux-server --help
glaux-server check-config /protected/glaux.json
glaux-server serve /protected/glaux.json
```

`check-config` reads, validates, constructs the selected authenticator/policy and resolves
secrets without opening a database connection or HTTP listener, fetching issuer
keys, or issuing tokens. Success prints only `Configuration valid; secrets
redacted.` It does not prove database reachability, schema compatibility, ownership
of a signing key or successful future authentication. Invalid configuration exits 2.

The file is strict JSON, at most 65,536 bytes, with these required fields:

```json
{
  "listener": "127.0.0.1:8080",
  "authentication": "disabled",
  "database": { "url_env": "GLAUX_DATABASE_URL" },
  "health_timeout_ms": 1000
}
```

- `listener` is a numeric IPv4/IPv6 socket address with nonzero port. There is
  no hostname resolution or implicit default listener.
- `authentication` explicitly selects `disabled`, `jwt` or `development`.
  The mode-specific sections below are mutually exclusive; missing, mismatched
  or explicitly null sections fail. No mode grants resource permissions, and
  `disabled` is not anonymous permission: a protected route using that adapter
  fails unavailable.
- `database` has exactly one nonempty `url_env` or `url_file` reference. An
  inline connection string is not a supported field. Referenced values must be
  UTF-8, nonempty and at most 16,384 bytes; outer whitespace is trimmed.
  Environment names use ASCII letters, digits or underscore, at most 256 bytes.
  Files are regular files; relative secret-file paths resolve from the working
  directory. Protect both configuration and secret files and their parent
  directories using deployment access controls. The program does not create,
  chmod, print or provision secrets. Examples never contain real credentials.
- `health_timeout_ms` is an integer from 100 to 10,000. It bounds the complete
  readiness acquisition/schema probe, as well as database statement/lock waits.
  Two pooled database connections are the fixed bound for this health-only slice,
  not a throughput recommendation for the eventual API.

An optional `http` section adds an explicit public API root and bounded request
settings; see the [HTTP contract and exact fields](http-boundary.md).
Omission keeps safe default limits without guessing a public origin.

Unknown fields, duplicate members (including inside public keys), missing required values, unsupported
authentication modes and wrong types fail without quoting the input. Future
adapter and resource-specific settings are not
silently accepted placeholders: their owning tasks will add validated fields.

The optional [local permission policy](authorization.md) adds strict `policy`
grants and finite denial-audit limits. Omission denies all protected resource
operations; authentication alone never grants them. The configured admission
object is available for resource handlers without adding routes to this
health-only binary.

## Authentication selection

With `authentication: "disabled"`, omit both `jwt` and `development`. There is no
implicit fallback identity if credentials are absent or invalid.

For an isolated example, explicitly name its fictional caller:

```json
{
  "listener": "127.0.0.1:8080",
  "authentication": "development",
  "development": {
    "subject": "example-caller",
    "groups": ["example-group"],
    "scopes": ["example-read"]
  },
  "database": { "url_env": "GLAUX_DATABASE_URL" },
  "health_timeout_ms": 1000
}
```

The `subject` is required; `groups` and `scopes` default to empty lists. These are
test labels, not accounts created in an identity provider and not permissions
on Systems or streams. The verified context is explicitly marked `Development`
with issuer `urn:glaux:development`. Both listener configuration and the actual
transport peer must be loopback; request headers cannot supply that peer. Wildcard,
public and IPv4-mapped IPv6 listener addresses are rejected. A supplied Authorization
header is rejected rather than ignored in favor of the development caller.
Do not forward a development listener through a public proxy or published
container port: a loopback check cannot discover an external forwarding path.

For externally issued JWT access tokens, select `authentication: "jwt"`, omit
`development`, and supply a `jwt` object with:

- `issuer`: exact expected issuer identifier, not discovered from the request.
- `audience`: exact service audience identifier.
- `keys`: 1–16 explicitly trusted public RSA JWK objects, each with a unique
  `kid`, `kty: "RSA"`, and canonical base64url `n` and `e` components. The selected
  profile permits 2048–4096-bit RSA keys for RS256. No private key material is
  accepted. Optional `alg`, `use` and `key_ops` must agree with RS256 signature
  verification.
- `required_scopes`: optional list of globally required scope names; defaults
  to empty. These checks do not replace subsequent action/resource authorization.

Use issuer-managed public key values, not invented example key material.
Alternatively omit `keys` (or leave it empty) and configure `jwks`:

```json
{
  "issuer": "https://issuer.example.test",
  "audience": "https://api.example.test",
  "jwks": {
    "url": "https://issuer.example.test/keys",
    "cache_ttl_seconds": 300,
    "refresh_interval_seconds": 10,
    "request_timeout_ms": 1000
  },
  "required_scopes": ["example-read"]
}
```

This is the `jwt` object inside the existing full configuration, not a new
top-level format or a provisioned provider. The three timing fields are required:
cache lifetime 1–3600 seconds, minimum attempt interval 1–300 seconds no greater
than the lifetime, timeout 100–10000 ms no greater than the interval. The URL
must be HTTPS, at most 2048 ASCII bytes, with no credentials, query, fragment or
backslash. No token-directed discovery or redirect is supported. Optional
`trusted_ca_pem` supplies at most eight public CA certificates within 16 KiB,
supplementing platform trust; private key PEM and malformed bundles fail.
Normal TLS name/certificate verification cannot be disabled.

Static nonempty `keys` and `jwks` cannot be combined. Configuration checking
constructs the bounded client but performs no issuer request; it does not prove
endpoint availability or operational trust. The cache starts empty and is loaded
on an eligible protected request. Its finite lifetime is independent of HTTP
cache headers. See [rotation, cooldown and outage semantics](authentication.md#bounded-issuer-key-refresh).
Unknown key IDs fail closed. Configuration bounds
and token type/signature/claim checks are documented in [the authentication
contract](authentication.md). Bearer material is not stored in configuration.

`Configuration::authenticator()` returns the validated adapter for a route group
to wrap with `Authenticator::protect`. The current health-only server does not
attach authentication to its deliberately public minimal health routes or expose
a synthetic protected example endpoint. It does supply actual socket peer
information for future protected groups. Configuring an adapter is not a claim
that resource access policy, identity administration or trusted proxy integration
has been implemented.

## Database secret and transport boundary

Supply the PostgreSQL connection URL through the selected protected reference.
Network connections always use certificate/hostname-verifying TLS even if the
URL asks to disable it. Only an actual Unix-domain socket route disables TLS.
No TLS server or proxy is configured by this issue. A non-loopback health listener
serves only minimal health states over HTTP; a deployment remains responsible
for its ingress/isolation and must not mistake it for secure resource serving.

## Startup, health and shutdown

`serve` validates configuration and secrets, connects to existing storage and
runs the packaged migration-ledger compatibility check **before binding**. Missing,
failed, modified or unknown migration records fail startup. Unavailable storage
also fails startup. This checks the packaged migration history, not an exhaustive
catalog-corruption audit or a guarantee against a database administrator's changes.

Normal startup and probes perform no migrations, resets, seeding or resource
writes. The existing `migrate` and `check-schema` administrative commands still
require `GLAUX_DATABASE_URL`; only an explicit `migrate` changes schema. Their
privileges are separate from serving. For these health-only routes a login role
needs database connection/schema access and SELECT on `public._sqlx_migrations`;
it needs no mutation or migration authority.

| GET route | Exact result | Meaning |
| --- | --- | --- |
| `/health/live` | 200, `alive\n` | The process can respond; no database query. |
| `/health/ready` | 200, `ready\n` | A current bounded connection/schema check passed. |
| `/health/ready` | 503, `not ready\n` | Storage unavailable, incompatible, timed out or no pool capacity. |

Health responses are plain text with `Cache-Control: no-store`. They disclose
no component failure, queue/source state, credentials, configuration, SQL or
policy details. Readiness is recomputed per probe, so loss and recovery of the
required store affect it without changing liveness. A ready result concerns
only this declared health foundation, not future CSAPI operations. Axum handles
HEAD for GET routes; unknown routes do not expose data.

The shared boundary wraps these routes for request bounds and safe 404/405
problems. Successful health and storage-unavailable health responses keep their
plain-text contract; an earlier HTTP limit rejection uses its own safe problem.
The optional public-root setting does not add CSAPI/discovery routes or change
the internal listener paths.

Startup diagnostics are fixed safe messages, never raw parser/driver errors or
effective secret-bearing configuration. A successful bind prints `Health listener
ready.` Optional brokers, metrics, traces, dashboards and public diagnostics are
absent, not implicitly healthy. Their later checks must not become substitutes
for this required storage check.

On Ctrl-C (or SIGTERM on Unix), stop accepting and allow up to five seconds for
in-flight health requests before returning a shutdown error. No background
publication/command work exists here to resume. No local software installation
is part of these instructions. [Real-listener verification](runtime-health-tests.md)
records the accepted/rejected table, secret canaries, storage outage, schema
drift, unchanged data and deliberate wrong-readiness detection.

Dependencies use the approved Axum/Tokio direction with minimal HTTP/1 serving
features: [Axum 0.8.8 serving API](https://docs.rs/axum/0.8.8/axum/fn.serve.html).
The [dependency inventory](dependencies.md) separately records exact resolved
versions/features and licences; no outbound HTTP client/schema retrieval is enabled.
