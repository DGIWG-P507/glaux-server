# Resource identity boundaries

Issue [#9](https://github.com/DGIWG-P507/glaux-server/issues/9) implements Guide
§§4.2 and 6.1 in `glaux_domain::identity`. These are domain values, not resource
families, database keys enforced by a database, route handlers or authorization.

| Type | Meaning and local contract |
| --- | --- |
| `LocalId` | UUIDv7 locator. Parse exactly 36 lowercase hexadecimal/hyphen characters with version 7 and RFC variant bits `10`. Display preserves that canonical spelling. Reject uppercase, compact/braced/URN forms, percent encoding, whitespace, wrong versions/variants and nil/max; never repair input. |
| `Uid` | Absolute RFC 3986 URI with a scheme. Preserve the supplied bytes, including case, percent spelling and fragments. No normalization, retrieval, existence/ownership check or conversion to a local ID. Unknown schemes are allowed; a raw Unicode IRI is not silently converted into a URI. |
| `SourceAuthority` | Opaque namespace/context name; not necessarily a URI, an authenticated issuer or a permission. |
| `SourceIdentifier` | Opaque source-assigned text; never enough by itself to identify a source resource. |
| `SourceIdentity` | The authority and identifier pair. Equality/hashing include both; neither labels nor matching source values automatically merge resources. |

UIDs and source fields have a 4,096-byte **local parsing budget**, not a claimed
standards limit. Source fields must be nonempty and have no Unicode control
characters; other bytes, including spaces, case and Unicode, remain unchanged.
The types deliberately lack implicit conversions into each other. String parsing
does not infer meaning: UUID-looking source text stays source text, and a
`urn:uuid:…` is a UID rather than a Glaux local-ID spelling.

## Generation and its limits

`LocalId::generate` combines the current Unix millisecond (48 bits) and fresh OS
randomness (74 retained bits), setting the UUIDv7 version and RFC variant. A
pre-epoch clock, out-of-range timestamp or entropy failure returns a typed error;
no fallback clock, truncation, weak randomness or partially filled entropy buffer
is used. OS entropy can block during early boot; no time bound is promised.

Generation promises neither monotonic ordering nor impossible collisions. Clock
rollback and multiple calls in a millisecond still use fresh randomness. Later
persistence tasks own uniqueness/conflict enforcement, required UID uniqueness
and non-reuse of deleted local IDs. No database guarantee is claimed here.

"Opaque locator" is an API rule, **not confidentiality**: UUIDv7 exposes approximate
minting time. No API extracts that time or turns an ID into permission. It must
never replace observation occurrence/result time or an authorization decision.
A caller can deliberately decode a UUID string; type boundaries prevent accidental
substitution, not malicious interpretation or all future policy errors.

## Sources and dependency selection

- [RFC 9562 §5.7 and Appendix A.6](https://www.rfc-editor.org/rfc/rfc9562.html):
  UUIDv7 layout and the independently published deterministic test vector;
  §§6.9, 6.12 and 8 discuss randomness, opacity and security/privacy limits.
- [uuid 1.26.1 Builder](https://github.com/uuid-rs/uuid/blob/v1.26.1/src/builder.rs):
  explicit timestamp/random-byte construction; no convenience API that hides
  fallible clock or entropy handling. Exact version, default features disabled.
- [getrandom 0.4.3](https://docs.rs/getrandom/0.4.3/getrandom/fn.fill.html): fallible OS
  entropy. The direct dependency uses the supported current release under
  [upstream's security policy](https://github.com/rust-random/getrandom/security),
  rather than selecting the older transitive version solely to share it. This
  is a maintenance choice, not a claim that an older version has a vulnerability.
- [fluent-uri 0.4.1](https://docs.rs/fluent-uri/0.4.1/fluent_uri/struct.Uri.html#method.parse):
  borrowed RFC 3986 URI parsing with a scheme; no normalization. Defaults disabled.

The exact resolved graph, feature unions and packaged licence/notice hashes are
reviewed in [the dependency snapshot](cargo-dependencies.json). Direct no-default
declarations do not mean a transitive user cannot enable a shared crate's features.

## Verification

Seven named Rust tests are required to be discovered **and actually pass**. They
cover the RFC vector/bytes, exact text, malformed forms, all 256 version/variant
nibble combinations, clock/range/entropy errors, zero/max timestamps, repeated and
rolled-back clocks, 64 real-entropy generations, URI spelling/rejection/budgets,
and source-pair separation/budgets. Expectations are literals or directly derived
from the published layout, not another call to the production serializer.

The hosted boundary harness first builds and executes valid external-client code,
then requires specific Rust type/missing-method diagnostics for identity, time and
permission confusion. It must reject an unexpectedly compiling control; missing
dependencies or arbitrary compiler failure do not count. A disposable source
copy separately removes the UUID version check: its exact assertion must first
pass normally and then fail with the named wrong-version example. That is a
behavioral failure proof, not a build failure or a passing round-trip alone.

These bounded checks are not statistical proof of randomness or uniqueness,
authorization-policy tests, endpoint conformance, storage tests or a new broad
fuzz campaign. Existing corpus, schema fuzzing, database and false-green checks
remain required. All runtime execution is on the GitHub-hosted runner.
