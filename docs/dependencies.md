# Initial dependency and licence inventory

[Issue #6](https://github.com/DGIWG-P507/glaux-server/issues/6) records the initial
build/test dependencies. This is evidence of what the job used, not a new
dependency-approval service or permission to add packages. Later owning tasks
must update the pins, notices and checks deliberately when approved scope needs
new dependencies.

## Components used now

| Component | Identity and purpose | Licence/notice source |
| --- | --- | --- |
| Glaux's three workspace packages | Local `0.1.0` packages; no external Cargo package or feature. `Cargo.lock` is committed and reproduced offline. | [Apache-2.0](../LICENSE), as declared by each package. |
| Rust, Cargo, rustfmt and Clippy | Rust `1.98.1`, minimal toolchain plus the two explicit components; actual component versions appear in each run. | Rust's [Apache-2.0 OR MIT terms and third-party notice instructions](https://github.com/rust-lang/rust/blob/1.98.1/COPYRIGHT); bundled components retain their own notices. |
| Checkout action | `actions/checkout` v7.0.1, `3d3c42e5aac5ba805825da76410c181273ba90b1`. | [MIT project licence](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/LICENSE). |
| Inventory upload action | `actions/upload-artifact` v7.0.1, `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a`. | [MIT project licence](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/LICENSE). Bundled action dependencies are not relicensed by that heading. |
| Python test/check support | Runner Python 3.11+ and its standard library only; no pip dependencies. Actual version is recorded, not silently treated as fixed by `ubuntu-24.04`. | [Python's licence and incorporated-software notices](https://docs.python.org/3/license.html). |
| Disposable database image | Linux/amd64 `postgis/postgis@sha256:7e00e8c3539fdd43f513b98806c8204714dcd09dea683c259e333d7690317119`; PostgreSQL 18.6 / PostGIS 3.6.4. | [Image packaging: MIT](https://github.com/postgis/docker-postgis/blob/2bcd236e3af9ec6e668db51eb37162a79f0eaeaa/LICENSE); [PostgreSQL License](https://www.postgresql.org/about/licence/); [PostGIS: GPL-2.0-or-later](https://github.com/postgis/postgis/blob/3.6.4/LICENSE.TXT); separately licensed image packages. |
| Hosted Linux build environment | `ubuntu-24.04`, with Docker and native compiler supplied by the runner. The run records its actual image, Docker and compiler versions. | [GitHub runner-image inventory](https://github.com/actions/runner-images/tree/main/images/ubuntu); underlying packages retain their own licences. This rolling runner label is not an immutable OS image pin. |

The image's registry package versions are PostgreSQL `18.6-1.pgdg13+2` and PostGIS
`3.6.4+dfsg-2.pgdg13+1`. The platform manifest above is the execution pin; the
floating `18-3.6` tag is provenance only. [Database-test documentation](database-tests.md#pin-provenance-and-limits)
records the registry/Dockerfile sources and actual SQL version assertions.
Neither the database image as a whole nor the distributed Rust toolchain is
covered merely by Glaux's Apache-2.0 licence.

## Per-run evidence

After the workflow provisions the pinned Rust components and pulls the database
image, it runs from the checked-out repository root:

```sh
python3 scripts/dependency_inventory.py > "$RUNNER_TEMP/dependency-inventory.json"
```

The [inventory script](../scripts/dependency_inventory.py) emits JSON only after
every required operation and cleanup succeeds. Diagnostics go to stderr. The
workflow retains the JSON as an ordinary CI artifact for 14 days; the checked-in
script and pins reproduce it after that artifact expires. It contains:

- Commit and lockfile SHA-256, declared workspace licences, actual resolved Cargo
  graph/features, and actual Rust component, Python, Docker and runner versions.
- Exact action revisions checked against the currently reviewed set. Unexpected
  actions, changed pins, external Cargo packages, changed package edges or
  original-code licence drift fail rather than silently become approved entries.
- The pinned image identity and every installed package reported by `dpkg-query`,
  with package/source versions and architecture. Every package is accounted for
  with the path, resolved path and SHA-256 of its available
  `/usr/share/doc/<package>/copyright` file, or an explicit unavailable entry.

The database read uses a fresh, owned container and the existing harness's
target validation and mandatory cleanup. It does not accept a user connection,
mount host data, publish a port or use a networked container. It does not install
anything inside that container. PostgreSQL and PostGIS identities are checked
through actual SQL. The image must already have been pulled explicitly; missing
tools, image, package data, malformed accounting or cleanup failures are errors.

## What this does not establish

Debian packaging copyright text is evidence, not a reliably machine-inferred
SPDX expression for every file. The JSON therefore leaves those expressions
unset. Missing copyright files are visible limitations, not guessed licences or
a clean legal result. Installed package accounting does not cover files outside
the package manager, bundled npm components within GitHub actions, every Rust
toolchain component's transitive notices, or every package on the hosted runner.
It is not a complete release SBOM, CVE scan, legal approval, security certification
or redistribution analysis. No server release or dependency redistribution is
performed by this task. Any later third-party code, schema or fixture reuse needs
its actual terms and notices considered under [CONTRIBUTING](../CONTRIBUTING.md#license-and-contributions).
