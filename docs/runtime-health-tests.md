# Runtime configuration and health proof

Task [#18](https://github.com/DGIWG-P507/glaux-server/issues/18) implements only
typed runtime configuration, explicit read-only startup checks and health routes.
The controlling expectations are Implementation Guide §§4.10, 4.12 and 8.1.1,
not the implementation's own parser or a mock database.

Run only in the approved GitHub-hosted Linux environment, after the pinned
database image and locked Cargo archives are available:

```text
python3 scripts/test_runtime_health.py
python3 scripts/test-runtime-health-failures.py
```

The wrapper builds the actual `glaux-server` executable and the independent
`runtime-health-proof` example. Both run as the container's `postgres` OS user
inside the existing owned, network-isolated PostgreSQL/PostGIS harness. No host
port, user database, external identity provider or operational credential is
accepted. The server itself connects as a dedicated non-superuser database role
with only the migration-table read privilege needed by these health checks.

## What is checked

Eight required groups establish:

1. An independently authored configuration matrix rejects unknown keys, absent
   required values, incorrect JSON types, malformed/oversized JSON and public
   development-authentication combinations. Numeric IPv4/IPv6 loopback and the
   timeout boundaries are exercised. `check-config` accepts syntactically valid
   but unusable connection details, proving that it does not connect to storage.
2. Environment/file secret references work; missing, malformed and oversized
   values fail. Synthetic password, payload, path, variable-name and database-name
   canaries must not occur in public diagnostics, including failure paths.
3. The independent raw HTTP interpretation checks exact status, body,
   `Cache-Control: no-store` and body length. Six known-bad response fixtures
   demonstrate that status-only, wrong-body, missing/wrong-cache and wrong-length
   mistakes cannot pass that oracle. It does not deserialize production types.
4. Serving an uninitialized database fails without installing migration state.
   Only the explicit `migrate` command installs the packaged schema. A supplied
   `sslmode=disable` cannot enable an unverified TCP database connection; the
   harness server does not offer TLS, so this route must fail before binding.
5. A post-bind process signal establishes listener startup. Actual HTTP requests
   receive `200/alive` and `200/ready`, minimal non-cacheable bodies, while root,
   Systems, conformance and metrics routes remain absent.
6. Disabling and terminating connections for only the dedicated health role
   makes readiness `503/not ready` while liveness remains `200/alive`. Restoring
   that role restores readiness. A real observed database lock wait separately
   proves bounded readiness failure and an independently responsive liveness
   route while the storage probe is blocked. The shared database is not stopped.
7. Changing a migration checksum makes running readiness fail and a second
   server's startup fail. Neither path repairs the checksum. Explicit test-admin
   restoration makes the original listener ready again.
8. SIGTERM ends an idle listener successfully within the bounded wait. A second
   instance has a ten-second storage probe blocked on an observed schema lock;
   SIGTERM must stop it with the documented non-successful forced-drain result
   within seven seconds (the five-second application bound plus runner allowance),
   rather than waiting for the longer storage operation. Exact
   sentinel System identity/UID/label and all migration version/checksum/success
   entries remain unchanged across normal probes, failures, recovery and stop.

The schema checksum changes, role operations and sentinel insertion belong only
to the disposable test setup. No startup migration, reset or recovery repair is
authorized. The fixture owns its process and files; failure paths terminate only
that process and the harness validates and removes only its own container.
Setup, compilation, timeout and cleanup failures are failures, not evidence that
a behavioral fault was detected.

## Failure sensitivity and limits

The control campaign first builds and runs an unchanged source copy against a
new isolated cluster. A second copy forces the readiness branch to report
healthy even after the actual storage check fails. It must build, start, pass
the preceding configuration/listener checks, and then fail exactly the
`unavailable storage incorrectly reported ready` assertion. The original source
must remain unchanged; a rebuilt restored copy must pass again. Logs and the
baseline/fault/restored record are retained in the normal CI evidence artifact.

This proves the bounded health contract, not JWT validation, CSAPI resource
authorization, a reverse-proxy deployment, telemetry access, broker health,
load capacity, full deployment packaging or graceful recovery of command work.
Those capabilities are not implemented or advertised by this task. The
database-image version and immutable image digest remain the existing harness
pins; this proof creates no additional platform or tool requirement.
