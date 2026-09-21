# Glaux Server

Glaux Server aims to be a **gold-standard, open-source Rust reference implementation of OGC API - Connected Systems** for the DGIWG Glaux ecosystem.

This repository is home to the server implementation. Research and planning documents live in the [parent Glaux repository](https://github.com/DGIWG-P507/glaux). The server is intended to provide the same standards-based APIs to Glaux applications and external clients; it does not include the Glaux web or mobile application.

## Current status

**Initial build foundation — 21 September 2026.**

- Initial design research, implementation planning and the pre-implementation review are complete.
- The approved technical follow-ups and subsequent Part 5 scope adjustment are documented. The Roadmap now defines **302 implementation tasks**: the original 286 plus 16 experimental Protobuf tasks, each linked to its published issue. These are planned tasks, not completed software.
- Apache-2.0 licensing and the contributor/review workflow are in place. Required automated-check enforcement is approved but still awaits implementation under [issue #6](https://github.com/DGIWG-P507/glaux-server/issues/6).
- The initial three-package Rust workspace and GitHub build/test workflow are present. They establish the build foundation, not a runnable CSAPI service: resource behavior, HTTP endpoints and database integration remain unimplemented. [PR #312](https://github.com/DGIWG-P507/glaux-server/pull/312) records actual runs, limitations and review.

Use the [build/test instructions](docs/setup.md#current-build-and-test-commands) and the [Build workflow](https://github.com/DGIWG-P507/glaux-server/actions/workflows/build.yml). Builds run on GitHub-hosted Linux; no Rust/database installation on the company laptop or permanent cloud service is required. #5 adds temporary PostgreSQL/PostGIS testing, and #6 completes the check suite and merge enforcement. The [follow-up action list](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/Review/action-list.md) records planning decisions; issues and PRs record execution.

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

Verification will combine standards-derived expectations, real database tests and independent-client exercises, including failure and access-control cases. The initial build pins Rust 1.98.1 and its workspace lockfile; it has no third-party Cargo dependencies yet. Axum/Tokio, SQLx and other libraries are added and verified by their owning tasks, not included merely to fill out the bootstrap. These packages currently define boundaries, not the planned resource model or server capabilities.

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

Assistant review is launched by the working session, not by an enabled GitHub AI-review service. The approved required-check policy still needs configuration and proof under issue #6; written instructions are not a claim that GitHub already enforces it. Detailed review, merge and pre-CI verification rules remain in CONTRIBUTING.

## License

Original Glaux Server code and accompanying documentation in this repository are licensed under the [Apache License, Version 2.0](LICENSE) (`Apache-2.0`), unless explicitly identified otherwise. The project lead approved this choice on 21 September 2026.

Third-party dependencies, standards/schema artifacts and other externally sourced material retain their own licences and required notices. This licence does not grant access to, or license, data held by a running server; it does not relicense the separate parent Glaux planning repository.
