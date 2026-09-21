# Disposable database tests

[Issue #5 / task 1.1.3](https://github.com/DGIWG-P507/glaux-server/issues/5)
owns this initial harness. Its execution record links the tested head, failures,
separate review and final result. This checks a real database through the pinned
image's own PostgreSQL client; it does **not** establish Rust SQLx connectivity,
application storage, a running CSAPI server or production deployment.

## Run and dispose

On the selected GitHub-hosted Ubuntu runner, from the repository root:

```text
python3 -c 'import sys; sys.path.insert(0, "scripts"); from database_harness import PIN, docker; print(docker("pull", "--platform", PIN["platform"], PIN["image"], timeout=180))'
python3 -u scripts/test_database.py
```

The existing [Build workflow](../.github/workflows/build.yml) runs these after the
Rust checks. Prerequisites are Python 3.11+ (standard library only) and a local
Linux Docker engine at `unix:///var/run/docker.sock`, as supplied by the hosted
runner. There is no pip dependency or new Cargo dependency. The pull is an
explicit network step; test containers themselves use `--network none`.
This is not an instruction to install Docker/Python on the company laptop.
An approved Linux environment may run the same explicitly requested disposable
test command; no connection string or existing database/container can be supplied.

Each harness instance creates a fresh, nonce-labelled container. Before SQL,
reset or removal it verifies that exact full container ID, name, ownership label,
image, isolated network, absence of host binds/published ports, and expected tmpfs
configuration. It never searches for databases or containers to reset. Test data
lives on a container-private tmpfs at `/var/lib/postgresql`; there is no persistent
volume or host-directory data mount. Removal is limited to the verified container
and any associated anonymous volume, never a prune or wildcard operation.

The container uses trust authentication solely inside this network-isolated test
instance, with no operational credentials. psql connects to `127.0.0.1` **inside**
that container. No PostgreSQL port is published to the runner or outside it.
Do not reuse these authentication settings for a reference deployment.

Cleanup is registered on creation and runs on setup/assertion failures as well as
success. Removal is verified; cleanup errors fail rather than disappear. If both
test and cleanup fail, both errors remain in the exception group. A failed cleanup
fault test explicitly clears its injected fault and removes the retained target;
that is an asserted negative case, not retry-until-green for an unknown failure.
Abrupt runner termination can prevent Python cleanup; GitHub's disposable runner
is the outer lifetime boundary, not evidence that a cancelled test passed.

## What is asserted

- SQL identity, connected user, exact PostgreSQL version number, exact PostGIS
  extension version/namespace and actual `postgis_full_version()` output.
- The [initial immutable SQL migration](../crates/glaux-server/migrations/0001_enable_postgis.sql)
  enables PostGIS explicitly in a fresh `template0` database. There are no
  CSAPI-family tables yet. The harness logs its SHA-256 and runs it transactionally
  with psql `-X -w --set=ON_ERROR_STOP=1`; no implicit server-startup migration.
- Independently specified geometry/value/time fixtures, not results generated
  from the database and then accepted as expectations.
- Reset, a second container and a later fresh container do not inherit a mutable
  marker. A control-database sentinel remains unchanged by target resets.
- A transactional setup fault rolls back the extension and blocks fixture use.
  A reset fault also blocks fixture use instead of continuing on stale data.
  Stopped storage causes an actual connection failure.
- Wrong ownership prevents lifecycle operations, and a narrow injected cleanup
  failure is reported and retains the target until explicit cleanup.

Readiness is bounded actual TCP SQL polling, not a sleep or socket-open claim.
TCP avoids accepting the image entrypoint's temporary socket-only initialization
server. The deadline is 45 seconds, each command is bounded, and SQL has a
5-second statement timeout / 1-second lock timeout. Missing Docker/image/storage,
setup or runner errors cannot become skipped or successful tests.

The test runner requires all seven named lifecycle tests, seven executions,
no failures and no skips. The workflow also requires its exact success summary.
The tests print the fixed synthetic fixture inputs; randomness only gives the
container its collision-resistant ownership identity, which is logged. Database
or host wall-clock time is not an expected observation value.

## Pin provenance and limits

[database-image.json](../scripts/database-image.json) pins Linux/amd64 to:

```text
postgis/postgis@sha256:7e00e8c3539fdd43f513b98806c8204714dcd09dea683c259e333d7690317119
```

This is the platform manifest for the PostGIS project's `18-3.6` image, not a
moving tag used at execution. Registry metadata reports PostgreSQL
`18.6-1.pgdg13+2`, PostGIS `3.6.4+dfsg-2.pgdg13+1`, and build
`2026-08-31T11:33:54.076962224Z`. The harness asserts SQL version number
`180006` and extension version `3.6.4`, rather than trusting image labels.
The relevant run records actual Docker, runner, SQL, extension and migration
identities; a pin alone is not executed compatibility or a security audit.

Sources: [registry metadata](https://hub.docker.com/v2/repositories/postgis/postgis/tags/18-3.6),
[upstream Dockerfile](https://github.com/postgis/docker-postgis/blob/2bcd236e3af9ec6e668db51eb37162a79f0eaeaa/18-3.6/Dockerfile),
[PostgreSQL entrypoint](https://github.com/docker-library/postgres/blob/4a1f78ff7e7a6e7ecb6a584c540c07946ad66e80/docker-entrypoint.sh),
[psql error handling](https://www.postgresql.org/docs/18/app-psql.html),
[template0](https://www.postgresql.org/docs/18/sql-createdatabase.html),
[Docker network isolation](https://docs.docker.com/engine/network/drivers/none/).

Container packaging is [MIT](https://github.com/postgis/docker-postgis/blob/2bcd236e3af9ec6e668db51eb37162a79f0eaeaa/LICENSE);
PostgreSQL has its [PostgreSQL License](https://www.postgresql.org/about/licence/);
PostGIS is [GPL-2.0-or-later](https://github.com/postgis/postgis/blob/3.6.4/LICENSE.TXT),
with separately licensed bundled dependencies. The entire image is not MIT or
Apache-2.0. Full dependency/security inventory remains #6; no peer source is copied.

These are lifecycle and initial-extension tests, not backup/restore, load,
spatial-filter conformance, Rust-driver, production authentication or family-schema
tests. The fixture SQL and lifecycle helpers are test support, not application
storage interfaces. Later owning issues add SQLx queries, resource migrations and
their real-database assertions without weakening this isolation boundary.
