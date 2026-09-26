# Trusted-key refresh verification

Issue [#21](https://github.com/DGIWG-P507/glaux-server/issues/21), Roadmap 1.4.4,
owns this proof under Guide §§4.10, 4.12 and 8.1.1. The following expected state
transitions are specified before execution, not inferred from a green result.
These are local adapter choices implementing bounded trust; they are not claims
that a JWT standard mandates these exact timeout values or cache policy.

## Independent expected state transitions

The ordinary fixture uses a ten-second trusted-key lifetime, two-second global
refresh interval and 300 ms network timeout. A manual key-cache clock starts at
100 seconds; a separate token-validity clock starts at Unix time 1700000000.
Issuer network timing is real operating-system time, not either manual clock.
Explicit barrier cases use a one-second fetch timeout to allow control round
trips. Their gate must be released explicitly within five seconds; gate timeout
is a fixture failure. The streamed-body deadline case retains 300 ms and checks
that the pending fetch completes within a one-second outer bound.

| Event | Expected decision and fetch count |
| --- | --- |
| Empty cache, valid token A | One HTTPS fetch; exact verified caller A. |
| Known key while fresh | Same verified caller; no additional fetch. |
| Unknown key during cooldown | Invalid token; no additional fetch, even for many different IDs. |
| Eligible unknown key after interval | At most one fetch; a successful complete key set replaces rather than merges the previous one. |
| Successful A-to-B rotation | B succeeds; removed A fails even before the old set's former expiry. |
| Failed replacement while A remains fresh | Initiating request is unavailable; later A uses only the original deadline, never an extended one. |
| Original lifetime expires during issuer outage | Unavailable; never stale-key acceptance. Cooldown requests cannot retry or extend trust. |
| Outage ends after next eligible attempt | One fetch restores verification; earlier failed requests remain failures. |
| Concurrent fetch in progress | No queued refresh/waiter fanout. Empty/expired requests fail unavailable, fresh unknown keys fail invalid, and already-known fresh keys remain usable. |
| Aborted request | The in-flight flag is cleared, but the attempt's global cooldown remains. |
| Key clock absent or moving backward | Unavailable, not newly fresh trust. |
| Token expires while refresh is in flight | Reject after retrieval using the reread token clock. |

Request-controlled key URLs/material, malformed compact/profile headers and
unknown issuers cannot redirect retrieval. Only the one configured HTTPS URL
may be contacted. Redirects, untrusted certificates, wrong certificate names,
non-200 responses, unsupported content types, malformed/duplicate/private-key
JWKS and bodies beyond 64 KiB fail without partial cache replacement. Fetch
deadlines include streamed response bodies. The existing #20 proof continues to
own token cryptography/profile and precise NumericDate behavior.

## Fixture, observations and safety

The Python standard-library wrapper creates an owned temporary directory,
ephemeral RSA signing keys and a synthetic certificate authority/leaf using the
hosted runner's already-installed OpenSSL. The leaf has only the loopback IP
subject alternative name. An HTTPS issuer and a separate control listener bind
only `127.0.0.1:0`. No external issuer, operational credential, user database,
package installation, persistent service or machine trust-store change is used.
The public CA is supplied explicitly to each applicable adapter configuration.

The control listener changes a finite issuer response mode, returns actual
request counts and establishes event barriers. It is not a production endpoint.
Barriers observe a received request before testing in-flight behavior; sleeps
are not used to establish ordering. Counts distinguish attempted configured
JWKS retrieval from control traffic and the attacker-target path. TLS failures
occur before the issuer HTTP handler and therefore cannot be described as an
HTTP request count. Issuer handlers never log request credentials.

Rust uses the real asynchronous adapter, independently checks exact caller
fields and denial variants, and also exercises the middleware through an owned
HTTP listener with raw status/header and ordinary JSON interpretation. It does
not deserialize a wire response into a production caller/problem type. Wrong
identity and mistyped/missing wire status control fixtures demonstrate the
observations are discriminating. Protected handler counts prove rejection does
not reach the operation.

Every process/listener/thread belongs to the fixture and has bounded shutdown.
The wrapper records OpenSSL/Python TLS versions, rejects missing/duplicate or
reordered group markers, and propagates setup, timeout, cleanup or assertion
failure. A failed phase is not made successful by retrying until the issuer
eventually answers. Evidence records actual checks and source commits elsewhere;
this document does not assert that execution has already occurred.

## Commands and limits

Run only in the approved GitHub-hosted Linux environment, from the repository root:

```text
python3 scripts/test_key_refresh.py
python3 scripts/test-key-refresh-failures.py
```

The required groups are initial cache reuse, rotation and bounded unknown keys,
expired trust/outage/recovery, hostile hints and bounded responses, TLS and HTTP
transport failures, concurrency/cancellation/clocks, and middleware/cleanup.
The owned local issuer fixture is reused by the disposable-source failure check.
That check first passes unmodified source, then compiles a deliberately stale-key
accepting variant and requires the named stale-rejection assertion to fail; a
compile/setup/timeout failure is not mutation detection. It verifies the real
source is unchanged, rebuilds and reruns the restored source. Logs and exact
outcomes enter the ordinary CI evidence artifact.

The implementation source was already drafted when this proof was completed.
This is a source-first iteration, not a claimed pre-implementation behavioral
red. The required alternative is the compiled stale-trust mutation above: its
baseline must first pass, its targeted denial assertion must then fail for the
specified reason, and restored source must pass. Build, setup, fixture, timeout
or unrelated assertion failures do not satisfy that evidence requirement.

This is not a production identity-provider deployment, resource authorization,
OAuth token issuance, revocation/introspection, throughput result, exhaustive
network fuzz campaign or CSAPI conformance claim. Cache TTL bounds stale trust;
it does not promise immediate revocation of a key still in a fresh accepted set.
