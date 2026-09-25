# Credential and development-identity verification

Issue [#20](https://github.com/DGIWG-P507/glaux-server/issues/20), Roadmap 1.4.3,
owns these checks against Guide §4.10 and the test-quality rules in §8.1.1.
They test authentication, not the later action/source/resource policy decision.
A verified caller is not automatically an authorized sensor, producer or user.

## Independent fixtures and observations

The Python standard-library wrapper invokes the GitHub-hosted runner's existing
OpenSSL CLI to generate two ephemeral 2048-bit RSA keys and sign synthetic JWTs
using SHA-256. It records the OpenSSL version. No package is installed, external
identity provider contacted, operational token used or key fetched from a token.
The fixture signer does not call the server's JWT library, encoder or verifier.
The second key produces a genuinely incorrect signature against the configured
first public key. A separate standard-library HMAC fixture exercises asymmetric
versus symmetric algorithm confusion; an unsigned fixture exercises `alg=none`.

Private keys are restricted to the wrapper's owned temporary directory and
deleted before the proof executable runs. Only public JWK configuration and
synthetic tokens enter its fixture JSON file. Whole-directory cleanup is
mandatory; private key contents and token bytes are not printed as evidence.
Keys/signatures vary per run; claim values, exact expected decisions and the
injected clock are fixed. Reproducibility means the same decisions, not identical
random key material. The OpenSSL version is part of the run's evidence.

The Rust proof mounts one synthetic `/who` route through the production
authentication middleware and existing HTTP boundary on an owned ephemeral
loopback listener. An independent blocking TCP client runs on a separate thread
and sends real HTTP/1.1 bytes. It reads status, headers and general-purpose JSON;
it does not deserialize the response into a production caller type. The route
returns the context actually installed by middleware and increments an atomic
counter. Every rejected fixture must leave that handler counter unchanged.
No synthetic route is added to the production binary.

The public fixture file is capped at 128 KiB, client response collection at
16 KiB, client I/O at three seconds, whole Rust proof at 30 seconds and wrapper
execution at 40 seconds. A bound port establishes readiness without sleeps.
The client reports completion even after an assertion panic; the proof shuts
down the owned listener, awaits cleanup and verifies that its address is closed
before propagating the failure. Setup, build, clock, timeout or cleanup failure
is not recorded as a successful authentication check.

## Required groups

The wrapper requires these seven markers once each, in order, followed by one
final success marker. There is no optional target, suite filter or skip mode.

| Group | Independently checked answer and plausible wrong behavior |
| --- | --- |
| `independent-wire-oracle-controls` | The expected complete caller JSON is authored independently. Missing, null, numeric and changed subjects fail its wire-level assertion. |
| `verified-caller-context` | Independently signed access tokens yield the exact issuer, subject, client ID, scopes, groups and JWT kind. Accepted type spellings, an audience array containing the configured service, permitted extensions, `b64:true`, absent optional not-before, deduplicated groups/scopes and case-insensitive Bearer scheme work. A decoded header of exactly 2,048 bytes works. Forged user/group/forwarding fields cannot change the caller; raw Authorization is removed before the protected handler. |
| `signature-profile-and-claim-rejections` | Wrong signature, issuer or audience; unsupported algorithm/key/type/critical header; missing key ID or request-selected key URLs/material; unencoded payload; unsigned/HMAC confusion; duplicate header/claim members; mistyped or absent required claims; malformed/oversized group/scope lists; private-number-marker objects and noncanonical base64 all reject before the protected handler. A decoded header of 2,049 bytes is rejected. Both duplicate-claim values would otherwise pass, so permissive last-wins parsing cannot conceal the defect. Each gets the safe 401 problem and applicable challenge. |
| `exact-validity-clock-and-scope` | The injected instant is exactly `1700000000.500000000` seconds after the Unix epoch. Expiry is exclusive while not-before/issued-at are inclusive, with independently signed values one nanosecond on either side; no integer rounding or default clock skew may alter the verdict. Expiry before issuance fails. Missing required scope is 403, an unavailable clock 503, and restoration returns to success without fallback. |
| `header-framing-and-no-disclosure` | Missing or unsupported authentication scheme receives a Bearer challenge without an error code. Malformed/repeated Authorization fields receive 400; malformed JWTs receive 401. URL parameters and identity headers do not provide credentials. Problems have exact safe type/title/status/detail, matching correlation and no-store, without token/caller/group leakage. |
| `explicit-loopback-development-boundary` | Public/wildcard listener configuration is rejected. Missing/non-loopback transport peers are rejected even with valid development configuration; IPv4/IPv6 loopback peers work. A real loopback request gets only the configured fictional identity, never header-selected identity. Supplying either a valid JWT or invalid credential to development mode fails instead of falling back. Disabled authentication is unavailable, not anonymous permission. |
| `bounded-generated-input-and-clean-shutdown` | Independently signed, otherwise valid tokens of exactly 16,384 and 16,385 bytes exercise the adapter's inclusive size boundary directly; both decoded component sizes are within their individual limits. This deliberately avoids conflating outer HTTP header accounting with JWT size. Seed `0x2001`, a fixed wrapping LCG and 96 malformed compact-token cases require safe rejection and unchanged handler count, with all three generated partitions required. Every owned listener is closed. This is bounded parser regression/property evidence, not an exhaustive cryptographic or JWT fuzz campaign. |

The current adapter's strict profile is intentionally narrower than every JWT
format an identity provider might issue. Full provider integration, issuer-key
refresh/rotation/outages, resource authorization, reverse-proxy trust, TLS
deployment and CSAPI conformance remain separate owning tasks.

## Execution and demonstrated sensitivity

Run only in the approved GitHub-hosted Linux environment from the repository root:

```text
python3 scripts/test_authentication.py
python3 scripts/test-authentication-failures.py
```

The wrapper builds the actual example using locked/offline Cargo. `cargo test`
alone does not execute this real-listener proof. The CI execution guard requires
the full proof's success marker as well as successful process completion.

For test-first evidence, the initial compiling adapter deliberately cannot
authenticate: a valid independently signed fixture must fail specifically at
`valid signed access token did not produce exact verified caller`, after the
oracle controls pass. Formatting, setup or compilation failure is not that red.
The issue/PR identifies the actual run and commit; this document does not claim
that an execution has occurred merely because the fixture exists.

The controlled-fault script first runs a passing unmodified source copy. It then
changes only the configured-audience comparison in a disposable copy, compiles
it successfully, and requires failure at `invalid access-token case accepted:
wrong-audience` after the valid-caller group has passed. Setup failure, a different
assertion or compilation error is not a detected audience-bypass fault. The real
source is byte-compared against its original, rebuilt and rerun to establish
restoration. Baseline, fault and restored logs and JSON results go to the ordinary
CI evidence artifact. Existing HTTP, runtime, parser and database suites remain
required and distinct from authentication evidence.
