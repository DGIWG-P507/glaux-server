# Development setup: GitHub-hosted builds and tests

## Current build and test commands

The initial workspace belongs to [issue #4 / task 1.1.2](https://github.com/DGIWG-P507/glaux-server/issues/4); #5 adds the disposable database harness. [Issue #6 / PR #314](https://github.com/DGIWG-P507/glaux-server/pull/314) extends these into the [complete initial CI and enforcement path](ci.md). This is a build/test foundation, **not a working CSAPI service**.

Run from the workspace root in GitHub-hosted Linux or an already approved Rust environment:

```text
cargo build --workspace --locked --offline
python3 scripts/check-bootstrap.py
python3 -u scripts/check-execution.py rust
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked --offline -- -D warnings
```

The Python helpers need Python 3.11+ and only its standard library. They check this initial package graph/licence metadata, required discovery and actual execution; they are not needed just to compile the Rust packages. Cargo's normal workspace test command includes applicable doctests. There are no executable documentation examples or domain/codec behaviors yet, so those targets currently report zero tests; that is explicitly not domain/standards verification. The executable has one initial integration test. These workspace commands alone are not the full CI suite: follow [clean reproduction](ci.md#reproduce-the-checks) for lockfile checking, Python syntax, real database tests, failure controls and inventory.

The `glaux-server` binary deliberately writes `glaux-server: bootstrap only; no server commands are implemented yet.` to standard error and exits with code 2. It does not bind a listener, accept commands or access storage. A successful build, or the names of the packages, is not proof of any planned server capability. Do not treat `cargo run -p glaux-server` as a successful server startup at this stage.

## Packages and dependency boundary

| Package | Current local dependencies | Current implementation |
|---|---|---|
| `glaux-domain` | None | Library boundary only; resource rules belong to later issues. |
| `glaux-standards` | `glaux-domain` | Library boundary only; no schemas, validators or codecs yet. |
| `glaux-server` | Both libraries | Explicit unfinished-startup diagnostic and its process-level test. |

The three packages are the exact initial production set in [Guide v1.21 §2.2](https://github.com/DGIWG-P507/glaux/blob/f2d9f912b1a75c14315b4555b21ae545fd6caaee/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#22-component-boundaries). No extra empty publication, tasking, policy or other subsystem packages are created. The libraries are not padded with invented domain behavior just to increase test counts.

`scripts/check-bootstrap.py` compares Cargo's resolved graph with independently specified Guide-derived edges, checks original-package licence/edition/toolchain metadata and requires the named executable regression to be discovered exactly once. This is the initial graph inventory, not a prohibition on all future third-party dependencies. Later owning issues must update it deliberately against their approved changes.

## Pins and hosted execution

- Rust **1.98.1**, edition 2024, resolver 3, pinned by [rust-toolchain.toml](../rust-toolchain.toml). Package `rust-version` matches the actually selected toolchain; no compatibility with older compilers is claimed. [Official release](https://blog.rust-lang.org/2026/09/03/Rust-1.98.1/).
- [Cargo.lock](../Cargo.lock) contains only the three original Apache-2.0 packages at `0.1.0`. There are no third-party Cargo dependencies/features yet. CI regenerates this initial lockfile offline and checks it is unchanged, then builds/tests with `--locked --offline`. Offline describes Cargo operations after toolchain provisioning, not the entire GitHub job.
- Checkout and evidence-upload actions use exact reviewed commit pins. Their project licences are MIT; Rust, database-image and runner components retain their own upstream terms. See the [dependency/licence inventory](dependencies.md) for identities, evidence and limits.
- The [Build workflow](../.github/workflows/build.yml) uses standard `ubuntu-24.04` hosted runners, a 10-minute job limit, `contents: read`, no persisted checkout credentials and no production secrets. It runs on pull requests and main pushes, not `pull_request_target`. No paid runner upgrade or self-hosted machine is introduced.
- On a PR, checkout selects its exact head SHA, verifies/logs that identity and tests it rather than the synthetic merge commit. Reconcile any base movement and re-test before merging. Main pushes test the merged commit. The active required-check rule and procedural separate assistant review are distinct from these runs.
- The job explicitly installs the repository's toolchain pin using rustup **inside the disposable runner**, then logs compiler/Cargo/rustfmt/native-compiler/Python versions and the actual image identity. The runner label is rolling, not an immutable VM pin. Initial observed image: `ubuntu24 20260907.300.1`, Rust/Cargo `1.98.1`, rustfmt `1.9.0-stable`, GCC `13.3.0`, Python `3.12.3`; use the relevant run's log as evidence.
- No tool was installed on the company laptop. GitHub is the durable source and hosted jobs use fresh checkouts. The temporary local checkout is an editing convenience, not a prerequisite for a permanent development machine.

## What the initial checks establish

The workflow checks formatting, Clippy, Python syntax and lockfile reproduction, compiles the three packages, checks their actual resolved edges and required test discovery, then executes workspace/database tests, failure controls and inventory. Bash pipefail and the execution guard preserve failure through the diagnostic tee and require the named regression to actually pass. Suite/control logs and inventory are uploaded for 14 days. No step swallows failures or uses retry-until-green. Setup/compiler errors are failures, not behavioral-red evidence. [CI documentation](ci.md) states the exact scope and limitations.

The startup expectation is independently authored in `tests/bootstrap.rs`: exit 2, no standard output and the exact limitation on standard error. In [run 35664583178](https://github.com/DGIWG-P507/glaux-server/actions/runs/35664583178), a silent-success placeholder compiled, the required test was discovered and ran, and its exit-code assertion failed (`Some(0)` versus `Some(2)`); Cargo exited 101 and the job failed. The implementation was then changed to meet the unchanged assertion. This is a narrow bootstrap regression/failure-propagation proof, not a conformance or full CI-quality claim.

The preceding [run 35664492990](https://github.com/DGIWG-P507/glaux-server/actions/runs/35664492990) stopped at formatting, so its build/tests were skipped. It is retained as a real initial failure, not counted as the intended red test. The log also prompted replacing deprecated implicit rustup installation with an explicit hosted install. Final passing/review/merge evidence belongs to PR #312, including any later corrections; a stale green run cannot cover a changed head.

HTTP, resource schemas/behavior, codecs, brokers and conformance checks remain unimplemented. #5 adds the [real PostgreSQL/PostGIS lifecycle harness](database-tests.md), using the image's own client and a minimal explicit extension migration. It does not prove the future Rust SQLx adapter, resource queries, full restore or application storage; those owners retain their checks.

## Delivery sequence and boundaries

[#5](https://github.com/DGIWG-P507/glaux-server/issues/5) adds the pinned disposable PostgreSQL/PostGIS harness and real isolation/lifecycle/failure checks to the existing workflow. See [its command, safety boundary, exact pin and licence notes](database-tests.md). Each test target is created and removed within the hosted job, with no exposed database port or user-data mount. The issue/PR records actual run evidence and any initial failures; code presence alone is not acceptance.

[#6](https://github.com/DGIWG-P507/glaux-server/issues/6) owns the initial formatting/lint/build/unit/database suite, dependency/licence inventory, clean reproduction and false-green checks. [PR #314](https://github.com/DGIWG-P507/glaux-server/pull/314) records the actual main-rule application and failed/missing-check blocking probes, final execution and review. Automated checks do not mechanically enforce separate review. The eventual native/Compose reference instructions remain deliverables.

Issues #4 and #5 are complete. Once #6 is merged and closed with its evidence, the next candidate is [#7 / task 1.2.1](https://github.com/DGIWG-P507/glaux-server/issues/7): package the pinned standards/schema corpus and initial independent fixtures. It requires its own `proceed`; this CI change does not implement it. No local installation, persistent cloud provisioning or production access is implicit.

## Initial inspection record

The [completed #3 setup snapshot](https://github.com/DGIWG-P507/glaux-server/blob/150bb7cd438009441b416c7157a46fb8bccd82ae/docs/setup.md) retains the original clean checkout, local Git/tool inventory and authenticated read-only GitHub settings checks. It records the corrected decision: missing laptop build tools do not block hosted development. Actions was enabled but had zero listed workflows/runs at that earlier inspection; that historical zero is superseded by the actual #4 runs above, not erased.

Planning remains [Goal v1.10, Guide v1.21 and Roadmap v1.37](https://github.com/DGIWG-P507/glaux/tree/f2d9f912b1a75c14315b4555b21ae545fd6caaee/Docs/Plans/glaux-server), with the dated issue amendments controlling the hosted sequencing. Research/review archives and the unrelated planning PITON file are unchanged.
