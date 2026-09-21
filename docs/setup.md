# Development setup: initial prerequisite inspection

**Inspected:** 21 September 2026, approximately 22:13 UTC.<br>
**Task:** [1.1.1 / issue #3](https://github.com/DGIWG-P507/glaux-server/issues/3).<br>
**Result:** checkout/prerequisite inspection complete; the current session is not ready for the Rust build or database tasks.

This is an inventory, not installation instructions or a successful build report. It uses [Goal v1.10](https://github.com/DGIWG-P507/glaux/blob/1432876b10aa6eb19b832102cd28c0689f535669/Docs/Plans/glaux-server/glaux-server-goal-and-definition.md), [Guide v1.21 §§2.1, 2.4 and 4.12](https://github.com/DGIWG-P507/glaux/blob/1432876b10aa6eb19b832102cd28c0689f535669/Docs/Plans/glaux-server/glaux-server-implementation-guide.md), and [Roadmap v1.36 task 1.1.1](https://github.com/DGIWG-P507/glaux/blob/1432876b10aa6eb19b832102cd28c0689f535669/Docs/Plans/glaux-server/glaux-server-roadmap.md#phase-1-running-foundation-and-first-registration). The historical preparation pins in issue #3 remain intact; current CONTRIBUTING and these approved sources control execution.

## Checkout inspected

- Repository: `DGIWG-P507/glaux-server`, fetched `origin/main` at `0723ec2194e7d80057ce466e2d602f4bd8abfcfd`.
- Initial branch: `main`, equal to `origin/main`. Initial tracked changes and untracked files: none. No existing server work was overwritten.
- Tracked files at that baseline: `README.md`, `LICENSE`, `CONTRIBUTING.md`, `AGENTS.md`, and `.github/ISSUE_TEMPLATE/implementation_task.md`.
- No Cargo workspace, toolchain pin, lockfile, Rust sources, tests, migrations, database settings, Compose file, CI workflows or earlier setup notes exist at the inspected baseline. These are later deliverables, not failed existing components.
- This session used the existing server checkout under the Windows temporary directory, separate from the planning checkout. It is not an established durable development location. Confirm an appropriate persistent checkout in the selected approved environment before later build work; no checkout was moved by this inspection. User-specific paths are intentionally not published.
- Documentation work uses branch `task/1.1.1-prerequisites`. The task PR records the reviewed head, before/after status, actual checks and final merge. The separate planning checkout's unrelated PITON document is out of scope and remains untouched.

## Observed availability and approval

The host reports Windows x64 (`Microsoft Windows 10.0.26200`) and PowerShell `7.6.6`. These identify this inspection session, not a tested supported-build platform.

| Prerequisite or option | What was actually observed | Consequence / approval boundary |
|---|---|---|
| Git | `git --version` returned `2.55.0.windows.5`; fetch and repository inspection worked. | Sufficient for this authorised documentation workflow; not evidence of Rust or database readiness. |
| Rust compiler, Cargo and rustup | No `rustc`, `cargo` or `rustup` application on PATH. No corresponding executables in the configured/default Cargo bin location and no installed toolchains in the configured/default rustup toolchain directory. | No usable Rust toolchain discovered by these probes. An approved environment with Rust stable and Cargo is needed for #4; exact versions are pinned only after verification there. |
| Native build tools | No `cl`, `link`, `clang`, `clang-cl`, `gcc`, `cmake`, `ninja` or `make` on PATH. The standard Visual Studio Installer `vswhere.exe` location was absent; selected uninstall registrations showed Git and VS Code, not a Visual Studio C++ build installation. No Windows SDK root was returned from the inspected Windows Kits registration. | A working native toolchain/linker for the selected Rust target is not established. VS Code is an editor, not proof of a compiler. The alternatives listed are probes, not a requirement to install every tool; #4 establishes actual platform/dependency needs. |
| PostgreSQL tools/service | No `psql`, `pg_config`, `postgres` or `pg_isready` application on PATH; no matching PostgreSQL Windows service was returned. | No local database tool/service discovered through these probes. This does not establish that no remote or differently named service exists. |
| Approved PostgreSQL/PostGIS target | The checkout documents no approved test endpoint; the selected database environment-variable names below were absent. No approved alternative target was supplied for this inspection. No connection or SQL was attempted. | Availability, approval, PostgreSQL version and PostGIS installation remain unverified. #5 requires a designated isolated test service and real connectivity/PostGIS/lifecycle evidence, not a guessed endpoint. |
| Container option | No `docker` or `podman` application on PATH and no matching Windows service returned. Selected uninstall registrations did not identify either product. | No container runtime established here. A local container runtime is not required merely to inspect the project or use an otherwise approved PostgreSQL/PostGIS service. The eventual Compose example remains a planned deliverable. |
| WSL option | The Windows `wsl.exe` launcher exists, but the current user's Lxss registration root is absent and no registered distribution was found there. No distribution was started or inspected internally. | A launcher is not evidence of an approved Linux development environment, installed Rust or a running database. Other users, VMs and remote environments were not surveyed. |
| Organisational approval | This read-only inspection was authorised. No Rust/native-toolchain or database-service approval record/target was provided. | Approval is **unconfirmed**, not inferred from presence and not recorded as refusal. Obtain the appropriate organisational direction before installations or using an additional environment. |

These are bounded discovery results, not a machine-wide software audit. PATH, configured/default Rust directories, selected development-software registry entries, standard VS/SDK locations, matching services and current-user WSL registrations do not cover every custom installation or remote environment.

## Checks performed and limits

The expected inventory came from the Guide: native Rust development, the selected build dependencies and PostgreSQL/PostGIS, with an eventual Compose deployment example. Inspection compared that expectation with direct observations:

- `git status --short --branch`, `git rev-parse HEAD`, `git fetch origin main`, `git rev-parse origin/main`, `git ls-files`, `git diff --exit-code`, `git diff --cached --exit-code` and `git ls-files --others --exclude-standard` established baseline identity and the clean server checkout.
- `Get-Command -CommandType Application -All` checked the named executables. `git --version` supplied the Git version; absent Rust/database/container commands were not invoked.
- `Test-Path` and directory listings checked only the configured/default Cargo binaries and rustup toolchains, the standard `vswhere.exe` location, and the named WSL registration. Selected registry reads checked VS/SDK, Rust, Git, PostgreSQL/PostGIS and container registrations; matching-service queries did not start or stop services.
- Only **presence**, never values, was checked for `DATABASE_URL`, `TEST_DATABASE_URL`, `PGHOST`, `PGPORT`, `PGDATABASE`, `PGUSER`, `PGPASSWORD`, `PGSERVICE` and `PGSERVICEFILE`. All were absent. No credential store, connection file or password value was opened; no network endpoint or local port was probed for database access.
- After the inspection and before authoring these notes, tracked and untracked server status remained clean. The intended delivery changes are this file and the README link/status; the PR verifies that boundary and link/whitespace consistency.

No package installation, toolchain activation/download, dependency resolution, workspace creation, build, container startup, database connection/reset/migration, service change or runtime test was performed. Behavioural red–green, mutation and fuzz checks are inapplicable to this inspection-only task. No available executable, empty test suite or missing CI result is represented as a passing build, database test or conformance result.

## What is needed next

1. **Before #4's build:** identify an approved environment accessible to the implementer, with Rust stable/Cargo and the native build tools appropriate to its selected target. Confirm whether that is this machine after the organisation's normal installation process, or an already approved development environment elsewhere. Also establish a durable checkout location. Do not choose or install versions merely from these notes; #4 must build/test and record the actual pins.
2. **Before #5's database work:** designate an approved isolated PostgreSQL/PostGIS test service and its permitted lifecycle. Supply access through the approved secret mechanism, not issue comments, public documents or chat. #5 will verify versions, PostGIS, migrations, isolation and safe setup/cleanup; inspection establishes none of those results.
3. **Keep #6's required-check work separate:** the approved CI/enforcement decision is not a configured or passing check. #6 follows its build/database prerequisites and must prove real checks and effective enforcement.

Issue #3 can close when these inspection notes are reviewed and merged, even though prerequisites are missing. [#4](https://github.com/DGIWG-P507/glaux-server/issues/4) is next in the task graph, but is **not execution-ready in the inspected session**. A future `proceed` does not authorise installation or waive the missing environment; identify the approved path first. [#5](https://github.com/DGIWG-P507/glaux-server/issues/5) remains dependent on #4 and its own approved database.
