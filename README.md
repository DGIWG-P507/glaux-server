# Glaux Server

Glaux Server aims to be a **gold-standard, open-source Rust reference implementation of OGC API - Connected Systems** for the DGIWG Glaux ecosystem.

This repository is home to the server implementation. Research and planning documents live in the [parent Glaux repository](https://github.com/DGIWG-P507/glaux). The server is intended to provide the same standards-based APIs to Glaux applications and external clients; it does not include the Glaux web or mobile application.

## Current status

**Initial build, enforced CI, offline schema validation, typed identities, exact numbers and time — 22 September 2026 UTC.**

- Initial design research, implementation planning and the pre-implementation review are complete.
- The approved technical follow-ups and subsequent Part 5 scope adjustment are documented. The Roadmap now defines **302 implementation tasks**: the original 286 plus 16 experimental Protobuf tasks, each linked to its published issue. These are planned tasks, not completed software.
- Apache-2.0 licensing and the contributor/review workflow are in place. The [active main-branch rule](https://github.com/DGIWG-P507/glaux-server/rules/23796335) requires a pull request and passing, up-to-date CI, with no bypass or mandatory human approval. [Issue #6](https://github.com/DGIWG-P507/glaux-server/issues/6) and [PR #314](https://github.com/DGIWG-P507/glaux-server/pull/314) record settings, failed/missing-check blocking proofs, actual runs and review.
- The initial three-package Rust workspace and [CI suite](docs/ci.md) cover formatting, Clippy, build, the initial executable regression, seven real database lifecycle tests and nine controls against false-green results. The [dependency/licence inventory](docs/dependencies.md) records what those runs use. This is not a runnable CSAPI service: resource behavior, HTTP endpoints and Rust application storage remain unimplemented.

Use the [clean check instructions](docs/ci.md#reproduce-the-checks) and the [Build workflow](https://github.com/DGIWG-P507/glaux-server/actions/workflows/build.yml). Builds and disposable database tests run on GitHub-hosted Linux; no Rust/database installation on the company laptop or permanent cloud service is required. The [follow-up action list](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/Review/action-list.md) records planning decisions; issues and PRs record execution.

The [initial schema corpus](docs/standards-corpus.md) now preserves 138 original
source/notice files with exact-byte provenance, a complete local schema-reference
graph and 23 independently authored recursive/source-conflict expectations.
CI checks packaging and deliberate corruption/missing-target failures offline.
The [structural validator](docs/structural-validation.md) executes those cases
against pinned, embedded sources, with fixed entry points and bounded input.
It rejects external schema retrieval and non-progressing reference cycles.
Structural success is not full SWE semantics, codec support or conformance.

[Typed resource identities](docs/resource-identities.md) now separate UUIDv7 local
locators, URI-form published UIDs and authority-qualified source identifiers.
Strict parsing, fallible generation and type-boundary checks prevent accidental
mixing; identifiers do not grant permission or establish observation time.
Database uniqueness, resource-family models and HTTP behavior remain later work.

[Exact numeric primitives](docs/exact-numbers.md) now preserve large Counts and
decimal measured values without silent rounding, compare values exactly and keep
non-finite states explicit. Original decimal spelling or finite binary64 bits
remain available. Complete SWE codecs, unit conversion and numeric storage are
still later work.

[Exact time primitives](docs/exact-time.md) preserve timestamp fractions and
source context, normalize offsets and keep known leap seconds distinct. Their
real-database proof stores and compares exact values beyond timestamp resolution.
Observation endpoints, interval filters and the application database adapter
remain later work.

## Planned capabilities

- **Describe and discover systems:** register systems, procedures, deployments, sampling features and properties, and follow their relationships.
- **Work with observations:** submit, retrieve and filter observations and measurements, including location-based searches through related sampling geometry, while preserving timestamps, units and meaning.
- **Follow changes and status:** receive supported live updates and retrieve reported system status and events, distinguishing current evidence from stale or last-known information.
- **Task connected systems:** assess feasibility, submit authorized commands and follow their status and results, without confusing an accepted request with a confirmed physical outcome.
- **Control access and record accountability:** enforce configured read, write and command permissions and retain the required audit evidence.
- **Preserve data context:** retain supplied provenance—where a result came from and how it was produced—and quality information without inventing missing history.
- **Recover and exchange data:** support backup/restore and controlled synchronization across interruptions, with explicit handling of retries, conflicts and incomplete history.

These describe the intended completed server. For the capability-by-capability **“to do this, the implementation uses…”** explanation, start with [Implementation Guide §1.1](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#11-how-the-implementation-fulfills-the-goal).

## Standards scope

- **Core target:** OGC API - Connected Systems Parts 1 and 2, version 1.0, with applicable SensorML 3.0 and SWE Common 3.0 requirements. The completion target is all 25 direct CSAPI conformance classes and their applicable prerequisites.
- **Explicit experiments:** draft CSAPI Part 3 publish/subscribe and a bounded Part 4 subset—static sampling points, curves and surfaces. These use pinned draft revisions and are not claims of approved-standard conformance.
- **Additional filtering:** OGC API - Features Part 3 and CQL2 for direct sampling geometry applicable at observation time and typed scalar results within a datastream—not arbitrary joins or a general analytics service.
- **Experimental Protobuf:** a [bounded OSH-targeted Part 5 subset](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#431-experimental-part-5-protobuf-subset) for flat scalar records, selected observation and command operations, and outbound native observation MQTT. It targets pinned peer revisions, is disabled by default, and requires interoperability proof. It is not full OSH compatibility or approved Part 5 conformance; required SWE Binary remains in scope.

Exact dependencies, interpretations and experimental boundaries are defined in the [Goal](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-goal-and-definition.md#4-standardization-basis) and [Implementation Guide](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md#1-purpose-and-scope-baseline). **Conformance is an implementation and verification target, not a current claim.**

## Planned implementation

One deployable **Rust service** will use Axum/Tokio for HTTP and asynchronous work, SQLx with PostgreSQL for transactional storage, and PostGIS for spatial queries. Typed resource models and standards-specific validation/encoding will preserve the meaning of descriptions and data across supported representations.

Live publication will use an optional Server-Sent Events (SSE) interface as a Glaux extension and outbound MQTT 5 for the selected experimental Part 3 binding. The basic reference server is designed to be exercised without first completing other Glaux applications or installing enterprise identity infrastructure; broker-backed features have their own dependencies.

Verification will combine standards-derived expectations, real database tests and independent-client exercises, including failure and access-control cases. The build pins Rust 1.98.1, its workspace lockfile, and the initial `jsonschema`/`serde_json` dependency graph. Schema tests include independently expected outcomes, bounds, retrieval denial, parser regressions, a bounded mutation campaign and deliberate validation faults. Axum/Tokio, SQLx and other libraries are added and verified by their owning tasks; the planned resource model and service capabilities remain unimplemented.

## Project documents

| Document | What it explains |
|---|---|
| [Goal and Definition](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-goal-and-definition.md) | What the server must accomplish and what is outside its scope. |
| [Implementation Guide](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-implementation-guide.md) | How the capabilities will work, including design, standards interpretations and testing. |
| [Roadmap](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-roadmap.md) | Delivery order, prerequisites and links to the implementation issues. |
| [Review follow-up actions](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/Review/action-list.md) | Decisions and changes made after review, what is delivered and what remains. |

The [research synthesis and supplements](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Research/Initial%20Designs/IDR/glaux-server/IDR%20Reports/final-idr-research-report.md) and [completed review assessment, including its erratum](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/Review/evidence/51-pass-3c-45-final-consolidated-assessment.md#erratum-a---two-statements-corrected) preserve the supporting evidence. Use the follow-up action list for subsequent decisions rather than treating the historical assessment as the current work queue.

## Contributing and following progress

Start with [CONTRIBUTING.md](CONTRIBUTING.md), the [published implementation issues](https://github.com/DGIWG-P507/glaux-server/issues) and, for coding assistants, [AGENTS.md](AGENTS.md). The assisted workflow advances one authorized issue at a time through a task branch, pull request, applicable checks and a separate assistant review of the actual change and evidence. Only completed, reviewed work is merged; an incomplete or blocking review leaves the PR open.

Assistant review is launched by the working session, not by an enabled GitHub AI-review service. GitHub enforces the required CI result; it does not mechanically enforce completion of that separate review. Detailed review and merge rules remain in CONTRIBUTING.

## License

Original Glaux Server code and accompanying documentation in this repository are licensed under the [Apache License, Version 2.0](LICENSE) (`Apache-2.0`), unless explicitly identified otherwise. The project lead approved this choice on 21 September 2026.

Third-party dependencies, standards/schema artifacts and other externally sourced material retain their own licences and required notices. This licence does not grant access to, or license, data held by a running server; it does not relicense the separate parent Glaux planning repository.
