# Verified callers and development identities

Issue [#20](https://github.com/DGIWG-P507/glaux-server/issues/20), Roadmap 1.4.3,
implements the authentication boundary in Guide §4.10. Authentication answers
who supplied a credential; resource policy, producer authority, delegation and
permission to change a System remain separate decisions.

## Selected access-token contract

The first adapter accepts externally issued, signed compact JWT access tokens
following [RFC 9068 §§2–4](https://www.rfc-editor.org/rfc/rfc9068.html), with
[RFC 8725](https://www.rfc-editor.org/rfc/rfc8725.html) algorithm, issuer, audience
and token-type separation. This is an explicit provider contract, not support
for arbitrary JWTs or a claim that every OpenID Connect provider uses this format.

- Only RS256 with configured public RSA JWKs is enabled. The JWT header must
  identify an existing unique `kid` and `typ` must be `at+jwt` or
  `application/at+jwt`, case-insensitively. An ID token's `JWT` type is rejected.
- `jsonwebtoken =11.1.0` verifies the signature through its AWS-LC backend.
  Unsigned tokens, algorithm substitution, invalid signatures and unknown keys
  fail closed. No home-grown signing or signature verification is used.
- Issuer and audience must match the operator's exact configured identifiers;
  a typed audience array may contain the selected audience. Required `iss`,
  `sub`, `client_id` and `jti` are nonempty bounded strings.
- Required `exp` and `iat`, and optional `nbf`, are JSON NumericDates. Exact
  decimal comparison against the injected clock preserves fractional boundaries:
  expiry is exclusive, not-before inclusive. Zero skew, rejecting future
  issuance, and requiring expiry after issuance are explicit Glaux choices.
  Production uses the system clock; clock failure denies service, not validity.
- Optional `scope` uses the OAuth space-separated token grammar. Optional
  `groups` uses this adapter's string-array profile. Duplicates are folded in
  first-occurrence order after bounds/type checks. Any configured required
  scope must be present. Neither scopes nor groups grant resource permission
  by themselves.

The cryptographic library's integer-rounded wall-clock claim checks are disabled
only because the adapter performs its own exact, typed, injected-clock checks.
Signature and RS256 verification remain mandatory. Claim JSON is duplicate-safe
and retains wire types before checking; library-normalized claim values are not
used as authority. No caller context is constructed until every check succeeds.

Tokens are capped at 16,384 bytes; decoded headers at 2,048 bytes and claims at
12,288 bytes. Canonical unpadded base64url is required. RSA keys are 2048–4096
bits, with 1–16 configured keys. Identifiers are at most 1,024 bytes, key IDs 128,
and NumericDate spellings 128. Lists have at most 64 input members, scope tokens
128 bytes and group names 256. Configuration has its existing 64 KiB total bound.
The HTTP layer's independent aggregate-header budget may reject a request before
the token parser. These are documented resource limits, not universal JWT rules.

No critical-header extensions, unencoded payload, JWE, compression, remote
key/certificate hint, symmetric key, private key or provider discovery is
supported. Unknown noncritical ordinary extension members are tolerated without
authority. Public keys come from trusted configuration, never from token URLs.
Static keys do not promise rotation, revocation or outage refresh: issue #21
owns bounded trusted-key refresh and its unavailable behavior.

## HTTP and caller boundary

Only an Authorization Bearer header supplies a credential; query parameters,
cookies and forwarded/user/group headers cannot select a caller. Repeated or
malformed Authorization framing fails. The scheme is case-insensitive.
[RFC 6750 §3](https://www.rfc-editor.org/rfc/rfc6750.html#section-3) controls the
Bearer challenge: missing/unsupported credentials yield 401 with `Bearer`;
invalid tokens 401/`invalid_token`; malformed framing 400/`invalid_request`;
insufficient configured scope 403/`insufficient_scope`. Unavailable verification
is a safe 503. Fixed problem bodies contain no token or protected reason.

`Authenticator::protect` wraps explicitly selected route groups and installs a
non-deserializable caller context with issuer, subject, optional client ID,
scopes, groups and caller kind. It discards any previous context and removes raw
Authorization before the protected handler. Successful protected responses are
`private, no-store`; errors use the shared no-store problem contract. Use the
shared HTTP boundary outside protected routes for correlation, safe errors,
request bounds and timeouts.

The production binary still serves only minimal public health routes. Its
validated configuration exposes the authenticator for future resource groups;
the synthetic protected route exists only in the proof executable. There is no
identity-inspection endpoint or anonymous write path in the server. Deployment
TLS, proxy trust and resource policy are not implemented by this task.

## Explicit development identities

Select development mode explicitly and configure its subject, groups and scopes
as shown in [runtime configuration](runtime-configuration.md). These are fictional
test labels, not identity-provider accounts. There is no implicit default caller
and no request-header identity selection. The resulting context carries kind
`Development` and issuer `urn:glaux:development`.

Both configured listener and actual socket peer must be loopback; missing peer
information fails closed. Development mode rejects a supplied Authorization
header, including an otherwise valid JWT, rather than silently falling back.
Disabled mode rejects protected operations rather than granting anonymous access.
Never expose a development listener through port forwarding or a public proxy:
the socket's loopback address cannot reveal an upstream forwarding path.

See [independent authentication checks](authentication-tests.md) for real-listener
evidence, exact expected callers, rejected credentials, boundary clocks, cleanup,
and a deliberately bypassed audience comparison that the tests must detect.
