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
| Structural validation | `jsonschema =0.56.0`, defaults disabled, `arbitrary-precision`; `serde_json =1.0.151`, `arbitrary_precision` and `raw_value`. Exact transitive graph, all-target features, archive checksums and notice hashes: [reviewed Cargo snapshot](cargo-dependencies.json). | Package terms and actual packaged notices are recorded per dependency; no HTTP/filesystem retrieval feature or HTTP client is reachable from the standards/domain packages. See [selection and limits](structural-validation.md). |
| Typed identities | `uuid =1.26.1`, `getrandom =0.4.3`, `fluent-uri =0.4.1`, each with defaults disabled. [Selection, contracts and limits](resource-identities.md). | Actual packaged licences/notices and unified features are recorded in the reviewed Cargo snapshot; existing transitive `getrandom` remains separately accounted. |
| Exact numbers | `num-bigint =0.4.8`, `num-rational =0.4.2`, `num-traits =0.2.19`, defaults disabled; rational requests `num-bigint`. [Contracts and limits](exact-numbers.md). | Existing locked packages reused as direct domain dependencies; actual unified features and packaged MIT/Apache notices remain in the reviewed snapshot. |
| UCUM declarations | UCUM 2.1, official `ucum-org/ucum` commit `910f502003269a492e32dd5cb96e5503b2351ac9`; complete original essence and specification, derived lookup for 24 prefixes and 310 atoms. [Source digests, scope and checks](ucum.md). | The source's contemporaneous UCUM terms apply separately: [full licence](../crates/glaux-standards/src/units/license.txt) and [required short notice](../crates/glaux-standards/src/units/UCUM_short_license.txt), verified against the unchanged original. Definition values and units remain with the incorporated codes. No Cargo or pip dependency is added. |
| Initial PostgreSQL repository/runtime | `sqlx =0.9.0` (defaults disabled: `postgres`, `runtime-tokio`, `migrate`, `tls-rustls-ring-webpki`); `tokio =1.53.1` (defaults disabled: `rt`, `time`, `net`, `sync`, `signal`, `macros`). Server package only. [Storage contract](system-storage.md). | Actual SQLx MIT/Apache and Tokio MIT packaged notices, plus the full transitive feature/archive/notice inventory, are in the reviewed snapshot. TLS incorporates separately recorded ring Apache/ISC, webpki ISC and WebPKI-root CDLA-Permissive terms; these are not all relabelled MIT/Apache. |
| Health-only HTTP and typed config | `axum =0.8.8`, defaults disabled, `http1`/`tokio`; `serde =1.0.229`, defaults disabled, `derive`/`std`; existing `serde_json =1.0.151` reused. [Runtime contract](runtime-configuration.md). | Axum/Tower/Hyper MIT; serde MIT OR Apache-2.0. The 19 archives added by #18 carry packaged notices. `matchit` is MIT AND BSD-3-Clause and `sync_wrapper` is Apache-2.0; exact compound terms and notice hashes remain in the snapshot. #21 additionally enables Hyper client features for the issuer-key transport below. |
| Rust, Cargo, rustfmt and Clippy | Rust `1.98.1`, minimal toolchain plus the two explicit components; actual component versions appear in each run. | Rust's [Apache-2.0 OR MIT terms and third-party notice instructions](https://github.com/rust-lang/rust/blob/1.98.1/COPYRIGHT); bundled components retain their own notices. |
| JWT verification | `jsonwebtoken =11.1.0`, defaults disabled, `aws_lc_rs` only; `base64 =0.22.1`, defaults disabled, `std`. Static RS256 public-key verification; no PEM or HTTP discovery. [Contract](authentication.md). | JWT MIT; AWS-LC Rust ISC AND (Apache-2.0 OR ISC); native AWS-LC has additional MIT/BSD/ISC/Apache terms preserved exactly in the snapshot, not relabelled as the wrapper's licence. No FIPS feature/certification is claimed. |
| Independent JWT fixture signer | Hosted runner's existing OpenSSL CLI, version recorded in proof logs and inventory. Ephemeral synthetic RSA keys only; not a server runtime dependency. | Runner OpenSSL's own packaged terms/notices apply; [OpenSSL licence information](https://www.openssl.org/source/license.html). No package is installed by the fixture wrapper. |
| Bounded issuer-key HTTPS | `reqwest =0.13.5`, defaults disabled, `rustls` only. Fixed configured endpoint, verified TLS, disabled redirects/retries/proxy and streaming bounds. [Contract](authentication.md#bounded-issuer-key-refresh). | Reqwest MIT OR Apache-2.0; Hyper-Rustls Apache-2.0 OR ISC OR MIT; platform-verifier MIT OR Apache-2.0. Exact graph and platform-specific terms/notices remain in the snapshot. No HTTP/2/3, compression, cookie-store or system-proxy feature is selected. |
| Checkout action | `actions/checkout` v7.0.1, `3d3c42e5aac5ba805825da76410c181273ba90b1`. | [MIT project licence](https://github.com/actions/checkout/blob/3d3c42e5aac5ba805825da76410c181273ba90b1/LICENSE). |
| Inventory upload action | `actions/upload-artifact` v7.0.1, `043fb46d1a93c77aae656e7c1c64a875d1fc6a0a`. | [MIT project licence](https://github.com/actions/upload-artifact/blob/043fb46d1a93c77aae656e7c1c64a875d1fc6a0a/LICENSE). Bundled action dependencies are not relicensed by that heading. |
| Python test/check support | Runner Python 3.11+ and its standard library only; no pip dependencies. Actual version is recorded, not silently treated as fixed by `ubuntu-24.04`. | [Python's licence and incorporated-software notices](https://docs.python.org/3/license.html). |
| Disposable database image | Linux/amd64 `postgis/postgis@sha256:7e00e8c3539fdd43f513b98806c8204714dcd09dea683c259e333d7690317119`; PostgreSQL 18.6 / PostGIS 3.6.4. | [Image packaging: MIT](https://github.com/postgis/docker-postgis/blob/2bcd236e3af9ec6e668db51eb37162a79f0eaeaa/LICENSE); [PostgreSQL License](https://www.postgresql.org/about/licence/); [PostGIS: GPL-2.0-or-later](https://github.com/postgis/postgis/blob/3.6.4/LICENSE.TXT); separately licensed image packages. |
| Hosted Linux build environment | `ubuntu-24.04`, with Docker and native compiler supplied by the runner. The run records its actual image, Docker and compiler versions. | [GitHub runner-image inventory](https://github.com/actions/runner-images/tree/main/images/ubuntu); underlying packages retain their own licences. This rolling runner label is not an immutable OS image pin. |
| Local documentation renderer | Swagger UI `5.33.0`, commit `cfd4a6c3cbaeeb7c13a8bada7c754de42d78cd5b`; exact prebuilt bundle/CSS and three notice files in the [asset manifest](../crates/glaux-server/assets/swagger-ui/manifest.json). No npm install, CDN or remote validator. | Retained upstream Apache-2.0 LICENSE/NOTICE plus emitted bundle notices, including MIT/BSD components and DOMPurify's Apache-2.0 OR MPL-2.0 notice; not all bundled code is relabelled Apache-2.0. All three notice files are served alongside documentation. |
| Browser rendering fixture | Hosted runner's already-provisioned Chrome, actual version and runner image recorded by each proof. No browser installation by this task. | Chrome and its incorporated components retain their own terms; this executable is a hosted test tool, not a redistributed Glaux component. |

The image's registry package versions are PostgreSQL `18.6-1.pgdg13+2` and PostGIS
`3.6.4+dfsg-2.pgdg13+1`. The platform manifest above is the execution pin; the
floating `18-3.6` tag is provenance only. [Database-test documentation](database-tests.md#pin-provenance-and-limits)
records the registry/Dockerfile sources and actual SQL version assertions.
Neither the database image as a whole nor the distributed Rust toolchain is
covered merely by Glaux's Apache-2.0 licence.

Task #28 enables `jsonschema`'s `arbitrary-precision` feature for Count and
Quantity schema checks. This adds dependency edges from `jsonschema` and
`jsonschema-value` to the already locked `num-bigint`; it does not change package
versions or introduce a new archive. The corresponding features and graph are
recorded in the reviewed Cargo snapshot. Exact component comparisons still use
the domain numeric primitives; the schema feature does not implement conversion
or replace the component's bounded semantic checks.

## Per-run evidence

The [initial standards corpus](standards-corpus.md) separately retains 129 schemas,
four publication source headers and five licence/notice files. Its manifest records
source revisions or digest-pinned snapshots, retrieval times and exact URI mappings.
OGC, GeoJSON MIT and JSON Schema BSD/AFL notices remain with the originals; Glaux's
Apache-2.0 licence does not relicense them. Corpus packaging itself adds no Cargo or pip dependency.
The corpus check and its log complement, rather than modify, the runtime dependency
inventory below. The optional PowerShell acquisition script is not a CI dependency.

The separately licensed [UCUM source package](ucum.md) lives beside its validator,
without replacing or extending that original schema corpus. The static lane runs
`python3 crates/glaux-standards/src/units/verify.py --self-test` to check original
file digests, reproduce every lookup field and both notices, and prove that
changed bytes in each checked asset are rejected. It needs no network or package
installation. UCUM notices must accompany distributions containing that data;
the repository's Apache-2.0 licence does not replace those terms.

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
- The five renderer files' exact sizes and SHA-256 values against separately
  recorded upstream-byte expectations; changed bytes, file sets or identity fail.
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

The #23 renderer selection checked the maintainer's published
[URL-parameter advisory](https://github.com/swagger-api/swagger-ui/security/advisories/GHSA-qrmm-w75w-3wpx)
on 27 September 2026. Its affected range ends at 4.1.2; the selected 5.33.0 is
outside it. Glaux still explicitly disables query configuration, remote
validation and Try-it-out, and tests malicious document/configuration URL
parameters in the actual browser. This is targeted source diligence, not a
comprehensive scan of the bundled JavaScript graph or proof of no unknown defect.

Task #21's reviewed lock/snapshot contains 279 registry packages (40 more than
task #20's 239), including target-specific and optional-driver metadata not
necessarily compiled into the Linux server. Previously locked package versions
are retained. SQLx enables only the selected PostgreSQL driver, but Cargo's
all-target metadata also accounts its optional driver/macro packages; presence
in that inventory is not a claim that MySQL/SQLite or query macros are enabled.
AWS-LC uses the hosted native compiler/CMake support. The snapshot's unified
target-specific Wasm features are metadata, not enabled JavaScript execution in
the Linux server. Reqwest adds platform certificate handling and all-target
optional metadata, including Quinn; their presence does not select HTTP/3 for
this client. Hyper client features now belong to the server only. Schema
HTTP/filesystem retrieval remains disabled, and dependency traversal rejects a
network client reachable from the standards or domain packages. All declared expressions and
compound Unicode/TLS/data terms remain recorded rather than reduced
to a crate's MIT/Apache heading. This is dependency-selection accounting, not
a legal opinion or a completed release-redistribution check.

Ten archives contain no separately named licence/notice file. Six predate #21: `jsonschema-regex`
and `jsonschema-value` 0.56.0 (MIT, [upstream workspace](https://github.com/Stranger6667/jsonschema/tree/rust-v0.56.0)),
`uuid-simd` and `vsimd` 0.8.0 (MIT, [upstream](https://github.com/Nugine/simd)), and
`r-efi` 5.3.0 and 6.0.0 (MIT OR Apache-2.0 OR LGPL-2.1-or-later,
[upstream](https://github.com/r-efi/r-efi)). Their package metadata is present;
the snapshot explicitly lists the absent packaged files. Do not describe them as
missing licence declarations or silently claim their notices were present.
The four added by #21 are `jni` and `jni-macros` 0.22.4, `jni-sys-macros` 0.4.1
([JNI upstream](https://github.com/jni-rs/jni-rs)) and
`rustls-platform-verifier-android` 0.2.0
([platform-verifier upstream](https://github.com/rustls/rustls-platform-verifier)).
All four declare MIT OR Apache-2.0; missing packaged notices are recorded, not
silently inferred from that declaration. Release packaging must collect the
applicable notices before redistribution. No release is performed here.

The #21 selection checked the published GitHub advisory query for
`reqwest@0.13.5` on 26 September 2026; it returned no matching published entry.
That narrow query is not a comprehensive transitive vulnerability scan or proof
of no undisclosed defect. Client configuration and real TLS/failure fixtures
remain necessary independently of the version label.
The selected Rustls 0.23.45 also contains the maintainer's fix for
[GHSA-2mjx-qc3c-rqvc](https://github.com/rustls/rustls/security/advisories/GHSA-2mjx-qc3c-rqvc)
(affected through 0.23.44). The two TLS callers select providers explicitly:
[SQLx 0.9.0](https://github.com/launchbadge/sqlx/blob/v0.9.0/sqlx-core/src/net/tls/tls_rustls.rs)
uses Ring for its selected feature, while
[Reqwest 0.13.5](https://github.com/seanmonstar/reqwest/blob/v0.13.5/src/async_impl/client.rs)
uses an installed provider or its AWS-LC fallback. Both features in the resolved
graph therefore do not require guessing a global default provider.

To propose a dependency change on the hosted runner, deliberately update the
manifests/lock, fetch locked archives, and run `python3 scripts/cargo_inventory.py
--candidate`. Review the emitted package, feature, checksum and notice differences
before replacing `docs/cargo-dependencies.json`. Ordinary CI never regenerates or
approves that snapshot: missing or differing entries fail.

The #20 selection also checked the maintainers' published advisories on
25 September 2026. [JWT claim-type advisory GHSA-h395-gr6q-cpjc](https://github.com/Keats/jsonwebtoken/security/advisories/GHSA-h395-gr6q-cpjc)
affects versions before 10.3.0; the 11.1.0 pin is outside that range.
The five advisories listed by [AWS-LC-rs](https://github.com/aws/aws-lc-rs/security/advisories)
covered PKCS7, AES-CCM, X.509 and CRL paths and were fixed by `aws-lc-sys`
0.38.0 or 0.39.0; the selected 0.45.0 is outside those listed ranges.
This targeted maintainer/source check is not a comprehensive vulnerability scan,
promise of no undisclosed issue or security certification. The regression fixture
also checks a signed but string-valued not-before claim rather than relying only
on the patched-version label.

Debian packaging copyright text is evidence, not a reliably machine-inferred
SPDX expression for every file. The JSON therefore leaves those expressions
unset. Missing copyright files are visible limitations, not guessed licences or
a clean legal result. Installed package accounting does not cover files outside
the package manager, bundled npm components within GitHub actions, every Rust
toolchain component's transitive notices, or every package on the hosted runner.
It is not a complete release SBOM, CVE scan, legal approval, security certification
or redistribution analysis. No server release is performed here. Task #23 does
redistribute the selected renderer subset in the repository and embedded binary,
with its upstream notices retained and downloadable; the original dependency
inventory task did not redistribute it. Any later third-party code, schema or fixture reuse needs
its actual terms and notices considered under [CONTRIBUTING](../CONTRIBUTING.md#license-and-contributions).
