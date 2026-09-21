# Development setup: GitHub-hosted builds and tests

**Inspection:** 21 September 2026; local probes approximately 22:13 UTC, GitHub checks approximately 22:26–22:28 UTC.<br>
**Task:** [1.1.1 / issue #3](https://github.com/DGIWG-P507/glaux-server/issues/3).<br>
**Result:** inspection complete. GitHub-hosted Linux is the lead-selected build/test path and Actions is enabled. No Rust build, workflow or database test has run.

The earlier pending notes treated missing laptop tools as a project blocker. That inference is withdrawn. The company laptop need not run Rust, Docker or PostgreSQL for this path, and the lead need not supply a permanent database or another cloud machine before #4.

Use current [CONTRIBUTING](../CONTRIBUTING.md) and [Roadmap v1.37](https://github.com/DGIWG-P507/glaux/blob/main/Docs/Plans/glaux-server/glaux-server-roadmap.md#phase-1-running-foundation-and-first-registration) for the approved sequencing. The inspection's unchanged design sources are [Goal v1.10 and Guide v1.21](https://github.com/DGIWG-P507/glaux/tree/1432876b10aa6eb19b832102cd28c0689f535669/Docs/Plans/glaux-server), particularly Guide §§2.1, 2.4 and 4.12. Original issue preparation pins remain historical sources; the dated hosted-development amendments control the changed task allocation.

## What runs where

- **GitHub repository:** durable code, issues, pull requests and workflow definitions.
- **GitHub-hosted Linux job:** a fresh checkout, pinned Rust/native prerequisites and actual build/tests. #5 adds a temporary, isolated PostgreSQL/PostGIS service for synthetic test data.
- **Company laptop:** editing/Git access only for this approach. No compiler, local database, container runtime or WSL installation is required.
- **Persistent deployment:** a separate later decision. No Oracle Cloud, Fly.io, paid runner upgrade or self-hosted runner is required or provisioned here.

GitHub documents [hosted runners](https://docs.github.com/en/actions/concepts/runners/github-hosted-runners), [Rust build/test workflows](https://docs.github.com/en/actions/tutorials/build-and-test-code/rust), and [PostgreSQL service containers on Ubuntu](https://docs.github.com/en/actions/tutorials/use-containerized-services/create-postgresql-service-containers). The [PostGIS container project](https://github.com/postgis/docker-postgis) supplies a PostGIS-enabled option to pin and test in #5. These establish a supported approach, not successful Glaux execution.

## GitHub availability inspected

Repository: `DGIWG-P507/glaux-server`, main at `0723ec2194e7d80057ce466e2d602f4bd8abfcfd`.

| Read-only check | Observed result | Meaning |
|---|---|---|
| `GET /repos/DGIWG-P507/glaux-server/actions/permissions` | HTTP 200; `enabled: true`, `allowed_actions: all`, `sha_pinning_required: false` | Repository policy permits Actions. The last value does not waive project action/dependency pinning. |
| `GET .../actions/permissions/workflow` | HTTP 200; default workflow permissions `read`; workflow PR-review approval `false` | Preserve least privilege. This is about the workflow token, not the separate assistant-review procedure. |
| `GET .../actions/workflows` and `GET .../actions/runs?per_page=100` | Both listed totals: zero | No listed workflow or run demonstrates build readiness. This does not survey deleted historical runs. |
| Main branch and effective rules | `protected: false`; rulesets including parents and effective main rules both empty | Required-check enforcement is not enabled; #6 still owns configuration and proof. No absent check is a pass. |

The connector rejected some read endpoints and anonymous permissions access required authentication. The existing noninteractive Git credential was used only in process memory for authenticated read-only metadata requests; no credential was printed, written, changed or published. No settings were changed and no workflow/job was created or triggered.

This confirms permitted use, not future runner availability, toolchain/dependency compatibility, a tested database image or execution success. #4/#5 must establish those facts through actual runs and retain failures honestly. No permission to bypass repository controls is implied.

## Checkout and local inventory retained

- Initial server branch `main` equalled the inspected `origin/main`; tracked changes and untracked files were absent. Read-only inspection left it clean.
- Five files existed: `README.md`, `LICENSE`, `CONTRIBUTING.md`, `AGENTS.md`, and `.github/ISSUE_TEMPLATE/implementation_task.md`. No Cargo workspace, toolchain pin, lockfile, Rust source, tests, migration, database configuration, Compose or CI workflow existed.
- This session reused an existing temporary server checkout, separate from the planning checkout. The remote repository is the durable source; future hosted jobs check it out afresh. A permanent local development checkout is not a prerequisite. No checkout was moved.
- Windows x64 reported `Microsoft Windows 10.0.26200`, PowerShell `7.6.6`, Git `2.55.0.windows.5`. Git fetch/inspection worked.
- No `rustup`, `rustc`, `cargo`, `cl`, `link`, `clang`, `clang-cl`, `gcc`, `cmake`, `ninja` or `make` application was discovered on PATH. Configured/default Cargo binaries and installed rustup toolchains were absent. Standard VS Installer/Windows SDK checks did not establish a native build environment.
- No `psql`, `pg_config`, `postgres`, `pg_isready`, `docker` or `podman` application was discovered on PATH; matching Windows services were absent. Selected uninstall registrations found Git and VS Code, not those build/database/container installations.
- The `wsl.exe` launcher existed but the current-user Lxss registration was absent; no distribution was started. This is not a machine-wide audit of custom/other-user/remote installations.
- Only presence, never values, was checked for `DATABASE_URL`, `TEST_DATABASE_URL`, `PGHOST`, `PGPORT`, `PGDATABASE`, `PGUSER`, `PGPASSWORD`, `PGSERVICE` and `PGSERVICEFILE`; all were absent. No database credential file, network target or SQL connection was opened.
- These local findings remain accurate but do not block GitHub-hosted development. No company-laptop installation is approved by the hosted choice.

The inventory used git status/revision/file listings, named `Get-Command` probes, bounded `Test-Path`/directory/registry/service reads and presence-only environment checks. The PR records the reviewed commits and static/link checks. Work is confined to setup/README/contributor documentation and companion planning/issue amendments; the unrelated planning PITON document and archived research/review evidence are untouched.

## Next tasks and acceptance boundary

1. **#4 — initial Rust build:** create the three approved workspace packages and minimum hosted Linux PR workflow together. Pin the actually tested toolchain, dependencies and actions; record the runner image. Run the existing build/initial-test checks and disposable failing-assertion proof against the actual task commits. Nothing is built by this inspection.
2. **#5 — database harness:** extend that same workflow with a pinned disposable PostgreSQL/PostGIS service, synthetic fixtures and the existing connection/PostGIS/isolation/setup/reset/cleanup failure checks. No existing user database or permanent cloud endpoint is needed. Never expose operational credentials.
3. **#6 — full initial CI and enforcement:** extend, do not duplicate, the bootstrap workflow. Complete formatting/lint/build/unit/database coverage, clean reproduction, dependency/licence inventory and the full false-green tests; configure/prove the approved PR/required-check policy before dependent #7 merges.

The order remains #3 → #4 → #5 → #6, one authorised issue per iteration. When the owning issue is authorised, its needed pinned tools/services can be provisioned inside disposable GitHub jobs; that is not permission for laptop installations, production access or persistent paid infrastructure. The eventual native/Compose reference instructions remain required deliverables.

Issue #3 closes only after its inspection documentation passes separate review and merges. #4 is the next candidate after that closure and a subsequent `proceed`; unknown runtime results are work for #4, not a demand for IT to equip the laptop. A failed or unavailable hosted run must remain a recorded failure/unrun result and leave its owning issue open.

No package/toolchain installation, Rust source, build workflow, container start, database connection/reset/migration or runtime test occurred in this inspection/correction. Behavioural red–green, mutation and fuzz tests are inapplicable to this documentation deliverable. Static inspection, source comparisons, link/whitespace checks and separate review do not constitute a successful build or conformance result.
