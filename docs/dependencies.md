# Initial dependency and licence inventory

[Issue #6](https://github.com/DGIWG-P507/glaux-server/issues/6) records the initial
build/test dependencies. This is evidence of what the job used, not a new
dependency-approval service or permission to add packages. Later owning tasks
must update the pins, notices and checks deliberately when approved scope needs
new dependencies.

## Components used now

| Component | Identity and purpose | Licence/notice source |
| --- | --- | --- |
| Glaux's three workspace packages | Local `0.1.0` packages with the same inward boundaries; `Cargo.lock` is committed and used without re-resolution. | [Apache-2.0](../LICENSE), as declared by each package. |
| Structural validation | `jsonschema =0.56.0`, defaults disabled; `serde_json =1.0.151`, `arbitrary_precision` and `raw_value`. Exact transitive graph, all-target features, archive checksums and notice hashes: [reviewed Cargo snapshot](cargo-dependencies.json). | Package terms and actual packaged notices are recorded per dependency; no HTTP/filesystem retrieval feature or HTTP client is enabled. See [selection and limits](structural-validation.md). |
| Typed identities | `uuid =1.26.1`, `getrandom =0.4.3`, `fluent-uri =0.4.1`, each with defaults disabled. [Selection, contracts and limits](resource-identities.md). | Actual packaged licences/notices and unified features are recorded in the reviewed Cargo snapshot; existing transitive `getrandom` remains separately accounted. |
| Exact numbers | `num-bigint =0.4.8`, `num-rational =0.4.2`, `num-traits =0.2.19`, defaults disabled; rational requests `num-bigint`. [Contracts and limits](exact-numbers.md). | Existing locked packages reused as direct domain dependencies; actual unified features and packaged MIT/Apache notices remain in the reviewed snapshot. |
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

The [initial standards corpus](standards-corpus.md) separately retains 129 schemas,
four publication source headers and five licence/notice files. Its manifest records
source revisions or digest-pinned snapshots, retrieval times and exact URI mappings.
OGC, GeoJSON MIT and JSON Schema BSD/AFL notices remain with the originals; Glaux's
Apache-2.0 licence does not relicense them. Corpus packaging itself adds no Cargo or pip dependency.
The corpus check and its log complement, rather than modify, the runtime dependency
inventory below. The optional PowerShell acquisition script is not a CI dependency.

After the workflow provisions the pinned Rust components and pulls the database
image, it runs from the checked-out repository root:

```sh
mkdir -p "$RUNNER_TEMP/glaux-ci-evidence"
python3 scripts/dependency_inventory.py > "$RUNNER_TEMP/glaux-ci-evidence/dependency-inventory.json"
```

The [inventory script](../scripts/dependency_inventory.py) emits JSON only after
every required operation and cleanup succeeds. Diagnostics go to stderr. The
workflow retains the JSON as an ordinary CI artifact for 14 days; the checked-in
script and pins reproduce it after that artifact expires. It contains:

- Commit and lockfile SHA-256, declared workspace licences, actual resolved Cargo
  graph/features, and actual Rust component, Python, Docker and runner versions.
- Exact action revisions checked against the currently reviewed set. Unexpected
  actions, changed pins, unreviewed Cargo packages/features, changed package edges or
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

Task #9's reviewed lock contains 82 registry packages (three more than #8), including target-specific
dependencies not necessarily compiled on Linux. No HTTP client or jsonschema
HTTP/filesystem retrieval feature is enabled. All declared expressions offer
permissive terms; compound Unicode data terms remain recorded rather than reduced
to the crate's MIT/Apache heading. This is dependency-selection accounting, not
a legal opinion or a completed release-redistribution check.

Six archives contain no separately named licence/notice file: `jsonschema-regex`
and `jsonschema-value` 0.56.0 (MIT, [upstream workspace](https://github.com/Stranger6667/jsonschema/tree/rust-v0.56.0)),
`uuid-simd` and `vsimd` 0.8.0 (MIT, [upstream](https://github.com/Nugine/simd)), and
`r-efi` 5.3.0 and 6.0.0 (MIT OR Apache-2.0 OR LGPL-2.1-or-later,
[upstream](https://github.com/r-efi/r-efi)). Their package metadata is present;
the snapshot explicitly lists the absent packaged files. Do not describe them as
missing licence declarations or silently claim their notices were present.
Release packaging must collect the applicable notices before redistribution.

To propose a dependency change on the hosted runner, deliberately update the
manifests/lock, fetch locked archives, and run `python3 scripts/cargo_inventory.py
--candidate`. Review the emitted package, feature, checksum and notice differences
before replacing `docs/cargo-dependencies.json`. Ordinary CI never regenerates or
approves that snapshot: missing or differing entries fail.

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
